"""Exercise the installer's DLL architecture checks without installing Npcap."""
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest

from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires Windows and Inno Setup 6")
class InstallerNpcapTests(unittest.TestCase):
    def test_npcap_libraries_must_match_the_application_architecture(self):
        with tempfile.TemporaryDirectory(prefix="netauth npcap checks ") as location:
            directory = Path(location).resolve()
            for name, machine in (("x86", 0x14c), ("x64", 0x8664), ("arm64", 0xaa64), ("arm64x", 0xa64e)):
                binary = bytearray(70)
                binary[:2] = b"MZ"
                struct.pack_into("<I", binary, 0x3c, 64)
                binary[64:68] = b"PE\0\0"
                struct.pack_into("<H", binary, 68, machine)
                (directory / f"{name}.dll").write_bytes(binary)
            (directory / "truncated.dll").write_bytes(b"MZ")
            struct.pack_into("<I", binary, 0x3c, 0xffffffff)
            (directory / "bad-offset.dll").write_bytes(binary)
            struct.pack_into("<I", binary, 0x3c, 64)
            binary[64:68] = b"nope"
            (directory / "bad-signature.dll").write_bytes(binary)
            for arch in ("x64", "arm64"):
                with self.subTest(arch=arch):
                    subprocess.run(
                        [str(find_compiler()), "/Q", f"/O{directory}", f"/DMyAppArch={arch}",
                         f"/DTestFixtureDir={directory}", str(ROOT / "tests/fixtures/installer_npcap.iss")],
                        check=True, capture_output=True, timeout=60,
                    )
                    log = directory / f"{arch}.log"
                    result = subprocess.run(
                        [str(directory / f"installer-npcap-check-{arch}.exe"), "/SP-", "/VERYSILENT",
                         "/SUPPRESSMSGBOXES", "/NORESTART", f"/LOG={log}"],
                        capture_output=True, timeout=30,
                    )
                    contents = log.read_text(encoding="utf-8-sig")
                    self.assertEqual(result.returncode, 0, contents)
                    self.assertIn("Npcap architecture regression checks passed", contents)
