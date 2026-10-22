use super::{Adapter, Monitor, virtual_mac, virtual_name};
use crate::{engine::Signal, protocol::MAX_FRAME};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{self, Read},
    mem,
    net::Ipv4Addr,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::Path,
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};

fn fd(raw: i32) -> Result<OwnedFd> {
    if raw < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

pub struct Cancel(OwnedFd);
impl Cancel {
    pub fn new() -> Result<Self> {
        Ok(Self(fd(unsafe {
            libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)
        })?))
    }
    pub fn set(&self) {
        let bytes = 1u64.to_ne_bytes();
        unsafe {
            libc::write(self.0.as_raw_fd(), bytes.as_ptr().cast(), 8);
        }
    }
}
fn wait(raw: i32, stop: &Cancel) -> Result<bool> {
    let mut fds = [
        libc::pollfd {
            fd: raw,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stop.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e.into());
        }
        if fds[1].revents != 0 {
            return Ok(false);
        }
        ensure!(
            fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0,
            "network handle closed"
        );
        if fds[0].revents & libc::POLLIN != 0 {
            return Ok(true);
        }
    }
}
pub struct Capture(OwnedFd);
impl Capture {
    pub fn open(adapter: &Adapter) -> Result<Self> {
        let socket = fd(unsafe {
            libc::socket(
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                (0x888eu16.to_be()) as i32,
            )
        })
        .context("open EAPOL socket (root or CAP_NET_RAW is required)")?;
        let address = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as u16,
            sll_protocol: 0x888eu16.to_be(),
            sll_ifindex: adapter.index as i32,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        };
        ensure!(
            unsafe {
                libc::bind(
                    socket.as_raw_fd(),
                    (&address as *const libc::sockaddr_ll).cast(),
                    mem::size_of_val(&address) as u32,
                )
            } == 0,
            "bind EAPOL: {}",
            io::Error::last_os_error()
        );
        // Join only the 802.1X PAE group; do not enable promiscuous capture.
        let mut membership: libc::packet_mreq = unsafe { mem::zeroed() };
        membership.mr_ifindex = adapter.index as i32;
        membership.mr_type = libc::PACKET_MR_MULTICAST as u16;
        membership.mr_alen = 6;
        membership.mr_address[..6].copy_from_slice(&crate::protocol::PAE);
        ensure!(
            unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_PACKET,
                    libc::PACKET_ADD_MEMBERSHIP,
                    (&membership as *const libc::packet_mreq).cast(),
                    mem::size_of_val(&membership) as u32,
                )
            } == 0,
            "join PAE multicast: {}",
            io::Error::last_os_error()
        );
        Ok(Self(socket))
    }
    pub fn send(&self, bytes: &[u8]) -> Result<()> {
        let sent = unsafe { libc::send(self.0.as_raw_fd(), bytes.as_ptr().cast(), bytes.len(), 0) };
        ensure!(
            sent == bytes.len() as isize,
            "send EAPOL: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }
    pub fn receive(&self, stop: &Cancel) -> Result<Option<Vec<u8>>> {
        if !wait(self.0.as_raw_fd(), stop)? {
            return Ok(None);
        }
        let mut bytes = vec![0; MAX_FRAME];
        let n = unsafe {
            libc::recv(
                self.0.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
            )
        };
        ensure!(n >= 0, "receive EAPOL: {}", io::Error::last_os_error());
        bytes.truncate(n as usize);
        Ok(Some(bytes))
    }
}

pub fn adapters() -> Result<Vec<Adapter>> {
    let mut head = ptr::null_mut();
    ensure!(
        unsafe { libc::getifaddrs(&mut head) } == 0,
        "getifaddrs: {}",
        io::Error::last_os_error()
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
        let family = unsafe { (*item.ifa_addr).sa_family } as i32;
        let i = match result.iter().position(|a| a.name == name) {
            Some(i) => i,
            None => {
                result.push(Adapter {
                    name: name.clone(),
                    id: name.clone(),
                    index: unsafe { libc::if_nametoindex(item.ifa_name) },
                    ..Adapter::default()
                });
                result.len() - 1
            }
        };
        let a = &mut result[i];
        let base = Path::new("/sys/class/net").join(&name);
        a.up = item.ifa_flags & libc::IFF_RUNNING as u32 != 0
            && item.ifa_flags & libc::IFF_UP as u32 != 0;
        if family == libc::AF_PACKET {
            let addr = unsafe { &*(item.ifa_addr as *const libc::sockaddr_ll) };
            if addr.sll_halen == 6 {
                a.mac.copy_from_slice(&addr.sll_addr[..6]);
            }
            a.ethernet = addr.sll_hatype == 1
                && !base.join("wireless").exists()
                && (base.join("device").exists()
                    || std::env::var_os("NETAUTH_TEST_VETH").is_some())
                && !virtual_name(&name)
                && !virtual_mac(a.mac);
            a.driver = fs::read_link(base.join("device/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_default();
        } else if family == libc::AF_INET {
            let addr = unsafe { &*(item.ifa_addr as *const libc::sockaddr_in) };
            a.ipv4 = Ipv4Addr::from(addr.sin_addr.s_addr.to_ne_bytes()).to_string();
        }
        a.score = if a.up { 30 } else { 0 }
            + if a.ethernet { 20 } else { -100 }
            + if a.mac != [0; 6] { 10 } else { 0 };
    }
    if let Ok(routes) = fs::read_to_string("/proc/net/route") {
        for line in routes.lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() > 3 && fields[1] == "00000000" {
                if let (Some(a), Ok(gateway)) = (
                    result.iter_mut().find(|a| a.name == fields[0]),
                    u32::from_str_radix(fields[2], 16),
                ) {
                    a.gateway = Ipv4Addr::from(gateway.to_le_bytes()).to_string();
                }
            }
        }
    }
    let dns = fs::read_to_string("/etc/resolv.conf")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("nameserver ").map(str::trim))
        .take(2)
        .collect::<Vec<_>>()
        .join(", ");
    for a in &mut result {
        a.dns = dns.clone();
    }
    result.sort_by_key(|a| std::cmp::Reverse(a.score));
    Ok(result)
}

pub fn monitor(events: Arc<Signal>) -> Result<Monitor> {
    let socket = fd(unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        )
    })?;
    let mut addr: libc::sockaddr_nl = unsafe { mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as u16;
    addr.nl_groups =
        (libc::RTMGRP_LINK | libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV4_ROUTE) as u32;
    ensure!(
        unsafe {
            libc::bind(
                socket.as_raw_fd(),
                (&addr as *const libc::sockaddr_nl).cast(),
                mem::size_of_val(&addr) as u32,
            )
        } == 0,
        "bind rtnetlink: {}",
        io::Error::last_os_error()
    );
    let stop = Arc::new(Cancel::new()?);
    let done = stop.clone();
    let thread = std::thread::Builder::new()
        .name("adapter-notifications".into())
        .spawn(move || {
            let mut bytes = [0u8; 8192];
            while wait(socket.as_raw_fd(), &done).unwrap_or(false) {
                unsafe {
                    libc::recv(
                        socket.as_raw_fd(),
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                        0,
                    );
                }
                events.notify();
            }
        })?;
    Ok(Monitor {
        stop,
        thread: Some(thread),
    })
}

pub fn https_get(url: &str, hosts: &[&str], limit: usize) -> Result<Vec<u8>> {
    ensure!(
        crate::update::safe_url(url, hosts),
        "untrusted download URL"
    );
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let response = agent
        .get(url)
        .header(
            "User-Agent",
            concat!("SYSUNetAuth/", env!("NETAUTH_APP_VERSION")),
        )
        .call()?;
    ensure!(
        response.status().as_u16() == 200,
        "HTTP {}",
        response.status()
    );
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "download exceeds size limit");
    Ok(bytes)
}

fn checksum(bytes: &[u8]) -> u16 {
    let sum = bytes.chunks(2).fold(0u32, |sum, c| {
        sum + u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32
    });
    let sum = (sum & 0xffff) + (sum >> 16);
    !((sum & 0xffff) + (sum >> 16)) as u16
}
pub fn ping(target: Ipv4Addr, stop: &Cancel) -> bool {
    let Ok(socket) = fd(unsafe {
        libc::socket(
            libc::AF_INET,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::IPPROTO_ICMP,
        )
    }) else {
        return false;
    };
    let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    addr.sin_family = libc::AF_INET as u16;
    addr.sin_addr.s_addr = u32::from_ne_bytes(target.octets());
    let id = (std::process::id() as u16).to_be_bytes();
    let mut packet = [8, 0, 0, 0, id[0], id[1], 0, 1];
    let check = checksum(&packet);
    packet[2..4].copy_from_slice(&check.to_be_bytes());
    if unsafe {
        libc::sendto(
            socket.as_raw_fd(),
            packet.as_ptr().cast(),
            packet.len(),
            0,
            (&addr as *const libc::sockaddr_in).cast(),
            mem::size_of_val(&addr) as u32,
        )
    } < 0
    {
        return false;
    }
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        let mut fds = [
            libc::pollfd {
                fd: socket.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop.0.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                2,
                until
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(3000) as i32,
            )
        } <= 0
            || fds[1].revents != 0
        {
            return false;
        }
        let mut bytes = [0; 2048];
        let n = unsafe {
            libc::recv(
                socket.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
            )
        };
        if n >= 28 {
            let iplen = (bytes[0] & 15) as usize * 4;
            if iplen + 8 <= n as usize
                && bytes[iplen] == 0
                && bytes[iplen + 4..iplen + 6] == id
                && bytes[12..16] == target.octets()
            {
                return true;
            }
        }
    }
    false
}
pub fn cancellable_wait(stop: &Cancel, duration: Duration) -> bool {
    let mut p = libc::pollfd {
        fd: stop.0.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut p, 1, duration.as_millis().min(i32::MAX as u128) as i32) == 0 }
}

pub fn signal_thread(events: Arc<Signal>) -> Result<()> {
    let mut set: libc::sigset_t = unsafe { mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
    }
    ensure!(
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut()) } == 0,
        "block termination signals"
    );
    let socket = fd(unsafe { libc::signalfd(-1, &set, libc::SFD_CLOEXEC) })?;
    std::thread::Builder::new()
        .name("shutdown-signal".into())
        .spawn(move || {
            let mut info = [0u8; 128];
            unsafe {
                libc::read(socket.as_raw_fd(), info.as_mut_ptr().cast(), info.len());
            }
            events.notify();
        })?;
    Ok(())
}
pub fn is_admin() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn instance_lock(paths: &crate::config::Paths) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = paths.endpoint.parent().context("lock parent")?;
    crate::config::private_dir(parent)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(parent.join("instance.lock"))?;
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "another service or direct authentication process is running"
    );
    Ok(file)
}
