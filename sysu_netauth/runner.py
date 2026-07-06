"""
Unified entry: tray (no args) or CLI (with args).

Single-instance protection via QLocalServer / QLocalSocket (named pipe IPC).
"""

from __future__ import annotations

import sys

from PySide6.QtWidgets import QApplication

from sysu_netauth.core.config import APP_ID
from sysu_netauth.core.single_instance import SingleInstanceManager

GUI_FLAGS = {"--startup"}
SERVICE_FLAGS = {"--service"}


def _app_icon() -> object:
    """Load the app icon; returns a QIcon."""
    from pathlib import Path

    from PySide6.QtGui import QIcon, QPixmap
    from PySide6.QtCore import Qt

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
        import os

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
        from sysu_netauth.core.shared_store import ensure_shared_config

        cfg = ensure_shared_config()
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

    # ── 创建 QApplication（QLocalServer 需要） ──
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setWindowIcon(_app_icon())

    # ── 单实例保护 ──
    single = SingleInstanceManager(APP_ID)
    if single.notify_existing():
        sys.exit(0)
    if not single.start_server():
        if single.notify_existing(timeout_ms=2000):
            sys.exit(0)
        print(f"[{APP_ID}] 无法创建单实例管道", file=sys.stderr)
        sys.exit(1)

    # ── 启动 GUI ──
    from sysu_netauth.app.tray import main as tray_main

    tray_main(app=app, single_instance=single, started_by_startup=started_by_startup)


if __name__ == "__main__":
    main()
