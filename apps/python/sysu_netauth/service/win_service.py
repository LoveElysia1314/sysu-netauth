from __future__ import annotations

import logging
import logging.handlers
import sys
import threading

import servicemanager
import win32event
import win32service
import win32serviceutil

from sysu_netauth.core.config import APP_DIR, APP_DISPLAY_NAME, APP_ID
from sysu_netauth.service.engine import AuthServiceEngine

SERVICE_DESCRIPTION = "SYSU wired campus network 802.1X authentication service"


def _bootstrap_log(message: str) -> None:
    try:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        path = APP_DIR / "service_bootstrap.log"
        with open(path, "a", encoding="utf-8") as f:
            f.write(message + "\n")
    except Exception:
        pass


def setup_logging() -> None:
    APP_DIR.mkdir(parents=True, exist_ok=True)
    log_path = APP_DIR / "service.log"
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    if root.handlers:
        return
    try:
        handler = logging.handlers.RotatingFileHandler(
            log_path,
            maxBytes=512 * 1024,
            backupCount=3,
            encoding="utf-8",
        )
    except OSError:
        handler = logging.NullHandler()
    handler.setFormatter(
        logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    )
    root.addHandler(handler)


class SYSUNetAuthService(win32serviceutil.ServiceFramework):
    _svc_name_ = APP_ID
    _svc_display_name_ = APP_DISPLAY_NAME
    _svc_description_ = SERVICE_DESCRIPTION

    def __init__(self, args: list[str]) -> None:
        super().__init__(args)
        _bootstrap_log(f"service __init__ args={args!r}")
        self.stop_handle = win32event.CreateEvent(None, 0, 0, None)
        self.stop_event = threading.Event()
        self.engine: AuthServiceEngine | None = None

    def SvcStop(self) -> None:
        self.ReportServiceStatus(win32service.SERVICE_STOP_PENDING)
        self.stop_event.set()
        win32event.SetEvent(self.stop_handle)

    def SvcDoRun(self) -> None:
        _bootstrap_log("SvcDoRun entered")
        setup_logging()
        self.ReportServiceStatus(win32service.SERVICE_RUNNING)
        _bootstrap_log("SERVICE_RUNNING reported")
        servicemanager.LogMsg(
            servicemanager.EVENTLOG_INFORMATION_TYPE,
            servicemanager.PYS_SERVICE_STARTED,
            (self._svc_name_, ""),
        )
        try:
            self.engine = AuthServiceEngine(self.stop_event)
            _bootstrap_log("engine created")
            self.engine.run()
        except Exception:
            _bootstrap_log("service crashed")
            logging.getLogger(__name__).exception("service crashed")
            raise


def _configure_failure_actions() -> None:
    try:
        import subprocess

        subprocess.run(
            [
                "sc.exe",
                "failure",
                APP_ID,
                "reset=",
                "86400",
                "actions=",
                "restart/60000/none/0/none/0",
            ],
            check=False,
            capture_output=True,
            creationflags=0x08000000,
        )
    except Exception:
        logging.getLogger(__name__).warning("failed to configure failure actions")


def handle_command_line(argv: list[str] | None = None) -> None:
    argv = argv or sys.argv
    _bootstrap_log(
        f"handle_command_line frozen={getattr(sys, 'frozen', False)} argv={argv!r}"
    )
    if getattr(sys, "frozen", False) and len(argv) == 1:
        try:
            _bootstrap_log("entering frozen service dispatcher")
            servicemanager.Initialize()
            servicemanager.PrepareToHostSingle(SYSUNetAuthService)
            servicemanager.StartServiceCtrlDispatcher()
            _bootstrap_log("dispatcher returned")
        except Exception as exc:
            _bootstrap_log(f"dispatcher failed: {exc!r}")
            raise
        return
    win32serviceutil.HandleCommandLine(SYSUNetAuthService, argv=argv)
    if argv and any(arg.lower() in {"install", "update"} for arg in argv[1:]):
        _configure_failure_actions()


def debug_run() -> None:
    setup_logging()
    stop_event = threading.Event()
    engine = AuthServiceEngine(stop_event)
    try:
        engine.run()
    except KeyboardInterrupt:
        stop_event.set()


if __name__ == "__main__":
    handle_command_line(sys.argv)
