"""
Unified entry: tray (no args) or CLI (with args).

Single-instance protection uses a Win32 mutex.
"""

from __future__ import annotations

import sys

from PySide6.QtCore import Qt
from PySide6.QtGui import QIcon, QPixmap
from PySide6.QtWidgets import QApplication

from sysu_netauth.core.config import APP_ID, load_config

GUI_FLAGS = {"--startup"}
SERVICE_FLAGS = {"--service"}


def _app_icon() -> QIcon:
    """Load the app icon; returns a QIcon."""
    from sysu_netauth.core.assets import resolve_asset_path

    path = resolve_asset_path("icon-ethernet")
    if path.is_file():
        icon = QIcon(str(path))
        if not icon.isNull():
            return icon
    fb = QPixmap(16, 16)
    fb.fill(Qt.GlobalColor.darkGray)
    return QIcon(fb)


def _attach_parent_console() -> None:
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.windll.kernel32
    kernel32.AttachConsole.argtypes = [wintypes.DWORD]
    kernel32.AttachConsole.restype = wintypes.BOOL
    if kernel32.AttachConsole(-1):
        sys.stdout = open("CONOUT$", "w", encoding="utf-8", errors="replace")
        sys.stderr = open("CONOUT$", "w", encoding="utf-8", errors="replace")


def main() -> None:
    if any(arg in SERVICE_FLAGS for arg in sys.argv[1:]):
        from sysu_netauth.service.win_service import handle_command_line

        service_args = [
            sys.argv[0],
            *(a for a in sys.argv[1:] if a not in SERVICE_FLAGS),
        ]
        handle_command_line(service_args)
        return

    gui_flags = [a for a in sys.argv[1:] if a in GUI_FLAGS]
    cli_args = [a for a in sys.argv[1:] if a not in GUI_FLAGS]

    if cli_args:
        _attach_parent_console()
        from sysu_netauth.cli import main as cli_main

        sys.argv = [sys.argv[0], *cli_args]
        cli_main()
        return

    # ── --startup 参数处理 ──
    started_by_startup = "--startup" in gui_flags
    if started_by_startup:
        cfg = load_config()
        if cfg.service_mode:
            import subprocess

            subprocess.run(
                ["sc", "config", APP_ID, "start=", "auto"],
                capture_output=True,
                creationflags=0x08000000,
            )
            subprocess.run(
                ["sc", "start", APP_ID], capture_output=True, creationflags=0x08000000
            )
        if not cfg.launch_gui_on_login:
            return

    # ── 单实例保护 (Win32 Mutex) ──
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.windll.kernel32
    kernel32.CreateMutexW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.LPCWSTR]
    kernel32.CreateMutexW.restype = wintypes.HANDLE
    _mutex = kernel32.CreateMutexW(None, False, APP_ID)
    if kernel32.GetLastError() == 183:  # ERROR_ALREADY_EXISTS
        sys.exit(0)

    # ── 创建 QApplication ──
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setWindowIcon(_app_icon())

    # ── 启动 GUI ──
    from sysu_netauth.app.tray import main as tray_main

    tray_main(app=app, started_by_startup=started_by_startup)


if __name__ == "__main__":
    main()
