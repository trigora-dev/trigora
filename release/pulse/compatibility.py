#!/usr/bin/env python3
"""Read Pulse release identity from the tree and check built artifacts.

The version numbers are taken from Cargo.toml and the protocol constants.
This script does not keep a second copy of them.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tarfile
from pathlib import Path

PLATFORMS = ("linux-x64", "linux-arm64", "darwin-arm64", "darwin-x64")
REPOSITORY = "trigora-dev/trigora"
RELEASE_TAG = "pulse-v1.0.0"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def cargo_package_version(path: Path) -> str:
    text = read_text(path)
    match = re.search(
        r'(?ms)^\[package\]\s.*?^version\s*=\s*"([^"]+)"',
        text,
    )
    if not match:
        raise SystemExit(f"missing package version in {path}")
    return match.group(1)


def tcc_engine_version(path: Path) -> str:
    text = read_text(path)
    section = re.search(r"(?ms)^\[dependencies\]\s(.*?)(?:^\[|\Z)", text)
    if not section:
        raise SystemExit(f"missing dependencies in {path}")
    found: dict[str, str] = {}
    for match in re.finditer(
        r'(?m)^(tcc-[\w-]*)\s*=\s*\{([^}]*)\}',
        section.group(1),
    ):
        version = re.search(r'version\s*=\s*"([^"]+)"', match.group(2))
        if not version:
            raise SystemExit(f"{match.group(1)} has no version in {path}")
        found[match.group(1)] = version.group(1)
    if not found:
        raise SystemExit(f"no TCC dependencies in {path}")
    versions = set(found.values())
    if len(versions) != 1:
        raise SystemExit(f"TCC dependency versions differ in {path}: {found}")
    return versions.pop()


def rust_constant(path: Path, name: str) -> str:
    text = read_text(path)
    match = re.search(rf"{name}:\s*(?:&str|u32)\s*=\s*([^;]+);", text)
    if not match:
        raise SystemExit(f"missing {name} in {path}")
    raw = match.group(1).strip()
    if raw.startswith('"') and raw.endswith('"'):
        return raw[1:-1]
    return raw


def identity(root: Path) -> dict[str, object]:
    pulse_version = cargo_package_version(root / "crates/trigora-pulse/Cargo.toml")
    protocol = int(
        rust_constant(
            root / "crates/trigora-pulse/src/socket/protocol.rs",
            "ADAPTER_PROTOCOL",
        )
    )
    cli_version = rust_constant(root / "crates/trigora-cli/src/pulse/binary.rs", "PULSE_VERSION")
    cli_protocol = int(
        rust_constant(root / "crates/trigora-cli/src/pulse/binary.rs", "PULSE_PROTOCOL")
    )
    if cli_version != pulse_version or cli_protocol != protocol:
        raise SystemExit(
            "CLI Pulse expectation "
            f"{cli_version} protocol {cli_protocol} does not match host "
            f"{pulse_version} protocol {protocol}"
        )
    return {
        "pulse_version": pulse_version,
        "adapter_protocol": protocol,
        "tcc_engine": tcc_engine_version(root / "crates/trigora-pulse/Cargo.toml"),
        "cli_min_version": cargo_package_version(root / "crates/trigora-cli/Cargo.toml"),
    }


def parse_host_version(output: str) -> tuple[str, int]:
    parts = output.split()
    if len(parts) != 4 or parts[0] != "trigora-pulse" or parts[2] != "protocol":
        raise SystemExit(f"unexpected host version line: {output.strip()}")
    try:
        protocol = int(parts[3])
    except ValueError as error:
        raise SystemExit(f"unexpected host protocol: {output.strip()}") from error
    return parts[1], protocol


def check_host(root: Path, output: str) -> None:
    expected = identity(root)
    version, protocol = parse_host_version(output)
    if version != expected["pulse_version"] or protocol != expected["adapter_protocol"]:
        raise SystemExit(
            f"host reported {version} protocol {protocol}; "
            f"tree expects {expected['pulse_version']} protocol {expected['adapter_protocol']}"
        )


def archive_name(version: str, slot: str) -> str:
    return f"trigora-pulse-{version}-{slot}.tar.gz"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def write_sums(directory: Path, names: list[str]) -> None:
    lines = [f"{sha256(directory / name)}  {name}" for name in names]
    (directory / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


def verify_sums(directory: Path) -> None:
    sums = directory / "SHA256SUMS"
    if not sums.is_file():
        raise SystemExit(f"missing {sums}")
    seen: list[str] = []
    for line in sums.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        digest, name = line.split(maxsplit=1)
        path = directory / name
        if not path.is_file():
            raise SystemExit(f"checksum entry missing file: {name}")
        actual = sha256(path)
        if actual != digest:
            raise SystemExit(f"checksum mismatch for {name}")
        seen.append(name)
    if not seen:
        raise SystemExit("SHA256SUMS is empty")


def assemble(root: Path, bin_dir: Path, out_dir: Path) -> None:
    expected = identity(root)
    version = str(expected["pulse_version"])
    out_dir.mkdir(parents=True, exist_ok=True)
    names: list[str] = []
    for slot in PLATFORMS:
        binary = bin_dir / slot / "trigora-pulse"
        if not binary.is_file():
            raise SystemExit(f"missing binary for {slot}: {binary}")
        name = archive_name(version, slot)
        destination = out_dir / name
        with tarfile.open(destination, "w:gz") as archive:
            info = archive.gettarinfo(binary, arcname="trigora-pulse")
            info.mode = 0o755
            with binary.open("rb") as handle:
                archive.addfile(info, handle)
        names.append(name)
    write_sums(out_dir, names)
    manifest = {
        **expected,
        "platforms": list(PLATFORMS),
    }
    (out_dir / "pulse-compatibility.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )
    verify_bundle(root, out_dir)


def verify_bundle(root: Path, directory: Path) -> dict[str, object]:
    expected = identity(root)
    version = str(expected["pulse_version"])
    manifest_path = directory / "pulse-compatibility.json"
    if not manifest_path.is_file():
        raise SystemExit(f"missing {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("platforms") != list(PLATFORMS):
        raise SystemExit(f"platform list is {manifest.get('platforms')}")
    for key, value in expected.items():
        if manifest.get(key) != value:
            raise SystemExit(f"{key} is {manifest.get(key)}; tree has {value}")
    names = [archive_name(version, slot) for slot in PLATFORMS]
    for name in names:
        if not (directory / name).is_file():
            raise SystemExit(f"missing archive {name}")
    verify_sums(directory)
    recorded = [
        line.split(maxsplit=1)[1]
        for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    if recorded != names:
        raise SystemExit(f"SHA256SUMS lists {recorded}")
    return manifest


def preflight(root: Path, directory: Path, repository: str, ref: str) -> None:
    if repository != REPOSITORY:
        raise SystemExit(f"repository is {repository}")
    if ref != f"refs/tags/{RELEASE_TAG}":
        raise SystemExit(f"ref is {ref}")
    manifest = verify_bundle(root, directory)
    tag_version = RELEASE_TAG.removeprefix("pulse-v")
    if manifest["pulse_version"] != tag_version:
        raise SystemExit(
            f"tag {RELEASE_TAG} does not match pulse_version {manifest['pulse_version']}"
        )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=repo_root())
    sub = parser.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check-host")
    check.add_argument("--version-output", required=True)

    build = sub.add_parser("assemble")
    build.add_argument("--bin-dir", type=Path, required=True)
    build.add_argument("--out", type=Path, required=True)

    verify = sub.add_parser("verify")
    verify.add_argument("--dir", type=Path, required=True)

    gate = sub.add_parser("preflight")
    gate.add_argument("--dir", type=Path, required=True)
    gate.add_argument("--repository", required=True)
    gate.add_argument("--ref", required=True)

    args = parser.parse_args(argv)
    root = args.root.resolve()
    if args.command == "check-host":
        check_host(root, args.version_output)
    elif args.command == "assemble":
        assemble(root, args.bin_dir.resolve(), args.out.resolve())
    elif args.command == "verify":
        verify_bundle(root, args.dir.resolve())
    elif args.command == "preflight":
        preflight(root, args.dir.resolve(), args.repository, args.ref)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except BrokenPipeError:
        raise SystemExit(1) from None
