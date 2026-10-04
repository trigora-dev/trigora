from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


def _packaged(name: str) -> Path | None:
    filename = f"{name}.exe" if os.name == "nt" else name
    path = Path(__file__).resolve().parent / "_vendor" / filename
    if path.is_file():
        return path
    return None


def trigora_binary() -> Path | None:
    override = os.environ.get("TRIGORA_BIN")
    if override:
        path = Path(override)
        return path if path.is_file() else None
    return _packaged("trigora")


def main() -> None:
    binary = trigora_binary()
    if binary is None:
        print("The trigora binary is missing. Reinstall trigora-cli.", file=sys.stderr)
        raise SystemExit(1)
    env = os.environ.copy()
    local = _packaged("trigora-local")
    if local is not None:
        env.setdefault("TRIGORA_LOCAL_BIN", str(local))
    command = [str(binary), *sys.argv[1:]]
    # Windows has no exec that replaces this interpreter. The console-script
    # launcher stays alive and forwards the native binary's status.
    if os.name == "nt":
        raise SystemExit(subprocess.run(command, env=env, check=False).returncode)
    os.execvpe(command[0], command, env)


if __name__ == "__main__":
    main()
