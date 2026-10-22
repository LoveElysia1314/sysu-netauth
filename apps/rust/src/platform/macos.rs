use super::{
    Adapter, Monitor, bpf,
    unix::{fd, wait_readable},
    virtual_mac, virtual_name,
};
use crate::{
    engine::{PowerNotifier, Signal},
    protocol::{MAX_FRAME, PAE},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io, mem,
    net::Ipv4Addr,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
mod https;
mod system;
pub use https::https_get;

pub struct Cancel {
    reader: UnixStream,
    writer: UnixStream,
    signalled: AtomicBool,
}
impl Cancel {
    pub fn new() -> Result<Self> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        Ok(Self {
            reader,
            writer,
            signalled: AtomicBool::new(false),
        })
    }
    pub fn set(&self) {
        if !self.signalled.swap(true, Ordering::AcqRel) {
            let byte = 1u8;
            loop {
                let result =
                    unsafe { libc::write(self.writer.as_raw_fd(), (&byte as *const u8).cast(), 1) };
                if result >= 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
    pub(super) fn raw(&self) -> i32 {
        self.reader.as_raw_fd()
    }
}
pub fn cancellable_wait(stop: &Cancel, duration: Duration) -> bool {
    let mut p = libc::pollfd {
        fd: stop.raw(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut p, 1, duration.as_millis().min(i32::MAX as u128) as i32) == 0 }
}

fn interface_request(name: &str) -> Result<libc::ifreq> {
    ensure!(
        !name.as_bytes().contains(&0) && name.len() < libc::IFNAMSIZ,
        "invalid interface name"
    );
    let mut request: libc::ifreq = unsafe { mem::zeroed() };
    for (out, byte) in request.ifr_name.iter_mut().zip(name.bytes()) {
        *out = byte as _;
    }
    Ok(request)
}

// Balance the multicast reference when the capture is closed.
struct Membership {
    socket: OwnedFd,
    name: String,
}
impl Membership {
    fn join(name: &str) -> Result<Self> {
        let socket = fd(unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) })?;
        ensure!(
            unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
            "mark multicast socket close-on-exec"
        );
        let request = Self::request(name)?;
        ensure!(
            unsafe { libc::ioctl(socket.as_raw_fd(), libc::SIOCADDMULTI, &request) } == 0,
            "join PAE multicast: {}",
            io::Error::last_os_error()
        );
        Ok(Self {
            socket,
            name: name.into(),
        })
    }
    fn request(name: &str) -> Result<libc::ifreq> {
        let mut request = interface_request(name)?;
        let mut address: libc::sockaddr_dl = unsafe { mem::zeroed() };
        address.sdl_len = 16;
        address.sdl_family = libc::AF_LINK as u8;
        address.sdl_alen = 6;
        for (out, byte) in address.sdl_data.iter_mut().zip(PAE) {
            *out = byte as _;
        }
        // ifreq's address union is only 16 bytes; sockaddr_dl's useful prefix is 14.
        unsafe {
            ptr::copy_nonoverlapping(
                (&address as *const libc::sockaddr_dl).cast::<u8>(),
                (&mut request.ifr_ifru as *mut libc::__c_anonymous_ifr_ifru).cast::<u8>(),
                16,
            );
        }
        Ok(request)
    }
}
impl Drop for Membership {
    fn drop(&mut self) {
        if let Ok(request) = Self::request(&self.name) {
            unsafe {
                libc::ioctl(self.socket.as_raw_fd(), libc::SIOCDELMULTI, &request);
            }
        }
    }
}

struct ReadBuffer {
    bytes: Vec<u8>,
    used: usize,
    offset: usize,
}
pub struct Capture {
    device: File,
    buffer: Mutex<ReadBuffer>,
    _membership: Membership,
}
impl Capture {
    pub fn open(adapter: &Adapter) -> Result<Self> {
        let mut device = None;
        for index in 0..1024 {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/bpf{index}"))
            {
                Ok(file) => {
                    device = Some(file);
                    break;
                }
                Err(e) if e.raw_os_error() == Some(libc::EBUSY) => continue,
                Err(e) => return Err(e).context("open BPF (root privileges are required)"),
            }
        }
        let device = device.context("no free BPF device")?;
        let raw = device.as_raw_fd();
        let request = interface_request(&adapter.name)?;
        let mut size = 32768u32;
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCSBLEN, &mut size) } == 0,
            "set BPF buffer size"
        );
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCSETIF, &request) } == 0,
            "bind BPF to {}: {}",
            adapter.name,
            io::Error::last_os_error()
        );
        let mut link = 0u32;
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCGDLT, &mut link) } == 0 && link == libc::DLT_EN10MB,
            "BPF interface is not Ethernet"
        );
        let yes = 1u32;
        let no = 0u32;
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCIMMEDIATE, &yes) } == 0,
            "enable immediate BPF delivery"
        );
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCSHDRCMPLT, &yes) } == 0,
            "preserve Ethernet headers"
        );
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCSSEESENT, &no) } == 0,
            "disable BPF outgoing echoes"
        );
        let mut instructions = [
            libc::bpf_insn {
                code: 0x28,
                jt: 0,
                jf: 0,
                k: 12,
            }, // ldh [12]
            libc::bpf_insn {
                code: 0x15,
                jt: 0,
                jf: 1,
                k: 0x888e,
            },
            libc::bpf_insn {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: MAX_FRAME as u32,
            },
            libc::bpf_insn {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: 0,
            },
        ];
        let program = libc::bpf_program {
            bf_len: instructions.len() as u32,
            bf_insns: instructions.as_mut_ptr(),
        };
        let set_filter = 0x80000000u64 | ((mem::size_of_val(&program) as u64) << 16) | 0x4267;
        ensure!(
            unsafe { libc::ioctl(raw, set_filter as libc::c_ulong, &program) } == 0,
            "install EAPOL BPF filter"
        );
        ensure!(
            unsafe { libc::ioctl(raw, libc::BIOCGBLEN, &mut size) } == 0
                && (MAX_FRAME as u32..=1024 * 1024).contains(&size),
            "invalid BPF buffer size"
        );
        ensure!(
            unsafe { libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) } == 0,
            "set nonblocking BPF"
        );
        let membership = Membership::join(&adapter.name)?;
        Ok(Self {
            device,
            buffer: Mutex::new(ReadBuffer {
                bytes: vec![0; size as usize],
                used: 0,
                offset: 0,
            }),
            _membership: membership,
        })
    }
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        let n = unsafe { libc::write(self.device.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        ensure!(
            n == bytes.len() as isize,
            "send EAPOL: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }
    pub fn receive(&self, stop: &Cancel) -> Result<Option<Vec<u8>>> {
        let mut buffer = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if stop.signalled.load(Ordering::Acquire) {
                return Ok(None);
            }
            let ReadBuffer {
                bytes,
                used,
                offset,
            } = &mut *buffer;
            if let Some(frame) = bpf::next_frame(&bytes[..*used], offset)? {
                return Ok(Some(frame.to_vec()));
            }
            if !wait_readable(self.device.as_raw_fd(), stop, None)? {
                return Ok(None);
            }
            let n = unsafe {
                libc::read(
                    self.device.as_raw_fd(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                )
            };
            if n < 0 {
                let error = io::Error::last_os_error();
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return Err(error.into());
            }
            ensure!(n > 0, "BPF device closed");
            *used = n as usize;
            *offset = 0;
        }
    }
}

pub fn adapters() -> Result<Vec<Adapter>> {
    let hardware = system::ethernet_interfaces()?;
    let mut head = ptr::null_mut();
    ensure!(
        unsafe { libc::getifaddrs(&mut head) } == 0,
        "getifaddrs failed"
    );
    struct List(*mut libc::ifaddrs);
    impl Drop for List {
        fn drop(&mut self) {
            unsafe {
                libc::freeifaddrs(self.0);
            }
        }
    }
    let _list = List(head);
    let mut result: Vec<Adapter> = Vec::new();
    let mut current = head;
    while !current.is_null() {
        let item = unsafe { &*current };
        current = item.ifa_next;
        if item.ifa_addr.is_null() {
            continue;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(item.ifa_name) }
            .to_string_lossy()
            .into_owned();
        let index = match result.iter().position(|a| a.name == name) {
            Some(i) => i,
            None => {
                result.push(Adapter {
                    id: name.clone(),
                    name: name.clone(),
                    index: unsafe { libc::if_nametoindex(item.ifa_name) },
                    ..Adapter::default()
                });
                result.len() - 1
            }
        };
        let adapter = &mut result[index];
        adapter.up = item.ifa_flags & (libc::IFF_UP | libc::IFF_RUNNING) as u32
            == (libc::IFF_UP | libc::IFF_RUNNING) as u32;
        match unsafe { (*item.ifa_addr).sa_family } as i32 {
            libc::AF_LINK => {
                let address = unsafe { &*(item.ifa_addr as *const libc::sockaddr_dl) };
                let offset = address.sdl_nlen as usize;
                // sockaddr_dl is variable length; validate against its kernel length.
                let base = mem::offset_of!(libc::sockaddr_dl, sdl_data);
                if address.sdl_alen == 6 && base + offset + 6 <= address.sdl_len as usize {
                    let data = unsafe {
                        std::slice::from_raw_parts(item.ifa_addr.cast::<u8>().add(base + offset), 6)
                    };
                    adapter.mac.copy_from_slice(data);
                }
                adapter.ethernet = hardware.contains_key(&name)
                    && !virtual_name(&name)
                    && !virtual_mac(adapter.mac);
                adapter.driver = hardware.get(&name).cloned().unwrap_or_default();
            }
            libc::AF_INET => {
                let address = unsafe { &*(item.ifa_addr as *const libc::sockaddr_in) };
                adapter.ipv4 = Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()).to_string();
            }
            _ => {}
        }
        adapter.score = if adapter.up { 30 } else { 0 } + if adapter.ethernet { 20 } else { -100 };
    }
    system::network_details(&mut result);
    result.sort_by_key(|a| std::cmp::Reverse(a.score));
    Ok(result)
}

pub fn monitor(events: Arc<Signal>, power: Arc<PowerNotifier>) -> Result<Monitor> {
    system::monitor(events, power)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_latched_and_releases_waits() {
        let stop = Arc::new(Cancel::new().unwrap());
        let waiter = stop.clone();
        let thread = std::thread::spawn(move || cancellable_wait(&waiter, Duration::from_secs(10)));
        for _ in 0..1000 {
            stop.set();
        }
        assert!(!thread.join().unwrap());
        assert!(!cancellable_wait(&stop, Duration::from_millis(1)));
    }
    #[test]
    fn enumeration_uses_real_ethernet_types() {
        let adapters = adapters().unwrap();
        assert!(adapters.iter().any(|a| a.name == "lo0"));
        assert!(!adapters.iter().find(|a| a.name == "lo0").unwrap().ethernet);
    }
}
