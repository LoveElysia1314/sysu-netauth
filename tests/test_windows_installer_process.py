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
    def run_setup(self, installer, log, *, timeout=90):
        # Allow for CI startup/scanning overhead. The fixture still enforces
        # its own one-second command timeout and five-second upper bound.
        timed_out = False
        cleanup_output = ""
        with subprocess.Popen(
            [str(installer), "/SP-", "/VERYSILENT", "/SUPPRESSMSGBOXES",
             "/NORESTART", f"/LOG={log}"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        ) as process:
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
            finally:
                if process.poll() is None:
                    # Inno Setup launches a separate worker. Kill the tree
                    # before the launcher exits so the worker releases its log.
                    try:
                        cleanup = subprocess.run(
                            ["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                            capture_output=True, timeout=15,
                        )
                        cleanup_output = (cleanup.stdout + cleanup.stderr).decode(errors="replace")
                    except (OSError, subprocess.SubprocessError) as error:
                        cleanup_output = f"Unable to terminate setup process tree: {error}"
                    finally:
                        # Reap the launcher even if taskkill fails or races
                        # with its exit; Popen.__exit__ must not wait forever.
                        if process.poll() is None:
                            process.kill()
                        process.wait(timeout=10)

        contents = log.read_text(encoding="utf-8-sig") if log.exists() else "No setup log was created."
        if timed_out:
            self.fail(f"Installer process check timed out after {timeout} seconds.\n"
                      f"{contents}\n{cleanup_output}")
        # InitializeSetup deliberately returns False after the checks; this
        # fixture has no installation to perform. Require its success marker.
        self.assertEqual(process.returncode, 1, contents)
        self.assertIn("Installer process regression checks passed", contents)
        return contents

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
            contents = self.run_setup(directory / "installer-process-check.exe", log)
            self.assertIn(f'"{child}" hang', contents)
            self.assertIn("Setup command timed out", contents)
