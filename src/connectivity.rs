//! One cancellable reachability policy, shared by both authentication entry points.
use crate::{
    engine::Event,
    platform::{self, Cancel},
};
use anyhow::Result;
use std::{
    sync::{Arc, mpsc::SyncSender},
    time::Duration,
};

pub struct Probe {
    stop: Arc<Cancel>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Probe {
    pub fn start(events: SyncSender<Event>, generation: u64) -> Result<Self> {
        let stop = Arc::new(Cancel::new()?);
        let cancel = stop.clone();
        let thread = std::thread::Builder::new()
            .name("reachability-check".into())
            .spawn(move || {
                let reachable = check_with(
                    |target| platform::ping(target.into(), &cancel),
                    |delay| platform::cancellable_wait(&cancel, delay),
                );
                if platform::cancellable_wait(&cancel, Duration::ZERO) {
                    platform::deliver(&events, Event::Connectivity(generation, reachable), &cancel);
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.stop.set();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn check_with(
    mut ping: impl FnMut([u8; 4]) -> bool,
    mut wait: impl FnMut(Duration) -> bool,
) -> bool {
    for round in 0..3 {
        for target in [[119, 29, 29, 29], [114, 114, 114, 114]] {
            if ping(target) {
                return true;
            }
        }
        if round < 2 && !wait(Duration::from_secs(10)) {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_success_never_waits_before_or_after_the_probe() {
        let mut targets = Vec::new();
        assert!(check_with(
            |target| {
                targets.push(target);
                true
            },
            |_| panic!("successful first probe must be immediate")
        ));
        assert_eq!(targets, [[119, 29, 29, 29]]);
    }
    #[test]
    fn only_failed_rounds_wait_and_cancellation_stops_retries() {
        let (mut pings, mut waits) = (0, 0);
        assert!(!check_with(
            |_| {
                pings += 1;
                false
            },
            |delay| {
                assert_eq!(delay, Duration::from_secs(10));
                waits += 1;
                true
            }
        ));
        assert_eq!((pings, waits), (6, 2));
        pings = 0;
        assert!(!check_with(
            |_| {
                pings += 1;
                false
            },
            |_| false
        ));
        assert_eq!(pings, 2);
    }
}
