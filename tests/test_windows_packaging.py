"""Release packaging contracts; application binaries are never executed."""

import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

from test_release_version import versioning, set_package_version

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("package_windows", ROOT / "scripts/package_windows.py")
packaging = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packaging)


class WindowsPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="netauth packaging ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.original_path = sys.path[:]
        self.addCleanup(lambda: sys.path.__setitem__(slice(None), self.original_path))
        for relative in (
            "Cargo.toml",
            "Cargo.lock",
            "scripts/setup.template.iss",
            "scripts/windows/process.iss",
            "scripts/windows/service.iss",
            "scripts/windows/data.iss",
            "scripts/windows/legacy-driver.iss",

        ):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        set_package_version(self.root, "1.2.3")
        self.root_patch = patch.object(packaging, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.source = self.root / "downloaded artifact"
        self.source.mkdir()
        self.write_binaries("x64")
        self.output = self.root / "release output"
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "fixture"], check=True)
        subprocess.run(["git", "-C", str(self.root), "tag", "v1.2.3"], check=True)
        self.info = versioning.resolve(self.root, "v1.2.3")
        versioning.stamp(self.root, self.info)
        (self.source / "release-version.json").write_text(json.dumps(self.info), encoding="utf-8")

    def write_binaries(self, arch):
        binary = bytearray(70)
        binary[:2] = b"MZ"
        struct.pack_into("<I", binary, 0x3c, 64)
        binary[64:68] = b"PE\0\0"
        struct.pack_into("<H", binary, 68, {"x64": 0x8664, "arm64": 0xaa64}[arch])
        for name in ("sysu_netauth.exe", "sysu_netauth_service.exe", "sysu_netauth_gui.exe", "sysu_netauth_notify.exe"):
            (self.source / name).write_bytes(binary)

    def compile(self, command, **kwargs):
        self.assertTrue(kwargs["check"])
        script = Path(command[-1])
        self.assertEqual(kwargs["cwd"], script.parent)
        contents = script.read_text(encoding="utf-8")
        self.assertTrue(any(arg.startswith("/DMyAppVersion=") for arg in command))
        source_arg = next(arg for arg in command if arg.startswith("/DAppSourceDir="))
        self.assertTrue(Path(source_arg.split("=", 1)[1]).samefile(self.source))
        self.assertIn("{#AppSourceDir}\\sysu_netauth_gui.exe", contents)
        output = Path(next(arg[2:] for arg in command if arg.startswith("/O")))
        name = next(arg[2:] for arg in command if arg.startswith("/F"))
        (output / f"{name}.exe").write_bytes(b"MZ compiled setup fixture")
        return subprocess.CompletedProcess(command, 0)

    def test_windows_architectures_emit_only_installers_and_checksums(self):
        for arch in ("x64", "arm64"):
            self.write_binaries(arch)
            with patch.object(packaging.subprocess, "run", side_effect=self.compile):
                setup, checksums = packaging.package(self.source, self.output, Path("ISCC.exe"), arch=arch)
            self.assertEqual(setup.name, f"SYSUNetAuth_windows_{arch}_Setup_v1.2.3.exe")
            self.assertEqual(len(checksums.read_text().splitlines()), 1)
            digest, name = checksums.read_text().strip().split("  ")
            self.assertEqual(name, setup.name)
            self.assertEqual(digest, hashlib.sha256(setup.read_bytes()).hexdigest())
        self.assertFalse(list(self.output.glob("*.zip")))

    def test_one_prerelease_tag_drives_all_installers_and_checks_artifact_identity(self):
        subprocess.run(["git", "-C", str(self.root), "tag", "-d", "v1.2.3"], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["git", "-C", str(self.root), "tag", "v0.7.0-alpha.0"], check=True)
        set_package_version(self.root, "0.7.0-alpha.0")
        info = versioning.resolve(self.root, "v0.7.0-alpha.0")
        versioning.stamp(self.root, info)
        (self.source / "release-version.json").write_text(json.dumps(info), encoding="utf-8")
        with patch.object(packaging.subprocess, "run", side_effect=self.compile):
            installer = packaging.package(self.source, self.output, Path("ISCC.exe"))
        self.assertIn("_v0.7.0-alpha.0.exe", installer[0].name)
        (self.source / "release-version.json").write_text(json.dumps(info | {"commit": "another-commit"}), encoding="utf-8")
        with patch.object(packaging.subprocess, "run") as compile_setup:
            with self.assertRaisesRegex(ValueError, "does not match"):
                packaging.package(self.source, self.output, Path("ISCC.exe"))
            compile_setup.assert_not_called()

    def test_incomplete_artifacts_fail_before_compilation(self):
        (self.source / "sysu_netauth_service.exe").unlink()
        with patch.object(packaging.subprocess, "run") as compile_setup:
            with self.assertRaisesRegex(FileNotFoundError, "sysu_netauth_service.exe"):
                packaging.package(self.source, self.output, Path("ISCC.exe"))
            compile_setup.assert_not_called()

    def test_compiler_failure_cannot_reuse_stale_installer(self):
        self.output.mkdir()
        for arch in ("x64", "arm64"):
            self.write_binaries(arch)
            with self.subTest(arch=arch):
                stale = self.output / f"SYSUNetAuth_windows_{arch}_Setup_v1.2.3.exe"
                stale.write_bytes(b"stale setup")
                with patch.object(packaging.subprocess, "run", side_effect=subprocess.CalledProcessError(2, "ISCC.exe")):
                    with self.assertRaises(subprocess.CalledProcessError):
                        packaging.package(self.source, self.output, Path("ISCC.exe"), arch=arch)
                self.assertFalse(stale.exists())

    def test_success_exit_without_installer_is_rejected(self):
        with patch.object(packaging.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
            with self.assertRaisesRegex(FileNotFoundError, "did not produce"):
                packaging.package(self.source, self.output, Path("ISCC.exe"))

    def test_arm64_installer_and_checksums_are_distinct(self):
        self.write_binaries("arm64")
        with patch.object(packaging.subprocess, "run", side_effect=self.compile) as compiler:
            setup, checksums = packaging.package(self.source, self.output, Path("ISCC.exe"), arch="arm64")
        self.assertEqual(setup.name, "SYSUNetAuth_windows_arm64_Setup_v1.2.3.exe")
        self.assertIn("/DMyAppArch=arm64", compiler.call_args.args[0])
        self.assertEqual(checksums.name, "SHA256SUMS-windows-arm64.txt")
        self.assertEqual(len(checksums.read_text().splitlines()), 1)
        self.assertFalse(list(self.output.glob("*.zip")))

    def test_mislabeled_and_mixed_architecture_artifacts_are_rejected(self):
        with patch.object(packaging.subprocess, "run") as compiler:
            with self.assertRaisesRegex(ValueError, "does not match arm64"):
                packaging.package(self.source, self.output, Path("ISCC.exe"), arch="arm64")
            x64_gui = (self.source / "sysu_netauth_gui.exe").read_bytes()
            self.write_binaries("arm64")
            (self.source / "sysu_netauth_gui.exe").write_bytes(x64_gui)
            with self.assertRaisesRegex(ValueError, "does not match arm64.*gui"):
                packaging.package(self.source, self.output, Path("ISCC.exe"), arch="arm64")
            compiler.assert_not_called()

    def test_unsupported_architecture_is_rejected_before_compilation(self):
        with patch.object(packaging.subprocess, "run") as compiler:
            with self.assertRaisesRegex(ValueError, "Unsupported Windows architecture"):
                packaging.package(self.source, self.output, Path("ISCC.exe"), arch="x86")
            compiler.assert_not_called()


if __name__ == "__main__":
    unittest.main()
