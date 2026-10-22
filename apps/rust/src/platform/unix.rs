use super::Cancel;
use crate::engine::Signal;
use anyhow::{Context, Result, ensure};
use std::{
    fs, io, mem,
    net::Ipv4Addr,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) fn fd(raw: i32) -> Result<OwnedFd> {
    if raw < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

pub(super) fn wait_readable(raw: i32, stop: &Cancel, timeout: Option<Duration>) -> Result<bool> {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        let mut fds = [
            libc::pollfd {
                fd: raw,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop.raw(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let milliseconds = deadline.map_or(-1, |d| {
            d.saturating_duration_since(Instant::now())
                .as_millis()
                .min(i32::MAX as u128) as i32
        });
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, milliseconds) };
        if n < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if n == 0 || fds[1].revents != 0 {
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

#[cfg(target_os = "linux")]
pub fn https_get(url: &str, hosts: &[&str], limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    ensure!(
        crate::update::safe_url(url, hosts),
        "untrusted download URL"
    );
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .build();
    let response = ureq::Agent::new_with_config(config)
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
    #[cfg(target_os = "linux")]
    let kind = libc::SOCK_RAW | libc::SOCK_CLOEXEC;
    #[cfg(target_os = "macos")]
    let kind = libc::SOCK_RAW;
    let Ok(socket) = fd(unsafe { libc::socket(libc::AF_INET, kind, libc::IPPROTO_ICMP) }) else {
        return false;
    };
    #[cfg(target_os = "macos")]
    if unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return false;
    }
    let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    #[cfg(target_os = "macos")]
    {
        addr.sin_len = mem::size_of_val(&addr) as u8;
    }
    addr.sin_family = libc::AF_INET as _;
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
        // Both backends wake this wait through their cancellation descriptor.
        if !wait_readable(
            socket.as_raw_fd(),
            stop,
            Some(until.saturating_duration_since(Instant::now())),
        )
        .unwrap_or(false)
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
            if iplen >= 20
                && iplen + 8 <= n as usize
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
    std::thread::Builder::new()
        .name("shutdown-signal".into())
        .spawn(move || {
            let mut signal = 0;
            if unsafe { libc::sigwait(&set, &mut signal) } == 0 {
                events.notify();
            }
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
