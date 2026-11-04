//! A windowless user-session companion. It subscribes to the service and owns
//! desktop notifications independently of the configuration window.
use crate::{
    config::{self, Paths},
    engine::Snapshot,
    gui::notifications::StateNotifications,
    ipc::{self, Action},
    platform::{self, wide},
};
use anyhow::{Result, ensure};
use std::{
    collections::VecDeque,
    mem,
    os::windows::process::CommandExt,
    ptr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

const MUTEX: &str = r"Local\SYSUNetAuthNotifications-v1";
const CLASS: &str = "SYSUNetAuthNotificationAgent";
const EVENT: u32 = WM_APP + 10;
const TRAY: u32 = WM_APP + 11;
const TEST: u32 = WM_APP + 12;
const OUTAGE_TIMER: usize = 1;
const ICON_TIMER: usize = 2;
enum Event {
    Status(Box<Snapshot>, bool),
    Unavailable,
}

pub fn running() -> bool {
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, wide(MUTEX).as_ptr()) };
    if handle.is_null() {
        return false;
    }
    drop(platform::Handle(handle));
    true
}
pub fn ensure_running() -> Result<()> {
    if !running() {
        let exe = std::env::current_exe()?.with_file_name("sysu_netauth_notify.exe");
        ensure!(exe.is_file(), "notification assistant is missing");
        std::process::Command::new(exe)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    }
    Ok(())
}
pub fn update_was_notified(version: &str) -> bool {
    ["notification_state.json", "ui_state.json"]
        .iter()
        .any(|name| {
            let state: serde_json::Value =
                config::read(&Paths::default().state.join(name)).unwrap_or_default();
            state["notified_version"].as_str() == Some(version)
        })
}
fn copy<const N: usize>(target: &mut [u16; N], value: &str) {
    target.fill(0);
    let units = wide(value);
    let n = (N - 1).min(units.len().saturating_sub(1));
    target[..n].copy_from_slice(&units[..n]);
}
struct Agent {
    hwnd: HWND,
    tray: NOTIFYICONDATAW,
    icon_added: bool,
    queue: Arc<Mutex<VecDeque<Event>>>,
    notices: StateNotifications,
    enabled: bool,
    connected: bool,
    connection_request: Option<u64>,
    update_url: Option<String>,
}
impl Agent {
    fn remove_icon(&mut self) {
        unsafe {
            KillTimer(self.hwnd, ICON_TIMER);
            Shell_NotifyIconW(NIM_DELETE, &self.tray);
        }
        self.icon_added = false;
    }
    fn show(&mut self, title: &str, body: &str, url: Option<String>) -> bool {
        self.update_url = url;
        self.tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        if !self.icon_added {
            self.icon_added = unsafe { Shell_NotifyIconW(NIM_ADD, &self.tray) } != 0;
        }
        if !self.icon_added {
            return false;
        }
        self.tray.uFlags = NIF_INFO;
        self.tray.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
        copy(&mut self.tray.szInfoTitle, title);
        copy(&mut self.tray.szInfo, body);
        let sent = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.tray) } != 0;
        if !sent {
            self.remove_icon();
        } else {
            unsafe {
                SetTimer(self.hwnd, ICON_TIMER, 30000, None);
            }
            crate::diagnostics::record(
                &Paths::default(),
                &format!("notification assistant submitted: {title}"),
            );
        }
        sent
    }
    fn schedule(&self) {
        unsafe {
            KillTimer(self.hwnd, OUTAGE_TIMER);
        }
        if self.connected
            && let Some(deadline) = self.notices.deadline(self.enabled)
        {
            let ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .saturating_add(1)
                .clamp(10, u32::MAX as u128) as u32;
            unsafe {
                SetTimer(self.hwnd, OUTAGE_TIMER, ms, None);
            }
        }
    }
    fn receive(&mut self) {
        loop {
            let event = self
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front();
            match event {
                Some(Event::Status(snapshot, enabled)) => {
                    if self
                        .connection_request
                        .is_some_and(|old| old != snapshot.connection_request)
                        && snapshot.connection_request != 0
                    {
                        self.notices.service_connect_requested(&snapshot.state);
                    }
                    self.connection_request = Some(snapshot.connection_request);
                    self.enabled = enabled;
                    self.connected = true;
                    if let Some(notice) = self.notices.observe(
                        &snapshot.state,
                        snapshot.suspended || snapshot.manual_disconnect,
                        enabled,
                        Instant::now(),
                    ) {
                        self.show("SYSU NetAuth", notice.message(&snapshot.message), None);
                    }
                    self.schedule();
                    let update = &snapshot.update;
                    let ui: serde_json::Value =
                        config::read(&Paths::default().state.join("ui_state.json"))
                            .unwrap_or_default();
                    if update.available
                        && ui["ignored_version"].as_str() != Some(update.latest_version.as_str())
                        && !update_was_notified(&update.latest_version)
                        && let Some(message) = update.announcement()
                        && self.show(
                            "SYSU NetAuth 更新",
                            &message,
                            Some(update.release_url.clone()),
                        )
                    {
                        let _ = config::atomic_write(
                            &Paths::default().state.join("notification_state.json"),
                            &serde_json::json!({"notified_version": update.latest_version}),
                        );
                    }
                }
                Some(Event::Unavailable) => {
                    self.connection_request = None;
                    self.connected = false;
                    self.schedule();
                }
                None => break,
            }
        }
    }
}
impl Drop for Agent {
    fn drop(&mut self) {
        self.remove_icon();
    }
}
unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    let agent = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Agent;
    if !agent.is_null() {
        let agent = unsafe { &mut *agent };
        match message {
            EVENT => {
                agent.receive();
                return 0;
            }
            TEST => {
                agent.show(
                    "SYSU NetAuth · 通知助手测试",
                    "状态程序可以退出；本通知来自独立助手，未更改网络认证。",
                    None,
                );
                return 0;
            }
            WM_TIMER if wparam == OUTAGE_TIMER => {
                if let Some(notice) = agent.notices.poll(agent.enabled, Instant::now()) {
                    agent.show("SYSU NetAuth", notice.message(""), None);
                }
                agent.schedule();
                return 0;
            }
            WM_TIMER if wparam == ICON_TIMER => {
                agent.remove_icon();
                return 0;
            }
            TRAY => {
                match lparam as u32 {
                    NIN_BALLOONUSERCLICK => {
                        if let Some(url) = agent.update_url.take() {
                            platform::open_url(&url);
                        } else if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(
                                exe.with_file_name("sysu_netauth_gui.exe"),
                            )
                            .creation_flags(CREATE_NO_WINDOW)
                            .spawn();
                        }
                        agent.remove_icon();
                    }
                    NIN_BALLOONHIDE | NIN_BALLOONTIMEOUT => agent.remove_icon(),
                    _ => {}
                }
                return 0;
            }
            WM_CLOSE => {
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_DESTROY => {
                unsafe {
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
    ensure!(
        platform::is_admin(),
        "notification assistant requires the installed elevated logon task"
    );
    let mutex = unsafe { CreateMutexW(ptr::null(), 0, wide(MUTEX).as_ptr()) };
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    ensure!(!mutex.is_null(), "notification assistant mutex");
    let _mutex = platform::Handle(mutex);
    let test = std::env::args().any(|a| a == "--test-notification");
    if exists {
        if test {
            unsafe {
                PostMessageW(FindWindowW(wide(CLASS).as_ptr(), ptr::null()), TEST, 0, 0);
            }
        }
        return Ok(());
    }
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = wide(CLASS);
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    ensure!(
        unsafe { RegisterClassW(&wc) } != 0,
        "register notification window"
    );
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide("SYSU NetAuth Notifications").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    ensure!(!hwnd.is_null(), "create notification window");
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let mut tray: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    tray.cbSize = mem::size_of_val(&tray) as u32;
    tray.hWnd = hwnd;
    tray.uID = 1;
    tray.uCallbackMessage = TRAY;
    tray.hIcon = crate::gui::resource_icon(1, IDI_APPLICATION);
    copy(&mut tray.szTip, "SYSU NetAuth 通知助手");
    let mut agent = Box::new(Agent {
        hwnd,
        tray,
        icon_added: false,
        queue: queue.clone(),
        notices: StateNotifications::default(),
        enabled: false,
        connected: false,
        connection_request: None,
        update_url: None,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *agent as *mut Agent) as isize);
    }
    let target = hwnd as usize;
    std::thread::Builder::new()
        .name("notification-subscription".into())
        .spawn(move || {
            let post = |event| {
                let mut events = queue.lock().unwrap_or_else(|e| e.into_inner());
                if events.len() == 32 {
                    events.pop_front();
                }
                events.push_back(event);
                drop(events);
                unsafe {
                    PostMessageW(target as HWND, EVENT, 0, 0);
                }
            };
            let mut delay = 1;
            loop {
                let _ = ipc::watch(&Paths::default(), |snapshot| {
                    let enabled = ipc::request(&Paths::default(), Action::Preferences)
                        .ok()
                        .and_then(|r| r.data["desktop_notify"].as_bool())
                        .unwrap_or(false);
                    delay = 1;
                    post(Event::Status(Box::new(snapshot), enabled));
                    Ok(())
                });
                post(Event::Unavailable);
                std::thread::sleep(Duration::from_secs(delay));
                delay = (delay * 2).min(30);
            }
        })?;
    crate::diagnostics::record(&Paths::default(), "notification assistant started");
    if test {
        unsafe {
            PostMessageW(hwnd, TEST, 0, 0);
        }
    }
    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
    Ok(())
}
