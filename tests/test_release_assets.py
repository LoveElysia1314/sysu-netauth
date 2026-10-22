"""Required release assets stay complete while advisory platforms publish independently."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("prepare_release_assets", ROOT / "scripts/prepare_release_assets.py")
assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(assets)


class ReleaseAssetsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="netauth release assets ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.info = {"tag": "v0.7.0-alpha.0", "version": "0.7.0-alpha.0", "python_version": "0.7.0a0", "commit": "expected-commit"}
        self.metadata = self.root / "release-version.json"
        self.metadata.write_text(json.dumps(self.info), encoding="utf-8")
        self.output = self.root / "output"
        self.output.mkdir()
        for label, version in (("Rust_msvc_x64", "0.7.0-alpha.0"), ("Rust_msvc_arm64", "0.7.0-alpha.0"), ("Rust_gnu_x64", "0.7.0-alpha.0"), ("Python_x64", "0.7.0a0")):
            names = [f"SYSUNetAuth_{label}_Setup_v{version}.exe", f"SYSUNetAuth_{label}_Portable_v{version}.zip"]
            checksums = []
            for name in names:
                payload = name.encode()
                (self.output / name).write_bytes(payload)
                checksums.append(f"{hashlib.sha256(payload).hexdigest()}  {name}\n")
            (self.output / f"SHA256SUMS-{label.lower().replace('_', '-')}.txt").write_text("".join(checksums), encoding="utf-8")
        self.linux = self.root / "linux.tar.gz"
        self.write_linux(self.info)
        self.macos = {arch: self.root / f"macos-{arch}.tar.gz" for arch in ("arm64", "x64")}
        for arch in self.macos:
            self.write_macos(arch, self.info)

    def prepare(self):
        return assets.prepare(self.metadata, self.linux, self.output, macos_inputs=self.macos)

    def write_macos(self, arch, info, omit=None):
        with tarfile.open(self.macos[arch], "w:gz") as archive:
            for name in ("sysu_netauth", "sysu_netauth_service", "release-version.json",
                         "io.github.peterrobot.sysu-netauth.plist", "install-macos.sh", "uninstall-macos.sh", "launchd-common.sh"):
                if name == omit:
                    continue
                payload = json.dumps(info).encode() if name.endswith(".json") else b"binary or script fixture"
                member = tarfile.TarInfo(f"sysu-netauth-macos-{arch}/{name}")
                member.size = len(payload)
                member.mode = 0o755
                archive.addfile(member, io.BytesIO(payload))

    def write_linux(self, info, omit=None):
        with tarfile.open(self.linux, "w:gz") as archive:
            for name in ("sysu_netauth", "sysu_netauth_service", "sysu-netauth.service", "install-linux.sh", "release-version.json"):
                if name == omit:
                    continue
                payload = json.dumps(info).encode() if name.endswith(".json") else b"binary or script fixture"
                member = tarfile.TarInfo(f"sysu-netauth-linux-x64/{name}")
                member.size = len(payload)
                member.mode = 0o755
                archive.addfile(member, io.BytesIO(payload))

    def test_collects_all_seven_variants_and_checksums(self):
        result = self.prepare()
        self.assertEqual(len(result), 18)
        self.assertEqual(sum(path.name.endswith(".exe") for path in result), 4)
        self.assertEqual(sum("_Portable_" in path.name for path in result), 4)
        self.assertEqual(sum(path.suffix == ".zip" for path in result), 4)
        linux = self.output / "SYSUNetAuth_Rust_linux_x64_v0.7.0-alpha.0.tar.gz"
        self.assertEqual(linux.read_bytes(), self.linux.read_bytes())
        self.assertEqual((self.output / "SHA256SUMS-rust-linux-x64.txt").read_text(), f"{hashlib.sha256(linux.read_bytes()).hexdigest()}  {linux.name}\n")
        for arch, source in self.macos.items():
            name = f"SYSUNetAuth_Rust_macos_{arch}_v0.7.0-alpha.0.tar.gz"
            self.assertEqual((self.output / name).read_bytes(), source.read_bytes())
            self.assertEqual((self.output / f"SHA256SUMS-rust-macos-{arch}.txt").read_text(),
                             f"{hashlib.sha256(source.read_bytes()).hexdigest()}  {name}\n")

    def test_missing_gnu_variant_blocks_release(self):
        for path in self.output.glob("*gnu*"):
            path.unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*gnu"):
            self.prepare()

    def test_missing_windows_arm64_variant_blocks_release(self):
        for path in self.output.glob("*msvc*arm64*"):
            path.unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*msvc_arm64"):
            self.prepare()

    def test_obsolete_installer_zip_blocks_release(self):
        obsolete = self.output / "SYSUNetAuth_Rust_msvc_x64_Setup_v0.7.0-alpha.0.zip"
        obsolete.write_bytes(b"obsolete installer ZIP")
        with self.assertRaisesRegex(ValueError, "unexpected=.*_Setup_.*zip"):
            self.prepare()

    def test_corrupt_windows_package_blocks_release(self):
        next(self.output.glob("*msvc*Portable*.zip")).write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            self.prepare()

    def test_linux_from_another_commit_blocks_release(self):
        self.write_linux(self.info | {"commit": "another-commit"})
        with self.assertRaisesRegex(ValueError, "version/commit"):
            self.prepare()

    def test_linux_without_service_blocks_release(self):
        self.write_linux(self.info, omit="sysu_netauth_service")
        with self.assertRaises(KeyError):
            self.prepare()

    def test_missing_macos_architecture_blocks_release(self):
        self.macos.pop("x64")
        with self.assertRaisesRegex(ValueError, "Missing macOS archives"):
            self.prepare()

    def test_macos_from_another_commit_blocks_release(self):
        self.write_macos("arm64", self.info | {"commit": "another-commit"})
        with self.assertRaisesRegex(ValueError, "macos-arm64.*version/commit"):
            self.prepare()

    def test_macos_without_uninstaller_blocks_release(self):
        self.write_macos("x64", self.info, omit="uninstall-macos.sh")
        with self.assertRaises(KeyError):
            self.prepare()

    def required_platforms(self):
        return {platform for platform, tier in assets.PLATFORM_TIERS.items() if tier <= 2}

    def test_required_release_does_not_depend_on_macos(self):
        for path in self.macos.values():
            path.unlink()
        result = assets.prepare(self.metadata, self.linux, self.output,
                                macos_inputs={}, platforms=self.required_platforms())
        self.assertEqual(len(result), 14)
        self.assertEqual(sum(path.suffix == ".exe" for path in result), 4)
        self.assertTrue(any("linux_x64" in path.name for path in result))
        self.assertFalse(any("macos" in path.name for path in result))

    def test_required_tiers_still_reject_missing_arm64_and_linux(self):
        with self.assertRaisesRegex(ValueError, "Missing Linux archive"):
            assets.prepare(self.metadata, None, self.output,
                           macos_inputs={}, platforms=self.required_platforms())
        for path in self.output.glob("*msvc*arm64*"):
            path.unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*msvc_arm64"):
            assets.prepare(self.metadata, self.linux, self.output,
                           macos_inputs={}, platforms=self.required_platforms())

    def test_each_advisory_architecture_publishes_without_other_platforms(self):
        required = assets.prepare(self.metadata, self.linux, self.output,
                                  macos_inputs={}, platforms=self.required_platforms())
        published = {path.name for path in required}
        for arch in ("arm64", "x64"):
            output = self.root / f"advisory-{arch}"
            result = assets.prepare(self.metadata, None, output, macos_inputs={arch: self.macos[arch]},
                                    platforms={f"rust-macos-{arch}"})
            self.assertEqual(len(result), 2)
            self.assertEqual({path.suffix for path in result}, {".gz", ".txt"})
            for path in result:
                self.assertNotIn(path.name, published)
                shutil.copyfile(path, self.output / path.name)
                published.add(path.name)
        expected = {name for checksum, names in assets.expected_assets(self.info).items() for name in (checksum, *names)}
        self.assertEqual(published, expected)

    def test_bad_advisory_artifact_fails_its_own_validation(self):
        self.write_macos("arm64", self.info | {"commit": "another-commit"})
        with self.assertRaisesRegex(ValueError, "macos-arm64.*version/commit"):
            assets.prepare(self.metadata, None, self.root / "advisory-bad",
                           macos_inputs={"arm64": self.macos["arm64"]}, platforms={"rust-macos-arm64"})
        self.assertFalse((self.root / "advisory-bad").exists())

    def test_cli_selects_required_tiers_and_one_advisory_architecture(self):
        command = [sys.executable, str(ROOT / "scripts/prepare_release_assets.py"), "--metadata", str(self.metadata)]
        required = subprocess.run(command + ["--tier", "1", "--tier", "2", "--linux-input", str(self.linux),
                                             "--output-dir", str(self.output)], capture_output=True, text=True, check=True)
        self.assertEqual(len(required.stdout.splitlines()), 14)
        advisory = subprocess.run(command + ["--tier", "3", "--macos-arch", "arm64",
                                             "--macos-arm64-input", str(self.macos["arm64"]),
                                             "--output-dir", str(self.root / "advisory-cli")],
                                  capture_output=True, text=True, check=True)
        self.assertEqual(len(advisory.stdout.splitlines()), 2)
        self.assertIn("macos_arm64", advisory.stdout)
        self.assertNotIn("macos_x64", advisory.stdout)

    def test_invalid_platform_selection_cannot_produce_an_empty_release(self):
        for selection in (set(), {"unknown-platform"}):
            with self.assertRaisesRegex(ValueError, "Invalid release platform selection"):
                assets.prepare(self.metadata, None, self.output, macos_inputs={}, platforms=selection)


if __name__ == "__main__":
    unittest.main()
