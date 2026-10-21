"""Compile and execute the actual process-wait code without installing the app."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires Windows and Inno Setup 6")
class InstallerProcessTests(unittest.TestCase):
    def test_hung_and_interactive_children_cannot_stall_setup(self):
        with tempfile.TemporaryDirectory(prefix="netauth process checks ") as location:
            directory = Path(location).resolve()
            child = directory / "installer child.py"
            shutil.copyfile(ROOT / "tests/fixtures/installer_child.py", child)
            subprocess.run(
                [str(find_compiler()), "/Q", f"/O{directory}",
                 f"/DTestCommand={sys.executable}", f"/DTestScript={child}",
                 str(ROOT / "tests/fixtures/installer_process.iss")],
                check=True, capture_output=True, timeout=60,
            )
            log = directory / "setup.log"
            result = subprocess.run(
                [str(directory / "installer-process-check.exe"), "/SP-", "/VERYSILENT",
                 "/SUPPRESSMSGBOXES", "/NORESTART", f"/LOG={log}"],
                capture_output=True, timeout=30,
            )
            contents = log.read_text(encoding="utf-8-sig")
            self.assertEqual(result.returncode, 0, contents)
            self.assertIn(f'"{child}" hang', contents)
            self.assertIn("Installer process regression checks passed", contents)
            self.assertIn("Setup command timed out", contents)
