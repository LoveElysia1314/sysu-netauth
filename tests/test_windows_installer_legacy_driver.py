"""Run the real Inno version policy without installing or uninstalling a driver."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent

@unittest.skipUnless(os.name == 'nt', 'requires Windows and Inno Setup')
class LegacyDriverTests(unittest.TestCase):
    def test_cleanup_is_limited_to_known_dependency_versions_with_a_present_driver(self):
        with tempfile.TemporaryDirectory(prefix='netauth legacy policy ') as location:
            directory=Path(location).resolve()
            subprocess.run([str(find_compiler()),'/Q',f'/O{directory}',str(ROOT/'tests/fixtures/installer_legacy_driver.iss')],check=True,capture_output=True,timeout=60)
            log=directory/'policy.log'
            result=subprocess.run([str(directory/'installer-legacy-driver-check.exe'),'/SP-','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',f'/LOG={log}'],capture_output=True,timeout=60)
            contents=log.read_text(encoding='utf-8-sig')
            self.assertEqual(result.returncode,1,contents)
            self.assertIn('Legacy driver cleanup regression checks passed',contents)
