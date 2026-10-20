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
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Controls::*, Shell::*, WindowsAndMessaging::*},
};

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
const MENU_SHOW: u16 = 201;
const MENU_QUIT: u16 = 202;
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
    status: HWND,
    network: HWND,
    update: HWND,
    config: Config,
    adapters: Vec<Adapter>,
    snapshot: Option<Snapshot>,
    tasks: mpsc::SyncSender<Task>,
    events: Arc<Mutex<Vec<UiEvent>>>,
    tray: NOTIFYICONDATAW,
    last_notice: Option<(State, Instant)>,
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
        &self,
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
                x,
                y,
                w,
                h,
                self.hwnd,
                id as usize as HMENU,
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        unsafe {
            SendMessageW(
                handle,
                WM_SETFONT,
                GetStockObject(DEFAULT_GUI_FONT) as usize,
                1,
            );
        }
        handle
    }
    fn build(&mut self) {
        self.control("STATIC", "SYSU NetAuth", 0, 20, 16, 450, 25, 0);
        self.control("STATIC", "NetID", 0, 20, 55, 100, 22, 0);
        self.username = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            130,
            51,
            420,
            25,
            0,
        );
        self.control("STATIC", "密码", 0, 20, 90, 100, 22, 0);
        self.password = self.control(
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
            130,
            86,
            325,
            25,
            0,
        );
        self.control(
            "BUTTON",
            "显示",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            465,
            86,
            85,
            25,
            SHOW_PASSWORD,
        );
        self.manual = self.control(
            "BUTTON",
            "手动选择网卡",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            20,
            122,
            145,
            25,
            0,
        );
        self.iface = self.control(
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            170,
            120,
            380,
            220,
            0,
        );
        self.auto_auth = self.control(
            "BUTTON",
            "启动后自动认证",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            20,
            160,
            245,
            25,
            0,
        );
        self.service_mode = self.control(
            "BUTTON",
            "服务独立运行",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            300,
            160,
            250,
            25,
            0,
        );
        self.login = self.control(
            "BUTTON",
            "登录时启动配置面板",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            20,
            193,
            260,
            25,
            0,
        );
        self.hidden = self.control(
            "BUTTON",
            "登录启动时隐藏窗口",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            300,
            193,
            250,
            25,
            0,
        );
        self.notify = self.control(
            "BUTTON",
            "桌面通知",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            20,
            226,
            245,
            25,
            0,
        );
        self.control("STATIC", "重试间隔（秒）", 0, 300, 230, 150, 22, 0);
        self.retry = self.control(
            "EDIT",
            "60",
            WS_TABSTOP | WS_BORDER | ES_NUMBER as u32,
            465,
            225,
            85,
            25,
            0,
        );
        for (id, label, x) in [
            (SAVE, "保存设置", 20),
            (CONNECT, "重新连接", 155),
            (DISCONNECT, "断开连接", 290),
            (UPDATE, "检查更新", 425),
        ] {
            self.control(
                "BUTTON",
                label,
                WS_TABSTOP | BS_PUSHBUTTON as u32,
                x,
                270,
                125,
                32,
                id,
            );
        }
        self.status = self.control("STATIC", "正在连接后台服务…", 0, 20, 320, 530, 40, 0);
        self.network = self.control(
            "EDIT",
            "",
            WS_BORDER | ES_MULTILINE as u32 | ES_READONLY as u32,
            20,
            365,
            530,
            140,
            0,
        );
        self.update = self.control("STATIC", "", 0, 20, 520, 530, 45, 0);
        for (id, label, x) in [
            (RELEASE, "打开发布页", 20),
            (IGNORE, "忽略此版本", 200),
            (NPCAP, "安装 Npcap", 380),
        ] {
            self.control("BUTTON", label, WS_TABSTOP, x, 575, 170, 30, id);
        }
        self.control(
            "STATIC",
            concat!(
                "Rust 版 ",
                env!("NETAUTH_APP_VERSION"),
                " · 关闭窗口后驻留托盘"
            ),
            0,
            20,
            625,
            530,
            22,
            0,
        );
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
            iface: if index >= 0 {
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
        let info = if let Some(a) = &snapshot.adapter {
            format!(
                "网卡：{}\r\nMAC：{}\r\nIPv4：{}\r\n网关：{}\r\nDNS：{}\r\n驱动：{}",
                a.name,
                crate::protocol::mac_text(a.mac),
                a.ipv4,
                a.gateway,
                a.dns,
                a.driver
            )
        } else {
            "暂无活动网卡".into()
        };
        set_text(self.network, &info);
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
        if self
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.state != snapshot.state)
            && matches!(snapshot.state, State::Authenticated | State::Failed)
        {
            let cooldown = if snapshot.state == State::Authenticated {
                180
            } else {
                300
            };
            if self.last_notice.as_ref().is_none_or(|(state, time)| {
                *state != snapshot.state || time.elapsed() >= Duration::from_secs(cooldown)
            }) {
                self.notice("SYSU NetAuth", &snapshot.message);
                self.last_notice = Some((snapshot.state.clone(), Instant::now()));
            }
        }
        let update = &snapshot.update;
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
                UiEvent::Disconnected(e) => set_text(self.status, &format!("服务连接已断开：{e}")),
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
        SetProcessDPIAware();
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
            590,
            700,
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
        status: ptr::null_mut(),
        network: ptr::null_mut(),
        update: ptr::null_mut(),
        config: Config::default(),
        adapters: Vec::new(),
        snapshot: None,
        tasks,
        events: events.clone(),
        tray: unsafe { mem::zeroed() },
        last_notice: None,
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
