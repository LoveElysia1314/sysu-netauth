from __future__ import annotations

import os
import socket
import sys
import time

import subprocess

import psutil
from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QAction, QIcon, QPixmap
from PySide6.QtWidgets import QApplication, QMenu, QMessageBox, QSystemTrayIcon

from sysu_netauth.app.views import MainWindow
from sysu_netauth.core.config import (
    APP_DISPLAY_NAME,
    APP_ID,
    CONFIG_PATH,
    AppConfig,
    save_config,
    set_gui_launch_on_login,
)
from sysu_netauth.core.interfaces import (
    list_auth_candidate_interfaces,
    pick_best_candidate,
)
from sysu_netauth.core.assets import resolve_asset_path
from sysu_netauth.core.npcap import has_npcap
from sysu_netauth.core.config import (
    load_config,
    read_status,
    write_command,
)


def _service_sc(args: str) -> tuple[bool, str]:
    """运行 sc.exe 命令管理 Windows 服务（服务名 SYSUNetAuth）。返回 (成功?, 输出)。"""
    try:
        r = subprocess.run(
            ["sc", *args.split()],
            capture_output=True,
            text=True,
            creationflags=0x08000000,  # CREATE_NO_WINDOW
        )
        ok = r.returncode == 0
        return ok, (r.stdout or r.stderr).strip()
    except Exception as exc:
        return False, str(exc)


def _ensure_service_running() -> None:
    """尝试启动 Windows 服务。失败时静默处理，GUI 会通过轮询展示"服务无响应"。"""
    _service_sc(f'start "{APP_ID}"')


def _adjust_service_start_type(start_type: str) -> None:
    """调整 Windows 服务自启动类型：auto（开机自启）或 demand（手动/GUI 管理）。"""
    _service_sc(f'config "{APP_ID}" start={start_type}')


def _stop_service() -> None:
    """停止 Windows 服务。"""
    _service_sc(f'stop "{APP_ID}"')


STATUS_POLL_INTERVAL_MS = 2000
INFO_REFRESH_INTERVAL_MS = 30000
SERVICE_STALE_SECONDS = 15.0
# 通知冷却（秒）：同类型通知最小间隔，防止显示器关闭期间积累轰炸
NOTIFY_COOLDOWN: dict[str, float] = {
    "failed": 300,
    "authenticated": 180,  # 3 分钟（好消息可略频繁）
}


TRAY_STATES = ("blue", "gray", "orange", "green", "red")


def _load_icon(name: str) -> QIcon:
    path = resolve_asset_path(name)
    if path.is_file():
        icon = QIcon(str(path))
        if not icon.isNull():
            return icon
    fallback = QPixmap(16, 16)
    fallback.fill(Qt.GlobalColor.darkGray)
    return QIcon(fallback)


def make_tray_icon(state: str) -> QIcon:
    return _load_icon(f"tray-{state}")


def make_window_icon() -> QIcon:
    return _load_icon("icon-ethernet")


class CampusTray:
    def __init__(self, app: QApplication, started_by_startup: bool = False) -> None:
        self.app = app
        self.started_by_startup = started_by_startup
        self.config = load_config()
        self.status_window = MainWindow()
        self._last_service_state: str | None = None
        self._last_notify_at: dict[str, float] = {}
        self._last_stale_state = False

        self.icons = {key: make_tray_icon(key) for key in TRAY_STATES}
        self.tray = QSystemTrayIcon(self.icons["blue"])
        self.tray.setToolTip(APP_DISPLAY_NAME)
        self.tray.setContextMenu(self._build_menu())
        self.tray.activated.connect(self._on_activated)
        self.tray.show()

        self.status_window.auth_btn.clicked.connect(self._on_user_authenticate)
        self.status_window.logoff_btn.clicked.connect(self.logoff)
        self.status_window.open_logs_btn.clicked.connect(self._open_logs)
        self.status_window.quit_btn.clicked.connect(self._on_quit)
        self.status_window.npcap_installed.connect(self._on_npcap_installed)
        self.status_window.advanced_settings_changed.connect(self._on_advanced_changed)

        self.status_window.winId()
        try:
            self._populate_forms()
        except Exception as exc:
            self.status_window.append_log(f"配置面板初始化异常: {exc}")

        # 尝试启动 Windows 服务（若未运行）；失败时 GUI 会轮询显示"服务无响应"
        _ensure_service_running()

        self._status_poll_timer = QTimer()
        self._status_poll_timer.setInterval(STATUS_POLL_INTERVAL_MS)
        self._status_poll_timer.timeout.connect(self._poll_service_status)
        self._status_poll_timer.start()

        self._info_refresh_timer = QTimer()
        self._info_refresh_timer.setInterval(INFO_REFRESH_INTERVAL_MS)
        self._info_refresh_timer.timeout.connect(self._refresh_info_panel)
        self._info_refresh_timer.start()
        QTimer.singleShot(2500, self._refresh_info_panel)

        if not has_npcap():
            self.set_state("orange", "Npcap 未安装")
            self.status_window.append_log("Npcap 未安装，认证功能不可用")
            self.show_status()
            QTimer.singleShot(300, self._prompt_install_npcap)
        else:
            self._poll_service_status()
            if not self._should_start_hidden():
                QTimer.singleShot(100, self.show_status)

    def _populate_forms(self) -> None:
        config = self.config
        sw = self.status_window
        sw.username_edit.setText(config.username)
        sw.password_edit.setText(config.password)
        sw.auto_auth_check.setChecked(config.auto_auth)

        sw.iface_combo.blockSignals(True)
        sw.iface_combo.clear()
        sw.iface_combo.addItem("自动探测有线网卡", "")
        sw.iface_combo.setItemData(
            0,
            "自动选择已连接的物理有线网卡；优先使用上次认证成功的网卡。",
            Qt.ItemDataRole.ToolTipRole,
        )
        for candidate in list_auth_candidate_interfaces():
            status = "已连接" if candidate.is_up else "已断开"
            speed = (
                f"{candidate.speed_mbps // 1000}Gbps"
                if candidate.speed_mbps and candidate.speed_mbps >= 1000
                else (
                    f"{candidate.speed_mbps}Mbps"
                    if candidate.speed_mbps
                    else "速率未知"
                )
            )
            tooltip = (
                f"网卡：{candidate.name}\n"
                f"驱动：{candidate.adapter_description or '-'}\n"
                f"MAC：{candidate.mac or '-'}\n"
                f"状态：{status}\n"
                f"速率：{speed}\n"
                f"类型：有线\n"
                f"判定：{candidate.reason}"
            )
            sw.iface_combo.addItem(candidate.name, candidate.name)
            sw.iface_combo.setItemData(
                sw.iface_combo.count() - 1,
                tooltip,
                Qt.ItemDataRole.ToolTipRole,
            )
        index = sw.iface_combo.findData(config.iface)
        if config.iface_mode == "manual" and index >= 0:
            sw.iface_combo.setCurrentIndex(index)
        else:
            sw.iface_combo.setCurrentIndex(0)
        sw.iface_combo.blockSignals(False)
        sw.load_advanced_config(config)

    def _should_start_hidden(self) -> bool:
        # 仅开机自启（--startup）场景允许隐藏窗口
        if not self.started_by_startup:
            return False
        if not self.config.hide_window_on_login:
            return False
        return bool(self.config.username) and bool(self.config.password) and has_npcap()

    def _build_menu(self) -> QMenu:
        menu = QMenu()
        self.status_action = QAction("状态：未认证")
        self.status_action.setEnabled(False)
        menu.addAction(self.status_action)
        menu.addSeparator()
        menu.addAction("重新连接", self._on_user_authenticate)
        menu.addAction("断开连接", self.logoff)
        menu.addSeparator()
        menu.addAction("显示窗口", self.show_status)
        menu.addSeparator()
        menu.addAction("退出程序", self._on_quit)
        return menu

    def _on_activated(self, reason: QSystemTrayIcon.ActivationReason) -> None:
        if reason == QSystemTrayIcon.ActivationReason.Trigger:
            self.show_status()

    def _prompt_install_npcap(self) -> None:
        box = QMessageBox(self.status_window)
        box.setWindowTitle("安装 Npcap")
        box.setIcon(QMessageBox.Icon.Warning)
        box.setText("缺少核心依赖 Npcap")
        box.setInformativeText("SYSU NetAuth 需要 Npcap 收发 EAPOL 认证帧。")
        install_btn = box.addButton("安装", QMessageBox.ButtonRole.AcceptRole)
        box.addButton("取消", QMessageBox.ButtonRole.RejectRole)
        box.setDefaultButton(install_btn)
        box.exec()
        if box.clickedButton() == install_btn:
            self.status_window.install_npcap()

    def _on_npcap_installed(self) -> None:
        self.set_state("blue", "Npcap 已安装")
        self.status_window.append_log("Npcap 检测通过，已请求服务重新加载配置")
        write_command("reload_config")

    def _open_logs(self) -> None:
        try:
            os.startfile(str(CONFIG_PATH.parent))
        except Exception as exc:
            QMessageBox.warning(None, APP_DISPLAY_NAME, f"无法打开日志目录：{exc}")

    def set_state(self, icon_key: str, message: str) -> None:
        self.tray.setIcon(self.icons[icon_key])
        self.tray.setToolTip(f"{APP_DISPLAY_NAME} - {message}")
        self.status_action.setText(f"状态：{message}")
        self.status_window.set_state(icon_key, message)

    def _notify_state(self, state: str, message: str) -> None:
        cooldown = NOTIFY_COOLDOWN.get(state)
        if cooldown is None:
            return
        now = time.monotonic()
        last = self._last_notify_at.get(state, 0.0)
        if now - last < cooldown:
            return
        self._last_notify_at[state] = now
        templates: dict[str, tuple[str, str, int]] = {
            "authenticated": ("校园网已连接", "green", 3000),
            "failed": (f"认证失败：{message}", "red", 5000),
        }
        tmpl = templates.get(state)
        if tmpl is None:
            return
        self.tray.showMessage(APP_DISPLAY_NAME, tmpl[0], self.icons[tmpl[1]], tmpl[2])

    def _status_is_stale(self, updated_at: float) -> bool:
        if not updated_at:
            return True
        return time.monotonic() - updated_at > SERVICE_STALE_SECONDS

    def _poll_service_status(self) -> None:
        status = read_status()
        stale = self._status_is_stale(status.updated_at)
        if stale and status.state != "stopped":
            self.set_state("red", "服务无响应")
            if not self._last_stale_state:
                self.status_window.append_log("服务状态超过 15 秒未更新")
            self._last_stale_state = True
            return

        self._last_stale_state = False
        icon_key = {
            "authenticated": "green",
            "authenticating": "orange",
            "failed": "red",
            "stopped": "blue",
            "idle": "blue",
        }.get(status.state, "blue")
        # 手动断开（IDLE + message="已断开"）→ 灰色
        if icon_key == "blue" and status.message == "已断开":
            icon_key = "gray"
        message = status.message or status.state
        self.set_state(icon_key, message)
        if status.iface or status.ipv4 or status.mac:
            self.status_window.network_panel.set_value("网卡", status.iface or "-")
            self.status_window.network_panel.set_value("IPv4", status.ipv4 or "-")
            self.status_window.network_panel.set_value("MAC", status.mac or "-")
        if status.gateway or status.dns:
            self.status_window.network_panel.set_value("网关", status.gateway or "-")
            self.status_window.network_panel.set_value("DNS", status.dns or "-")
        if status.state != self._last_service_state:
            self._last_service_state = status.state
            if self.config.desktop_notify:
                self._notify_state(status.state, message)

    def _refresh_info_panel(self) -> None:
        """Refresh the info panel with local adapter info (MAC/IP) and cached service status (gateway/DNS).

        服务端在每次认证成功后刷新完整的网络信息（含网关/DNS）到 status.json，
        GUI 通过 2s 轮询读取，无需再独立调用 PowerShell。
        MAC/IP/IPv6 来自 psutil（本地即时数据）。
        """
        candidates = list_auth_candidate_interfaces()
        supported = {candidate.name: candidate for candidate in candidates}
        iface = self.config.iface if self.config.iface in supported else ""
        if not iface and self.config.iface_mode == "auto":
            best = pick_best_candidate()
            iface = best.name if best else ""
        if not iface:
            iface_text = (
                self.config.iface if self.config.iface_mode == "manual" else "自动"
            )
            self.status_window.update_info(
                iface=iface_text,
                driver="-",
                ip="-",
                ipv6="-",
                mac="-",
                gateway="-",
                dns="-",
            )
            return

        addrs = psutil.net_if_addrs()
        mac = "-"
        ip = "-"
        ipv6 = "-"
        for addr in addrs.get(iface, []):
            if getattr(addr, "family", None) == psutil.AF_LINK and addr.address:
                mac = addr.address
            elif getattr(addr, "family", None) == socket.AF_INET and addr.address:
                ip = addr.address
            elif getattr(addr, "family", None) == socket.AF_INET6 and addr.address:
                ipv6_addr = addr.address.split("%")[0]
                if not ipv6_addr.startswith("fe80:") or ipv6 == "-":
                    ipv6 = ipv6_addr

        # 从 status.json 读取服务端缓存的网关/DNS（由 _poll_service_status 每 2s 更新）
        status = read_status()
        gateway = status.gateway or "-"
        dns = status.dns or "-"

        candidate = supported.get(iface)
        self.status_window.update_info(
            iface=iface if mac != "-" else "-",
            driver=(candidate.adapter_description if candidate else "-") or "-",
            mac=mac,
            ip=ip,
            ipv6=ipv6,
            gateway=gateway,
            dns=dns,
        )

    def show_status(self) -> None:
        self.status_window.setWindowState(
            self.status_window.windowState() & ~Qt.WindowState.WindowMinimized
            | Qt.WindowState.WindowActive
        )
        self.status_window.show()
        self.status_window.raise_()
        self.status_window.activateWindow()
        # Windows API 兜底：Qt 的 activateWindow 有时因前台锁定策略静默失败
        try:
            import ctypes
            from ctypes import wintypes

            user32 = ctypes.windll.user32
            hwnd = int(self.status_window.winId())
            if user32.IsWindowVisible(hwnd):
                user32.ShowWindow(wintypes.HWND(hwnd), 9)  # SW_RESTORE
                user32.SetForegroundWindow(wintypes.HWND(hwnd))
        except Exception:
            pass

    def _flush_quick_form(self) -> None:
        """快速保存表单（不重新填充，避免阻塞主线程）。"""
        self.config = self.status_window.collect_behavior_config(self.config)
        save_config(self.config)

    def _on_advanced_changed(self) -> None:
        new_config = self.status_window.collect_behavior_config(self.config)
        self._apply_config_change(new_config)

    def _apply_config_change(self, new_config: AppConfig) -> None:
        old_service_mode = self.config.service_mode
        old_launch_gui = self.config.launch_gui_on_login
        self.config = new_config
        save_config(self.config)

        # 服务模式切换 → 调整 Windows 服务自启动类型（轻量 sc.exe）
        if new_config.service_mode != old_service_mode:
            if new_config.service_mode:
                _adjust_service_start_type("auto")
            else:
                _adjust_service_start_type("demand")
                _ensure_service_running()

        # 开机启动快捷方式（win32com COM 调用可能较慢，异步执行）
        if new_config.launch_gui_on_login != old_launch_gui:
            QTimer.singleShot(0, lambda: self._apply_launch_on_login())

        # 通知服务重载配置 + 状态轮询
        write_command("reload_config")
        self._poll_service_status()
        # 延迟刷新网络信息面板，避免阻塞主线程
        QTimer.singleShot(0, self._refresh_info_panel)

    def _apply_launch_on_login(self) -> None:
        """单独执行的异步任务：创建/删除开机自启快捷方式。"""
        try:
            set_gui_launch_on_login(self.config.launch_gui_on_login)
        except Exception as exc:
            QMessageBox.warning(None, APP_DISPLAY_NAME, str(exc))

    def _on_user_authenticate(self) -> None:
        self._flush_quick_form()
        write_command("authenticate")
        self.status_window.append_log("已请求服务重新连接")
        self.set_state("orange", "已请求重新连接")

    def logoff(self) -> None:
        self._flush_quick_form()
        write_command("logoff")
        self.set_state("orange", "已请求断开连接")
        self.status_window.append_log("已请求服务断开连接并停止自动重试/续期")

    def _on_quit(self) -> None:
        self._status_poll_timer.stop()
        self._info_refresh_timer.stop()
        # 非服务模式：GUI 退出时停止服务
        if not self.config.service_mode:
            _stop_service()
        self.app.quit()
        sys.exit(0)


def main(
    app: QApplication | None = None,
    started_by_startup: bool = False,
) -> None:
    try:
        import ctypes

        appid = f"{APP_ID}.Application"
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(appid)
    except Exception:
        pass

    if app is None:
        app = QApplication(sys.argv)
        app.setQuitOnLastWindowClosed(False)
        app.setWindowIcon(make_window_icon())

    tray = CampusTray(app, started_by_startup=started_by_startup)

    if not QSystemTrayIcon.isSystemTrayAvailable():
        QMessageBox.warning(None, APP_DISPLAY_NAME, "系统托盘不可用")
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
