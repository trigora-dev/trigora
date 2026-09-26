#!/usr/bin/env python3
"""Copy the host trigora and trigora-local binaries into package data."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path


def host_name(name: str) -> str:
    return f"{name}.exe" if sys.platform == "win32" else name


def copy_binaries(source_dir: Path, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    for name in ("trigora", "trigora-local"):
        filename = host_name(name)
        source = source_dir / filename
        if not source.is_file():
            raise SystemExit(f"{filename} was not found in {source_dir}.")
        target = dest / filename
        shutil.copy2(source, target)
        target.chmod(0o755)


def cargo_target() -> Path | None:
    repo = Path(__file__).resolve().parents[3]
    for profile in ("release", "debug"):
        directory = repo / "target" / profile
        if (directory / host_name("trigora")).is_file() and (
            directory / host_name("trigora-local")
        ).is_file():
            return directory
    return None


def main() -> None:
    source = cargo_target()
    if source is None:
        raise SystemExit("trigora and trigora-local were not found. Build them in this repo first.")
    dest = Path(__file__).resolve().parents[1] / "src" / "trigora_cli" / "_vendor"
    copy_binaries(source, dest)


if __name__ == "__main__":
    main()
