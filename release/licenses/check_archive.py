#!/usr/bin/env python3
"""Fail when a packed Trigora artifact is missing its license bundle."""

from __future__ import annotations

import re
import sys
import tarfile
import zipfile
from pathlib import Path


def npm(archive: Path) -> None:
    with tarfile.open(archive) as tar:
        names = tar.getnames()
        required = [
            "package/THIRD_PARTY_LICENSES",
            "package/LICENSE",
            "package/LICENSE_NOTICES",
            "package/licenses/tcc-engine/LICENSE",
        ]
        for item in required:
            if item not in names:
                raise SystemExit(f"missing {item}")
        if not any(name.startswith("package/licenses/third-party/") and name.endswith(".txt") for name in names):
            raise SystemExit("missing third-party license text")
        notices = tar.extractfile("package/THIRD_PARTY_LICENSES").read().decode()
        if "tcc-rust-frontend" not in notices:
            raise SystemExit("npm notices are missing the compiler closure")
        adapter = tar.extractfile("package/helper/ts-adapter.js").read().decode()
        for spec in ("@tcc-engine/frontend-typescript", "typescript"):
            if not re.search(rf"from ['\"]{re.escape(spec)}['\"]", adapter):
                raise SystemExit(f"helper/ts-adapter.js no longer imports {spec}")
        index = "package/dist/index.js"
        if index in names:
            body = tar.extractfile(index).read().decode()
            if not re.search(r"from ['\"]picocolors['\"]", body):
                raise SystemExit("dist/index.js no longer imports picocolors")
        for name in names:
            if re.search(r"node_modules/(picocolors|typescript|@tcc-engine/frontend-typescript)/", name):
                raise SystemExit(f"dependency was packed into the tarball: {name}")
    print("npm compliance ok")


def wheel(archive: Path) -> None:
    with zipfile.ZipFile(archive) as packed:
        names = packed.namelist()
        notices = [name for name in names if name.endswith("THIRD_PARTY_LICENSES")]
        if not notices:
            raise SystemExit("wheel is missing THIRD_PARTY_LICENSES")
        text = packed.read(notices[0]).decode()
        if "tcc-rust-frontend" in text:
            raise SystemExit("wheel notices include the compiler closure")
        if "rusqlite" not in text or "SQLite" not in text:
            raise SystemExit("wheel notices are missing the runtime closure")
        if not any(name.endswith("LICENSE_NOTICES") for name in names):
            raise SystemExit("wheel is missing LICENSE_NOTICES")
        if any(name.endswith("tcc-rust-compile") or name.endswith("tcc-rust-compile.exe") for name in names):
            raise SystemExit("wheel contains the compiler binary")
    print("wheel compliance ok")


def resolve(pattern: str) -> Path:
    path = Path(pattern)
    if path.is_file():
        return path
    matches = sorted(Path().glob(pattern))
    if len(matches) != 1:
        raise SystemExit(f"expected one archive for {pattern}, found {len(matches)}")
    return matches[0]


def main() -> None:
    kind, path = sys.argv[1], resolve(sys.argv[2])
    if kind == "npm":
        npm(path)
    elif kind == "wheel":
        wheel(path)
    else:
        raise SystemExit(f"unknown archive kind {kind}")


if __name__ == "__main__":
    main()
