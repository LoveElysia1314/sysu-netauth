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
import zipfile

from test_release_version import versioning

ROOT = Path(__file__).resolve().parent.parent
RUST_VERSION = "1.2.3"
PYTHON_VERSION = "1.2.3"
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
            "apps/rust/Cargo.toml",
            "Cargo.lock",
            "apps/rust/scripts/setup.template.iss",
            "apps/rust/scripts/npcap.iss",
            "scripts/windows/process.iss",
            "scripts/windows/service.iss",
            "apps/python/scripts/build.py",
            "apps/python/pyproject.toml",
            "apps/python/scripts/setup.template.iss",
            "apps/python/sysu_netauth/core/config.py",
            "apps/python/updates/release.json",
        ):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        self.root_patch = patch.object(packaging, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.source = self.root / "downloaded artifact"
        self.source.mkdir()
        self.write_binaries("x64")
        (self.source / "_internal").mkdir()
        (self.source / "_internal/python313.dll").write_bytes(b"runtime fixture")
        self.output = self.root / "release output"
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "fixture"], check=True)
        subprocess.run(["git", "-C", str(self.root), "tag", "v1.2.3"], check=True)
        self.info = versioning.resolve(self.root, "v1.2.3")
        versioning.stamp(self.root, self.info)
        (self.source / "release-version.json").write_text(json.dumps(self.info), encoding="utf-8")
        self.python_version = PYTHON_VERSION

    def write_binaries(self, arch):
        binary = bytearray(70)
        binary[:2] = b"MZ"
        struct.pack_into("<I", binary, 0x3c, 64)
        binary[64:68] = b"PE\0\0"
        struct.pack_into("<H", binary, 68, {"x64": 0x8664, "arm64": 0xaa64}[arch])
        for name in ("sysu_netauth.exe", "sysu_netauth_service.exe", "sysu_netauth_gui.exe"):
            (self.source / name).write_bytes(binary)

    def compile(self, command, **kwargs):
        self.assertTrue(kwargs["check"])
        script = Path(command[-1])
        self.assertEqual(kwargs["cwd"], script.parent)
        contents = script.read_text(encoding="utf-8")
        if any(arg.startswith("/DMyAppVersion=") for arg in command):
            source_arg = next(arg for arg in command if arg.startswith("/DAppSourceDir="))
            self.assertTrue(Path(source_arg.split("=", 1)[1]).samefile(self.source))
            self.assertIn("{#AppSourceDir}\\sysu_netauth_gui.exe", contents)
        else:
            self.assertNotIn("@APP_", contents)
            self.assertIn(f'MyAppVersion "{self.python_version}"', contents)
            self.assertRegex(contents, r"VersionInfoVersion=\d+\.\d+\.\d+\s")
            self.assertIn("recursesubdirs", contents)
            self.assertIn(str(self.source.relative_to(self.root)), contents)
        output = Path(next(arg[2:] for arg in command if arg.startswith("/O")))
        name = next(arg[2:] for arg in command if arg.startswith("/F"))
        (output / f"{name}.exe").write_bytes(b"MZ compiled setup fixture")
        return subprocess.CompletedProcess(command, 0)

    def test_all_windows_variants_have_installers_portable_binaries_and_checksums(self):
        self.python_version = PYTHON_VERSION
        self.output.mkdir()
        for label in ("Rust_msvc_x64", "Rust_gnu_x64", "Python_x64"):
            (self.output / f"SYSUNetAuth_{label}_Setup_v1.2.3.zip").write_bytes(b"obsolete installer ZIP")
        with patch.object(packaging.subprocess, "run", side_effect=self.compile) as compile_setup:
            rust = packaging.package("rust", self.source, self.output, Path("ISCC.exe"))
            gnu = packaging.package("rust", self.source, self.output, Path("ISCC.exe"), abi="gnu")
            python = packaging.package("python", self.source, self.output, Path("ISCC.exe"))
        self.assertEqual(compile_setup.call_count, 3)
        self.assertEqual(rust[0].name, f"SYSUNetAuth_Rust_msvc_x64_Setup_v{RUST_VERSION}.exe")
        self.assertEqual(python[0].name, f"SYSUNetAuth_Python_x64_Setup_v{PYTHON_VERSION}.exe")
        self.assertEqual(gnu[0].name, f"SYSUNetAuth_Rust_gnu_x64_Setup_v{RUST_VERSION}.exe")
        self.assertTrue(any("/DMyAppAbi=gnu" in call.args[0] for call in compile_setup.call_args_list))
        for setup, portable, checksums in (rust, gnu, python):
            with zipfile.ZipFile(portable) as bundle:
                self.assertEqual(bundle.read("sysu_netauth/sysu_netauth.exe"), (self.source / "sysu_netauth.exe").read_bytes())
                self.assertIn("sysu_netauth/sysu_netauth_service.exe", bundle.namelist())
                if "Python" in portable.name:
                    self.assertIn("sysu_netauth/_internal/python313.dll", bundle.namelist())
                else:
                    self.assertEqual(len(bundle.namelist()), 3)
                    self.assertIn("sysu_netauth/sysu_netauth_gui.exe", bundle.namelist())
            self.assertEqual(len(checksums.read_text().splitlines()), 2)
            self.assertEqual({line.split("  ")[1] for line in checksums.read_text().splitlines()}, {setup.name, portable.name})
            for line in checksums.read_text(encoding="utf-8").splitlines():
                digest, name = line.split("  ")
                self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        self.assertFalse(list(self.output.glob("*_Setup_*.zip")))
        self.assertFalse((self.root / "apps/python/scripts/setup.iss").exists())

    def test_one_prerelease_tag_drives_both_installers_and_checks_artifact_identity(self):
        subprocess.run(["git", "-C", str(self.root), "tag", "-d", "v1.2.3"], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["git", "-C", str(self.root), "tag", "v0.7.0-alpha.0"], check=True)
        info = versioning.resolve(self.root, "v0.7.0-alpha.0")
        versioning.stamp(self.root, info)
        (self.source / "release-version.json").write_text(json.dumps(info), encoding="utf-8")
        self.python_version = "0.7.0a0"
        with patch.object(packaging.subprocess, "run", side_effect=self.compile):
            rust = packaging.package("rust", self.source, self.output, Path("ISCC.exe"))
            gnu = packaging.package("rust", self.source, self.output, Path("ISCC.exe"), abi="gnu")
            python = packaging.package("python", self.source, self.output, Path("ISCC.exe"))
        self.assertIn("_v0.7.0-alpha.0.exe", rust[0].name)
        self.assertIn("_v0.7.0-alpha.0.exe", gnu[0].name)
        self.assertIn("_v0.7.0a0.exe", python[0].name)
        (self.source / "release-version.json").write_text(json.dumps(info | {"commit": "another-commit"}), encoding="utf-8")
        with patch.object(packaging.subprocess, "run") as compile_setup:
            with self.assertRaisesRegex(ValueError, "does not match"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"))
            compile_setup.assert_not_called()

    def test_incomplete_artifacts_fail_before_compilation(self):
        (self.source / "_internal").rename(self.source / "missing runtime")
        with patch.object(packaging.subprocess, "run") as compile_setup:
            with self.assertRaisesRegex(FileNotFoundError, "_internal"):
                packaging.package("python", self.source, self.output, Path("ISCC.exe"))
            (self.source / "sysu_netauth_service.exe").unlink()
            with self.assertRaisesRegex(FileNotFoundError, "sysu_netauth_service.exe"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"))
            compile_setup.assert_not_called()

    def test_compiler_failure_cannot_reuse_stale_installer(self):
        self.output.mkdir()
        for variant, abi, label, version in (
            ("rust", "msvc", "Rust_msvc_x64", RUST_VERSION),
            ("rust", "gnu", "Rust_gnu_x64", RUST_VERSION),
            ("python", None, "Python_x64", PYTHON_VERSION),
        ):
            with self.subTest(variant=variant):
                stale = self.output / f"SYSUNetAuth_{label}_Setup_v{version}.exe"
                stale.write_bytes(b"stale setup")
                with patch.object(packaging.subprocess, "run", side_effect=subprocess.CalledProcessError(2, "ISCC.exe")):
                    with self.assertRaises(subprocess.CalledProcessError):
                        packaging.package(variant, self.source, self.output, Path("ISCC.exe"), abi=abi)
                self.assertFalse(stale.exists())
                self.assertFalse((self.root / "apps/python/scripts/setup.iss").exists())

    def test_success_exit_without_installer_is_rejected(self):
        with patch.object(packaging.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
            with self.assertRaisesRegex(FileNotFoundError, "did not produce"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"))

    def test_arm64_installer_portable_and_checksums_are_distinct(self):
        self.write_binaries("arm64")
        with patch.object(packaging.subprocess, "run", side_effect=self.compile) as compiler:
            setup, portable, checksums = packaging.package("rust", self.source, self.output, Path("ISCC.exe"), arch="arm64")
        self.assertEqual(setup.name, "SYSUNetAuth_Rust_msvc_arm64_Setup_v1.2.3.exe")
        self.assertIn("/DMyAppArch=arm64", compiler.call_args.args[0])
        self.assertEqual(checksums.name, "SHA256SUMS-rust-msvc-arm64.txt")
        with zipfile.ZipFile(portable) as archive:
            self.assertEqual(len(archive.namelist()), 3)
            for name in archive.namelist():
                self.assertEqual(struct.unpack_from("<H", archive.read(name), 68)[0], 0xaa64)
        for line in checksums.read_text(encoding="utf-8").splitlines():
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())

    def test_mislabeled_and_mixed_architecture_artifacts_are_rejected(self):
        with patch.object(packaging.subprocess, "run") as compiler:
            with self.assertRaisesRegex(ValueError, "does not match arm64"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"), arch="arm64")
            x64_gui = (self.source / "sysu_netauth_gui.exe").read_bytes()
            self.write_binaries("arm64")
            (self.source / "sysu_netauth_gui.exe").write_bytes(x64_gui)
            with self.assertRaisesRegex(ValueError, "does not match arm64.*gui"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"), arch="arm64")
            compiler.assert_not_called()

    def test_unsupported_arm64_variants_are_rejected(self):
        with patch.object(packaging.subprocess, "run") as compiler:
            for variant, abi in (("python", None), ("rust", "gnu")):
                with self.assertRaisesRegex(ValueError, "ARM64 requires"):
                    packaging.package(variant, self.source, self.output, Path("ISCC.exe"), abi=abi, arch="arm64")
            compiler.assert_not_called()

    def test_invalid_or_misapplied_abi_is_rejected_before_compilation(self):
        with patch.object(packaging.subprocess, "run") as compile_setup:
            with self.assertRaisesRegex(ValueError, "Unsupported Rust ABI"):
                packaging.package("rust", self.source, self.output, Path("ISCC.exe"), abi="unknown")
            with self.assertRaisesRegex(ValueError, "only to Rust"):
                packaging.package("python", self.source, self.output, Path("ISCC.exe"), abi="gnu")
            compile_setup.assert_not_called()


if __name__ == "__main__":
    unittest.main()
