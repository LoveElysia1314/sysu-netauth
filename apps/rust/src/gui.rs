//! Win32 controls. The UI only talks to the service; it never owns EAPOL.
use crate::{
    config::{self, Config, Paths},
    engine::{Snapshot, State},
    ipc::{self, Action, Response},
    platform::{self, Adapter, wide},
};
use anyhow::{Context, Result, ensure};
use std::{
    mem, ptr,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_RIGHT},
        Threading::*,
    },
    UI::{
        Controls::*,
        HiDpi::*,
        Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled},
        Shell::*,
        WindowsAndMessaging::*,
    },
};
mod notifications;
mod theme;
use notifications::StateNotifications;
use theme::Theme;

const APP_EVENT: u32 = WM_APP + 1;
const TRAY: u32 = WM_APP + 2;
const SAVE: u16 = 101;
const CONNECT: u16 = 102;
const DISCONNECT: u16 = 103;
const UPDATE: u16 = 104;
const RELEASE: u16 = 105;
const NPCAP: u16 = 106;
const SHOW_PASSWORD: u16 = 107;
const IGNORE: u16 = 108;
const MANUAL: u16 = 109;
const LOGIN: u16 = 110;
const MENU_SHOW: u16 = 201;
const MENU_QUIT: u16 = 202;
const CLIENT_WIDTH: i32 = 720;
const CLIENT_HEIGHT: i32 = 396;

#[derive(Clone, Copy)]
enum Font {
    Body,
    Heading,
}
struct Control {
    hwnd: HWND,
    bounds: [i32; 4],
    font: Font,
}

// Geometry is kept in logical pixels so moving between monitors never compounds rounding.
fn scaled(value: i32, dpi: u32) -> i32 {
    (value * dpi as i32 + 48) / 96
}
const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    r | g << 8 | b << 16
}

enum UiEvent {
    Reply(u16, Result<Response, String>),
    Snapshot(Box<Snapshot>),
    Disconnected(String),
    Npcap(Result<(), String>),
}
enum Task {
    Request(u16, Action),
    InstallNpcap,
}
struct Window {
    hwnd: HWND,
    username: HWND,
    password: HWND,
    iface: HWND,
    manual: HWND,
    retry: HWND,
    auto_auth: HWND,
    service_mode: HWND,
    login: HWND,
    hidden: HWND,
    notify: HWND,
    state: HWND,
    status: HWND,
    network: [HWND; 6],
    update: HWND,
    controls: Vec<Control>,
    dpi: u32,
    font: HFONT,
    heading_font: HFONT,
    theme: Theme,
    status_brush: HBRUSH,
    status_color: COLORREF,
    status_background: COLORREF,
    config: Config,
    adapters: Vec<Adapter>,
    snapshot: Option<Snapshot>,
    tasks: mpsc::SyncSender<Task>,
    events: Arc<Mutex<Vec<UiEvent>>>,
    tray: NOTIFYICONDATAW,
    state_notifications: StateNotifications,
    ignored_version: String,
    notified_version: String,
    startup: bool,
    initial_config: bool,
    taskbar_created: u32,
}
fn resource_icon(id: u16, fallback: *const u16) -> HICON {
    let icon = unsafe { LoadIconW(GetModuleHandleW(ptr::null()), id as usize as *const u16) };
    if icon.is_null() {
        unsafe { LoadIconW(ptr::null_mut(), fallback) }
    } else {
        icon
    }
}
fn text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let mut bytes = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, bytes.as_mut_ptr(), bytes.len() as i32) };
    String::from_utf16_lossy(&bytes[..n as usize])
}
fn set_text(hwnd: HWND, value: &str) {
    unsafe {
        SetWindowTextW(hwnd, wide(value).as_ptr());
    }
}
fn checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize }
}
fn check(hwnd: HWND, value: bool) {
    unsafe {
        SendMessageW(
            hwnd,
            BM_SETCHECK,
            if value {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
    }
}
fn copy_wide<const N: usize>(dest: &mut [u16; N], value: &str) {
    let bytes = wide(value);
    let count = (N - 1).min(bytes.len().saturating_sub(1));
    dest.fill(0);
    dest[..count].copy_from_slice(&bytes[..count]);
}
fn post(hwnd: usize, events: &Arc<Mutex<Vec<UiEvent>>>, event: UiEvent) {
    let mut queue = events.lock().unwrap_or_else(|e| e.into_inner());
    if queue.len() >= 32 {
        queue.remove(0);
    }
    queue.push(event);
    drop(queue);
    unsafe {
        PostMessageW(hwnd as HWND, APP_EVENT, 0, 0);
    }
}

impl Window {
    #[allow(clippy::too_many_arguments)] // Mirrors CreateWindowExW geometry and control identity.
    fn control(
        &mut self,
        class: &str,
        label: &str,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: u16,
    ) -> HWND {
        let handle = unsafe {
            CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(label).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                scaled(x, self.dpi),
                scaled(y, self.dpi),
                scaled(w, self.dpi),
                scaled(h, self.dpi),
                self.hwnd,
                id as usize as HMENU,
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        unsafe {
            SendMessageW(handle, WM_SETFONT, self.font as usize, 1);
        }
        self.theme.install(handle, class);
        self.controls.push(Control {
            hwnd: handle,
            bounds: [x, y, w, h],
            font: Font::Body,
        });
        handle
    }
    fn layout(&mut self, dpi: u32) {
        self.dpi = dpi.max(96);
        let old_fonts = [self.font, self.heading_font];
        let mut metrics: NONCLIENTMETRICSW = unsafe { mem::zeroed() };
        metrics.cbSize = mem::size_of::<NONCLIENTMETRICSW>() as u32;
        unsafe {
            if SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
                0,
                self.dpi,
            ) == 0
            {
                GetObjectW(
                    GetStockObject(DEFAULT_GUI_FONT),
                    mem::size_of::<LOGFONTW>() as i32,
                    (&mut metrics.lfMessageFont as *mut LOGFONTW).cast(),
                );
            }
            metrics.lfMessageFont.lfHeight = -scaled(13, self.dpi);
            self.font = CreateFontIndirectW(&metrics.lfMessageFont);
            metrics.lfMessageFont.lfHeight = -scaled(18, self.dpi);
            metrics.lfMessageFont.lfWeight = FW_SEMIBOLD as i32;
            self.heading_font = CreateFontIndirectW(&metrics.lfMessageFont);
            for control in &self.controls {
                let [x, y, w, h] = control.bounds;
                MoveWindow(
                    control.hwnd,
                    scaled(x, self.dpi),
                    scaled(y, self.dpi),
                    scaled(w, self.dpi),
                    scaled(h, self.dpi),
                    1,
                );
                let font = match control.font {
                    Font::Body => self.font,
                    Font::Heading => self.heading_font,
                };
                SendMessageW(control.hwnd, WM_SETFONT, font as usize, 1);
            }
            for font in old_fonts {
                if !font.is_null() {
                    DeleteObject(font);
                }
            }
            SendMessageW(
                self.iface,
                CB_SETITEMHEIGHT,
                usize::MAX,
                scaled(20, self.dpi) as isize,
            );
            SendMessageW(
                self.iface,
                CB_SETITEMHEIGHT,
                0,
                scaled(22, self.dpi) as isize,
            );
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: scaled(CLIENT_WIDTH, self.dpi),
                bottom: scaled(CLIENT_HEIGHT, self.dpi),
            };
            AdjustWindowRectExForDpi(
                &mut rect,
                GetWindowLongW(self.hwnd, GWL_STYLE) as u32,
                0,
                0,
                self.dpi,
            );
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            InvalidateRect(self.hwnd, ptr::null(), 1);
        }
    }
    fn update_options(&self) {
        unsafe {
            EnableWindow(self.iface, checked(self.manual) as i32);
            EnableWindow(self.hidden, checked(self.login) as i32);
        }
    }
    fn update_actions(&self, available: bool, checking: bool) {
        unsafe {
            for id in [RELEASE, IGNORE] {
                ShowWindow(
                    GetDlgItem(self.hwnd, id as i32),
                    if available { SW_SHOW } else { SW_HIDE },
                );
            }
            let button = GetDlgItem(self.hwnd, UPDATE as i32);
            EnableWindow(button, !checking as i32);
            set_text(
                button,
                if checking {
                    "检查中…"
                } else {
                    "检查更新"
                },
            );
        }
    }
    fn status_style(&mut self, state: &State, title: &str) {
        let (foreground, background) = self.theme.status(state);
        unsafe {
            if background != self.status_background || self.status_brush.is_null() {
                if !self.status_brush.is_null() {
                    DeleteObject(self.status_brush);
                }
                self.status_brush = CreateSolidBrush(background);
            }
            self.status_color = foreground;
            self.status_background = background;
            set_text(self.state, title);
            InvalidateRect(self.hwnd, ptr::null(), 1);
        }
    }
    fn paint(&self) {
        unsafe {
            let mut paint: PAINTSTRUCT = mem::zeroed();
            let dc = BeginPaint(self.hwnd, &mut paint);
            FillRect(dc, &paint.rcPaint, self.theme.background_brush);
            let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
            let pen = SelectObject(dc, GetStockObject(DC_PEN));
            SetDCBrushColor(dc, self.status_background);
            SetDCPenColor(dc, self.status_background);
            RoundRect(
                dc,
                scaled(388, self.dpi),
                scaled(34, self.dpi),
                scaled(692, self.dpi),
                scaled(114, self.dpi),
                scaled(10, self.dpi),
                scaled(10, self.dpi),
            );
            SelectObject(dc, brush);
            SelectObject(dc, pen);
            EndPaint(self.hwnd, &paint);
        }
    }
    fn build(&mut self) {
        self.control(
            "BUTTON",
            "账户与连接",
            BS_GROUPBOX as u32,
            14,
            12,
            348,
            316,
            0,
        );
        self.control(
            "BUTTON",
            "网络状态",
            BS_GROUPBOX as u32,
            374,
            12,
            332,
            316,
            0,
        );
        self.control("STATIC", "NetID", SS_CENTERIMAGE, 28, 38, 46, 26, 0);
        self.username = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            78,
            38,
            270,
            26,
            0,
        );
        self.control("STATIC", "密码", SS_CENTERIMAGE, 28, 72, 46, 26, 0);
        self.password = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
            78,
            72,
            208,
            26,
            0,
        );
        self.control(
            "BUTTON",
            "显示",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            294,
            72,
            54,
            26,
            SHOW_PASSWORD,
        );
        self.control("STATIC", "网卡", SS_CENTERIMAGE, 28, 106, 46, 26, 0);
        self.iface = self.control(
            "COMBOBOX",
            "",
            WS_TABSTOP
                | CBS_DROPDOWNLIST as u32
                | CBS_OWNERDRAWFIXED as u32
                | CBS_HASSTRINGS as u32
                | WS_VSCROLL,
            78,
            106,
            270,
            220,
            0,
        );
        self.manual = self.control(
            "BUTTON",
            "手动选择网卡",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            140,
            142,
            24,
            MANUAL,
        );
        self.control("STATIC", "重试（秒）", SS_CENTERIMAGE, 196, 140, 88, 24, 0);
        self.retry = self.control(
            "EDIT",
            "60",
            WS_TABSTOP | WS_BORDER | ES_NUMBER as u32,
            286,
            140,
            62,
            24,
            0,
        );
        self.service_mode = self.control(
            "BUTTON",
            "认证服务独立于程序运行",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            178,
            320,
            24,
            0,
        );
        self.auto_auth = self.control(
            "BUTTON",
            "启动后自动认证",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            206,
            156,
            24,
            0,
        );
        self.notify = self.control(
            "BUTTON",
            "状态变化时通知",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            192,
            206,
            156,
            24,
            0,
        );
        self.login = self.control(
            "BUTTON",
            "登录时启动程序",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            234,
            156,
            24,
            LOGIN,
        );
        self.hidden = self.control(
            "BUTTON",
            "启动时隐藏窗口",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            192,
            234,
            156,
            24,
            0,
        );
        for (id, label, x, style) in [
            (SAVE, "保存设置", 28, BS_PUSHBUTTON),
            (CONNECT, "重新连接", 138, BS_DEFPUSHBUTTON),
            (DISCONNECT, "断开连接", 248, BS_PUSHBUTTON),
        ] {
            self.control(
                "BUTTON",
                label,
                WS_TABSTOP | style as u32,
                x,
                282,
                100,
                30,
                id,
            );
        }
        self.state = self.control("STATIC", "正在连接", SS_CENTERIMAGE, 400, 42, 280, 26, 0);
        self.controls.last_mut().unwrap().font = Font::Heading;
        self.status = self.control(
            "EDIT",
            "正在连接后台服务…",
            WS_TABSTOP | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_READONLY as u32,
            400,
            74,
            280,
            36,
            0,
        );
        for (i, label) in ["网卡", "驱动", "IPv4", "MAC", "网关", "DNS"]
            .iter()
            .enumerate()
        {
            let y = 124 + i as i32 * 25;
            self.control("STATIC", label, SS_CENTERIMAGE, 388, y, 48, 23, 0);
            self.network[i] = self.control(
                "EDIT",
                "—",
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                440,
                y + 3,
                250,
                20,
                0,
            );
        }
        self.control("BUTTON", "安装 Npcap", WS_TABSTOP, 388, 282, 112, 30, NPCAP);
        self.update = self.control(
            "STATIC",
            "",
            SS_CENTERIMAGE | SS_ENDELLIPSIS,
            14,
            340,
            422,
            28,
            0,
        );
        for (id, label, x, width) in [
            (RELEASE, "查看版本", 444, 82),
            (IGNORE, "忽略", 534, 68),
            (UPDATE, "检查更新", 612, 94),
        ] {
            self.control("BUTTON", label, WS_TABSTOP, x, 340, width, 28, id);
        }
        self.control(
            "STATIC",
            concat!("Rust · v", env!("NETAUTH_APP_VERSION")),
            0,
            14,
            372,
            340,
            20,
            0,
        );
        self.control(
            "STATIC",
            "关闭窗口后驻留托盘",
            SS_RIGHT,
            444,
            372,
            262,
            20,
            0,
        );
        self.update_options();
        self.update_actions(false, false);
        self.layout(self.dpi);
        self.theme.title_bar(self.hwnd);
        self.status_style(&State::Idle, "正在连接");
        self.tray.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        self.tray.hWnd = self.hwnd;
        self.tray.uID = 1;
        self.tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        self.tray.uCallbackMessage = TRAY;
        self.tray.hIcon = resource_icon(2, IDI_APPLICATION);
        copy_wide(&mut self.tray.szTip, "SYSU NetAuth");
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &self.tray);
        }
    }
    fn populate(&mut self) {
        set_text(self.username, &self.config.username);
        set_text(self.password, &self.config.password);
        check(self.manual, self.config.iface_mode == "manual");
        check(self.auto_auth, self.config.auto_auth);
        check(self.service_mode, self.config.service_mode);
        check(self.login, self.config.launch_gui_on_login);
        check(self.hidden, self.config.hide_window_on_login);
        check(self.notify, self.config.desktop_notify);
        set_text(self.retry, &self.config.retry_interval.to_string());
        self.populate_adapters();
        self.update_options();
        if !self.initial_config {
            self.initial_config = true;
            if self.startup {
                unsafe {
                    ShowWindow(
                        self.hwnd,
                        if self.config.hide_window_on_login {
                            SW_HIDE
                        } else {
                            SW_SHOW
                        },
                    );
                }
            }
        }
    }
    fn populate_adapters(&self) {
        unsafe {
            SendMessageW(self.iface, CB_RESETCONTENT, 0, 0);
        }
        if !checked(self.manual) {
            unsafe {
                SendMessageW(
                    self.iface,
                    CB_ADDSTRING,
                    0,
                    wide("自动探测有线网卡").as_ptr() as isize,
                );
                SendMessageW(self.iface, CB_SETCURSEL, 0, 0);
            }
            return;
        }
        let mut selected = 0;
        for (i, adapter) in self.adapters.iter().enumerate() {
            let name = format!(
                "{}{}",
                adapter.name,
                if adapter.up { "" } else { "（未连接）" }
            );
            unsafe {
                SendMessageW(self.iface, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
            }
            if adapter.name == self.config.iface || adapter.id == self.config.iface {
                selected = i;
            }
        }
        unsafe {
            SendMessageW(self.iface, CB_SETCURSEL, selected, 0);
        }
    }
    fn collect(&self) -> Result<Config> {
        let index = unsafe { SendMessageW(self.iface, CB_GETCURSEL, 0, 0) };
        let c = Config {
            username: text(self.username),
            password: text(self.password),
            iface: if !checked(self.manual) {
                self.config.iface.clone()
            } else if index >= 0 {
                self.adapters
                    .get(index as usize)
                    .map(|a| a.name.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            iface_mode: if checked(self.manual) {
                "manual"
            } else {
                "auto"
            }
            .into(),
            auto_auth: checked(self.auto_auth),
            retry_interval: text(self.retry).parse().context("重试间隔必须是整数")?,
            service_mode: checked(self.service_mode),
            launch_gui_on_login: checked(self.login),
            hide_window_on_login: checked(self.hidden),
            desktop_notify: checked(self.notify),
        };
        c.validate()?;
        Ok(c)
    }
    fn task(&self, task: Task) {
        if self.tasks.try_send(task).is_err() {
            platform::show_error("操作队列已满，请稍后再试");
        }
    }
    fn notice(&mut self, title: &str, message: &str) {
        if !self.config.desktop_notify {
            return;
        }
        self.tray.uFlags = NIF_INFO;
        copy_wide(&mut self.tray.szInfoTitle, title);
        copy_wide(&mut self.tray.szInfo, message);
        self.tray.dwInfoFlags = NIIF_INFO;
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &self.tray);
        }
    }
    fn update_snapshot(&mut self, snapshot: Snapshot) {
        set_text(self.status, &snapshot.message);
        self.status_style(
            &snapshot.state,
            match snapshot.state {
                State::Authenticated => "已认证",
                State::Authenticating => "正在认证",
                State::Failed => "认证失败",
                State::Stopped => "服务已停止",
                State::Idle if snapshot.message == "已断开" => "已断开",
                State::Idle => "等待连接",
            },
        );
        let values = if let Some(a) = &snapshot.adapter {
            [
                a.name.clone(),
                a.driver.clone(),
                a.ipv4.clone(),
                crate::protocol::mac_text(a.mac),
                a.gateway.clone(),
                a.dns.clone(),
            ]
        } else {
            [
                "暂无活动网卡".into(),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
            ]
        };
        for (control, value) in self.network.iter().zip(values) {
            set_text(*control, if value.is_empty() { "—" } else { &value });
        }
        let (id, icon) = match snapshot.state {
            State::Authenticated => (4, IDI_INFORMATION),
            State::Authenticating => (5, IDI_WARNING),
            State::Failed => (6, IDI_ERROR),
            State::Stopped => (3, IDI_APPLICATION),
            State::Idle if snapshot.message == "已断开" => (3, IDI_APPLICATION),
            _ => (2, IDI_APPLICATION),
        };
        self.tray.hIcon = resource_icon(id, icon);
        self.tray.uFlags = NIF_TIP | NIF_ICON;
        copy_wide(
            &mut self.tray.szTip,
            &format!("SYSU NetAuth: {}", snapshot.message),
        );
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &self.tray);
        }
        if self.state_notifications.should_notify(
            self.snapshot.as_ref().map(|previous| &previous.state),
            &snapshot.state,
            self.config.desktop_notify,
            Instant::now(),
        ) {
            self.notice("SYSU NetAuth", &snapshot.message);
        }
        let update = &snapshot.update;
        self.update_actions(
            update.available && update.latest_version != self.ignored_version,
            update.checking,
        );
        if update.checking {
            set_text(self.update, "正在检查更新…");
        } else if update.available && update.latest_version != self.ignored_version {
            set_text(
                self.update,
                &format!("可更新至 {}：{}", update.latest_version, update.summary),
            );
            if self.notified_version != update.latest_version {
                self.notice(
                    "SYSU NetAuth 更新",
                    &format!("发现新版本 {}", update.latest_version),
                );
                self.notified_version = update.latest_version.clone();
                self.save_ui_state();
            }
        } else if !update.error.is_empty() {
            set_text(self.update, &format!("更新检查失败：{}", update.error));
        } else {
            set_text(self.update, "当前没有可用更新");
        }
        self.snapshot = Some(snapshot);
    }
    fn save_ui_state(&self) {
        let _ = config::atomic_write(
            &Paths::default().state.join("ui_state.json"),
            &serde_json::json!({"ignored_version":self.ignored_version,"notified_version":self.notified_version}),
        );
    }
    fn events(&mut self) {
        let events = mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()));
        for event in events {
            match event {
                UiEvent::Snapshot(s) => self.update_snapshot(*s),
                UiEvent::Disconnected(e) => {
                    self.status_style(&State::Failed, "服务未连接");
                    set_text(self.status, &format!("服务连接已断开：{e}"));
                }
                UiEvent::Npcap(result) => match result {
                    Ok(()) => set_text(self.status, "Npcap 安装程序已启动；完成安装后重新连接"),
                    Err(e) => {
                        platform::show_error(&e);
                        platform::open_url("https://npcap.com/#download");
                    }
                },
                UiEvent::Reply(id, result) => match result {
                    Err(e) => platform::show_error(&e),
                    Ok(response) => {
                        if !response.ok {
                            platform::show_error(response.error.as_deref().unwrap_or("操作失败"));
                            continue;
                        }
                        if id == 1 {
                            match serde_json::from_value(response.data) {
                                Ok(c) => {
                                    self.config = c;
                                    self.populate();
                                }
                                Err(e) => platform::show_error(&e.to_string()),
                            }
                        } else if id == 2 {
                            if let Ok(adapters) =
                                serde_json::from_value::<Vec<Adapter>>(response.data)
                            {
                                self.adapters =
                                    adapters.into_iter().filter(|a| a.ethernet).collect();
                                self.populate_adapters();
                            }
                        } else if id == SAVE || id == CONNECT {
                            if id == CONNECT {
                                self.task(Task::Request(999, Action::Authenticate));
                            }
                            set_text(self.status, "设置已保存");
                        }
                    }
                },
            }
        }
    }
    fn command(&mut self, id: u16) {
        match id {
            SAVE | CONNECT => match self.collect() {
                Ok(config) => {
                    self.config = config.clone();
                    self.task(Task::Request(id, Action::SetConfig { config }));
                }
                Err(e) => platform::show_error(&e.to_string()),
            },
            DISCONNECT => self.task(Task::Request(DISCONNECT, Action::Logoff)),
            UPDATE => self.task(Task::Request(UPDATE, Action::CheckUpdate)),
            MANUAL => {
                self.populate_adapters();
                self.update_options();
            }
            LOGIN => self.update_options(),
            SHOW_PASSWORD => {
                let show = checked(unsafe { GetDlgItem(self.hwnd, SHOW_PASSWORD as i32) });
                unsafe {
                    SendMessageW(
                        self.password,
                        EM_SETPASSWORDCHAR,
                        if show { 0 } else { '●' as usize },
                        0,
                    );
                    InvalidateRect(self.password, ptr::null(), 1);
                }
            }
            RELEASE => platform::open_url(
                self.snapshot
                    .as_ref()
                    .map(|s| s.update.release_url.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("https://github.com/LoveElysia1314/sysu-netauth/releases"),
            ),
            IGNORE => {
                if let Some(s) = &self.snapshot {
                    self.ignored_version = s.update.latest_version.clone();
                    self.save_ui_state();
                    set_text(self.update, "已忽略此版本");
                    self.update_actions(false, false);
                }
            }
            NPCAP => self.task(Task::InstallNpcap),
            MENU_SHOW => unsafe {
                ShowWindow(self.hwnd, SW_SHOW);
                SetForegroundWindow(self.hwnd);
            },
            MENU_QUIT => {
                if !self.config.service_mode {
                    if let Err(e) = platform::service_control(false) {
                        platform::show_error(&e.to_string());
                    }
                }
                unsafe {
                    DestroyWindow(self.hwnd);
                }
            }
            _ => {}
        }
    }
    fn tray_menu(&self) {
        unsafe {
            let menu = CreatePopupMenu();
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_SHOW as usize,
                wide("打开配置面板").as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, MENU_QUIT as usize, wide("退出").as_ptr());
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            SetForegroundWindow(self.hwnd);
            TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                self.hwnd,
                ptr::null(),
            );
            DestroyMenu(menu);
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            for object in [self.font, self.heading_font, self.status_brush] {
                if !object.is_null() {
                    DeleteObject(object);
                }
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Window;
    if !pointer.is_null() {
        let window = unsafe { &mut *pointer };
        if message == window.taskbar_created {
            window.tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            unsafe {
                Shell_NotifyIconW(NIM_ADD, &window.tray);
            }
            return 0;
        }
        match message {
            WM_MEASUREITEM => {
                let item = unsafe { &mut *(lparam as *mut MEASUREITEMSTRUCT) };
                if item.CtlType == ODT_COMBOBOX {
                    item.itemHeight = scaled(22, window.dpi) as u32;
                    return 1;
                }
            }
            WM_DRAWITEM => {
                let item = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
                if item.CtlType == ODT_COMBOBOX {
                    window.theme.combo_item(item, window.dpi);
                    return 1;
                }
            }
            WM_PAINT => {
                window.paint();
                return 0;
            }
            WM_ERASEBKGND => {
                unsafe {
                    let mut rect: RECT = mem::zeroed();
                    GetClientRect(hwnd, &mut rect);
                    FillRect(wparam as HDC, &rect, window.theme.background_brush);
                }
                return 1;
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let dc = wparam as HDC;
                let control = lparam as HWND;
                let is_status = control == window.state || control == window.status;
                let is_field = message == WM_CTLCOLORLISTBOX
                    || control == window.username
                    || control == window.password
                    || control == window.retry
                    || control == window.iface;
                unsafe {
                    SetTextColor(
                        dc,
                        if is_status {
                            window.status_color
                        } else if IsWindowEnabled(control) == 0 {
                            window.theme.muted
                        } else {
                            window.theme.text
                        },
                    );
                    SetBkColor(
                        dc,
                        if is_status {
                            window.status_background
                        } else if is_field {
                            window.theme.field
                        } else {
                            window.theme.background
                        },
                    );
                }
                return if is_status {
                    window.status_brush
                } else if is_field {
                    window.theme.field_brush
                } else {
                    window.theme.background_brush
                } as isize;
            }
            WM_DPICHANGED => {
                let suggested = unsafe { &*(lparam as *const RECT) };
                unsafe {
                    SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        suggested.left,
                        suggested.top,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                window.layout((wparam & 0xffff) as u32);
                return 0;
            }
            WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
                window.theme = Theme::system();
                window.theme.title_bar(hwnd);
                window.layout(window.dpi);
                let state = window
                    .snapshot
                    .as_ref()
                    .map(|s| s.state.clone())
                    .unwrap_or(State::Idle);
                window.status_style(&state, &text(window.state));
                unsafe {
                    RedrawWindow(
                        hwnd,
                        ptr::null(),
                        ptr::null_mut(),
                        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
                    );
                }
                return 0;
            }
            DM_GETDEFID => return ((DC_HASDEFID as usize) << 16 | CONNECT as usize) as isize,
            APP_EVENT => {
                window.events();
                return 0;
            }
            WM_COMMAND => {
                window.command((wparam & 0xffff) as u16);
                return 0;
            }
            TRAY => {
                match lparam as u32 {
                    WM_LBUTTONDBLCLK => window.command(MENU_SHOW),
                    WM_RBUTTONUP => window.tray_menu(),
                    _ => {}
                }
                return 0;
            }
            WM_CLOSE => {
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
                return 0;
            }
            WM_DESTROY => {
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &window.tray);
                    PostQuitMessage(0);
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub fn run() -> Result<()> {
    if !platform::is_admin() {
        let exe = std::env::current_exe()?;
        let args = if std::env::args().any(|a| a == "--startup") {
            "--startup"
        } else {
            ""
        };
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                wide("runas").as_ptr(),
                wide(exe.as_os_str()).as_ptr(),
                wide(args).as_ptr(),
                ptr::null(),
                SW_SHOW,
            )
        };
        ensure!(result as usize > 32, "需要管理员权限连接后台服务");
        return Ok(());
    }
    let name = wide(r"Local\SYSUNetAuth-GUI-v1");
    let mutex = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    let mutex_error = unsafe { GetLastError() };
    ensure!(!mutex.is_null(), "GUI mutex");
    let _guard = platform::Handle(mutex);
    if mutex_error == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let controls = INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&controls);
    }
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = wide("SYSUNetAuthRustWindow");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        hIcon: resource_icon(1, IDI_APPLICATION),
        lpszClassName: class.as_ptr(),
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
        ..unsafe { mem::zeroed() }
    };
    ensure!(unsafe { RegisterClassW(&wc) } != 0, "register window");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide("SYSU NetAuth").as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CLIENT_WIDTH,
            CLIENT_HEIGHT,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    ensure!(!hwnd.is_null(), "create window");
    let events = Arc::new(Mutex::new(Vec::new()));
    let (tasks, rx) = mpsc::sync_channel(16);
    let startup = std::env::args().any(|a| a == "--startup");
    let ui: serde_json::Value =
        config::read(&Paths::default().state.join("ui_state.json")).unwrap_or_default();
    let mut window = Box::new(Window {
        hwnd,
        username: ptr::null_mut(),
        password: ptr::null_mut(),
        iface: ptr::null_mut(),
        manual: ptr::null_mut(),
        retry: ptr::null_mut(),
        auto_auth: ptr::null_mut(),
        service_mode: ptr::null_mut(),
        login: ptr::null_mut(),
        hidden: ptr::null_mut(),
        notify: ptr::null_mut(),
        state: ptr::null_mut(),
        status: ptr::null_mut(),
        network: [ptr::null_mut(); 6],
        update: ptr::null_mut(),
        controls: Vec::new(),
        dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
        font: ptr::null_mut(),
        heading_font: ptr::null_mut(),
        theme: Theme::system(),
        status_brush: ptr::null_mut(),
        status_color: 0,
        status_background: 0,
        config: Config::default(),
        adapters: Vec::new(),
        snapshot: None,
        tasks,
        events: events.clone(),
        tray: unsafe { mem::zeroed() },
        state_notifications: StateNotifications::default(),
        ignored_version: ui["ignored_version"].as_str().unwrap_or_default().into(),
        notified_version: ui["notified_version"].as_str().unwrap_or_default().into(),
        startup,
        initial_config: false,
        taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *window as *mut Window) as isize);
    }
    window.build();
    if !startup {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
        }
    }
    let target = hwnd as usize;
    let queue = events.clone();
    std::thread::Builder::new()
        .name("ui-control".into())
        .spawn(move || {
            if let Err(e) = platform::service_control(true) {
                post(target, &queue, UiEvent::Disconnected(e.to_string()));
            }
            for task in rx {
                match task {
                    Task::Request(id, action) => {
                        let login = if let Action::SetConfig { config } = &action {
                            Some(config.launch_gui_on_login)
                        } else {
                            None
                        };
                        let result = ipc::request(&Paths::default(), action).and_then(|response| {
                            if response.ok {
                                if let Some(enabled) = login {
                                    platform::set_login_task(enabled)?;
                                }
                            }
                            Ok(response)
                        });
                        post(
                            target,
                            &queue,
                            UiEvent::Reply(id, result.map_err(|e| e.to_string())),
                        );
                    }
                    Task::InstallNpcap => post(
                        target,
                        &queue,
                        UiEvent::Npcap(platform::install_npcap().map_err(|e| e.to_string())),
                    ),
                }
            }
        })?;
    let queue = events;
    let requests = window.tasks.clone();
    std::thread::Builder::new()
        .name("ui-status-subscription".into())
        .spawn(move || {
            let mut delay = 1;
            loop {
                if unsafe { IsWindow(target as HWND) } == 0 {
                    break;
                }
                let _ = requests.try_send(Task::Request(1, Action::GetConfig));
                let _ = requests.try_send(Task::Request(2, Action::Interfaces));
                let result = ipc::watch(&Paths::default(), |s| {
                    delay = 1;
                    post(target, &queue, UiEvent::Snapshot(Box::new(s)));
                    Ok(())
                });
                if let Err(e) = result {
                    post(target, &queue, UiEvent::Disconnected(e.to_string()));
                }
                std::thread::sleep(Duration::from_secs(delay));
                delay = (delay * 2).min(30);
            }
        })?;
    let mut msg: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
    Ok(())
}
