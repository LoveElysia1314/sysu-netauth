//! Win32 controls. The UI only talks to the service; it never owns EAPOL.
use crate::{
    config::{self, Config, Paths},
    engine::{Snapshot, State},
    ipc::{self, Action, Response},
    platform::{self, Adapter, wide},
};
use anyhow::{Result, ensure};
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
        SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS},
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
mod adapters;
pub(crate) mod notifications;
mod theme;
use notifications::StateNotifications;
use theme::Theme;

const APP_EVENT: u32 = WM_APP + 1;
const TRAY: u32 = WM_APP + 2;
const NOTICE_TIMER: usize = 1;
const AUTO_AUTH: u16 = 101;
const HIDE: u16 = 115;
const NOTIFY: u16 = 116;
const AUTO_UPDATE: u16 = 117;
const UPDATE_NOTIFY: u16 = 118;
const PREFERENCES: u16 = 119;
const CONNECT: u16 = 102;
const DISCONNECT: u16 = 103;
const UPDATE: u16 = 104;
const RELEASE: u16 = 105;

const SHOW_PASSWORD: u16 = 107;
const IGNORE: u16 = 108;
const MANUAL: u16 = 109;
const LOGIN: u16 = 110;
const LOGS: u16 = 111;
const QUIT: u16 = 112;
const ICON_MONO: u16 = 203;
const ICON_COLOR: u16 = 204;
const MENU_SHOW: u16 = 201;
const MENU_QUIT: u16 = 202;
const MENU_RELEASE: u16 = 205;
const CLIENT_WIDTH: i32 = 720;
const CLIENT_HEIGHT: i32 = 448;

#[derive(Clone, Copy)]
enum Font {
    Body,
    Heading,
}
struct Control {
    hwnd: HWND,
    bounds: [i32; 4],
    font: Font,
    grid: Option<(crate::layout::Grid, usize, usize)>,
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
}
enum Task {
    Request(u16, Action),
}
struct Window {
    hwnd: HWND,
    username: HWND,
    password: HWND,
    iface: HWND,
    manual: HWND,
    auto_auth: HWND,
    login: HWND,
    hidden: HWND,
    notify: HWND,
    auto_update: HWND,
    update_notify: HWND,
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
    previous_account: Option<Config>,
    preference_pending: bool,
    adapters: Vec<Adapter>,
    snapshot: Option<Snapshot>,
    tasks: mpsc::SyncSender<Task>,
    events: Arc<Mutex<Vec<UiEvent>>>,
    tray: NOTIFYICONDATAW,
    state_notifications: StateNotifications,
    subscription_connected: bool,
    ignored_version: String,
    notified_version: String,
    startup: bool,
    initial_config: bool,
    taskbar_created: u32,
    colored_icons: bool,
}
pub(crate) fn resource_icon(id: u16, fallback: *const u16) -> HICON {
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
    fn new(
        hwnd: HWND,
        tasks: mpsc::SyncSender<Task>,
        events: Arc<Mutex<Vec<UiEvent>>>,
        startup: bool,
        ui: &serde_json::Value,
    ) -> Self {
        Self {
            hwnd,
            username: ptr::null_mut(),
            password: ptr::null_mut(),
            iface: ptr::null_mut(),
            manual: ptr::null_mut(),
            auto_auth: ptr::null_mut(),
            login: ptr::null_mut(),
            hidden: ptr::null_mut(),
            notify: ptr::null_mut(),
            auto_update: ptr::null_mut(),
            update_notify: ptr::null_mut(),
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
            previous_account: None,
            preference_pending: false,
            adapters: Vec::new(),
            snapshot: None,
            tasks,
            events: events.clone(),
            tray: unsafe { mem::zeroed() },
            state_notifications: StateNotifications::default(),
            subscription_connected: false,
            ignored_version: ui["ignored_version"].as_str().unwrap_or_default().into(),
            notified_version: ui["notified_version"].as_str().unwrap_or_default().into(),
            colored_icons: ui["colored_icons"].as_bool().unwrap_or(false),
            startup,
            initial_config: false,
            taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        }
    }
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
            grid: None,
        });
        handle
    }
    fn grid(
        &mut self,
        bounds: [i32; 4],
        columns: usize,
        rows: usize,
        gap: [i32; 2],
        cells: &[(HWND, usize, usize)],
    ) {
        let grid = crate::layout::Grid {
            bounds,
            columns,
            rows,
            gap,
        };
        for &(hwnd, row, column) in cells {
            self.controls
                .iter_mut()
                .find(|control| control.hwnd == hwnd)
                .unwrap()
                .grid = Some((grid, row, column));
        }
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
                let [x, y, w, h] = control
                    .grid
                    .map(|(grid, row, column)| grid.cell(row, column))
                    .unwrap_or(control.bounds);
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
    fn update_actions(&self, _available: bool, checking: bool) {
        unsafe {
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
        let (foreground, _) = self.theme.status(state);
        let background = self.theme.background;
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
            EndPaint(self.hwnd, &paint);
        }
    }
    fn build(&mut self) {
        self.control(
            "BUTTON",
            "账户与认证",
            BS_GROUPBOX as u32,
            14,
            12,
            340,
            248,
            0,
        );
        self.control(
            "BUTTON",
            "网络状态",
            BS_GROUPBOX as u32,
            366,
            12,
            340,
            248,
            0,
        );
        self.control(
            "BUTTON",
            "常规设置",
            BS_GROUPBOX as u32,
            14,
            272,
            340,
            136,
            0,
        );
        self.control(
            "BUTTON",
            "应用信息",
            BS_GROUPBOX as u32,
            366,
            272,
            340,
            136,
            0,
        );
        self.control("STATIC", "NetID", SS_CENTERIMAGE, 28, 42, 46, 26, 0);
        self.username = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            78,
            42,
            262,
            26,
            0,
        );
        self.control("STATIC", "密码", SS_CENTERIMAGE, 28, 80, 46, 26, 0);
        self.password = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
            78,
            80,
            200,
            26,
            0,
        );
        self.control(
            "BUTTON",
            "显示",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            286,
            80,
            54,
            26,
            SHOW_PASSWORD,
        );
        self.control("STATIC", "网卡", SS_CENTERIMAGE, 28, 118, 46, 26, 0);
        self.iface = self.control(
            "COMBOBOX",
            "",
            WS_TABSTOP
                | CBS_DROPDOWNLIST as u32
                | CBS_OWNERDRAWFIXED as u32
                | CBS_HASSTRINGS as u32
                | WS_VSCROLL,
            78,
            118,
            262,
            220,
            0,
        );
        self.manual = self.control(
            "BUTTON",
            "手动选择网卡",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            156,
            158,
            24,
            MANUAL,
        );
        self.auto_auth = self.control(
            "BUTTON",
            "服务启动后自动认证",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            192,
            156,
            148,
            24,
            AUTO_AUTH,
        );
        for (id, label, x, style) in [
            (CONNECT, "认证", 28, BS_DEFPUSHBUTTON),
            (DISCONNECT, "断开连接", 134, BS_PUSHBUTTON),
            (QUIT, "退出程序", 240, BS_PUSHBUTTON),
        ] {
            self.control(
                "BUTTON",
                label,
                WS_TABSTOP | style as u32,
                x,
                208,
                100,
                30,
                id,
            );
        }
        self.state = self.control("STATIC", "正在连接", SS_CENTERIMAGE, 392, 42, 288, 26, 0);
        self.controls.last_mut().unwrap().font = Font::Heading;
        self.status = self.control(
            "EDIT",
            "正在连接后台服务…",
            WS_TABSTOP | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_READONLY as u32,
            14,
            416,
            692,
            20,
            0,
        );
        for (i, label) in ["网卡", "驱动", "IPv4", "MAC", "网关", "DNS"]
            .iter()
            .enumerate()
        {
            let y = 84 + i as i32 * 23;
            self.control("STATIC", label, SS_CENTERIMAGE, 380, y, 44, 23, 0);
            self.network[i] = self.control(
                "EDIT",
                "—",
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                432,
                y + 3,
                248,
                20,
                0,
            );
        }
        self.login = self.control(
            "BUTTON",
            "开机后启动界面程序",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            296,
            158,
            24,
            LOGIN,
        );
        self.hidden = self.control(
            "BUTTON",
            "开机后隐藏配置窗口",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            192,
            296,
            148,
            24,
            HIDE,
        );
        self.notify = self.control(
            "BUTTON",
            "认证状态变化后通知",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            326,
            158,
            24,
            NOTIFY,
        );
        self.auto_update = self.control(
            "BUTTON",
            "自动检查更新",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            192,
            326,
            148,
            24,
            AUTO_UPDATE,
        );
        self.update_notify = self.control(
            "BUTTON",
            "有新版本时通知",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            28,
            356,
            158,
            24,
            UPDATE_NOTIFY,
        );
        self.update = self.control(
            "STATIC",
            &format!("当前版本 v{}", env!("NETAUTH_APP_VERSION")),
            SS_CENTERIMAGE | SS_ENDELLIPSIS,
            380,
            294,
            312,
            26,
            0,
        );
        for (id, label, x, y) in [
            (UPDATE, "检查更新", 380, 354),
            (LOGS, "运行日志", 486, 354),
            (RELEASE, "更新日志", 592, 354),
        ] {
            self.control("BUTTON", label, WS_TABSTOP, x, y, 100, 26, id);
        }
        // Checkboxes share two tracks across the account and settings panels.
        self.grid(
            [28, 156, 312, 24],
            2,
            1,
            [8, 6],
            &[(self.manual, 0, 0), (self.auto_auth, 0, 1)],
        );
        self.grid(
            [28, 296, 312, 84],
            2,
            3,
            [8, 6],
            &[
                (self.login, 0, 0),
                (self.hidden, 0, 1),
                (self.notify, 1, 0),
                (self.auto_update, 1, 1),
                (self.update_notify, 2, 0),
            ],
        );
        let handle = |id| unsafe { GetDlgItem(self.hwnd, id as i32) };
        let account_actions = [
            (handle(CONNECT), 0, 0),
            (handle(DISCONNECT), 0, 1),
            (handle(QUIT), 0, 2),
        ];
        let info_actions = [
            (handle(UPDATE), 0, 0),
            (handle(LOGS), 0, 1),
            (handle(RELEASE), 0, 2),
        ];
        self.grid([28, 208, 312, 30], 3, 1, [8, 8], &account_actions);
        self.grid([380, 354, 312, 30], 3, 1, [8, 8], &info_actions);
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
        self.tray.hIcon = resource_icon(
            if self.colored_icons {
                2
            } else if theme::taskbar_dark() {
                22
            } else {
                12
            },
            IDI_APPLICATION,
        );
        copy_wide(&mut self.tray.szTip, "SYSU NetAuth");
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &self.tray);
        }
    }
    fn populate(&mut self) {
        set_text(self.username, &self.config.username);
        set_text(self.password, &self.config.password);
        check(self.manual, self.config.iface_mode == "manual");
        self.populate_preferences();
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
    fn populate_preferences(&self) {
        check(self.auto_auth, self.config.auto_auth);
        check(self.login, self.config.launch_gui_on_login);
        check(self.hidden, self.config.hide_window_on_login);
        check(self.notify, self.config.desktop_notify);
        check(self.auto_update, self.config.auto_update_check);
        check(self.update_notify, self.config.update_notify);
        self.update_options();
        unsafe {
            for control in [
                self.auto_auth,
                self.login,
                self.hidden,
                self.notify,
                self.auto_update,
                self.update_notify,
            ] {
                EnableWindow(control, (!self.preference_pending) as i32);
            }
            if !self.preference_pending {
                EnableWindow(self.hidden, checked(self.login) as i32);
            }
        }
    }
    fn account_modified(&self) -> bool {
        let index = unsafe { SendMessageW(self.iface, CB_GETCURSEL, 0, 0) };
        let selected = self
            .adapters
            .get(index as usize)
            .map(|a| a.id.as_str())
            .unwrap_or("");
        text(self.username) != self.config.username
            || text(self.password) != self.config.password
            || checked(self.manual) != (self.config.iface_mode == "manual")
            || (checked(self.manual) && selected != self.config.iface)
    }
    fn update_authentication_button(&self) {
        let online = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.state == State::Authenticated);
        let authenticating = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.state == State::Authenticating);
        let control = unsafe { GetDlgItem(self.hwnd, CONNECT as i32) };
        set_text(
            control,
            if online && !self.account_modified() {
                "重新认证"
            } else {
                "认证"
            },
        );
        unsafe {
            EnableWindow(
                control,
                (self.previous_account.is_none() && !self.preference_pending && !authenticating)
                    as i32,
            );
        }
    }
    fn confirm_discard(&mut self) -> bool {
        if self.previous_account.is_some() || self.preference_pending {
            return false;
        }
        if !self.account_modified() {
            return true;
        }
        if unsafe {
            MessageBoxW(
                self.hwnd,
                wide("账户修改尚未应用，是否放弃修改？点击“认证”才会保存并使用当前账户。").as_ptr(),
                wide("账户修改尚未应用").as_ptr(),
                MB_YESNO | MB_DEFBUTTON2 | MB_ICONQUESTION,
            )
        } != IDYES
        {
            return false;
        }
        self.populate();
        true
    }
    fn change_preference(&mut self, id: u16) {
        if self.preference_pending {
            return;
        }
        let mut preferences = crate::config::PreferencesPatch::default();
        match id {
            AUTO_AUTH => preferences.auto_auth = Some(checked(self.auto_auth)),
            LOGIN => preferences.launch_gui_on_login = Some(checked(self.login)),
            HIDE => preferences.hide_window_on_login = Some(checked(self.hidden)),
            NOTIFY => preferences.desktop_notify = Some(checked(self.notify)),
            AUTO_UPDATE => preferences.auto_update_check = Some(checked(self.auto_update)),
            UPDATE_NOTIFY => preferences.update_notify = Some(checked(self.update_notify)),
            _ => return,
        }
        if self.task(Task::Request(
            PREFERENCES,
            Action::SetPreferences { preferences },
        )) {
            self.preference_pending = true;
            unsafe {
                for control in [
                    self.auto_auth,
                    self.login,
                    self.hidden,
                    self.notify,
                    self.auto_update,
                    self.update_notify,
                ] {
                    EnableWindow(control, 0);
                }
            }
        } else {
            self.populate_preferences();
        }
    }
    fn populate_adapters(&self) {
        self.populate_adapter_choices(&self.config.iface);
    }
    fn populate_adapter_choices(&self, preferred: &str) {
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
        let selected =
            adapters::selection(&self.adapters, preferred).map_or(-1, |index| index as isize);
        for adapter in &self.adapters {
            let name = format!(
                "{}{}",
                adapter.name,
                if adapter.up { "" } else { "（未连接）" }
            );
            unsafe {
                SendMessageW(self.iface, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
            }
        }
        unsafe {
            SendMessageW(self.iface, CB_SETCURSEL, selected as usize, 0);
        }
    }
    fn refresh_adapters(&mut self, all: &[Adapter]) {
        let selected = if checked(self.manual) {
            let index = unsafe { SendMessageW(self.iface, CB_GETCURSEL, 0, 0) };
            self.adapters.get(index as usize).cloned()
        } else {
            None
        };
        let items = adapters::refresh(all, selected.as_ref());
        if self.adapters != items {
            let preferred = selected
                .as_ref()
                .map(|a| a.id.as_str())
                .unwrap_or(&self.config.iface)
                .to_owned();
            self.adapters = items;
            self.populate_adapter_choices(&preferred);
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
                    .map(|a| a.id.clone())
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
            retry_interval: self.config.retry_interval,
            service_mode: true,
            launch_gui_on_login: checked(self.login),
            hide_window_on_login: checked(self.hidden),
            desktop_notify: checked(self.notify),
            auto_update_check: checked(self.auto_update),
            update_notify: checked(self.update_notify),
        };
        c.validate()?;
        ensure!(
            c.iface_mode != "manual" || !c.iface.is_empty(),
            "请选择有线网卡"
        );
        Ok(c)
    }
    fn task(&self, task: Task) -> bool {
        if self.tasks.try_send(task).is_err() {
            platform::show_error("操作队列已满，请稍后再试");
            return false;
        }
        true
    }
    fn notice(&mut self, title: &str, message: &str) {
        if crate::notify::running() || (title == "SYSU NetAuth" && !self.config.desktop_notify) {
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
        self.subscription_connected = true;
        self.refresh_adapters(&snapshot.adapters);
        set_text(self.status, "");
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
        let id = if self.colored_icons {
            id
        } else {
            id + if theme::taskbar_dark() { 20 } else { 10 }
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
        if let Some(notice) = self.state_notifications.observe(
            &snapshot.state,
            snapshot.suspended || snapshot.manual_disconnect,
            self.config.desktop_notify,
            Instant::now(),
        ) {
            self.notice("SYSU NetAuth", notice.message(&snapshot.message));
        }
        self.schedule_notice();
        let update = &snapshot.update;
        self.update_actions(
            update.available && update.latest_version != self.ignored_version,
            update.checking,
        );
        if update.checking {
            set_text(self.update, "正在检查正式版更新…");
        } else if update.available && update.latest_version != self.ignored_version {
            set_text(
                self.update,
                &format!("存在新版本：v{}", update.latest_version),
            );
            if self.config.update_notify
                && !crate::notify::running()
                && !crate::notify::update_was_notified(&update.latest_version)
                && self.notified_version != update.latest_version
            {
                self.notice(
                    "SYSU NetAuth 更新",
                    &format!("发现新版本 {}", update.latest_version),
                );
                self.notified_version = update.latest_version.clone();
                self.save_ui_state();
            }
        } else if update.available && update.latest_version == self.ignored_version {
            set_text(self.update, &format!("已忽略 v{}", update.latest_version));
        } else if !update.error.is_empty() {
            set_text(self.update, "检查失败，请稍后重试");
        } else {
            set_text(
                self.update,
                &format!("当前版本 v{}", env!("NETAUTH_APP_VERSION")),
            );
        }
        self.snapshot = Some(snapshot);
        self.update_authentication_button();
    }
    fn schedule_notice(&self) {
        unsafe {
            KillTimer(self.hwnd, NOTICE_TIMER);
        }
        if self.subscription_connected
            && let Some(deadline) = self
                .state_notifications
                .deadline(self.config.desktop_notify)
        {
            let milliseconds = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .saturating_add(1)
                .clamp(10, u32::MAX as u128) as u32;
            // A timer exists only while an unreported outage is pending.
            if unsafe { SetTimer(self.hwnd, NOTICE_TIMER, milliseconds, None) } == 0 {
                eprintln!("cannot schedule connection notification");
            }
        }
    }
    fn notice_timeout(&mut self) {
        unsafe {
            KillTimer(self.hwnd, NOTICE_TIMER);
        }
        if self.subscription_connected {
            if let Some(notice) = self
                .state_notifications
                .poll(self.config.desktop_notify, Instant::now())
            {
                self.notice("SYSU NetAuth", notice.message("连接已中断"));
            }
            self.schedule_notice();
        }
    }
    fn save_ui_state(&self) {
        let _ = config::atomic_write(
            &Paths::default().state.join("ui_state.json"),
            &serde_json::json!({"ignored_version":self.ignored_version,"notified_version":self.notified_version,"colored_icons":self.colored_icons}),
        );
    }
    fn events(&mut self) {
        let events = mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()));
        for event in events {
            match event {
                UiEvent::Snapshot(s) => self.update_snapshot(*s),
                UiEvent::Disconnected(e) => {
                    self.subscription_connected = false;
                    self.schedule_notice();
                    self.status_style(&State::Failed, "服务未连接");
                    let id = if self.colored_icons {
                        7
                    } else if theme::taskbar_dark() {
                        27
                    } else {
                        17
                    };
                    self.tray.hIcon = resource_icon(id, IDI_ERROR);
                    self.tray.uFlags = NIF_TIP | NIF_ICON;
                    copy_wide(&mut self.tray.szTip, "SYSU NetAuth: 服务未连接");
                    unsafe {
                        Shell_NotifyIconW(NIM_MODIFY, &self.tray);
                    }
                    set_text(self.status, &format!("服务连接已断开：{e}"));
                }

                UiEvent::Reply(id, result) => match result {
                    Err(e) => {
                        if id == PREFERENCES {
                            self.preference_pending = false;
                            self.populate_preferences();
                        }
                        if id == CONNECT {
                            if let Some(previous) = self.previous_account.take() {
                                self.config = previous;
                            }
                            self.task(Task::Request(PREFERENCES, Action::GetConfig));
                            self.state_notifications.cancel_connect_request();
                        }
                        platform::show_error(&e);
                    }
                    Ok(response) => {
                        if id == CONNECT {
                            self.previous_account = None;
                        }
                        if id == PREFERENCES {
                            self.preference_pending = false;
                            if let Ok(config) = serde_json::from_value(response.data) {
                                self.config = config;
                            }
                            self.populate_preferences();
                        } else if id == 1 {
                            match serde_json::from_value(response.data) {
                                Ok(c) => {
                                    self.config = c;
                                    self.populate();
                                }
                                Err(e) => platform::show_error(&e.to_string()),
                            }
                        } else if id == LOGS
                            && let Err(error) = self.show_diagnostics(id, response.data)
                        {
                            platform::show_error(&error.to_string());
                        }
                    }
                },
            }
        }
        self.update_authentication_button();
    }
    fn show_diagnostics(&self, _id: u16, data: serde_json::Value) -> Result<()> {
        {
            let path = Paths::default().state.join("ui-service-log.txt");
            let log = data["log"].as_str().unwrap_or("暂无日志");
            std::fs::write(&path, log)?;
            unsafe {
                ShellExecuteW(
                    self.hwnd,
                    wide("open").as_ptr(),
                    wide(path.as_os_str()).as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        Ok(())
    }
    fn command(&mut self, id: u16) {
        match id {
            CONNECT
                if self.previous_account.is_none()
                    && !self.preference_pending
                    && !self
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.state == State::Authenticating) =>
            {
                match self.collect().and_then(|config| {
                    config.validate_auth()?;
                    Ok(config)
                }) {
                    Ok(config) => {
                        self.previous_account = Some(self.config.clone());
                        self.config = config.clone();
                        if id == CONNECT {
                            self.state_notifications.connect_requested();
                        }
                        let action = Action::Connect { config };
                        if !self.task(Task::Request(id, action)) {
                            if let Some(previous) = self.previous_account.take() {
                                self.config = previous;
                            }
                            self.state_notifications.cancel_connect_request();
                        }
                        self.schedule_notice();
                    }
                    Err(e) => platform::show_error(&e.to_string()),
                }
            }
            DISCONNECT => {
                self.task(Task::Request(DISCONNECT, Action::Logoff));
            }
            UPDATE => {
                self.task(Task::Request(UPDATE, Action::CheckUpdate));
            }
            MANUAL => {
                self.populate_adapters();
                self.update_options();
            }
            AUTO_AUTH | LOGIN | HIDE | NOTIFY | AUTO_UPDATE | UPDATE_NOTIFY => {
                self.change_preference(id)
            }
            LOGS => {
                self.task(Task::Request(id, Action::Diagnostics));
            }
            ICON_MONO | ICON_COLOR => {
                self.colored_icons = id == ICON_COLOR;
                self.save_ui_state();
                if let Some(snapshot) = self.snapshot.clone() {
                    self.update_snapshot(snapshot);
                }
            }
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
            RELEASE => {
                let update = self
                    .snapshot
                    .as_ref()
                    .map(|s| s.update.clone())
                    .unwrap_or_default();
                platform::open_url(
                    &update.changelog_url(env!("NETAUTH_APP_VERSION"), &self.ignored_version),
                );
            }
            MENU_RELEASE => {
                if let Some(snapshot) = &self.snapshot
                    && snapshot.update.available
                    && !snapshot.update.release_url.is_empty()
                {
                    platform::open_url(&snapshot.update.release_url);
                }
            }
            IGNORE => {
                if let Some(s) = &self.snapshot {
                    self.ignored_version = s.update.latest_version.clone();
                    self.save_ui_state();
                    set_text(self.update, "已忽略此版本");
                    self.update_actions(false, false);
                }
            }

            MENU_SHOW => unsafe {
                ShowWindow(self.hwnd, SW_SHOW);
                SetForegroundWindow(self.hwnd);
            },
            QUIT | MENU_QUIT if self.confirm_discard() => unsafe {
                DestroyWindow(self.hwnd);
            },
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
            let update_menu = CreatePopupMenu();
            let checking = self.snapshot.as_ref().is_some_and(|s| s.update.checking);
            let available = self.snapshot.as_ref().is_some_and(|s| {
                s.update.available && s.update.latest_version != self.ignored_version
            });
            for (id, label, enabled) in [
                (UPDATE, "检查更新", !checking),
                (RELEASE, "更新日志", true),
                (MENU_RELEASE, "查看新版说明", available),
                (IGNORE, "忽略此版本", available),
            ] {
                AppendMenuW(
                    update_menu,
                    MF_STRING | if enabled { 0 } else { MF_GRAYED },
                    id as usize,
                    wide(label).as_ptr(),
                );
            }
            AppendMenuW(
                menu,
                MF_POPUP,
                update_menu as usize,
                wide("软件更新").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
            AppendMenuW(
                menu,
                MF_STRING | if self.colored_icons { 0 } else { MF_CHECKED },
                ICON_MONO as usize,
                wide("状态图标：系统单色").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING | if self.colored_icons { MF_CHECKED } else { 0 },
                ICON_COLOR as usize,
                wide("状态图标：彩色").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_QUIT as usize,
                wide("退出状态程序").as_ptr(),
            );
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
                if let Some(snapshot) = window.snapshot.clone() {
                    window.update_snapshot(snapshot);
                }
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
            WM_TIMER if wparam == NOTICE_TIMER => {
                window.notice_timeout();
                return 0;
            }
            WM_COMMAND => {
                window.command((wparam & 0xffff) as u16);
                window.update_authentication_button();
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
                if !window.confirm_discard() {
                    return 0;
                }
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
    if let Err(error) = crate::notify::ensure_running() {
        eprintln!("notification assistant: {error}");
    }
    let name = wide(r"Local\SYSUNetAuth-GUI-v1");
    let mutex = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    let mutex_error = unsafe { GetLastError() };
    ensure!(!mutex.is_null(), "GUI mutex");
    let _guard = platform::Handle(mutex);
    if mutex_error == ERROR_ALREADY_EXISTS {
        if !std::env::args().any(|a| a == "--startup") {
            unsafe {
                let existing = FindWindowW(wide("SYSUNetAuthWindow").as_ptr(), ptr::null());
                if !existing.is_null() {
                    ShowWindow(existing, SW_SHOW);
                    SetForegroundWindow(existing);
                }
            }
        }
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
    let class = wide("SYSUNetAuthWindow");
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
            wide(format!("SYSU NetAuth · v{}", env!("NETAUTH_APP_VERSION"))).as_ptr(),
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
    let mut window = Box::new(Window::new(hwnd, tasks, events.clone(), startup, &ui));
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
                        let login = match &action {
                            Action::SetConfig { config } | Action::Connect { config } => {
                                Some(config.launch_gui_on_login)
                            }
                            Action::SetPreferences { preferences } => {
                                preferences.launch_gui_on_login
                            }
                            _ => None,
                        };
                        let result = ipc::request(&Paths::default(), action).and_then(|response| {
                            if let Some(enabled) = login {
                                platform::set_login_task(enabled)?;
                            }
                            if id == PREFERENCES {
                                ipc::request(&Paths::default(), Action::GetConfig)
                            } else {
                                Ok(response)
                            }
                        });
                        post(
                            target,
                            &queue,
                            UiEvent::Reply(id, result.map_err(|e| e.to_string())),
                        );
                    }
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
                let mut initial = true;
                let result = ipc::watch(&Paths::default(), |s| {
                    if initial {
                        let _ = requests.try_send(Task::Request(1, Action::GetConfig));
                        initial = false;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Offline native visual review at 100%, 150% and 200% DPI"]
    fn render_layout_preview() {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn PrintWindow(hwnd: HWND, dc: HDC, flags: u32) -> i32;
        }
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(ptr::null());
            let class = wide("SYSUNetAuthOfflinePreview");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..mem::zeroed()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide(format!(
                    "SYSU NetAuth · v{} · 离线预览",
                    env!("NETAUTH_APP_VERSION")
                ))
                .as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
                80,
                80,
                CLIENT_WIDTH,
                CLIENT_HEIGHT,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            assert!(!hwnd.is_null());
            let (tasks, _rx) = mpsc::sync_channel(16);
            let mut window = Box::new(Window::new(
                hwnd,
                tasks,
                Arc::new(Mutex::new(Vec::new())),
                false,
                &serde_json::Value::Null,
            ));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *window as *mut Window) as isize);
            window.build();
            window.config.username = "demo".into();
            window.config.password = "preview-only".into();
            window.config.iface_mode = "manual".into();
            window.config.iface = "preview".into();
            let adapter = Adapter {
                id: "preview".into(),
                name: "以太网".into(),
                driver: "Realtek USB GbE Family Controller".into(),
                ipv4: "192.0.2.10".into(),
                gateway: "192.0.2.1".into(),
                dns: "192.0.2.53, 198.51.100.53".into(),
                up: true,
                ethernet: true,
                ..Adapter::default()
            };
            window.adapters = vec![adapter.clone()];
            window.populate();
            window.notified_version = "99.0.0".into();
            window.update_snapshot(Snapshot {
                state: State::Authenticated,
                suspended: false,
                manual_disconnect: false,
                connection_request: 0,
                message: "已认证，互联网连通".into(),
                adapter: Some(adapter.clone()),
                adapters: vec![adapter],
                authenticated_at: None,
                internet: true,
                update_notify: true,
                update: crate::update::Update {
                    available: true,
                    latest_version: "99.0.0".into(),
                    ..Default::default()
                },
            });
            let folder = std::path::Path::new("dist/gui-preview");
            std::fs::create_dir_all(folder).unwrap();
            for (appearance, dark) in [("light", false), ("dark", true)] {
                window.theme = Theme::appearance(dark, false);
                window.status_style(&State::Authenticated, "已认证");
                for dpi in [96, 144, 192] {
                    window.layout(dpi);
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    RedrawWindow(
                        hwnd,
                        ptr::null(),
                        ptr::null_mut(),
                        RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW | RDW_ALLCHILDREN,
                    );
                    let width = scaled(CLIENT_WIDTH, dpi);
                    let height = scaled(CLIENT_HEIGHT, dpi);
                    // The monitor keeps its actual DPI while we review logical layouts.
                    // Correct the non-client frame so each captured client has the intended size.
                    let mut client: RECT = mem::zeroed();
                    let mut outer: RECT = mem::zeroed();
                    GetClientRect(hwnd, &mut client);
                    GetWindowRect(hwnd, &mut outer);
                    SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        0,
                        0,
                        outer.right - outer.left + width - client.right,
                        outer.bottom - outer.top + height - client.bottom,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let dc = GetDC(hwnd);
                    let capture = CreateCompatibleDC(dc);
                    let bitmap = CreateCompatibleBitmap(dc, width, height);
                    let old = SelectObject(capture, bitmap);
                    assert_ne!(PrintWindow(hwnd, capture, 1), 0);
                    SelectObject(capture, old);
                    let mut info: BITMAPINFO = mem::zeroed();
                    info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
                    info.bmiHeader.biWidth = width;
                    info.bmiHeader.biHeight = -height;
                    info.bmiHeader.biPlanes = 1;
                    info.bmiHeader.biBitCount = 32;
                    let mut pixels = vec![0u8; (width * height * 4) as usize];
                    assert_ne!(
                        GetDIBits(
                            capture,
                            bitmap,
                            0,
                            height as u32,
                            pixels.as_mut_ptr().cast(),
                            &mut info,
                            DIB_RGB_COLORS
                        ),
                        0
                    );
                    let mut file = Vec::new();
                    file.extend_from_slice(b"BM");
                    file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                    file.extend_from_slice(&[0; 4]);
                    file.extend_from_slice(&54u32.to_le_bytes());
                    file.extend_from_slice(std::slice::from_raw_parts(
                        (&info.bmiHeader as *const BITMAPINFOHEADER).cast::<u8>(),
                        40,
                    ));
                    file.extend_from_slice(&pixels);
                    std::fs::write(folder.join(format!("windows-{appearance}-{dpi}.bmp")), file)
                        .unwrap();
                    DeleteObject(bitmap);
                    DeleteDC(capture);
                    ReleaseDC(hwnd, dc);
                }
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            Shell_NotifyIconW(NIM_DELETE, &window.tray);
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), instance);
        }
    }

    #[test]
    fn native_combo_tracks_hotplug_and_preserves_pending_selection() {
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("adapter test").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                400,
                200,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            assert!(!hwnd.is_null());
            let (tasks, _rx) = mpsc::sync_channel(16);
            let mut window = Window::new(
                hwnd,
                tasks,
                Arc::new(Mutex::new(Vec::new())),
                false,
                &serde_json::Value::Null,
            );
            window.manual = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide("manual").as_ptr(),
                WS_CHILD | BS_AUTOCHECKBOX as u32,
                0,
                0,
                100,
                30,
                hwnd,
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            window.iface = CreateWindowExW(
                0,
                wide("COMBOBOX").as_ptr(),
                ptr::null(),
                WS_CHILD | CBS_DROPDOWNLIST as u32,
                0,
                40,
                300,
                100,
                hwnd,
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            assert!(!window.manual.is_null() && !window.iface.is_null());
            check(window.manual, true);
            window.config.iface_mode = "manual".into();
            window.config.iface = "first".into();
            window.config.retry_interval = 125;
            window.config.service_mode = false;
            let first = Adapter {
                id: "first".into(),
                name: "Ethernet".into(),
                ethernet: true,
                up: true,
                ..Adapter::default()
            };
            let mut usb = Adapter {
                id: "usb".into(),
                name: "USB Ethernet".into(),
                ethernet: true,
                up: true,
                ..Adapter::default()
            };
            window.refresh_adapters(&[first.clone(), usb.clone()]);
            assert_eq!(SendMessageW(window.iface, CB_GETCOUNT, 0, 0), 2);
            SendMessageW(window.iface, CB_SETCURSEL, 1, 0); // Unsaved user choice.
            usb.name = "USB Ethernet renamed".into();
            window.refresh_adapters(&[usb.clone(), first.clone()]);
            assert_eq!(SendMessageW(window.iface, CB_GETCURSEL, 0, 0), 0);
            assert_eq!(window.collect().unwrap().retry_interval, 125);
            assert!(window.collect().unwrap().service_mode);
            assert_eq!(window.collect().unwrap().iface, "usb");
            window.refresh_adapters(&[first]);
            assert_eq!(window.collect().unwrap().iface, "usb");
            let selected = SendMessageW(window.iface, CB_GETCURSEL, 0, 0) as usize;
            assert!(!window.adapters[selected].up);
            window.refresh_adapters(&[usb]);
            assert_eq!(SendMessageW(window.iface, CB_GETCOUNT, 0, 0), 1);
            assert!(window.adapters[0].up);
            assert_eq!(window.collect().unwrap().iface, "usb");
            DestroyWindow(hwnd);
        }
    }
}
