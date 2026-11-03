"""Exercise the installer's actual ACL repair on an isolated data directory."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires elevated Windows and Inno Setup 6")
class InstallerPermissionsTests(unittest.TestCase):
    def powershell(self, script, *args):
        system = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0"
        environment = os.environ.copy()
        # PowerShell 7 / VS may export a module path incompatible with Windows
        # PowerShell 5.1. The ACL check needs only its built-in security module.
        environment["PSModulePath"] = str(system / "Modules")
        result = subprocess.run(
            [str(system / "powershell.exe"), "-NoProfile", "-NonInteractive", "-Command",
             "$ErrorActionPreference='Stop'; " + script, *args],
            capture_output=True, timeout=30, env=environment,
        )
        self.assertEqual(result.returncode, 0, (result.stdout + result.stderr).decode(errors="replace"))
        return result.stdout.decode("utf-8-sig").strip()

    def run_fixture(self, directory, data):
        subprocess.run(
            [str(find_compiler()), "/Q", f"/O{directory}", f"/DTestDirectory={data}",
             str(ROOT / "tests/fixtures/installer_permissions.iss")],
            check=True, capture_output=True, timeout=60,
        )
        log = directory / "setup.log"
        result = subprocess.run(
            [str(directory / "installer-permissions-check.exe"), "/SP-", "/VERYSILENT",
             "/SUPPRESSMSGBOXES", "/NORESTART", f"/LOG={log}"],
            capture_output=True, timeout=60,
        )
        contents = log.read_text(encoding="utf-8-sig")
        self.assertEqual(result.returncode, 1, contents)  # InitializeSetup returns False.
        self.assertIn("Installer data permissions regression checks passed", contents)

    def acl(self, path):
        # Pass the path through the environment, never PowerShell source text.
        old = os.environ.get("NETAUTH_TEST_ACL_PATH")
        os.environ["NETAUTH_TEST_ACL_PATH"] = str(path)
        try:
            return json.loads(self.powershell(
                "[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); "
                "$acl=Get-Acl -LiteralPath $env:NETAUTH_TEST_ACL_PATH; "
                "@($acl.Access | ForEach-Object { "
                "@{Sid=$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value; "
                "Rights=[int]$_.FileSystemRights; Type=$_.AccessControlType.ToString(); "
                "Inherited=$_.IsInherited; Propagation=$_.PropagationFlags.ToString()} "
                "}) | ConvertTo-Json -Compress"
            ))
        finally:
            if old is None:
                os.environ.pop("NETAUTH_TEST_ACL_PATH", None)
            else:
                os.environ["NETAUTH_TEST_ACL_PATH"] = old

    def assert_private_writable(self, path, *, inherited):
        acl = self.acl(path)
        self.assertEqual({entry["Sid"] for entry in acl}, {"S-1-5-18", "S-1-5-32-544"})
        for entry in acl:
            self.assertEqual(entry["Type"], "Allow")
            self.assertEqual(entry["Rights"] & 0x1F01FF, 0x1F01FF)  # FullControl
            self.assertEqual(entry["Inherited"], inherited)
            self.assertEqual(entry["Propagation"], "None")

    def test_upgrade_repairs_existing_files_and_preserves_new_file_inheritance(self):
        self.assertTrue(ctypes.windll.shell32.IsUserAnAdmin(), "run in elevated Windows CI")
        with tempfile.TemporaryDirectory(prefix="netauth ACL checks ") as location:
            directory = Path(location).resolve()
            data = directory / "private data"
            (data / "nested").mkdir(parents=True)
            files = [data / "service.log", data / "config.json", data / "nested/cache.json"]
            for file in files:
                file.write_text("fixture\n", encoding="utf-8")
            try:
                # Reproduce the released installer's empty file DACL before
                # running the exact helper used by the corrected installer.
                subprocess.run(
                    ["icacls.exe", str(data), "/inheritance:r", "/remove:g", "*S-1-5-32-545",
                     "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "/T"],
                    check=True, capture_output=True, timeout=30,
                )
                with self.assertRaises(PermissionError):
                    files[0].read_bytes()
                self.run_fixture(directory, data)
                self.assert_private_writable(data, inherited=False)
                for file in files:
                    self.assertEqual(file.read_text(encoding="utf-8"), "fixture\n")
                    with file.open("a", encoding="utf-8") as stream:
                        stream.write("append works\n")
                    self.assert_private_writable(file, inherited=True)
                files[0].rename(data / "service.log.1")
                files[0].write_text("rotated\n", encoding="utf-8")
                self.assert_private_writable(files[0], inherited=True)
                new_file = data / "nested/new.json"
                new_file.write_text("new\n", encoding="utf-8")
                self.assert_private_writable(new_file, inherited=True)
                self.run_fixture(directory, data)  # Repeated upgrades remain safe.
                self.assertEqual(files[0].read_text(encoding="utf-8"), "rotated\n")
            finally:
                # This directory contains fixture text only. Recover access
                # even when a regression leaves one of its file ACLs empty.
                subprocess.run(["icacls.exe", str(data), "/reset", "/T", "/Q"],
                               capture_output=True, timeout=30)

    def test_empty_fresh_directory_supports_its_first_log(self):
        self.assertTrue(ctypes.windll.shell32.IsUserAnAdmin(), "run in elevated Windows CI")
        with tempfile.TemporaryDirectory(prefix="netauth fresh ACL ") as location:
            directory = Path(location).resolve()
            data = directory / "fresh data"
            self.run_fixture(directory, data)
            log = data / "service.log"
            log.write_text("first log\n", encoding="utf-8")
            self.assert_private_writable(log, inherited=True)


if __name__ == "__main__":
    unittest.main()
