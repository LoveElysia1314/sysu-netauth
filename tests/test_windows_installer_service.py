"""Exercise the shared installer code against a real, isolated Windows service."""
import ctypes
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

from scripts.package_windows import find_compiler

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires Windows, MSVC and Inno Setup 6")
class InstallerServiceTests(unittest.TestCase):
    def command(self, *args, **kwargs):
        return subprocess.run(args, check=True, capture_output=True, timeout=60, **kwargs)

    def sc(self, *args, check=True):
        result = subprocess.run(["sc.exe", *args], capture_output=True, timeout=15)
        if check:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def state(self, name):
        # Read numeric state via SCM, independent of Windows display language.
        class Status(ctypes.Structure):
            _fields_ = [(field, ctypes.c_ulong) for field in (
                "kind", "state", "controls", "code", "specific", "checkpoint", "hint")]

        api = ctypes.WinDLL("advapi32", use_last_error=True)
        api.OpenSCManagerW.restype = ctypes.c_void_p
        api.OpenServiceW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_ulong]
        api.OpenServiceW.restype = ctypes.c_void_p
        api.QueryServiceStatus.argtypes = [ctypes.c_void_p, ctypes.POINTER(Status)]
        api.CloseServiceHandle.argtypes = [ctypes.c_void_p]
        manager = api.OpenSCManagerW(None, None, 1)
        self.assertTrue(manager)
        try:
            service = api.OpenServiceW(manager, name, 4)
            if not service:
                self.assertEqual(ctypes.get_last_error(), 1060)
                return None
            try:
                status = Status()
                self.assertTrue(api.QueryServiceStatus(service, ctypes.byref(status)))
                return status.state
            finally:
                api.CloseServiceHandle(service)
        finally:
            api.CloseServiceHandle(manager)

    def wait_state(self, name, expected):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.state(name) == expected:
                return
            time.sleep(0.1)
        self.fail(f"Service {name} did not reach state {expected}")

    def test_upgrade_uninstall_and_failed_upgrade_stop_real_services(self):
        self.assertTrue(ctypes.windll.shell32.IsUserAnAdmin(), "run in elevated Windows CI")
        import winreg

        with tempfile.TemporaryDirectory(prefix="netauth service checks ") as location:
            directory = Path(location).resolve()
            binary = directory / "fixture.exe"
            self.command("cl.exe", "/nologo", "/W4", "/WX", "/MT",
                         str(ROOT / "tests/fixtures/installer_service.c"),
                         f"/Fe{binary}", f"/Fo{directory / 'fixture.obj'}", "/link", "advapi32.lib")
            for hung in (False, True):
                with self.subTest(hung=hung):
                    name = f"NetAuthInstallerTest{os.getpid()}{int(hung)}"
                    service_exe, gui_exe, cli_exe = (f"{name}{suffix}.exe" for suffix in ("Service", "Gui", "Cli"))
                    app = directory / name
                    children = []
                    try:
                        installers = {}
                        for version in ("1.0", "2.0", "3.0"):
                            payload = directory / f"payload-{version}.txt"
                            payload.write_text(version, encoding="utf-8")
                            defines = [f"/DManagedServiceName={name}", f"/DManagedServiceExe={service_exe}",
                                       f"/DManagedGuiName={gui_exe}", f"/DManagedCliName={cli_exe}",
                                       "/DServiceStopSeconds=1", f"/DTestBinary={binary}",
                                       f"/DTestPayload={payload}", f"/DTestVersion={version}"]
                            if version == "3.0":
                                defines.append("/DTestCancelAfterStop")
                            self.command(str(find_compiler()), "/Q", f"/O{directory}", *defines,
                                         str(ROOT / "tests/fixtures/installer_service.iss"))
                            installers[version] = directory / f"installer-service-check-{version}.exe"

                        def install(version, check=True):
                            log = directory / f"{name}-{version}.log"
                            result = subprocess.run([str(installers[version]), "/SP-", "/VERYSILENT",
                                                     "/SUPPRESSMSGBOXES", "/NORESTART", f"/DIR={app}",
                                                     f"/LOG={log}"], capture_output=True, timeout=45)
                            if check:
                                self.assertEqual(result.returncode, 0, log.read_text(encoding="utf-8-sig"))
                            return result, log.read_text(encoding="utf-8-sig")

                        install("1.0")  # No service installed: shutdown is idempotent.
                        settings = app / "config.json"
                        settings.write_text('{"username":"keep-me"}', encoding="utf-8")
                        self.sc("create", name, "binPath=",
                                f'"{app / service_exe}" --service {name}' + (" --hang" if hung else ""),
                                "start=", "demand")
                        self.sc("failure", name, "reset=", "0", "actions=", "restart/60000")

                        def start():
                            self.sc("start", name)
                            self.wait_state(name, 4)
                            for image in (gui_exe, cli_exe):
                                children.append(subprocess.Popen([str(app / image)]))

                        def assert_children_stopped():
                            for child in children:
                                child.wait(timeout=5)

                        start()
                        _, log = install("2.0")
                        self.wait_state(name, 1)
                        assert_children_stopped()
                        self.assertEqual((app / "payload.txt").read_text(), "2.0")
                        self.assertIn("keep-me", settings.read_text())
                        if hung:
                            self.assertIn("stop timed out; terminating", log)
                        # A failed/cancelled upgrade must not leave the service disabled.
                        key_path = f"SYSTEM\\CurrentControlSet\\Services\\{name}"
                        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key_path) as key:
                            self.assertEqual(winreg.QueryValueEx(key, "Start")[0], 3)
                        start()
                        failed, _ = install("3.0", check=False)
                        self.assertNotEqual(failed.returncode, 0)
                        self.assertEqual((app / "payload.txt").read_text(), "2.0")
                        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key_path) as key:
                            self.assertEqual(winreg.QueryValueEx(key, "Start")[0], 3)
                        assert_children_stopped()

                        self.sc("failure", name, "reset=", "0", "actions=", "restart/1000")
                        start()
                        log = directory / f"{name}-uninstall.log"
                        self.command(str(app / "unins000.exe"), "/VERYSILENT", "/SUPPRESSMSGBOXES",
                                     "/NORESTART", f"/LOG={log}")
                        self.wait_state(name, None)
                        assert_children_stopped()
                        for image in (service_exe, gui_exe, cli_exe, "payload.txt"):
                            self.assertFalse((app / image).exists(), log.read_text(encoding="utf-8-sig"))
                        self.assertIn("keep-me", settings.read_text())
                        # Delayed SCM recovery must not resurrect a killed service.
                        time.sleep(1.5)
                        self.assertIsNone(self.state(name))
                    finally:
                        self.sc("stop", name, check=False)
                        for image in (service_exe, gui_exe, cli_exe):
                            subprocess.run(["taskkill.exe", "/f", "/im", image], capture_output=True, timeout=10)
                        self.sc("delete", name, check=False)
                        for child in children:
                            child.wait(timeout=5)
                        # On assertion failure, still remove this fixture's private install.
                        if (app / "unins000.exe").exists():
                            subprocess.run([str(app / "unins000.exe"), "/VERYSILENT", "/SUPPRESSMSGBOXES",
                                            "/NORESTART"], capture_output=True, timeout=45)
