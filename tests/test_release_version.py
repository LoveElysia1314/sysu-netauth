"""Version derivation, Git identity and stamping must agree before building."""

import importlib.util
import json
import os
import runpy
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("release_version", ROOT / "scripts/release_version.py")
versioning = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(versioning)


class ReleaseVersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="netauth version ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in ("apps/rust/Cargo.toml", "Cargo.lock", "apps/python/pyproject.toml", "apps/python/sysu_netauth/core/config.py", "apps/python/updates/release.json", "updates/release.json"):
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        self.git("init", "-q")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "fixture")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], text=True).strip()

    def test_stable_and_prerelease_conventions(self):
        for tag, python in (("v0.7.0", "0.7.0"), ("v0.7.0-alpha.0", "0.7.0a0"), ("v0.7.0-beta.2", "0.7.0b2"), ("v0.7.0-rc.1", "0.7.0rc1")):
            with self.subTest(tag=tag):
                info = versioning.tag_version(tag)
                self.assertEqual(info["python_version"], python)
                self.assertEqual(info["file_version"], "0.7.0")
                self.assertEqual(info["version"], tag[1:])
        for tag in ("v0.7.0a", "v01.7.0", "v0.7.0-alpha", "v0.7.0-rc.01", "v65536.0.0", "main", "v0.7.0;echo nope"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                versioning.tag_version(tag)

    def test_resolves_head_tag_and_rejects_another_commit(self):
        self.git("tag", "v0.7.0-alpha.0")
        with patch.dict(os.environ, {"NETAUTH_RELEASE_TAG": "", "GITHUB_REF_TYPE": "", "GITHUB_REF_NAME": ""}):
            info = versioning.resolve(self.root)
        self.assertEqual(info["commit"], self.git("rev-parse", "HEAD"))
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "next")
        with self.assertRaisesRegex(ValueError, "does not point"):
            versioning.resolve(self.root, "v0.7.0-alpha.0")

    def test_no_tag_is_not_silently_a_release(self):
        with patch.dict(os.environ, {"NETAUTH_RELEASE_TAG": "", "GITHUB_REF_TYPE": "", "GITHUB_REF_NAME": ""}):
            with self.assertRaisesRegex(ValueError, "exactly one"):
                versioning.resolve(self.root)

    def test_standard_scm_generation_does_not_rewrite_tracked_versions(self):
        self.git("tag", "v0.7.0-alpha.0")
        info = versioning.resolve(self.root, "v0.7.0-alpha.0")
        public_feed = (self.root / "updates/release.json").read_bytes()
        original_lock = tomllib.loads((self.root / "Cargo.lock").read_text(encoding="utf-8"))["package"]
        versioning.stamp(self.root, info)
        versioning.stamp(self.root, info)  # Re-running a CI job is idempotent.
        lock = tomllib.loads((self.root / "Cargo.lock").read_text(encoding="utf-8"))["package"]
        self.assertEqual([p for p in lock if p["name"] != "sysu-netauth"], [p for p in original_lock if p["name"] != "sysu-netauth"])
        self.assertEqual(lock, original_lock)
        self.assertEqual(runpy.run_path(str(self.root / "apps/python/sysu_netauth/_version.py"))["__version__"], "0.7.0a0")
        self.assertEqual((self.root / "updates/release.json").read_bytes(), public_feed)
        self.assertEqual(json.loads((self.root / "build/release-version.json").read_text()), info)


if __name__ == "__main__":
    unittest.main()
