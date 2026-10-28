use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        mpsc::{SyncSender, TrySendError},
    },
    time::Duration,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "linux")]
pub use unix::https_get;
#[cfg(unix)]
pub use unix::{instance_lock, is_admin, ping, signal_thread};
#[cfg(any(target_os = "macos", test))]
mod bpf;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("Only Windows, Linux and macOS are supported");

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

/// Joined workers must deliver results reliably without blocking cancellation.
pub fn deliver(
    events: &SyncSender<crate::engine::Event>,
    mut event: crate::engine::Event,
    stop: &Cancel,
) -> bool {
    loop {
        match events.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(pending)) => event = pending,
        }
        if !cancellable_wait(stop, Duration::from_millis(10)) {
            return false;
        }
    }
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
                            if !deliver(
                                &events,
                                crate::engine::Event::Packet(generation, bytes),
                                &done,
                            ) {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            deliver(
                                &events,
                                crate::engine::Event::CaptureError(generation, e.to_string()),
                                &done,
                            );
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

#[cfg(test)]
mod delivery_tests {
    use super::*;
    use crate::engine::Event;
    use std::sync::mpsc;

    #[test]
    fn full_queue_preserves_one_shot_results() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Event::NetworkChanged).unwrap();
        let stop = Arc::new(Cancel::new().unwrap());
        let worker_stop = stop.clone();
        let (ready, started) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            ready.send(()).unwrap();
            finished
                .send(deliver(
                    &tx,
                    Event::CaptureError(7, "driver reset".into()),
                    &worker_stop,
                ))
                .unwrap();
        });
        started.recv().unwrap();
        assert!(done.recv_timeout(Duration::from_millis(30)).is_err());
        assert!(matches!(rx.recv().unwrap(), Event::NetworkChanged));
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::CaptureError(7, _)
        ));
        assert!(done.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
    }

    #[test]
    fn full_queue_delivery_remains_cancellable_for_joined_workers() {
        let (tx, _rx) = mpsc::sync_channel(1);
        tx.send(Event::NetworkChanged).unwrap();
        let stop = Arc::new(Cancel::new().unwrap());
        let worker_stop = stop.clone();
        let (finished, done) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            finished
                .send(deliver(&tx, Event::Connectivity(8, true), &worker_stop))
                .unwrap();
        });
        stop.set();
        assert!(!done.recv_timeout(Duration::from_secs(2)).unwrap());
        thread.join().unwrap();
    }
}
