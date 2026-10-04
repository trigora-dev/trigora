#!/usr/bin/env python3
"""Write THIRD_PARTY_LICENSES for the binaries one artifact redistributes.

cargo deny is the license policy. The crate list is the names that appear in
the built binaries, so a dependency that was compiled but not linked stays
out. cargo-about supplies the license text for that set. SQLite, which
rusqlite compiles in, is recorded separately as public domain.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path


NOTICE_FILES = ("NOTICE", "NOTICE.md", "NOTICE.txt")


def run(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
    )


def arrow_path(workspace: Path, manifest: Path, crate: str) -> str:
    tree = run(
        [
            "cargo",
            "tree",
            "-i",
            crate,
            "--prefix",
            "depth",
            "--edges",
            "normal",
            "--manifest-path",
            str(manifest),
        ],
        workspace,
    )
    chain: list[str] = []
    expect = 0
    for line in tree.stdout.splitlines():
        match = re.match(r"^(\d+)(\S+)", line)
        if not match or int(match.group(1)) != expect:
            if chain:
                break
            continue
        chain.append(match.group(2))
        expect += 1
    if len(chain) < 2:
        return crate
    return " -> ".join(reversed(chain))


def deny(workspace: Path, manifest: Path) -> None:
    result = run(["cargo", "deny", "check", "licenses"], workspace)
    sys.stderr.write(result.stderr)
    sys.stderr.write(result.stdout)
    if result.returncode == 0:
        return
    output = result.stderr + result.stdout
    seen: set[str] = set()
    for crate in re.findall(r"├\s+([A-Za-z0-9_-]+) v\d+", output):
        if crate in seen:
            continue
        seen.add(crate)
        sys.stderr.write(
            f"\nlicense policy violation: {arrow_path(workspace, manifest, crate)}\n"
        )
    raise SystemExit(result.returncode or 1)


def metadata(workspace: Path, manifest: Path) -> dict:
    result = run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
            str(manifest),
        ],
        workspace,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return json.loads(result.stdout)


def crate_names(package: dict) -> set[str]:
    names = {package["name"].replace("-", "_")}
    for target in package.get("targets", []):
        if "lib" in target.get("kind", []) or "cdylib" in target.get("kind", []):
            names.add(target["name"])
    return names


def symbol_text(binary: Path) -> str:
    commands: list[list[str]] = []
    try:
        sysroot = run(["rustc", "--print", "sysroot"], binary.parent)
    except OSError:
        sysroot = None
    if sysroot is not None and sysroot.returncode == 0:
        root = Path(sysroot.stdout.strip())
        commands.extend(
            [str(tool), str(binary)]
            for tool in sorted(root.glob("lib/rustlib/*/bin/llvm-nm*"))
        )
    commands.extend(
        (
            ["llvm-nm", str(binary)],
            ["nm", str(binary)],
            ["strings", str(binary)],
        )
    )
    # link.exe drops the Rust symbol table that ELF and Mach-O retain. The
    # Windows package job passes the linker map, which is plain text.
    chunks: list[str] = []
    try:
        blob = binary.read_bytes().decode("latin-1")
    except OSError:
        blob = ""
    if blob.strip():
        chunks.append(blob)
    for cmd in commands:
        try:
            result = run(cmd, binary.parent)
        except OSError:
            continue
        if result.returncode == 0 and result.stdout.strip():
            chunks.append(result.stdout)
    if not chunks:
        raise SystemExit(f"could not read symbols from {binary}")
    return "\n".join(chunks)


def linked_names(binary: Path, known: set[str]) -> set[str]:
    by_length: dict[int, set[str]] = {}
    for name in known:
        by_length.setdefault(len(name), set()).add(name)
    blob = symbol_text(binary)
    found: set[str] = set()
    for match in re.finditer(r"(?<=_)(\d{1,3})", blob):
        length = int(match.group(1))
        name = blob[match.end() : match.end() + length]
        if name in by_length.get(length, ()) and re.fullmatch(
            r"[A-Za-z_][A-Za-z0-9_]*", name
        ):
            found.add(name)
    return found


def license_groups(expression: str) -> list[list[str]]:
    text = expression.replace("(", " ").replace(")", " ")
    groups: list[list[str]] = []
    for part in re.split(r"\bAND\b", text):
        atoms = []
        for atom in re.split(r"\bOR\b|/", part):
            atom = atom.strip()
            if atom and atom != "WITH":
                atoms.append(atom)
        if atoms:
            groups.append(atoms)
    return groups


def about_texts(workspace: Path, manifest: Path, config: Path) -> dict[str, str]:
    command = [
        "cargo",
        "about",
        "generate",
        "--manifest-path",
        str(manifest),
        "--config",
        str(config),
        "--format",
        "json",
        "--workspace",
        "--locked",
    ]
    result = run([*command, "--offline"], workspace)
    if result.returncode != 0:
        result = run(command, workspace)
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        sys.stderr.write(result.stdout)
        raise SystemExit(result.returncode or 1)
    data = json.loads(result.stdout)
    texts: dict[str, str] = {}
    for entry in data.get("licenses", []):
        text = entry.get("text") or ""
        ident = entry.get("id") or ""
        if ident and len(text) > len(texts.get(ident, "")):
            texts[ident] = text
    return texts


def project_busl(workspace: Path) -> str:
    candidates = [
        workspace / "licenses" / "tcc-engine" / "LICENSE",
        workspace / "LICENSE",
        workspace.parent / "tcc-engine" / "LICENSE",
    ]
    for path in candidates:
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if "Business Source License" in text[:1200]:
            return text
    return ""


def read_notice(root: Path) -> str | None:
    for name in NOTICE_FILES:
        path = root / name
        if path.is_file():
            return path.read_text(encoding="utf-8", errors="replace")
    return None


def sqlite_parts(packages: list[dict]) -> tuple[str, str]:
    package = next(
        (item for item in packages if item["name"] == "libsqlite3-sys"), None
    )
    if package is None:
        raise SystemExit(
            "rusqlite is linked with bundled SQLite, but libsqlite3-sys was not found"
        )
    root = Path(package["manifest_path"]).parent
    source = root / "sqlite3" / "sqlite3.c"
    header = root / "sqlite3" / "sqlite3.h"
    if not source.is_file():
        found = list(root.rglob("sqlite3.c"))
        source = next(
            (path for path in found if "sqlcipher" not in path.parts),
            found[0] if found else source,
        )
    if not header.is_file():
        headers = [
            path for path in root.rglob("sqlite3.h") if "sqlcipher" not in path.parts
        ]
        header = headers[0] if headers else header
    version = None
    if header.is_file():
        match = re.search(
            r'#define SQLITE_VERSION\s+"([^"]+)"',
            header.read_text(encoding="utf-8", errors="replace"),
        )
        if match:
            version = match.group(1)
    if version is None or not source.is_file():
        raise SystemExit(
            "rusqlite is linked with bundled SQLite, but sqlite3 sources were not found"
        )
    text = source.read_text(encoding="utf-8", errors="replace")
    blessing = ""
    marker = text.lower().find("the author disclaims copyright")
    if marker < 0:
        marker = text.lower().find("public domain")
    if marker >= 0:
        start = text.rfind("/*", 0, marker)
        end = text.find("*/", marker)
        if start >= 0 and end > start:
            blessing = text[start : end + 2].strip() + "\n"
    manifest = "\n".join(
        [
            "SQLite",
            f"Version: {version}",
            "Status: Public Domain",
            "Source: https://sqlite.org/",
            "License-text: licenses/third-party/SQLite.txt",
            "",
        ]
    )
    body = blessing or manifest
    return manifest, body if body.endswith("\n") else body + "\n"


def text_for(atom: str, texts: dict[str, str]) -> str | None:
    if atom in texts:
        return texts[atom]
    head = atom.split(" WITH ")[0].strip()
    if head in texts:
        return texts[head]
    return None


def write_bundle(
    out: Path,
    packages: list[dict],
    texts: dict[str, str],
    include_sqlite: bool,
) -> None:
    third = out / "licenses" / "third-party"
    if third.exists():
        shutil.rmtree(third)
    notices = third / "notices"
    third.mkdir(parents=True)
    used: dict[str, str] = {}
    blocks: list[str] = []
    missing: list[str] = []
    for package in sorted(packages, key=lambda item: (item["name"], item["version"])):
        expression = package.get("license") or ""
        if not expression:
            missing.append(f"{package['name']} {package['version']} has no license")
            continue
        groups = license_groups(expression)
        if not groups:
            missing.append(f"{package['name']} {package['version']} has no license")
            continue
        for group in groups:
            if not any(text_for(atom, texts) for atom in group):
                missing.append(f"{package['name']} {package['version']} ({expression})")
        authors = package.get("authors") or []
        lines = [f"{package['name']} {package['version']}"]
        if authors:
            lines.append("Copyright: " + "; ".join(authors))
        lines.append(f"License: {expression}")
        notice = read_notice(Path(package["manifest_path"]).parent)
        if notice is not None:
            notices.mkdir(parents=True, exist_ok=True)
            filename = f"{package['name']}-{package['version']}.NOTICE"
            (notices / filename).write_text(
                notice if notice.endswith("\n") else notice + "\n",
                encoding="utf-8",
            )
            lines.append(f"Notice: licenses/third-party/notices/{filename}")
        blocks.append("\n".join(lines))
        for group in groups:
            for atom in group:
                found = text_for(atom, texts)
                if found:
                    key = atom if atom in texts else atom.split(" WITH ")[0].strip()
                    used.setdefault(key, found)
    if missing:
        raise SystemExit("missing license text for " + "; ".join(missing))
    for ident, text in sorted(used.items()):
        filename = ident.replace("/", "-").replace(" ", "-") + ".txt"
        (third / filename).write_text(
            text if text.endswith("\n") else text + "\n",
            encoding="utf-8",
        )
    body = "\n\n".join(blocks) + "\n"
    if include_sqlite:
        manifest, blessing = sqlite_parts(packages)
        (third / "SQLite.txt").write_text(blessing, encoding="utf-8")
        body += "\n" + manifest
    if not blocks and not include_sqlite:
        body = "No third-party crates are statically linked into this artifact.\n"
    (out / "THIRD_PARTY_LICENSES").write_text(body, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--bin", action="append", default=[])
    parser.add_argument("--exclude", action="append", default=[])
    parser.add_argument("--out", type=Path)
    parser.add_argument("--sqlite", action="store_true")
    parser.add_argument("--skip-deny", action="store_true")
    parser.add_argument("--deny-only", action="store_true")
    parser.add_argument("--about", type=Path)
    args = parser.parse_args()
    manifest = args.manifest.resolve()
    workspace = manifest.parent
    if args.deny_only:
        deny(workspace, manifest)
        return
    if args.out is None:
        raise SystemExit("--out is required")
    if not args.bin:
        raise SystemExit("pass at least one --bin")
    if not args.skip_deny:
        deny(workspace, manifest)
    data = metadata(workspace, manifest)
    packages = data["packages"]
    known: set[str] = set()
    for package in packages:
        known |= crate_names(package)
    found: set[str] = set()
    for binary in args.bin:
        found |= linked_names(Path(binary).resolve(), known)
    if not found:
        raise SystemExit("no crate symbols were found in the given binaries")
    excluded = set(args.exclude)
    selected_names = {
        package["name"] for package in packages if crate_names(package) & found
    }
    selected = [
        package
        for package in packages
        if package["name"] in selected_names and package["name"] not in excluded
    ]
    links_sqlite = args.sqlite and (
        "rusqlite" in selected_names or "libsqlite3-sys" in selected_names
    )
    if links_sqlite:
        selected.extend(
            package
            for package in packages
            if package["name"] == "libsqlite3-sys" and package not in selected
        )
    config = (args.about or Path(__file__).with_name("about.toml")).resolve()
    texts = about_texts(workspace, manifest, config)
    busl = project_busl(workspace)
    if busl:
        texts["BUSL-1.1"] = busl
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    write_bundle(out, selected, texts, links_sqlite)
    print(f"wrote {out / 'THIRD_PARTY_LICENSES'} ({len(selected)} crates)")


if __name__ == "__main__":
    main()
