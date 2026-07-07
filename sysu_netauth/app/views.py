"""Compact GUI for SYSU NetAuth."""

from __future__ import annotations

from dataclasses import replace
from html import escape

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QPalette
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QFrame,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QSizePolicy,
    QTextBrowser,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from sysu_netauth.app.workers import NpcapDownloadWorker
from sysu_netauth.core.config import (
    APP_DISPLAY_NAME,
    APP_VERSION,
    CONFIG_PATH,
    AppConfig,
)

STATUS_COLORS: dict[str, str] = {
    "blue": "#3b82f6",
    "gray": "#6b7280",
    "orange": "#f59e0b",
    "green": "#22c55e",
    "red": "#ef4444",
}

STATUS_FRAME_STYLES: dict[str, str] = {
    "blue": (
        "QFrame#statusFrame { background: #eff6ff; border: 1px solid #bfdbfe;"
        " border-radius: 6px; }"
    ),
    "gray": (
        "QFrame#statusFrame { background: #f8fafc; border: 1px solid #cbd5e1;"
        " border-radius: 6px; }"
    ),
    "orange": (
        "QFrame#statusFrame { background: #fffbeb; border: 1px solid #fde68a;"
        " border-radius: 6px; }"
    ),
    "green": (
        "QFrame#statusFrame { background: #ecfdf5; border: 1px solid #bbf7d0;"
        " border-radius: 6px; }"
    ),
    "red": (
        "QFrame#statusFrame { background: #fef2f2; border: 1px solid #fecaca;"
        " border-radius: 6px; }"
    ),
}

STATUS_COLORS_DARK: dict[str, str] = {
    "blue": "#60a5fa",
    "gray": "#9ca3af",
    "orange": "#f59e0b",
    "green": "#22c55e",
    "red": "#ef4444",
}

STATUS_FRAME_STYLES_DARK: dict[str, str] = {
    "blue": (
        "QFrame#statusFrame { background: #172554; border: 1px solid #1e3a8a;"
        " border-radius: 6px; }"
    ),
    "gray": (
        "QFrame#statusFrame { background: #1e293b; border: 1px solid #334155;"
        " border-radius: 6px; }"
    ),
    "orange": (
        "QFrame#statusFrame { background: #451a03; border: 1px solid #92400e;"
        " border-radius: 6px; }"
    ),
    "green": (
        "QFrame#statusFrame { background: #052e16; border: 1px solid #166534;"
        " border-radius: 6px; }"
    ),
    "red": (
        "QFrame#statusFrame { background: #450a0a; border: 1px solid #991b1b;"
        " border-radius: 6px; }"
    ),
}


def _is_dark_mode() -> bool:
    """检测 Windows 当前是否为深色主题。"""
    palette = QApplication.style().standardPalette()
    window_color = palette.color(QPalette.ColorRole.Window)
    return window_color.lightness() < 128


class _NetworkTable(QFrame):
    """网络信息表格（无标题栏，内嵌于"网络状态"组框内）。"""

    def __init__(self, rows: list[tuple[str, str]]) -> None:
        super().__init__()
        self._values: dict[str, str] = dict(rows)
        self._row_order: list[str] = [key for key, _ in rows]

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._browser = QTextBrowser()
        self._browser.setFrameShape(QFrame.Shape.NoFrame)
        self._browser.setOpenLinks(False)
        self._browser.setOpenExternalLinks(False)
        self._browser.setTextInteractionFlags(
            Qt.TextInteractionFlag.TextBrowserInteraction
        )
        layout.addWidget(self._browser)
        self._render()

    def set_value(self, key: str, value: str) -> None:
        if key not in self._row_order:
            self._row_order.append(key)
        self._values[key] = value
        self._render()

    def _render(self) -> None:
        rows: list[str] = []
        for key in self._row_order:
            value = self._values.get(key, "")
            rows.append(
                "<tr>"
                f'<td width="58" style="padding:3px 10px 3px 0; white-space:nowrap;">{escape(key)}</td>'
                f'<td style="padding:3px 0; word-break:break-all;">{escape(value)}</td>'
                "</tr>"
            )

        self._browser.setHtml(
            '<html><body style="margin:0; font-size:12px;">'
            '<table cellspacing="0" cellpadding="0" width="100%" style="table-layout:fixed;">'
            + "".join(rows)
            + "</table></body></html>"
        )


class MainWindow(QMainWindow):
    """Compact control window for service-backed authentication."""

    npcap_installed = Signal()
    advanced_settings_changed = Signal()

    def __init__(self) -> None:
        super().__init__()
        self._loading_advanced = False
        self._npcap_detect_remaining = 0
        self._state_key = "blue"
        self._state_message = "需要配置"
        self._state_debounce = QTimer(self)
        self._state_debounce.setSingleShot(True)
        self._state_debounce.timeout.connect(self._apply_state)
        self.setWindowTitle(f"{APP_DISPLAY_NAME} v{APP_VERSION}")
        self._base_size = (660, 300)

        self._build_central_widget()

        # 窗口就绪后初始化 DPI 感知（延迟到 window handle 可用）
        QTimer.singleShot(0, self._init_dpi_awareness)

        self._npcap_detect_timer = QTimer(self)
        self._npcap_detect_timer.setInterval(2000)
        self._npcap_detect_timer.timeout.connect(self._poll_npcap_after_install)

    def _init_dpi_awareness(self) -> None:
        """连接屏幕 DPI 变化信号，使固定窗口尺寸跟随缩放。"""
        screen = self.screen()
        if screen is None:
            return
        screen.logicalDotsPerInchChanged.connect(self._on_logical_dpi_changed)
        self._on_logical_dpi_changed(screen.logicalDotsPerInch())

    def _on_logical_dpi_changed(self, dpi: float) -> None:
        scale = dpi / 96.0
        self.setFixedSize(
            round(self._base_size[0] * scale),
            round(self._base_size[1] * scale),
        )

    def _build_central_widget(self) -> None:
        self.setCentralWidget(self._build_connect_page())

    def _build_connect_page(self) -> QWidget:
        page = QWidget()
        root = QVBoxLayout(page)
        root.setContentsMargins(14, 14, 14, 14)
        root.setSpacing(0)

        body = QHBoxLayout()
        body.setSpacing(12)
        body.addWidget(self._build_network_panel(), 3)
        body.addWidget(self._build_control_group(), 2)
        root.addLayout(body, 1)
        return page

    def _build_control_group(self) -> QGroupBox:
        group = QGroupBox("配置面板")
        layout = QVBoxLayout(group)
        layout.setContentsMargins(14, 18, 14, 14)
        layout.setSpacing(8)

        self.username_edit = QLineEdit()
        self.username_edit.setPlaceholderText("NetID")
        self.password_edit = QLineEdit()
        self.password_edit.setEchoMode(QLineEdit.EchoMode.Password)
        self.password_edit.setPlaceholderText("NetID 密码")
        self.password_toggle_btn = QToolButton()
        self.password_toggle_btn.setCheckable(True)
        self.password_toggle_btn.setText("◉")
        self.password_toggle_btn.setToolTip("显示密码")
        self.password_toggle_btn.setFixedWidth(30)
        self.password_toggle_btn.toggled.connect(self._toggle_password_visible)

        password_row = QHBoxLayout()
        password_row.setSpacing(6)
        password_row.addWidget(self.password_edit, 1)
        password_row.addWidget(self.password_toggle_btn)

        self.iface_combo = QComboBox()
        self.iface_combo.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.iface_combo.addItem("自动探测有线网卡")

        form = QGridLayout()
        form.setHorizontalSpacing(8)
        form.setVerticalSpacing(8)
        form.addWidget(self._field_label("NetID"), 0, 0)
        form.addWidget(self.username_edit, 0, 1)
        form.addWidget(self._field_label("密码"), 1, 0)
        form.addLayout(password_row, 1, 1)
        form.addWidget(self._field_label("网卡"), 2, 0)
        form.addWidget(self.iface_combo, 2, 1)
        form.setColumnStretch(1, 1)
        form.setColumnMinimumWidth(0, 42)
        layout.addLayout(form)

        options = QGridLayout()
        options.setSpacing(6)

        self._service_mode_check = QCheckBox("自动认证服务独立于程序运转")
        self._service_mode_check.setChecked(True)
        self._launch_gui_on_login_check = QCheckBox("开机自启动程序")
        self.auto_auth_check = QCheckBox("启动后自动认证")
        self.auto_auth_check.setChecked(True)
        self._hide_window_on_login_check = QCheckBox("启动后隐藏窗口")
        self._notify_check = QCheckBox("状态变化时通知")
        for checkbox in (
            self._service_mode_check,
            self._launch_gui_on_login_check,
            self.auto_auth_check,
            self._hide_window_on_login_check,
            self._notify_check,
        ):
            checkbox.toggled.connect(self._on_advanced_changed)
        options.addWidget(self._service_mode_check, 0, 0, 1, 2)
        options.addWidget(self._launch_gui_on_login_check, 1, 0)
        options.addWidget(self._hide_window_on_login_check, 1, 1)
        options.addWidget(self.auto_auth_check, 2, 0)
        options.addWidget(self._notify_check, 2, 1)
        layout.addLayout(options)

        buttons = QGridLayout()
        buttons.setSpacing(8)
        self.auth_btn = QPushButton("重新连接")
        self.logoff_btn = QPushButton("断开连接")
        self.restart_service_btn = QPushButton("重启服务")
        self.restart_service_btn.setToolTip("停止并重新启动 Windows 服务")
        self.quit_btn = QPushButton("退出程序")
        buttons.addWidget(self.auth_btn, 0, 0)
        buttons.addWidget(self.logoff_btn, 0, 1)
        buttons.addWidget(self.restart_service_btn, 1, 0)
        buttons.addWidget(self.quit_btn, 1, 1)
        layout.addLayout(buttons)
        layout.addStretch(1)
        return group

    def _field_label(self, text: str) -> QLabel:
        label = QLabel(f"{text}：")
        label.setAlignment(Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter)
        return label

    def _toggle_password_visible(self, visible: bool) -> None:
        self.password_edit.setEchoMode(
            QLineEdit.EchoMode.Normal if visible else QLineEdit.EchoMode.Password
        )
        self.password_toggle_btn.setText("○" if visible else "◉")
        self.password_toggle_btn.setToolTip("隐藏密码" if visible else "显示密码")

    def _build_network_panel(self) -> QGroupBox:
        """网络状态组：内含状态卡片 + 网络信息表格。"""
        group = QGroupBox("状态面板")
        layout = QVBoxLayout(group)
        layout.setContentsMargins(12, 16, 12, 12)
        layout.setSpacing(8)

        # ── 状态卡片（内嵌在组顶部） ─────────────────
        self.status_frame = QFrame()
        self.status_frame.setObjectName("statusFrame")
        self.status_frame.setFrameShape(QFrame.Shape.StyledPanel)
        status_layout = QHBoxLayout(self.status_frame)
        status_layout.setContentsMargins(12, 6, 12, 6)
        self.state_label = QLabel("需要配置")
        self.state_label.setStyleSheet("font-size:16px; font-weight:700;")
        status_layout.addWidget(self.state_label)
        layout.addWidget(self.status_frame)

        # ── 网络信息表格 ───────────────────────────
        self.network_panel = _NetworkTable(
            [
                ("网卡", "自动探测"),
                ("驱动", "-"),
                ("IPv4", "-"),
                ("IPv6", "-"),
                ("MAC", "-"),
                ("网关", "-"),
                ("DNS", "-"),
            ]
        )
        layout.addWidget(self.network_panel, 1)
        return group

    def install_npcap(self) -> None:
        self.append_log("正在下载 Npcap 安装程序...")
        self._npcap_worker = NpcapDownloadWorker()
        self._npcap_worker.progress.connect(self.append_log)
        self._npcap_worker.finished.connect(self._on_npcap_download_finished)
        self._npcap_worker.start()

    def _on_npcap_download_finished(self, success: bool, path_or_msg: str) -> None:
        if not success:
            self.append_log(path_or_msg)
            QMessageBox.warning(self, APP_DISPLAY_NAME, path_or_msg)
            return

        self.append_log("正在启动 Npcap 安装向导（需管理员权限）...")
        from sysu_netauth.core.npcap import launch_npcap_installer

        ok, msg = launch_npcap_installer(path_or_msg)
        self.append_log(msg)
        if ok:
            self.append_log("安装时保持默认选项即可。程序会自动检测安装结果。")
            self._start_npcap_install_detection()
        else:
            QMessageBox.warning(self, APP_DISPLAY_NAME, msg)

    def _start_npcap_install_detection(self) -> None:
        self._npcap_detect_remaining = 90
        self._npcap_detect_timer.start()
        self.append_log("正在等待 Npcap 安装完成...")

    def _poll_npcap_after_install(self) -> None:
        from sysu_netauth.core.npcap import has_npcap

        if has_npcap():
            self._npcap_detect_timer.stop()
            self._npcap_detect_remaining = 0
            self.append_log("Npcap 已安装")
            self.npcap_installed.emit()
            return

        self._npcap_detect_remaining -= 2
        if self._npcap_detect_remaining <= 0:
            self._npcap_detect_timer.stop()
            self.append_log("尚未检测到 Npcap；如已完成安装，请稍后重启程序。")

    def closeEvent(self, event: QMainWindow.closeEvent) -> None:  # type: ignore[override]
        event.ignore()
        self.hide()

    def append_log(self, text: str) -> None:
        from PySide6.QtCore import QDateTime

        ts = QDateTime.currentDateTime().toString("MM-dd hh:mm:ss")
        self._write_log_file(f"[{ts}] {text}")

    def _write_log_file(self, line: str) -> None:
        try:
            log_dir = CONFIG_PATH.parent
            log_dir.mkdir(parents=True, exist_ok=True)
            log_path = log_dir / "app.log"
            if log_path.exists() and log_path.stat().st_size > 1_048_576:
                raw = log_path.read_bytes()
                tail = raw[-262144:].decode("utf-8", errors="backslashreplace")
                log_path.write_text(tail + "\n" + line + "\n", encoding="utf-8")
            else:
                with open(log_path, "a", encoding="utf-8") as f:
                    f.write(line + "\n")
        except OSError:
            pass

    def set_state(self, key: str, message: str) -> None:
        self._state_key = key
        self._state_message = message
        # 防闪烁：延迟 150ms 提交，期间的连续调用只保留最后一次
        if not self._state_debounce.isActive():
            self._state_debounce.start(150)

    def _apply_state(self) -> None:
        dark = _is_dark_mode()
        colors = STATUS_COLORS_DARK if dark else STATUS_COLORS
        frame_styles = STATUS_FRAME_STYLES_DARK if dark else STATUS_FRAME_STYLES
        color = colors.get(self._state_key, colors["blue"])
        self.state_label.setText(self._state_message)
        self.state_label.setStyleSheet(
            f"font-size:18px; font-weight:700; color:{color};"
        )
        frame_style = frame_styles.get(self._state_key, frame_styles["blue"])
        self.status_frame.setStyleSheet(frame_style)

    def update_info(
        self,
        iface: str = "-",
        driver: str = "-",
        mac: str = "-",
        ip: str = "-",
        ipv6: str = "-",
        gateway: str = "-",
        dns: str = "-",
    ) -> None:
        self.network_panel.set_value("网卡", iface)
        self.network_panel.set_value("驱动", driver)
        self.network_panel.set_value("IPv4", ip)
        self.network_panel.set_value("IPv6", ipv6)
        self.network_panel.set_value("MAC", mac)
        self.network_panel.set_value("网关", gateway)
        self.network_panel.set_value("DNS", dns)

    def load_advanced_config(self, config: AppConfig) -> None:
        self._loading_advanced = True
        self._service_mode_check.setChecked(config.service_mode)
        self._launch_gui_on_login_check.setChecked(config.launch_gui_on_login)
        self.auto_auth_check.setChecked(config.auto_auth)
        self._hide_window_on_login_check.setChecked(config.hide_window_on_login)
        self._notify_check.setChecked(config.desktop_notify)
        self._loading_advanced = False

    def _on_advanced_changed(self) -> None:
        if not self._loading_advanced:
            self.advanced_settings_changed.emit()

    def collect_behavior_config(self, config: AppConfig) -> AppConfig:
        idx = self.iface_combo.currentIndex()
        iface = ""
        iface_mode = "auto"
        if idx > 0 and self.iface_combo.currentData():
            iface = self.iface_combo.currentData()
            iface_mode = "manual"
        return replace(
            config,
            username=self.username_edit.text().strip(),
            password=self.password_edit.text(),
            iface=iface,
            iface_mode=iface_mode,
            service_mode=self._service_mode_check.isChecked(),
            auto_auth=self.auto_auth_check.isChecked(),
            launch_gui_on_login=self._launch_gui_on_login_check.isChecked(),
            hide_window_on_login=self._hide_window_on_login_check.isChecked(),
            desktop_notify=self._notify_check.isChecked(),
        )
