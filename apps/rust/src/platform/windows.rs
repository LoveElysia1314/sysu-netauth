use super::{Adapter, Monitor, virtual_mac, virtual_name};
use crate::{
    config::Paths,
    engine::{PowerEvent, PowerNotifier, Signal},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CStr, CString, OsStr, c_void},
    io::{self, Read, Write},
    mem,
    net::Ipv4Addr,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    NetworkManagement::IpHelper::*,
    Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN, SOCKET_ADDRESS},
    Security::*,
    Storage::FileSystem::*,
    System::{IO::*, LibraryLoader::*, Pipes::*, Services::*, Threading::*},
    UI::WindowsAndMessaging::{PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND},
};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}
pub struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Handle {
    fn new(h: HANDLE) -> Result<Self> {
        ensure!(
            !h.is_null() && h != INVALID_HANDLE_VALUE,
            "Windows API: {}",
            io::Error::last_os_error()
        );
        Ok(Self(h))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Cancel(Handle);
impl Cancel {
    pub fn new() -> Result<Self> {
        Ok(Self(Handle::new(unsafe {
            CreateEventW(ptr::null(), 1, 0, ptr::null())
        })?))
    }
    pub fn set(&self) {
        unsafe {
            SetEvent(self.0.0);
        }
    }
}
pub fn cancellable_wait(stop: &Cancel, duration: Duration) -> bool {
    unsafe {
        WaitForSingleObject(
            stop.0.0,
            duration.as_millis().min(u32::MAX as u128 - 1) as u32,
        ) == WAIT_TIMEOUT
    }
}

#[repr(C)]
struct PcapHeader {
    sec: i32,
    usec: i32,
    caplen: u32,
    len: u32,
}
#[repr(C)]
struct Bpf {
    len: u32,
    insns: *mut c_void,
}
struct PcapApi {
    _module: usize,
    open: unsafe extern "C" fn(*const i8, i32, i32, i32, *mut i8) -> *mut c_void,
    close: unsafe extern "C" fn(*mut c_void),
    next: unsafe extern "C" fn(*mut c_void, *mut *const PcapHeader, *mut *const u8) -> i32,
    send: unsafe extern "C" fn(*mut c_void, *const u8, i32) -> i32,
    compile: unsafe extern "C" fn(*mut c_void, *mut Bpf, *const i8, i32, u32) -> i32,
    filter: unsafe extern "C" fn(*mut c_void, *mut Bpf) -> i32,
    freecode: unsafe extern "C" fn(*mut Bpf),
    nonblock: unsafe extern "C" fn(*mut c_void, i32, *mut i8) -> i32,
    mintocopy: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    event: unsafe extern "C" fn(*mut c_void) -> HANDLE,
}
fn pcap_api() -> Result<Arc<PcapApi>> {
    static API: Mutex<Option<Arc<PcapApi>>> = Mutex::new(None);
    let mut cache = API.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(api) = &*cache {
        return Ok(api.clone());
    }
    let loaded = (|| -> Result<Arc<PcapApi>> {
        let mut path = [0u16; 32768];
        let n = unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                path.as_mut_ptr(),
                path.len() as u32,
            )
        };
        ensure!(
            n > 0 && n < path.len() as u32,
            "locate Windows system directory"
        );
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&path[..n as usize]))
            .join("Npcap/wpcap.dll");
        let module = unsafe {
            LoadLibraryExW(
                wide(path.as_os_str()).as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        ensure!(
            !module.is_null(),
            "Npcap is unavailable for {} ({}). Install the official Npcap package from https://npcap.com/#download; it selects the correct architecture automatically",
            std::env::consts::ARCH,
            io::Error::last_os_error()
        );
        macro_rules! symbol {
            ($name:literal,$ty:ty) => {{
                let address = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) }
                    .context(concat!("missing Npcap symbol ", $name))?;
                unsafe { mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(address) }
            }};
        }
        Ok(Arc::new(PcapApi {
            _module: module as usize,
            open: symbol!(
                "pcap_open_live",
                unsafe extern "C" fn(*const i8, i32, i32, i32, *mut i8) -> *mut c_void
            ),
            close: symbol!("pcap_close", unsafe extern "C" fn(*mut c_void)),
            next: symbol!(
                "pcap_next_ex",
                unsafe extern "C" fn(*mut c_void, *mut *const PcapHeader, *mut *const u8) -> i32
            ),
            send: symbol!(
                "pcap_sendpacket",
                unsafe extern "C" fn(*mut c_void, *const u8, i32) -> i32
            ),
            compile: symbol!(
                "pcap_compile",
                unsafe extern "C" fn(*mut c_void, *mut Bpf, *const i8, i32, u32) -> i32
            ),
            filter: symbol!(
                "pcap_setfilter",
                unsafe extern "C" fn(*mut c_void, *mut Bpf) -> i32
            ),
            freecode: symbol!("pcap_freecode", unsafe extern "C" fn(*mut Bpf)),
            nonblock: symbol!(
                "pcap_setnonblock",
                unsafe extern "C" fn(*mut c_void, i32, *mut i8) -> i32
            ),
            mintocopy: symbol!(
                "pcap_setmintocopy",
                unsafe extern "C" fn(*mut c_void, i32) -> i32
            ),
            event: symbol!("pcap_getevent", unsafe extern "C" fn(*mut c_void) -> HANDLE),
        }))
    })()?;
    *cache = Some(loaded.clone());
    Ok(loaded)
}
pub struct Capture {
    api: Arc<PcapApi>,
    raw: *mut c_void,
    event: HANDLE,
    lock: Mutex<()>,
}
unsafe impl Send for Capture {}
unsafe impl Sync for Capture {}
impl Capture {
    pub fn open(adapter: &Adapter) -> Result<Self> {
        let api = pcap_api()?;
        let name = CString::new(format!(r"\Device\NPF_{}", adapter.id))?;
        let mut error = [0i8; 256];
        let raw = unsafe {
            (api.open)(
                name.as_ptr(),
                crate::protocol::MAX_FRAME as i32,
                0,
                0,
                error.as_mut_ptr(),
            )
        };
        ensure!(
            !raw.is_null(),
            "open Npcap: {}",
            unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
        );
        let capture = Self {
            event: unsafe { (api.event)(raw) },
            raw,
            api,
            lock: Mutex::new(()),
        };
        ensure!(
            !capture.event.is_null() && capture.event != INVALID_HANDLE_VALUE,
            "Npcap returned invalid capture event"
        );
        let mut bpf = Bpf {
            len: 0,
            insns: ptr::null_mut(),
        };
        ensure!(
            unsafe {
                (capture.api.compile)(raw, &mut bpf, c"ether proto 0x888e".as_ptr(), 1, u32::MAX)
            } == 0,
            "compile EAPOL filter"
        );
        let filtered = unsafe { (capture.api.filter)(raw, &mut bpf) };
        unsafe {
            (capture.api.freecode)(&mut bpf);
        }
        ensure!(filtered == 0, "install EAPOL filter");
        ensure!(
            unsafe { (capture.api.nonblock)(raw, 1, error.as_mut_ptr()) } == 0,
            "set nonblocking capture"
        );
        ensure!(
            unsafe { (capture.api.mintocopy)(raw, 1) } == 0,
            "set immediate capture event"
        );
        Ok(capture)
    }
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        let _lock = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        ensure!(
            unsafe { (self.api.send)(self.raw, bytes.as_ptr(), bytes.len() as i32) } == 0,
            "Npcap send failed"
        );
        Ok(())
    }
    pub fn receive(&self, stop: &Cancel) -> Result<Option<Vec<u8>>> {
        loop {
            if unsafe { WaitForSingleObject(stop.0.0, 0) } == WAIT_OBJECT_0 {
                return Ok(None);
            }
            // Drain libpcap's user-space buffer before waiting on the driver event.
            // Reading a batch can reset that event while more packets remain buffered.
            {
                let _lock = self.lock.lock().unwrap_or_else(|e| e.into_inner());
                let mut header = ptr::null();
                let mut data = ptr::null();
                let result = unsafe { (self.api.next)(self.raw, &mut header, &mut data) };
                ensure!(result >= 0, "Npcap read failed");
                if result == 1 {
                    ensure!(
                        !header.is_null() && !data.is_null(),
                        "invalid captured packet"
                    );
                    let len = unsafe { (*header).caplen } as usize;
                    ensure!(
                        len <= crate::protocol::MAX_FRAME,
                        "oversized captured frame"
                    );
                    return Ok(Some(
                        unsafe { std::slice::from_raw_parts(data, len) }.to_vec(),
                    ));
                }
            }
            let handles = [stop.0.0, self.event];
            let wait = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
            if wait == WAIT_OBJECT_0 {
                return Ok(None);
            }
            ensure!(wait == WAIT_OBJECT_0 + 1, "capture event wait failed");
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.raw);
        }
    }
}

fn socket_ip(addr: SOCKET_ADDRESS) -> String {
    if addr.lpSockaddr.is_null() || addr.iSockaddrLength < (mem::size_of::<SOCKADDR_IN>()) as i32 {
        return String::new();
    }
    unsafe {
        if (*addr.lpSockaddr).sa_family != AF_INET {
            return String::new();
        }
        let v = &*(addr.lpSockaddr as *const SOCKADDR_IN);
        Ipv4Addr::from(v.sin_addr.S_un.S_addr.to_ne_bytes()).to_string()
    }
}
pub fn adapters() -> Result<Vec<Adapter>> {
    let mut size = 16384;
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    for _ in 0..3 {
        let code = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code == ERROR_BUFFER_OVERFLOW {
            buffer.resize((size as usize).div_ceil(8), 0);
            continue;
        }
        ensure!(code == 0, "GetAdaptersAddresses: {code}");
        let mut raw = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        let mut result = Vec::new();
        while !raw.is_null() {
            let a = unsafe { &*raw };
            raw = a.Next;
            if a.AdapterName.is_null() {
                continue;
            }
            let id = unsafe { CStr::from_ptr(a.AdapterName.cast()) }
                .to_string_lossy()
                .into_owned();
            let name = from_wide(a.FriendlyName);
            let driver = from_wide(a.Description);
            let mut mac = [0; 6];
            if a.PhysicalAddressLength == 6 {
                mac.copy_from_slice(&a.PhysicalAddress[..6]);
            }
            let ethernet =
                a.IfType == 6 && !virtual_name(&format!("{name} {driver}")) && !virtual_mac(mac);
            let up = a.OperStatus == 1;
            let mut ipv4 = String::new();
            let mut ip = a.FirstUnicastAddress;
            while !ip.is_null() {
                let addr = unsafe { &*ip };
                let text = socket_ip(addr.Address);
                if !text.is_empty() {
                    ipv4 = text;
                    break;
                }
                ip = addr.Next;
            }
            let gateway = if a.FirstGatewayAddress.is_null() {
                String::new()
            } else {
                socket_ip(unsafe { (*a.FirstGatewayAddress).Address })
            };
            let mut dns = Vec::new();
            let mut d = a.FirstDnsServerAddress;
            while !d.is_null() && dns.len() < 2 {
                let addr = unsafe { &*d };
                let ip = socket_ip(addr.Address);
                if !ip.is_empty() {
                    dns.push(ip);
                }
                d = addr.Next;
            }
            let score = if up { 30 } else { 0 }
                + if ethernet { 20 } else { -100 }
                + if mac != [0; 6] { 10 } else { 0 }
                + if a.TransmitLinkSpeed > 0 { 10 } else { 0 };
            result.push(Adapter {
                name,
                id,
                index: unsafe { a.Anonymous1.Anonymous.IfIndex },
                mac,
                up,
                ethernet,
                score,
                ipv4,
                gateway,
                dns: dns.join(", "),
                driver,
            });
        }
        result.sort_by_key(|a| std::cmp::Reverse(a.score));
        return Ok(result);
    }
    bail!("adapter list kept changing")
}
pub fn monitor(events: Arc<Signal>, _power: Arc<PowerNotifier>) -> Result<Monitor> {
    unsafe extern "system" fn changed(
        context: *const c_void,
        _: *const MIB_IPINTERFACE_ROW,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let notifier = unsafe { &*(context as *const Signal) };
        notifier.notify();
    }
    unsafe extern "system" fn address_changed(
        context: *const c_void,
        _: *const MIB_UNICASTIPADDRESS_ROW,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let notifier = unsafe { &*(context as *const Signal) };
        notifier.notify();
    }
    unsafe extern "system" fn route_changed(
        context: *const c_void,
        _: *const MIB_IPFORWARD_ROW2,
        _: MIB_NOTIFICATION_TYPE,
    ) {
        let notifier = unsafe { &*(context as *const Signal) };
        notifier.notify();
    }
    let stop = Arc::new(Cancel::new()?);
    let done = stop.clone();
    struct Notifications(Vec<usize>);
    impl Drop for Notifications {
        fn drop(&mut self) {
            for notification in &self.0 {
                unsafe {
                    CancelMibChangeNotify2(*notification as HANDLE);
                }
            }
        }
    }
    let mut notifications = Notifications(Vec::new());
    let context = Arc::as_ptr(&events).cast();
    let mut notification = ptr::null_mut();
    ensure!(
        unsafe {
            NotifyIpInterfaceChange(AF_UNSPEC, Some(changed), context, false, &mut notification)
        } == 0,
        "register interface notifications"
    );
    notifications.0.push(notification as usize);
    ensure!(
        unsafe {
            NotifyUnicastIpAddressChange(
                AF_UNSPEC,
                Some(address_changed),
                context,
                false,
                &mut notification,
            )
        } == 0,
        "register address notifications"
    );
    notifications.0.push(notification as usize);
    ensure!(
        unsafe {
            NotifyRouteChange2(
                AF_UNSPEC,
                Some(route_changed),
                context,
                false,
                &mut notification,
            )
        } == 0,
        "register route notifications"
    );
    notifications.0.push(notification as usize);
    let thread = std::thread::Builder::new()
        .name("adapter-notifications".into())
        .spawn(move || {
            unsafe {
                WaitForSingleObject(done.0.0, INFINITE);
            }
            drop(notifications);
            drop(events);
        })?;
    Ok(Monitor {
        stop,
        thread: Some(thread),
    })
}
pub fn ping(target: Ipv4Addr, stop: &Cancel) -> bool {
    // ICMP completion has its own event; cancellation closes the outstanding ICMP handle.
    let handle = unsafe { IcmpCreateFile() };
    if handle == INVALID_HANDLE_VALUE {
        return false;
    }
    struct Icmp(HANDLE);
    impl Drop for Icmp {
        fn drop(&mut self) {
            unsafe {
                IcmpCloseHandle(self.0);
            }
        }
    }
    let Ok(event) = Cancel::new() else {
        unsafe {
            IcmpCloseHandle(handle);
        }
        return false;
    };
    let mut reply = vec![0u64; 128];
    let payload = *b"SYSU";
    // Drop the ICMP handle before its output buffer and completion event.
    let icmp = Icmp(handle);
    let count = unsafe {
        IcmpSendEcho2(
            icmp.0,
            event.0.0,
            None,
            ptr::null(),
            u32::from_ne_bytes(target.octets()),
            payload.as_ptr().cast(),
            4,
            ptr::null(),
            reply.as_mut_ptr().cast(),
            (reply.len() * 8) as u32,
            3000,
        )
    };
    if count == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
        return false;
    }
    if count == 0 {
        let handles = [stop.0.0, event.0.0];
        if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, 3100) } != WAIT_OBJECT_0 + 1 {
            return false;
        }
        if unsafe { IcmpParseReplies(reply.as_mut_ptr().cast(), (reply.len() * 8) as u32) } == 0 {
            return false;
        }
    }
    unsafe { (*(reply.as_ptr() as *const ICMP_ECHO_REPLY)).Status == 0 }
}

pub fn https_get(url: &str, hosts: &[&str], limit: usize) -> Result<Vec<u8>> {
    use windows_sys::Win32::Networking::WinHttp::*;
    ensure!(
        crate::update::safe_url(url, hosts),
        "untrusted download URL"
    );
    let tail = url.strip_prefix("https://").context("HTTPS required")?;
    let (host, path) = tail.split_once('/').unwrap_or((tail, ""));
    let host = host.strip_suffix(":443").unwrap_or(host);
    struct Http(*mut c_void);
    impl Drop for Http {
        fn drop(&mut self) {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
    let session = Http(unsafe {
        WinHttpOpen(
            wide(concat!("SYSUNetAuth/", env!("NETAUTH_APP_VERSION"))).as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    });
    ensure!(!session.0.is_null(), "WinHTTP open");
    unsafe {
        WinHttpSetTimeouts(session.0, 8000, 8000, 8000, 8000);
    }
    let connection = Http(unsafe { WinHttpConnect(session.0, wide(host).as_ptr(), 443, 0) });
    ensure!(!connection.0.is_null(), "WinHTTP connect");
    let request = Http(unsafe {
        WinHttpOpenRequest(
            connection.0,
            wide("GET").as_ptr(),
            wide(format!("/{path}")).as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    ensure!(!request.0.is_null(), "WinHTTP request");
    let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    ensure!(
        unsafe {
            WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&policy as *const u32).cast(),
                4,
            )
        } != 0,
        "cannot disable HTTP redirects"
    );
    ensure!(
        unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) } != 0
            && unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } != 0,
        "HTTPS request failed: {}",
        io::Error::last_os_error()
    );
    let mut status = 0u32;
    let mut size = 4;
    ensure!(
        unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                ptr::null_mut(),
            )
        } != 0
            && status == 200,
        "HTTP {status}"
    );
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0u8; 8192];
        let mut n = 0;
        ensure!(
            unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut n,
                )
            } != 0,
            "HTTPS read failed"
        );
        if n == 0 {
            break;
        }
        ensure!(
            bytes.len() + n as usize <= limit,
            "download exceeds size limit"
        );
        bytes.extend_from_slice(&chunk[..n as usize]);
    }
    Ok(bytes)
}

pub fn is_admin() -> bool {
    let mut sid = ptr::null_mut();
    let authority = SID_IDENTIFIER_AUTHORITY {
        Value: [0, 0, 0, 0, 0, 5],
    };
    if unsafe { AllocateAndInitializeSid(&authority, 2, 32, 544, 0, 0, 0, 0, 0, 0, &mut sid) } == 0
    {
        return false;
    }
    let mut member = 0;
    let ok = unsafe { CheckTokenMembership(ptr::null_mut(), sid, &mut member) } != 0;
    unsafe {
        FreeSid(sid);
    }
    ok && member != 0
}
pub fn instance_lock(_paths: &Paths) -> Result<Handle> {
    let name = wide(r"Global\SYSUNetAuth-Engine-v1");
    let raw = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
    let error = unsafe { GetLastError() };
    let guard = Handle::new(raw)?;
    ensure!(
        error != ERROR_ALREADY_EXISTS,
        "another service or direct authentication process is running"
    );
    Ok(guard)
}

pub struct Pipe {
    handle: Handle,
    timeout: Option<Duration>,
}
impl Pipe {
    pub fn try_clone(&self) -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.handle.0,
                GetCurrentProcess(),
                &mut handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: Handle(handle),
            timeout: self.timeout,
        })
    }
    pub fn disconnect(&self) {
        unsafe {
            DisconnectNamedPipe(self.handle.0);
        }
    }
    pub fn connect(path: &Path) -> Result<Self> {
        let name = wide(path.as_os_str());
        unsafe {
            WaitNamedPipeW(name.as_ptr(), 3000);
        }
        let h = Handle::new(unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        })
        .context("service unavailable")?;
        // Never send credentials to an unelevated process squatting on the pipe name.
        let mut pid = 0;
        ensure!(
            unsafe { GetNamedPipeServerProcessId(h.0, &mut pid) } != 0,
            "identify service process"
        );
        let process =
            Handle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
        let mut token = ptr::null_mut();
        ensure!(
            unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } != 0,
            "query service identity"
        );
        let token = Handle::new(token)?;
        let mut elevation: TOKEN_ELEVATION = unsafe { mem::zeroed() };
        let mut len = 0;
        ensure!(
            unsafe {
                GetTokenInformation(
                    token.0,
                    TokenElevation,
                    (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                    mem::size_of_val(&elevation) as u32,
                    &mut len,
                )
            } != 0
                && elevation.TokenIsElevated != 0,
            "service pipe owner is not elevated"
        );
        Ok(Self {
            handle: h,
            timeout: Some(Duration::from_secs(10)),
        })
    }
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }
    fn transfer(&mut self, bytes: *mut u8, len: usize, write: bool) -> io::Result<usize> {
        let event = Cancel::new().map_err(io::Error::other)?;
        let mut ov: OVERLAPPED = unsafe { mem::zeroed() };
        ov.hEvent = event.0.0;
        let mut count = 0;
        let ok = unsafe {
            if write {
                WriteFile(
                    self.handle.0,
                    bytes,
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut ov,
                )
            } else {
                ReadFile(
                    self.handle.0,
                    bytes,
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut ov,
                )
            }
        };
        if ok == 0 {
            if unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(io::Error::last_os_error());
            }
            let timeout = if write {
                5000
            } else {
                self.timeout
                    .map(|d| d.as_millis().min(u32::MAX as u128 - 1) as u32)
                    .unwrap_or(INFINITE)
            };
            if unsafe { WaitForSingleObject(event.0.0, timeout) } != WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(self.handle.0, &ov);
                    GetOverlappedResult(self.handle.0, &ov, &mut count, 1);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pipe operation timed out",
                ));
            }
            if unsafe { GetOverlappedResult(self.handle.0, &ov, &mut count, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(count as usize)
    }
}
impl Read for Pipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.transfer(bytes.as_mut_ptr(), bytes.len(), false)
    }
}
impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.transfer(bytes.as_ptr() as *mut u8, bytes.len(), true)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub struct PipeListener {
    name: Vec<u16>,
    first: Mutex<Option<Handle>>,
}
impl PipeListener {
    pub fn bind(paths: &Paths) -> Result<Self> {
        let listener = Self {
            name: wide(paths.endpoint.as_os_str()),
            first: Mutex::new(None),
        };
        *listener.first.lock().unwrap() = Some(listener.create(true)?);
        Ok(listener)
    }
    fn create(&self, first: bool) -> Result<Handle> {
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        let mut descriptor = ptr::null_mut();
        ensure!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)").as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            } != 0,
            "pipe security descriptor"
        );
        let sa = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let h = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | FILE_FLAG_OVERLAPPED
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                17,
                65536,
                65536,
                5000,
                &sa,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        Handle::new(h)
    }
    pub fn accept(&self) -> Result<Pipe> {
        let handle = match self.first.lock().unwrap().take() {
            Some(h) => h,
            None => self.create(false)?,
        };
        let event = Cancel::new()?;
        let mut ov: OVERLAPPED = unsafe { mem::zeroed() };
        ov.hEvent = event.0.0;
        if unsafe { ConnectNamedPipe(handle.0, &mut ov) } == 0 {
            let error = unsafe { GetLastError() };
            ensure!(
                error == ERROR_IO_PENDING || error == ERROR_PIPE_CONNECTED,
                "connect pipe: {error}"
            );
            if error == ERROR_IO_PENDING {
                ensure!(
                    unsafe { WaitForSingleObject(event.0.0, INFINITE) } == WAIT_OBJECT_0,
                    "wait pipe connect"
                );
                let mut bytes = 0;
                ensure!(
                    unsafe { GetOverlappedResult(handle.0, &ov, &mut bytes, 0) } != 0,
                    "pipe connect completion"
                );
            }
        }
        Ok(Pipe {
            handle,
            timeout: Some(Duration::from_secs(5)),
        })
    }
}

static STOP: OnceLock<Arc<Signal>> = OnceLock::new();
static POWER: OnceLock<Arc<PowerNotifier>> = OnceLock::new();
static SCM: OnceLock<usize> = OnceLock::new();
fn report_service(state: u32, code: u32) {
    if let Some(handle) = SCM.get() {
        let pending = state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING;
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_POWEREVENT
            } else {
                0
            },
            dwWin32ExitCode: code,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: if pending { 1 } else { 0 },
            dwWaitHint: if pending { 10000 } else { 0 },
        };
        unsafe {
            SetServiceStatus(*handle as SERVICE_STATUS_HANDLE, &status);
        }
    }
}
pub fn service_ready() {
    report_service(SERVICE_RUNNING, 0);
}
pub fn register_stop_notifier(tx: Arc<Signal>) {
    let _ = STOP.set(tx);
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(console_stop), 1);
    }
}
pub fn register_power_notifier(power: Arc<PowerNotifier>) {
    let _ = POWER.set(power);
}
unsafe extern "system" fn console_stop(_: u32) -> i32 {
    if let Some(tx) = STOP.get() {
        tx.notify();
    }
    1
}
unsafe extern "system" fn control(
    code: u32,
    event_type: u32,
    _: *mut c_void,
    _: *mut c_void,
) -> u32 {
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        report_service(SERVICE_STOP_PENDING, 0);
        if let Some(tx) = STOP.get() {
            tx.notify();
        }
    }
    if code == SERVICE_CONTROL_POWEREVENT {
        if let Some(power) = POWER.get() {
            match event_type {
                PBT_APMSUSPEND => power.notify(PowerEvent::Suspend),
                // This arrives for unattended wakes too. RESUMESUSPEND is a
                // later user-presence notification; handling both would reset
                // an already recovering connection a second time.
                PBT_APMRESUMEAUTOMATIC => power.notify(PowerEvent::Resume),
                _ => {}
            }
        }
    }
    0
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let name = wide("SYSUNetAuth");
    let status =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), ptr::null()) };
    if status.is_null() {
        return;
    }
    let _ = SCM.set(status as usize);
    report_service(SERVICE_START_PENDING, 0);
    let result = crate::service::run(Paths::default());
    if let Err(e) = &result {
        eprintln!("service: {e:#}");
    }
    report_service(SERVICE_STOPPED, if result.is_ok() { 0 } else { 1 });
}
pub fn run_service_dispatcher() -> Result<()> {
    let mut name = wide("SYSUNetAuth");
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    ensure!(
        unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } != 0,
        "SCM dispatcher: {} (use --console for foreground mode)",
        io::Error::last_os_error()
    );
    Ok(())
}
pub fn service_control(start: bool) -> Result<()> {
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    ensure!(!manager.is_null(), "open service manager");
    struct Sc(SC_HANDLE);
    impl Drop for Sc {
        fn drop(&mut self) {
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
    let manager = Sc(manager);
    let service = unsafe {
        OpenServiceW(
            manager.0,
            wide("SYSUNetAuth").as_ptr(),
            SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS | SERVICE_CHANGE_CONFIG,
        )
    };
    ensure!(!service.is_null(), "SYSUNetAuth service is not installed");
    let service = Sc(service);
    let ok = if start {
        unsafe { StartServiceW(service.0, 0, ptr::null()) }
    } else {
        let mut status: SERVICE_STATUS = unsafe { mem::zeroed() };
        unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) }
    };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        ensure!(
            code == ERROR_SERVICE_ALREADY_RUNNING || code == ERROR_SERVICE_NOT_ACTIVE,
            "service control: {code}"
        );
    }
    Ok(())
}
pub fn install_service() -> Result<()> {
    ensure!(
        is_admin(),
        "service installation requires administrator privileges"
    );
    struct Sc(SC_HANDLE);
    impl Drop for Sc {
        fn drop(&mut self) {
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
    let manager = unsafe {
        OpenSCManagerW(
            ptr::null(),
            ptr::null(),
            SC_MANAGER_CREATE_SERVICE | SC_MANAGER_CONNECT,
        )
    };
    ensure!(!manager.is_null(), "open service manager");
    let manager = Sc(manager);
    let name = wide("SYSUNetAuth");
    let path = wide(format!("\"{}\"", std::env::current_exe()?.display()));
    let automatic = crate::config::read::<serde_json::Value>(&Paths::default().config)
        .ok()
        .and_then(|v| v["service_mode"].as_bool())
        .unwrap_or(true);
    let start = if automatic {
        SERVICE_AUTO_START
    } else {
        SERVICE_DEMAND_START
    };
    let raw = unsafe {
        CreateServiceW(
            manager.0,
            name.as_ptr(),
            wide("SYSU NetAuth").as_ptr(),
            SERVICE_CHANGE_CONFIG,
            SERVICE_WIN32_OWN_PROCESS,
            start,
            SERVICE_ERROR_NORMAL,
            path.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    };
    let service = if raw.is_null() {
        ensure!(
            unsafe { GetLastError() } == ERROR_SERVICE_EXISTS,
            "create service: {}",
            io::Error::last_os_error()
        );
        let raw = unsafe { OpenServiceW(manager.0, name.as_ptr(), SERVICE_CHANGE_CONFIG) };
        ensure!(!raw.is_null(), "open installed service");
        let service = Sc(raw);
        ensure!(
            unsafe {
                ChangeServiceConfigW(
                    service.0,
                    SERVICE_WIN32_OWN_PROCESS,
                    start,
                    SERVICE_ERROR_NORMAL,
                    path.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                    wide("").as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                )
            } != 0,
            "update service configuration"
        );
        service
    } else {
        Sc(raw)
    };
    let mut description = wide("SYSU wired campus network 802.1X authentication service");
    let info = SERVICE_DESCRIPTIONW {
        lpDescription: description.as_mut_ptr(),
    };
    ensure!(
        unsafe {
            ChangeServiceConfig2W(
                service.0,
                SERVICE_CONFIG_DESCRIPTION,
                (&info as *const SERVICE_DESCRIPTIONW).cast(),
            )
        } != 0,
        "set service description"
    );
    Ok(())
}
pub fn set_login_task(enabled: bool) -> Result<()> {
    // Administrative setup operation only; never used by the steady-state engine.
    let exe = std::env::current_exe()?.with_file_name("sysu_netauth_gui.exe");
    let mut command = std::process::Command::new("schtasks.exe");
    if enabled {
        command.args([
            "/Create",
            "/TN",
            "SYSUNetAuth GUI",
            "/TR",
            &format!("\"{}\" --startup", exe.display()),
            "/SC",
            "ONLOGON",
            "/RL",
            "HIGHEST",
            "/IT",
            "/F",
        ]);
    } else {
        command.args(["/Delete", "/TN", "SYSUNetAuth GUI", "/F"]);
    }
    let status = command.output()?;
    ensure!(
        status.status.success() || !enabled,
        "cannot configure login task: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    Ok(())
}
pub fn open_url(url: &str) {
    if !crate::update::safe_url(
        url,
        &["github.com", "gitee.com", "npcap.com", "inc.sysu.edu.cn"],
    ) {
        return;
    }
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            ptr::null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        );
    }
}
pub fn show_error(message: &str) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            ptr::null_mut(),
            wide(message).as_ptr(),
            wide("SYSU NetAuth").as_ptr(),
            0x10,
        );
    }
}

pub fn secure_directory(path: &Path) -> Result<()> {
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    std::fs::create_dir_all(path)?;
    let mut descriptor = ptr::null_mut();
    ensure!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)").as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } != 0,
        "data security descriptor"
    );
    let ok = unsafe {
        SetFileSecurityW(
            wide(path.as_os_str()).as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    ensure!(
        ok != 0,
        "restrict data directory: {}",
        io::Error::last_os_error()
    );
    Ok(())
}
pub fn configure_service(automatic: bool) -> Result<()> {
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    ensure!(!manager.is_null(), "service manager unavailable");
    let service =
        unsafe { OpenServiceW(manager, wide("SYSUNetAuth").as_ptr(), SERVICE_CHANGE_CONFIG) };
    let ok = if service.is_null() {
        0
    } else {
        unsafe {
            ChangeServiceConfigW(
                service,
                SERVICE_NO_CHANGE,
                if automatic {
                    SERVICE_AUTO_START
                } else {
                    SERVICE_DEMAND_START
                },
                SERVICE_NO_CHANGE,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
            )
        }
    };
    unsafe {
        if !service.is_null() {
            CloseServiceHandle(service);
        }
        CloseServiceHandle(manager);
    }
    ensure!(ok != 0, "cannot change service startup type");
    Ok(())
}
pub fn install_npcap() -> Result<()> {
    use windows_sys::Win32::Security::WinTrust::*;
    let bytes = https_get(
        "https://npcap.com/dist/npcap-1.88.exe",
        &["npcap.com"],
        100 * 1024 * 1024,
    )?;
    ensure!(bytes.len() >= 1024 * 1024, "Npcap installer is incomplete");
    let path = std::env::temp_dir().join(format!("SYSUNetAuth-Npcap-{}", std::process::id()));
    secure_directory(&path)?;
    let installer = path.join("npcap-1.88.exe");
    std::fs::write(&installer, bytes)?;
    let name = wide(installer.as_os_str());
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: name.as_ptr(),
        hFile: ptr::null_mut(),
        pgKnownSubject: ptr::null_mut(),
    };
    let mut trust: WINTRUST_DATA = unsafe { mem::zeroed() };
    trust.cbStruct = mem::size_of::<WINTRUST_DATA>() as u32;
    trust.dwUIChoice = WTD_UI_NONE;
    trust.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    trust.dwUnionChoice = WTD_CHOICE_FILE;
    trust.Anonymous.pFile = &mut file;
    trust.dwStateAction = WTD_STATEACTION_VERIFY;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let result = unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        );
    }
    ensure!(
        result == 0,
        "Npcap installer signature validation failed ({result:#x})"
    );
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            ptr::null_mut(),
            wide("runas").as_ptr(),
            name.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };
    ensure!(result as usize > 32, "could not launch Npcap installer");
    Ok(())
}
