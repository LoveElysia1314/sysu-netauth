"""Exercise the installer's actual ACL repair on an isolated data directory."""
import ctypes
import struct
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires elevated Windows and Inno Setup 6")
class InstallerPermissionsTests(unittest.TestCase):
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
        api = ctypes.WinDLL("advapi32", use_last_error=True)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        pointer = ctypes.c_void_p
        api.GetNamedSecurityInfoW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                            pointer, pointer, ctypes.POINTER(pointer), pointer,
                                            ctypes.POINTER(pointer)]
        api.GetNamedSecurityInfoW.restype = ctypes.c_uint32
        api.GetAce.argtypes = [pointer, ctypes.c_uint32, ctypes.POINTER(pointer)]
        api.GetAce.restype = ctypes.c_int
        api.ConvertSidToStringSidW.argtypes = [pointer, ctypes.POINTER(pointer)]
        api.ConvertSidToStringSidW.restype = ctypes.c_int
        kernel.LocalFree.argtypes = [pointer]
        kernel.LocalFree.restype = pointer
        dacl, descriptor = pointer(), pointer()
        error = api.GetNamedSecurityInfoW(str(path), 1, 4, None, None,
                                         ctypes.byref(dacl), None, ctypes.byref(descriptor))
        self.assertEqual(error, 0, f"GetNamedSecurityInfoW failed: {error}")
        try:
            self.assertTrue(dacl.value, "an unrestricted NULL DACL is not private")
            count = struct.unpack("<BBHHH", ctypes.string_at(dacl, 8))[3]
            result = []
            for index in range(count):
                ace = pointer()
                self.assertTrue(api.GetAce(dacl, index, ctypes.byref(ace)), ctypes.get_last_error())
                kind, flags, _, rights = struct.unpack("<BBHI", ctypes.string_at(ace, 8))
                self.assertIn(kind, (0, 1), "unexpected ACL entry type")
                sid = pointer()
                self.assertTrue(api.ConvertSidToStringSidW(pointer(ace.value + 8), ctypes.byref(sid)),
                                ctypes.get_last_error())
                try:
                    result.append({"Sid": ctypes.wstring_at(sid), "Rights": rights,
                                   "Type": "Allow" if kind == 0 else "Deny",
                                   "Inherited": bool(flags & 0x10),
                                   "Propagation": "Special" if flags & 0x0c else "None"})
                finally:
                    kernel.LocalFree(sid)
            return result
        finally:
            kernel.LocalFree(descriptor)

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

    def test_existing_explicit_grants_are_replaced(self):
        self.assertTrue(ctypes.windll.shell32.IsUserAnAdmin(), "run in elevated Windows CI")
        with tempfile.TemporaryDirectory(prefix="netauth explicit ACL ") as location:
            directory = Path(location).resolve()
            data = directory / "existing data"
            data.mkdir()
            subprocess.run(["icacls.exe", str(data), "/grant", "*S-1-3-4:(OI)(CI)F",
                            "*S-1-1-0:(OI)(CI)R"], check=True, capture_output=True, timeout=30)
            self.run_fixture(directory, data)
            self.assert_private_writable(data, inherited=False)
            log = data / "service.log"
            log.write_text("fixture\n", encoding="utf-8")
            self.assert_private_writable(log, inherited=True)


if __name__ == "__main__":
    unittest.main()
