use super::unix::{fd, wait_readable};
use super::{Adapter, Monitor, virtual_mac, virtual_name};
use crate::{engine::Signal, protocol::MAX_FRAME};
use anyhow::{Context, Result, ensure};
use std::{
    fs, io, mem,
    net::Ipv4Addr,
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
    ptr,
    sync::Arc,
    time::Duration,
};

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
    pub(super) fn raw(&self) -> i32 {
        self.0.as_raw_fd()
    }
}
fn wait(raw: i32, stop: &Cancel) -> Result<bool> {
    wait_readable(raw, stop, None)
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

pub fn monitor(events: Arc<Signal>, _power: Arc<crate::engine::PowerNotifier>) -> Result<Monitor> {
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

pub fn cancellable_wait(stop: &Cancel, duration: Duration) -> bool {
    let mut p = libc::pollfd {
        fd: stop.0.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut p, 1, duration.as_millis().min(i32::MAX as u128) as i32) == 0 }
}
