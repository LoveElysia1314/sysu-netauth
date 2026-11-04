"""Exercise the removal script in an isolated filesystem with mocked launchd commands."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "posix", "requires a POSIX shell")
class MacOSUninstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.app = self.root / 'Applications/SYSUNetAuth.app'
        self.app.mkdir(parents=True)
        (self.app / 'payload').write_text('application')
        self.data = self.root / 'Library/Application Support/SYSUNetAuth'
        self.data.mkdir(parents=True)
        (self.data / 'config.json').write_text('{"username":"preserved"}')
        self.log = self.root / 'operations'
        self.stub = self.root / 'system-command'
        self.stub.write_text('''#!/bin/sh
printf '%s\\n' "$*" >> "$HARNESS_LOG"
case "$1" in
    ps) printf '501\\n502\\n' ;;
    plist) printf '%s\\n' "$HARNESS_BUNDLE_ID" ;;
esac
''')
        self.stub.chmod(0o755)
        source = (ROOT / 'scripts/uninstall-macos.sh').read_text()
        for command, argument in [('/bin/ps', 'ps'), ('/bin/launchctl', 'launchctl'),
                                  ('/usr/libexec/PlistBuddy', 'plist'), ('/usr/sbin/pkgutil', 'pkgutil'),
                                  ('/usr/bin/pgrep', 'pgrep')]:
            source = source.replace(command, f'"{self.stub}" {argument}')
        for location in ['/Applications', '/Library', '/usr/local/bin', '/var/run']:
            source = source.replace(location, str(self.root) + location)
        self.script = self.root / 'uninstall-macos.sh'
        self.script.write_text(source)
        (self.root / 'launchd-common.sh').write_text(f'''
plist_path='{self.root}/Library/LaunchDaemons/service.plist'
service_path='{self.root}/Library/PrivilegedHelperTools/service'
data_path='{self.data}'
require_macos_root() {{ :; }}
stop_daemon() {{ echo stop-daemon >> "$HARNESS_LOG"; }}
''')

    def run_script(self, *args, identifier='io.github.peterrobot.sysu-netauth.gui'):
        return subprocess.run(['sh', str(self.script), *args], env={**os.environ,
                              'HARNESS_LOG': str(self.log), 'HARNESS_BUNDLE_ID': identifier},
                              text=True, capture_output=True)

    def test_default_preserves_application_and_credentials_and_unloads_users(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.app.exists())
        self.assertTrue((self.data / 'config.json').exists())
        operations = self.log.read_text()
        self.assertIn('gui/501/io.github.peterrobot.sysu-netauth.notifications', operations)
        self.assertIn('gui/502/io.github.peterrobot.sysu-netauth.notifications', operations)
        self.assertIn('stop-daemon', operations)

    def test_complete_removal_and_optional_purge(self):
        result = self.run_script('--remove-app', '--purge')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.app.exists())
        self.assertFalse(self.data.exists())
        self.assertIn('pkgutil --forget io.github.peterrobot.sysu-netauth', self.log.read_text())

    def test_complete_removal_preserves_credentials_by_default(self):
        result = self.run_script('--remove-app')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.app.exists())
        self.assertTrue((self.data / 'config.json').exists())

    def test_wrong_application_identifier_aborts_before_removing_components(self):
        result = self.run_script('--remove-app', identifier='another.app')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.app.exists())
        self.assertTrue(self.data.exists())
        self.assertNotIn('launchctl', self.log.read_text())

    def test_symlinked_application_is_rejected(self):
        (self.app / 'payload').unlink()
        self.app.rmdir()
        self.app.symlink_to(self.data, target_is_directory=True)
        result = self.run_script('--remove-app', '--purge')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.data / 'config.json').exists())
        self.assertFalse(self.log.exists())

    def test_unknown_arguments_have_no_side_effects(self):
        result = self.run_script('--remove-app', '--arbitrary-path')
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.app.exists())
        self.assertFalse(self.log.exists())
