from __future__ import annotations

import subprocess
import sys
import time

from PySide6.QtCore import Qt, QTimer, QUrl
from PySide6.QtGui import QAction, QDesktopServices, QIcon, QPixmap
from PySide6.QtWidgets import QApplication, QMenu, QMessageBox, QSystemTrayIcon

from sysu_netauth.app.startup import set_gui_launch_on_login
from sysu_netauth.app.views import MainWindow
from sysu_netauth.core.assets import resolve_asset_path
from sysu_netauth.core.config import (
    APP_DISPLAY_NAME,
    APP_ID,
    AppConfig,
    UpdateUiState,
    load_config,
    load_update_state,
    load_update_ui_state,
    read_status,
    save_config,
    save_update_ui_state,
    write_command,
)
from sysu_netauth.core.interfaces import list_auth_candidate_interfaces
from sysu_netauth.core.npcap import has_npcap
from sysu_netauth.core.update import is_safe_external_url


def _service_sc(*args: str) -> tuple[bool, str]:
    """运行 sc.exe 命令管理 Windows 服务（服务名 SYSUNetAuth）。返回 (成功?, 输出)。"""
    try:
        r = subprocess.run(
            ["sc.exe", *args],
            capture_output=True,
            text=True,
            creationflags=0x08000000,  # CREATE_NO_WINDOW
        )
        ok = r.returncode == 0
        return ok, (r.stdout or r.stderr).strip()
    except Exception as exc:
        return False, str(exc)


def _ensure_service_running() -> tuple[bool, str]:
    return _service_sc("start", APP_ID)


def _adjust_service_start_type(start_type: str) -> tuple[bool, str]:
    """调整 Windows 服务自启动类型：auto（开机自启）或 demand（手动/GUI 管理）。"""
    return _service_sc("config", APP_ID, "start=", start_type)


def _stop_service() -> tuple[bool, str]:
    return _service_sc("stop", APP_ID)


def _is_expected_sc_result(output: str, *codes: int) -> bool:
    return any(str(code) in output for code in codes)


STATUS_POLL_INTERVAL_MS = 2000
SERVICE_STALE_SECONDS = 15.0
NOTIFY_COOLDOWN_AUTHENTICATED = 180
NOTIFY_COOLDOWN_FAILED = 300


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
        self._restart_pending = False
        self._update_ui_state = load_update_ui_state()
        self._last_update_signature: tuple[object, ...] | None = None
        self._manual_update_pending = False
        self._current_release_url = ""

        self.icons = {key: make_tray_icon(key) for key in TRAY_STATES}
        self.tray = QSystemTrayIcon(self.icons["blue"])
        self.tray.setToolTip(APP_DISPLAY_NAME)
        self.tray.setContextMenu(self._build_menu())
        self.tray.activated.connect(self._on_activated)
        self.tray.show()

        self.status_window.auth_btn.clicked.connect(self._on_user_authenticate)
        self.status_window.logoff_btn.clicked.connect(self.logoff)
        self.status_window.restart_service_btn.clicked.connect(self._restart_service)
        self.status_window.quit_btn.clicked.connect(self._on_quit)
        self.status_window.npcap_installed.connect(self._on_npcap_installed)
        self.status_window.advanced_settings_changed.connect(self._on_advanced_changed)
        self.status_window.check_update_requested.connect(self._request_update_check)
        self.status_window.view_release_requested.connect(self._open_current_release)
        self.status_window.ignore_update_requested.connect(self._ignore_current_update)

        self.status_window.winId()
        try:
            self._populate_forms()
        except Exception as exc:
            self.status_window.append_log(f"配置面板初始化异常: {exc}")

        ok, output = _ensure_service_running()
        if not ok and not _is_expected_sc_result(output, 1056):
            self.status_window.append_log(f"服务启动失败：{output or '未知错误'}")

        self._status_poll_timer = QTimer()
        self._status_poll_timer.setInterval(STATUS_POLL_INTERVAL_MS)
        self._status_poll_timer.timeout.connect(self._poll_service_status)
        self._status_poll_timer.start()

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
        menu.addAction("检查更新", self._request_update_check)
        menu.addAction("帮助与资源", self.status_window.show_resources)
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
        self.status_window.append_log("Npcap 检测通过")
        self._restart_service()

    def _restart_service(self) -> None:
        if self._restart_pending:
            return
        self._restart_pending = True
        self.status_window.restart_service_btn.setEnabled(False)
        self.status_window.append_log("正在重启服务...")
        ok, output = _stop_service()
        if not ok and not _is_expected_sc_result(output, 1062):
            self.status_window.append_log(f"停止服务失败：{output or '未知错误'}")
        QTimer.singleShot(1500, self._finish_service_restart)

    def _finish_service_restart(self) -> None:
        ok, output = _ensure_service_running()
        if ok or _is_expected_sc_result(output, 1056):
            self.status_window.append_log("服务已启动")
        else:
            self.status_window.append_log(f"服务启动失败：{output or '未知错误'}")
        self._restart_pending = False
        self.status_window.restart_service_btn.setEnabled(True)

    def set_state(self, icon_key: str, message: str) -> None:
        self.tray.setIcon(self.icons[icon_key])
        self.tray.setToolTip(f"{APP_DISPLAY_NAME} - {message}")
        self.status_action.setText(f"状态：{message}")
        self.status_window.set_state(icon_key, message)

    def _notify_state(self, user_state: str, message: str) -> None:
        now = time.monotonic()
        if user_state == "authenticated":
            if (
                now - self._last_notify_at.get("authenticated", 0)
                < NOTIFY_COOLDOWN_AUTHENTICATED
            ):
                return
            self._last_notify_at["authenticated"] = now
            self.tray.showMessage(
                APP_DISPLAY_NAME, "校园网已连接", self.icons["green"], 3000
            )
        elif user_state == "failed":
            if now - self._last_notify_at.get("failed", 0) < NOTIFY_COOLDOWN_FAILED:
                return
            self._last_notify_at["failed"] = now
            self.tray.showMessage(
                APP_DISPLAY_NAME, f"认证失败：{message}", self.icons["red"], 5000
            )

    @staticmethod
    def _status_is_stale(updated_at: float) -> bool:
        if not updated_at:
            return True
        age = time.time() - updated_at
        # 未来时间戳通常表示系统时钟曾被校准；超过容差同样视为无效。
        return age > SERVICE_STALE_SECONDS or age < -SERVICE_STALE_SECONDS

    @staticmethod
    def _user_state(state: str, message: str) -> tuple[str, str]:
        """将服务内部状态映射为用户可见的 (icon_key, label)。

        7 种用户可见状态：已认证 / 认证中 / 未认证 / 无可认证网卡 /
        认证失败 / 服务不可用 / Npcap 未安装。
        """
        if state == "authenticated":
            return ("green", "已认证")
        if state == "authenticating":
            return ("blue", "认证中")
        if state == "failed":
            # 区分 Npcap 未安装 vs 一般认证失败
            if "Npcap" in message or "npcap" in message.lower():
                return ("red", "Npcap 未安装")
            return ("red", "认证失败")
        if state == "stopped":
            return ("red", "服务不可用")
        # IDLE — 按 message 细分
        if message in ("已断开", "待命"):
            return ("gray", "未认证")
        if message in ("未检测到网线", "未检测到已连接网线", "等待有线网卡"):
            return ("gray", "无可认证网卡")
        # 其他 IDLE（重试等待 / 准备认证 / 网卡就绪 等）→ 认证中
        return ("blue", "认证中")

    def _poll_service_status(self) -> None:
        status = read_status()
        stale = self._status_is_stale(status.updated_at)
        if stale and status.state != "stopped":
            icon_key, label = "red", "服务不可用"
            self.set_state(icon_key, label)
            if not self._last_stale_state:
                self.status_window.append_log("服务状态超过 15 秒未更新")
            self._last_stale_state = True
            self._poll_update_state()
            return

        self._last_stale_state = False
        icon_key, label = self._user_state(status.state, status.message)
        self.set_state(icon_key, label)

        # 通知：仅 authenticated ↔ 非authenticated 转换时弹窗
        prev_user_state = self._last_service_state
        curr_user_state = (
            "authenticated" if status.state == "authenticated" else "other"
        )
        if prev_user_state is not None and prev_user_state != curr_user_state:
            if self.config.desktop_notify:
                if status.state == "authenticated":
                    self._notify_state("authenticated", label)
                elif prev_user_state == "authenticated" and status.state == "failed":
                    self._notify_state("failed", label)
        self._last_service_state = curr_user_state

        # 状态面板字段：直接读 status.json（服务端权威数据）
        self.status_window.update_info(
            iface=status.iface or "-",
            driver=status.driver or "-",
            mac=status.mac or "-",
            ip=status.ipv4 or "-",
            ipv6="-",
            gateway=status.gateway or "-",
            dns=status.dns or "-",
        )
        self._poll_update_state()

    def _request_update_check(self) -> None:
        status = read_status()
        if status.state == "stopped" or self._status_is_stale(status.updated_at):
            QMessageBox.information(
                self.status_window,
                APP_DISPLAY_NAME,
                "后台服务运行后才能检查更新。",
            )
            return
        try:
            write_command("check_update")
        except OSError as exc:
            QMessageBox.warning(
                self.status_window,
                APP_DISPLAY_NAME,
                f"提交更新检查请求失败：{exc}",
            )
            return
        self._manual_update_pending = True
        self.status_window.set_update_checking()
        self.status_window.append_log("已请求服务检查更新")

    def _poll_update_state(self) -> None:
        state = load_update_state()
        signature = (
            state.status,
            state.current_version,
            state.latest_version,
            state.available,
            state.release_url,
            state.summary,
            state.checked_at,
            state.error,
        )
        if signature == self._last_update_signature:
            return
        self._last_update_signature = signature

        if state.status == "checking":
            self.status_window.set_update_checking()
            return

        if state.status == "success":
            self._current_release_url = state.release_url
            if state.available and state.latest_version:
                if self._update_ui_state.ignored_version == state.latest_version:
                    self.status_window.hide_update_available()
                    self.status_window.set_update_idle(
                        f"v{state.latest_version} 已忽略"
                    )
                else:
                    self.status_window.show_update_available(
                        state.latest_version,
                        state.summary,
                    )
                    self._notify_update_once(state.latest_version)
                if self._manual_update_pending:
                    self.status_window.append_log(f"发现新版本 v{state.latest_version}")
            else:
                self.status_window.hide_update_available()
                self.status_window.set_update_idle("已是最新版本")
                if self._manual_update_pending:
                    self.status_window.append_log("当前已是最新版本")
            self._manual_update_pending = False
            return

        if state.status == "error":
            self._current_release_url = state.release_url
            if (
                state.available
                and state.latest_version
                and self._update_ui_state.ignored_version != state.latest_version
            ):
                # 临时网络错误不应抹掉上一次已验证的新版本结果。
                self.status_window.show_update_available(
                    state.latest_version,
                    state.summary,
                )
                self._notify_update_once(state.latest_version)
            else:
                self.status_window.set_update_idle(
                    "检查失败" if self._manual_update_pending else ""
                )
            if self._manual_update_pending:
                self._manual_update_pending = False
                message = state.error or "无法获取更新信息"
                self.status_window.append_log(f"更新检查失败：{message}")
                QMessageBox.warning(
                    self.status_window,
                    APP_DISPLAY_NAME,
                    f"更新检查失败：{message}",
                )
            return

        self.status_window.set_update_idle()

    def _notify_update_once(self, version: str) -> None:
        if (
            not self.config.desktop_notify
            or self._update_ui_state.notified_version == version
        ):
            return
        self.tray.showMessage(
            APP_DISPLAY_NAME,
            f"发现新版本 v{version}，可在主窗口查看。",
            self.icons["blue"],
            6000,
        )
        self._update_ui_state = UpdateUiState(
            notified_version=version,
            ignored_version=self._update_ui_state.ignored_version,
        )
        try:
            save_update_ui_state(self._update_ui_state)
        except OSError as exc:
            self.status_window.append_log(f"保存更新提示状态失败：{exc}")

    def _ignore_current_update(self) -> None:
        state = load_update_state()
        if not state.available or not state.latest_version:
            return
        self._update_ui_state = UpdateUiState(
            notified_version=self._update_ui_state.notified_version,
            ignored_version=state.latest_version,
        )
        try:
            save_update_ui_state(self._update_ui_state)
        except OSError as exc:
            QMessageBox.warning(
                self.status_window,
                APP_DISPLAY_NAME,
                f"保存忽略设置失败：{exc}",
            )
            return
        self.status_window.hide_update_available()
        self.status_window.set_update_idle(f"v{state.latest_version} 已忽略")

    def _open_current_release(self) -> None:
        if not is_safe_external_url(
            self._current_release_url,
            allowed_hosts={"gitee.com", "github.com"},
        ):
            QMessageBox.warning(
                self.status_window,
                APP_DISPLAY_NAME,
                "更新页面地址无效或不受信任。",
            )
            return
        if not QDesktopServices.openUrl(QUrl(self._current_release_url)):
            QMessageBox.warning(
                self.status_window,
                APP_DISPLAY_NAME,
                "无法打开系统默认浏览器。",
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
        new_config = self.status_window.collect_behavior_config(self.config)
        save_config(new_config)
        self.config = new_config

    def _on_advanced_changed(self) -> None:
        new_config = self.status_window.collect_behavior_config(self.config)
        self._apply_config_change(new_config)

    def _apply_config_change(self, new_config: AppConfig) -> None:
        old_service_mode = self.config.service_mode
        old_launch_gui = self.config.launch_gui_on_login
        launch_changed = new_config.launch_gui_on_login != old_launch_gui
        if launch_changed:
            try:
                set_gui_launch_on_login(new_config.launch_gui_on_login)
            except Exception as exc:
                self.status_window.load_advanced_config(self.config)
                QMessageBox.warning(None, APP_DISPLAY_NAME, str(exc))
                return
        try:
            save_config(new_config)
        except OSError as exc:
            if launch_changed:
                try:
                    set_gui_launch_on_login(old_launch_gui)
                except Exception:
                    pass
            self.status_window.load_advanced_config(self.config)
            QMessageBox.warning(None, APP_DISPLAY_NAME, f"保存配置失败：{exc}")
            return
        self.config = new_config

        if new_config.service_mode != old_service_mode:
            if new_config.service_mode:
                ok, output = _adjust_service_start_type("auto")
            else:
                ok, output = _adjust_service_start_type("demand")
                if ok:
                    _ensure_service_running()
            if not ok:
                self.status_window.append_log(
                    f"调整服务启动类型失败：{output or '未知错误'}"
                )

        try:
            write_command("reload_config")
        except OSError as exc:
            self.status_window.append_log(f"通知服务重载配置失败：{exc}")
        self._poll_service_status()

    def _on_user_authenticate(self) -> None:
        try:
            self._flush_quick_form()
            write_command("authenticate")
        except OSError as exc:
            QMessageBox.warning(None, APP_DISPLAY_NAME, f"提交认证请求失败：{exc}")
            return
        self.status_window.append_log("已请求服务重新连接")

    def logoff(self) -> None:
        try:
            self._flush_quick_form()
            write_command("logoff")
        except OSError as exc:
            QMessageBox.warning(None, APP_DISPLAY_NAME, f"提交断开请求失败：{exc}")
            return
        self.status_window.append_log("已请求服务断开连接并停止自动重试/续期")

    def _on_quit(self) -> None:
        self._status_poll_timer.stop()
        # 非服务模式：GUI 退出时停止服务
        if not self.config.service_mode:
            ok, output = _stop_service()
            if not ok and not _is_expected_sc_result(output, 1062):
                self.status_window.append_log(f"停止服务失败：{output or '未知错误'}")
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

    _tray = CampusTray(app, started_by_startup=started_by_startup)

    if not QSystemTrayIcon.isSystemTrayAvailable():
        QMessageBox.warning(None, APP_DISPLAY_NAME, "系统托盘不可用")
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
