use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, mpsc::SyncSender};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("Only Windows and Linux are supported");

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub id: String,
    pub index: u32,
    pub mac: [u8; 6],
    pub up: bool,
    pub ethernet: bool,
    pub score: i32,
    pub ipv4: String,
    pub gateway: String,
    pub dns: String,
    pub driver: String,
}
impl Adapter {
    pub fn candidate(&self) -> bool {
        self.ethernet && self.up && self.mac != [0; 6]
    }
}

pub fn virtual_name(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "loopback",
        "virtual",
        "vmware",
        "virtualbox",
        "hyper-v",
        "vethernet",
        "docker",
        "veth",
        "bridge",
        "tap",
        "tun",
        "tailscale",
        "wireguard",
        "vpn",
    ]
    .iter()
    .any(|s| n.contains(s))
}
pub fn virtual_mac(mac: [u8; 6]) -> bool {
    [
        [0, 5, 0x69],
        [0, 0x0c, 0x29],
        [0, 0x50, 0x56],
        [0, 0x15, 0x5d],
        [8, 0, 0x27],
        [0x52, 0x54, 0],
    ]
    .contains(&mac[..3].try_into().unwrap_or([0; 3]))
}

/// Owns the capture thread and its cancellation primitive. Closing is immediate,
/// including when no packets arrive. A handle survives every renewal handshake.
pub struct Session {
    pub capture: Arc<Capture>,
    stop: Arc<Cancel>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Session {
    pub fn open(
        adapter: &Adapter,
        generation: u64,
        events: SyncSender<crate::engine::Event>,
    ) -> Result<Self> {
        let capture = Arc::new(Capture::open(adapter)?);
        let stop = Arc::new(Cancel::new()?);
        let reader = capture.clone();
        let done = stop.clone();
        let thread = std::thread::Builder::new()
            .name("eapol-receive".into())
            .spawn(move || {
                loop {
                    match reader.receive(&done) {
                        Ok(Some(bytes)) => {
                            let _ =
                                events.try_send(crate::engine::Event::Packet(generation, bytes));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = events.try_send(crate::engine::Event::CaptureError(
                                generation,
                                e.to_string(),
                            ));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            capture,
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop.set();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct Monitor {
    pub stop: Arc<Cancel>,
    pub thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.set();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
