from __future__ import annotations

import importlib.util
import os
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
VENDOR = PACKAGE / "src" / "trigora_cli" / "_vendor"


def wheel_command(output: Path) -> list[str]:
    probe = subprocess.run(
        [sys.executable, "-m", "pip", "--version"],
        capture_output=True,
        text=True,
        check=False,
    )
    version = probe.stdout.split()[1] if len(probe.stdout.split()) > 1 else "0"
    major = int(version.split(".", maxsplit=1)[0])
    if major >= 23:
        return [sys.executable, "-m", "pip", "wheel", "--no-deps", "-w", str(output), "."]
    venv = output / "build-venv"
    subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
    pip = venv / ("Scripts/pip.exe" if os.name == "nt" else "bin/pip")
    subprocess.run([str(pip), "install", "-U", "pip", "setuptools", "wheel"], check=True)
    return [str(pip), "wheel", "--no-deps", "-w", str(output), "."]


def load_vendor_script():
    path = PACKAGE / "scripts" / "vendor_cli.py"
    spec = importlib.util.spec_from_file_location("vendor_cli", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class LauncherTests(unittest.TestCase):
    def test_missing_binary_does_not_start_node(self) -> None:
        env = os.environ.copy()
        env["TRIGORA_BIN"] = str(Path(tempfile.gettempdir()) / "trigora-missing-binary")
        env.pop("TRIGORA_NODE_HELPER", None)
        result = subprocess.run(
            [sys.executable, "-c", "from trigora_cli.cli import main; main()"],
            check=False,
            cwd=PACKAGE,
            env={**env, "PYTHONPATH": str(PACKAGE / "src")},
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("The trigora binary is missing. Reinstall trigora-cli.", result.stderr)
        self.assertNotIn("node", result.stderr.lower())

    def test_console_script_execs_the_native_binary_without_node(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stub = root / "trigora"
            stub.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' \"$1\"\n"
                "printf 'helper=%s\\n' \"${TRIGORA_NODE_HELPER-unset}\"\n"
                "printf 'local=%s\\n' \"${TRIGORA_LOCAL_BIN-unset}\"\n"
            )
            stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
            created_local = False
            VENDOR.mkdir(parents=True, exist_ok=True)
            local_name = "trigora-local.exe" if os.name == "nt" else "trigora-local"
            local = VENDOR / local_name
            if not local.exists():
                local.write_text("#!/bin/sh\nexit 0\n")
                local.chmod(0o755)
                created_local = True
            env = os.environ.copy()
            env["TRIGORA_BIN"] = str(stub)
            env["PYTHONPATH"] = str(PACKAGE / "src")
            env.pop("TRIGORA_NODE_HELPER", None)
            env.pop("TRIGORA_LOCAL_BIN", None)
            try:
                result = subprocess.run(
                    [sys.executable, "-c", "from trigora_cli.cli import main; main()", "dev"],
                    check=False,
                    cwd=PACKAGE,
                    env=env,
                    capture_output=True,
                    text=True,
                )
            finally:
                if created_local:
                    local.unlink(missing_ok=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("dev", result.stdout)
            self.assertIn("helper=unset", result.stdout)
            self.assertIn(f"local={local}", result.stdout)

    def test_vendor_script_copies_both_host_binaries(self) -> None:
        vendor_cli = load_vendor_script()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "source"
            dest = root / "dest"
            source.mkdir()
            for name in ("trigora", "trigora-local"):
                filename = vendor_cli.host_name(name)
                path = source / filename
                path.write_text("#!/bin/sh\n")
                path.chmod(0o755)
            vendor_cli.copy_binaries(source, dest)
            for name in ("trigora", "trigora-local"):
                copied = dest / vendor_cli.host_name(name)
                self.assertTrue(copied.is_file())
                self.assertTrue(copied.stat().st_mode & stat.S_IXUSR)

    def test_wheel_is_platform_specific(self) -> None:
        created: list[Path] = []
        VENDOR.mkdir(parents=True, exist_ok=True)
        for name in ("trigora", "trigora-local"):
            filename = f"{name}.exe" if os.name == "nt" else name
            path = VENDOR / filename
            if not path.exists():
                path.write_bytes(b"#!/bin/sh\nexit 0\n")
                path.chmod(0o755)
                created.append(path)
        with tempfile.TemporaryDirectory() as tmp:
            try:
                result = subprocess.run(
                    wheel_command(Path(tmp)),
                    check=False,
                    cwd=PACKAGE,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                wheels = list(Path(tmp).glob("*.whl"))
                self.assertEqual(len(wheels), 1, result.stdout + result.stderr)
                self.assertNotIn("none-any", wheels[0].name)
                with zipfile.ZipFile(wheels[0]) as archive:
                    names = archive.namelist()
                packaged = [name for name in names if name.endswith(("/trigora", "/trigora.exe"))]
                self.assertTrue(packaged, names)
                self.assertFalse(any(name.startswith("trigora/") for name in names), names)
                self.assertFalse(
                    any("node-helper" in name or "tcc-rust-compile" in name for name in names),
                    names,
                )
            finally:
                for path in created:
                    path.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
