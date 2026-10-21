"""Every platform package must be present and intact before uploading a release."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
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
        for label, version in (("Rust_msvc_x64", "0.7.0-alpha.0"), ("Rust_gnu_x64", "0.7.0-alpha.0"), ("Python_x64", "0.7.0a0")):
            names = [f"SYSUNetAuth_{label}_Setup_v{version}.exe", f"SYSUNetAuth_{label}_Setup_v{version}.zip", f"SYSUNetAuth_{label}_Portable_v{version}.zip"]
            checksums = []
            for name in names:
                payload = name.encode()
                (self.output / name).write_bytes(payload)
                checksums.append(f"{hashlib.sha256(payload).hexdigest()}  {name}\n")
            (self.output / f"SHA256SUMS-{label.lower().replace('_', '-')}.txt").write_text("".join(checksums), encoding="utf-8")
        self.linux = self.root / "linux.tar.gz"
        self.write_linux(self.info)

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

    def test_collects_all_four_variants_and_checksums(self):
        result = assets.prepare(self.metadata, self.linux, self.output)
        self.assertEqual(len(result), 14)
        self.assertEqual(sum(path.name.endswith(".exe") for path in result), 3)
        self.assertEqual(sum("_Portable_" in path.name for path in result), 3)
        linux = self.output / "SYSUNetAuth_Rust_linux_x64_v0.7.0-alpha.0.tar.gz"
        self.assertEqual(linux.read_bytes(), self.linux.read_bytes())
        self.assertEqual((self.output / "SHA256SUMS-rust-linux-x64.txt").read_text(), f"{hashlib.sha256(linux.read_bytes()).hexdigest()}  {linux.name}\n")

    def test_missing_gnu_variant_blocks_release(self):
        for path in self.output.glob("*gnu*"):
            path.unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*gnu"):
            assets.prepare(self.metadata, self.linux, self.output)

    def test_corrupt_windows_package_blocks_release(self):
        next(self.output.glob("*msvc*Portable*.zip")).write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            assets.prepare(self.metadata, self.linux, self.output)

    def test_linux_from_another_commit_blocks_release(self):
        self.write_linux(self.info | {"commit": "another-commit"})
        with self.assertRaisesRegex(ValueError, "version/commit"):
            assets.prepare(self.metadata, self.linux, self.output)

    def test_linux_without_service_blocks_release(self):
        self.write_linux(self.info, omit="sysu_netauth_service")
        with self.assertRaises(KeyError):
            assets.prepare(self.metadata, self.linux, self.output)


if __name__ == "__main__":
    unittest.main()
