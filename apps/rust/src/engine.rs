use crate::{
    config::{self, Cache, Config, Paths},
    ipc::{Action, Call, Response, Subscription},
    platform::{self, Adapter, Cancel, Session},
    protocol,
    update::{self, Update},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant, SystemTime},
};

const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);

/// Callbacks latch a condition before attempting a bounded-queue wakeup.
/// A full queue must not lose a link change or a service stop request.
pub struct Signal {
    pending: AtomicBool,
    events: SyncSender<Event>,
    wake: fn() -> Event,
}
impl Signal {
    pub fn new(events: SyncSender<Event>, wake: fn() -> Event) -> Self {
        Self {
            pending: AtomicBool::new(false),
            events,
            wake,
        }
    }
    pub fn notify(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.events.try_send((self.wake)());
        }
    }
    pub fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

pub enum Event {
    Packet(u64, Vec<u8>),
    CaptureError(u64, String),
    NetworkChanged,
    PowerChanged,
    Connectivity(u64, bool),
    Update(Result<Update, String>),
    Call(Call),
    Stop,
}

#[derive(Clone, Copy)]
pub enum PowerEvent {
    Suspend = 1,
    Resume = 2,
}

/// SCM callbacks must return promptly, even if the engine's bounded queue is full.
/// Keep the latest power state separately; queued traffic will also wake the engine.
pub struct PowerNotifier {
    pending: AtomicU8,
    events: SyncSender<Event>,
}
impl PowerNotifier {
    pub fn notify(&self, event: PowerEvent) {
        self.pending.store(event as u8, Ordering::Release);
        let _ = self.events.try_send(Event::PowerChanged);
    }
    fn take(&self) -> Option<PowerEvent> {
        if self.pending.load(Ordering::Acquire) == 0 {
            return None;
        }
        match self.pending.swap(0, Ordering::AcqRel) {
            1 => Some(PowerEvent::Suspend),
            2 => Some(PowerEvent::Resume),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Authenticating,
    Authenticated,
    Failed,
    Stopped,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: State,
    pub message: String,
    pub adapter: Option<Adapter>,
    pub authenticated_at: Option<u64>,
    pub internet: bool,
    pub update: Update,
}

struct Probe {
    stop: Arc<Cancel>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.stop.set();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct Engine {
    pub config: Config,
    pub snapshot: Snapshot,
    paths: Paths,
    cache: Cache,
    events: SyncSender<Event>,
    power: Arc<PowerNotifier>,
    network: Arc<Signal>,
    stop: Arc<Signal>,
    suspended: bool,
    recovering_resume: bool,
    preserve_manual_disconnect: bool,
    pub generation: u64,
    session: Option<Session>,
    probe: Option<Probe>,
    username: Vec<u8>,
    password: Vec<u8>,
    adapters: Vec<Adapter>,
    candidates: Vec<Adapter>,
    peer: Option<[u8; 6]>,
    manual_disconnect: bool,
    retry: Option<Instant>,
    retries: u32,
    startup: Instant,
    startup_retry: usize,
    handshake_deadline: Option<Instant>,
    start_deadline: Option<Instant>,
    subscribers: Vec<Subscription>,
    update_deadline: Instant,
    last_manual_update: Option<Instant>,
    reconcile_deadline: Instant,
    config_version: Option<(SystemTime, u64)>,
    config_error: Option<String>,
}
impl Engine {
    pub fn paths(&self) -> &Paths {
        &self.paths
    }
    pub fn new(paths: Paths, events: SyncSender<Event>) -> Result<Self> {
        let initial_version = Self::file_version(&paths);
        let loaded = config::load(&paths);
        let (config, error) = match loaded {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!("Configuration unavailable: {e}")),
            ),
        };
        let cache = config::read(&paths.state.join("service_cache.json")).unwrap_or_default();
        let mut update: Update =
            config::read(&paths.state.join("update_state.json")).unwrap_or_default();
        update.checking = false;
        let config_version = error.is_none().then_some(initial_version).flatten();
        let config_error = error.clone();
        let now = Instant::now();
        let delay = if update.next_check_at > config::now() {
            update
                .next_check_at
                .saturating_sub(config::now())
                .min(7 * 86400)
        } else {
            120 + (std::process::id() as u64 % 180)
        };
        Ok(Self {
            config,
            snapshot: Snapshot {
                state: State::Idle,
                message: error.unwrap_or_else(|| "待命".into()),
                adapter: None,
                authenticated_at: None,
                internet: false,
                update,
            },
            paths,
            cache,
            power: Arc::new(PowerNotifier {
                pending: AtomicU8::new(0),
                events: events.clone(),
            }),
            network: Arc::new(Signal::new(events.clone(), || Event::NetworkChanged)),
            stop: Arc::new(Signal::new(events.clone(), || Event::Stop)),
            events,
            suspended: false,
            recovering_resume: false,
            preserve_manual_disconnect: false,
            generation: 0,
            session: None,
            probe: None,
            username: Vec::new(),
            password: Vec::new(),
            adapters: Vec::new(),
            candidates: Vec::new(),
            peer: None,
            manual_disconnect: false,
            retry: None,
            retries: 0,
            startup: now + Duration::from_secs(60),
            startup_retry: 0,
            handshake_deadline: None,
            start_deadline: None,
            subscribers: Vec::new(),
            update_deadline: now + Duration::from_secs(delay),
            last_manual_update: None,
            reconcile_deadline: now + RECONCILE_INTERVAL,
            config_version,
            config_error,
        })
    }
    fn file_version(paths: &Paths) -> Option<(SystemTime, u64)> {
        let metadata = std::fs::metadata(&paths.config).ok()?;
        Some((metadata.modified().ok()?, metadata.len()))
    }
    fn reload_changed_config(&mut self) {
        let Some(version) = Self::file_version(&self.paths) else {
            // Keep known-good credentials across a replacement or a temporary
            // sharing/access failure. A missing file is not an empty config.
            return;
        };
        if self.config_error.is_none() && self.config_version == Some(version) {
            return;
        }
        match config::load(&self.paths) {
            Ok(config) => {
                if let Err(e) = self.apply_config(config) {
                    self.failed(&e.to_string());
                }
                // Record the pre-read version: if a writer raced the read, the
                // next reconciliation must read the newer file again.
                self.config_version = Some(version);
                self.config_error = None;
            }
            Err(e) => {
                let message = e.to_string();
                if self.config_error.as_ref() != Some(&message) {
                    crate::diagnostics::record(
                        &self.paths,
                        &format!("config reload failed; retaining current settings: {message}"),
                    );
                }
                self.config_error = Some(message);
            }
        }
    }
    pub fn network_notifier(&self) -> Arc<Signal> {
        self.network.clone()
    }
    pub fn stop_notifier(&self) -> Arc<Signal> {
        self.stop.clone()
    }
    fn drain_network(&mut self) {
        if self.network.take() {
            if let Err(e) = self.network_changed() {
                self.failed(&e.to_string());
            }
        }
    }
    fn publish(&mut self) {
        self.subscribers.retain(|s| {
            let ok = s.alive.load(std::sync::atomic::Ordering::Relaxed)
                && s.updates.try_send(Some(self.snapshot.clone())).is_ok();
            if !ok {
                s.alive.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            ok
        });
    }
    fn status(&mut self, state: State, message: impl Into<String>) {
        let message = message.into();
        if self.snapshot.state == state && self.snapshot.message == message {
            return;
        }
        crate::diagnostics::record(&self.paths, &format!("{state:?}: {message}"));
        self.snapshot.state = state;
        self.snapshot.message = message;
        if self.snapshot.state != State::Authenticated {
            self.snapshot.authenticated_at = None;
            self.snapshot.internet = false;
        }
        self.publish();
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.probe = None;
        self.session = None;
        self.peer = None;
        self.handshake_deadline = None;
        self.start_deadline = None;
    }
    pub fn power_notifier(&self) -> Arc<PowerNotifier> {
        self.power.clone()
    }
    fn drain_power(&mut self) {
        let Some(event) = self.power.take() else {
            return;
        };
        // Resume also invalidates when the suspend notification was not observed.
        self.invalidate();
        self.candidates.clear();
        self.snapshot.adapter = None;
        self.retry = None;
        self.preserve_manual_disconnect |= self.manual_disconnect;
        self.recovering_resume = false;
        match event {
            PowerEvent::Suspend => {
                self.suspended = true;
                self.status(State::Idle, "系统休眠，等待恢复");
            }
            PowerEvent::Resume => {
                self.suspended = false;
                self.retries = 0;
                self.startup_retry = 0;
                self.startup = Instant::now() + Duration::from_secs(120);
                if self.config.auto_auth && !self.manual_disconnect {
                    self.recovering_resume = true;
                    self.retry = Some(Instant::now() + Duration::from_secs(2));
                    self.status(State::Idle, "系统已恢复，等待网卡就绪后重新认证");
                } else {
                    self.status(State::Idle, "系统已恢复，自动认证未启用或已手动断开");
                }
            }
        }
    }
    pub fn start(&mut self) {
        if let Err(e) = self.refresh_adapters() {
            self.failed(&e.to_string());
            return;
        }
        if self.config.auto_auth {
            if let Err(e) = self.authenticate() {
                self.failed(&e.to_string());
            }
        }
    }
    fn refresh_adapters(&mut self) -> Result<()> {
        self.adapters = platform::adapters()?;
        Ok(())
    }
    fn select(&self) -> Vec<Adapter> {
        let mut available: Vec<_> = self
            .adapters
            .iter()
            .filter(|a| a.candidate())
            .cloned()
            .collect();
        if self.config.iface_mode == "manual" {
            available.retain(|a| {
                a.name == self.config.iface
                    || a.id == self.config.iface
                    || (!self.cache.last_success_mac.is_empty()
                        && protocol::mac_text(a.mac) == self.cache.last_success_mac
                        && (self.config.iface == self.cache.iface
                            || self.config.iface == self.cache.manual_selector))
            });
            available.truncate(1);
        } else {
            available.sort_by_key(|a| {
                (
                    std::cmp::Reverse(protocol::mac_text(a.mac) == self.cache.last_success_mac),
                    std::cmp::Reverse(a.name == self.cache.iface),
                    std::cmp::Reverse(a.score),
                )
            });
        }
        available
    }
    fn authenticate(&mut self) -> Result<()> {
        ensure!(!self.suspended, "系统休眠中");
        self.retry = None;
        self.invalidate();
        self.candidates.clear();
        self.config.validate()?;
        ensure!(
            !self.config.username.is_empty() && !self.config.password.is_empty(),
            "需要配置 NetID 和密码"
        );
        ensure!(
            self.config.iface_mode != "manual" || !self.config.iface.is_empty(),
            "请选择网卡"
        );
        self.username = protocol::gbk(&self.config.username)?;
        self.password = protocol::gbk(&self.config.password)?;
        self.refresh_adapters()?;
        self.candidates = self.select();
        self.next_candidate()
    }
    fn next_candidate(&mut self) -> Result<()> {
        self.invalidate();
        if self.candidates.is_empty() {
            self.snapshot.adapter = None;
            if self.recovering_resume && Instant::now() < self.startup {
                self.failed("等待已连接的有线网卡");
                return Ok(());
            }
            self.recovering_resume = false;
            self.status(State::Idle, "等待已连接的有线网卡");
            return Ok(());
        }
        let adapter = self.candidates.remove(0);
        let session = match Session::open(&adapter, self.generation, self.events.clone()) {
            Ok(s) => s,
            Err(e) => {
                if !self.candidates.is_empty() {
                    return self.next_candidate();
                }
                return Err(e);
            }
        };
        session.capture.send(&protocol::control(adapter.mac, 1))?;
        self.snapshot.adapter = Some(adapter);
        self.session = Some(session);
        self.handshake_deadline = Some(Instant::now() + Duration::from_secs(10));
        self.start_deadline = Some(Instant::now() + Duration::from_secs(1));
        self.status(State::Authenticating, "正在认证");
        Ok(())
    }
    fn failed(&mut self, message: &str) {
        let mut message = message.to_owned();
        let was_online = self.snapshot.state == State::Authenticated;
        self.retry = None;
        self.invalidate();
        if was_online {
            // A lost established session deserves prompt recovery on the saved
            // adapter, rather than an old candidate queue or a full retry interval.
            self.candidates.clear();
            self.retries = 0;
            self.startup_retry = 0;
            self.startup = Instant::now() + Duration::from_secs(60);
        }
        if !self.candidates.is_empty() {
            match self.next_candidate() {
                Ok(()) => return,
                Err(e) => message = e.to_string(),
            }
        }
        if self.manual_disconnect || !self.config.auto_auth {
            self.recovering_resume = false;
            self.status(State::Failed, message);
            return;
        }
        if Instant::now() < self.startup {
            let delay = [3, 5, 8, 13, 21][self.startup_retry.min(4)];
            self.startup_retry += 1;
            self.retry = Some(Instant::now() + Duration::from_secs(delay));
        } else if self.retries < 5 {
            self.retries += 1;
            self.retry = Some(Instant::now() + Duration::from_secs(self.config.retry_interval));
        } else {
            self.recovering_resume = false;
            self.status(State::Failed, "多次认证失败，请检查 NetID 或网络");
            return;
        }
        self.status(State::Idle, format!("{message}，等待重试"));
    }
    fn packet(&mut self, generation: u64, raw: &[u8]) -> Result<()> {
        if generation != self.generation {
            return Ok(());
        }
        let Some(adapter) = self.snapshot.adapter.as_ref() else {
            return Ok(());
        };
        let Some(packet) = protocol::parse(raw) else {
            return Ok(());
        };
        if packet.source == adapter.mac
            || packet.source == [0; 6]
            || packet.source[0] & 1 != 0
            || (packet.destination != adapter.mac && packet.destination != protocol::PAE)
        {
            return Ok(());
        }
        let peer_changed = self.peer.is_some_and(|peer| peer != packet.source);
        let online = self.snapshot.state == State::Authenticated || self.probe.is_some();
        // A switch can change its EAP source during online reauthentication.
        // Adopt it only after answering a valid Request, never on an unsolicited
        // Success/Failure from another source.
        if (peer_changed && !(online && packet.code == 1))
            || (self.peer.is_none() && packet.code != 1)
        {
            return Ok(());
        }
        if packet.code == 1 {
            let destination = if online { protocol::PAE } else { packet.source };
            let response = protocol::answer(
                adapter.mac,
                destination,
                &packet,
                &self.username,
                &self.password,
            );
            let response = match response {
                Err(_) if peer_changed => return Ok(()),
                other => other?,
            };
            if let Some(response) = response {
                if let Some(session) = &self.session {
                    session.capture.send(&response)?;
                }
                if peer_changed {
                    crate::diagnostics::record(
                        &self.paths,
                        &format!(
                            "online authenticator changed to {}",
                            protocol::mac_text(packet.source)
                        ),
                    );
                }
                self.peer = Some(packet.source);
                self.start_deadline = None;
                // Online requests are keepalives as well as full reauthentication.
                // Like the Python listener, keep answering without requiring a
                // terminal Success for each probe. Only the initial handshake
                // has a deadline; an explicit Failure still restarts authentication.
            }
        } else if packet.code == 3 {
            self.handshake_deadline = None;
            self.start_deadline = None;
            if self.snapshot.state == State::Authenticated {
                return Ok(());
            }
            if self.probe.is_none() {
                self.start_connectivity()?;
            }
        } else if packet.code == 4 {
            self.failed("EAP 认证失败");
        }
        Ok(())
    }
    fn start_connectivity(&mut self) -> Result<()> {
        self.status(State::Authenticating, "已完成 EAP，验证互联网连通性");
        let stop = Arc::new(Cancel::new()?);
        let cancel = stop.clone();
        let tx = self.events.clone();
        let generation = self.generation;
        let thread = std::thread::Builder::new()
            .name("reachability-check".into())
            .spawn(move || {
                if !platform::cancellable_wait(&cancel, Duration::from_secs(5)) {
                    return;
                }
                for round in 0..3 {
                    for target in [[119, 29, 29, 29], [114, 114, 114, 114]] {
                        if platform::ping(target.into(), &cancel) {
                            platform::deliver(&tx, Event::Connectivity(generation, true), &cancel);
                            return;
                        }
                    }
                    if round < 2 && !platform::cancellable_wait(&cancel, Duration::from_secs(10)) {
                        return;
                    }
                }
                platform::deliver(&tx, Event::Connectivity(generation, false), &cancel);
            })?;
        self.probe = Some(Probe {
            stop,
            thread: Some(thread),
        });
        Ok(())
    }
    fn network_changed(&mut self) -> Result<()> {
        if self.suspended {
            return Ok(());
        }
        let previous_media = self.adapters.iter().any(Adapter::candidate);
        if let Err(e) = self.refresh_adapters() {
            // An enumeration failure is not evidence that the active link has
            // gone away. Preserve its capture and let the next scan try again.
            crate::diagnostics::record(
                &self.paths,
                &format!("adapter refresh failed; retaining current session: {e}"),
            );
            return Ok(());
        }
        let current_media = self.adapters.iter().any(Adapter::candidate);
        if !previous_media && current_media {
            if !self.preserve_manual_disconnect {
                self.manual_disconnect = false;
            }
            self.retries = 0;
        }
        // Link/address notifications during NIC initialization must not bypass
        // the resume settling delay or its bounded driver-recovery backoff.
        if self.recovering_resume && self.retry.is_some() {
            return Ok(());
        }
        if let Some(active) = &self.snapshot.adapter {
            if let Some(updated) = self
                .adapters
                .iter()
                .find(|a| a.id == active.id && a.candidate())
            {
                if updated != active {
                    self.snapshot.adapter = Some(updated.clone());
                    self.publish();
                }
                if self.session.is_some() {
                    return Ok(());
                }
            } else {
                self.invalidate();
                self.snapshot.adapter = None;
                self.retry = None;
                self.status(State::Idle, "网卡已断开");
            }
        }
        if self.config.auto_auth
            && !self.manual_disconnect
            && self.session.is_none()
            && self.retry.is_none()
            && (self.retries < 5 || Instant::now() < self.startup)
        {
            self.authenticate()?;
        }
        Ok(())
    }
    fn start_update(&mut self, manual: bool) -> Result<()> {
        ensure!(
            !self.snapshot.update.checking,
            "update check already running"
        );
        if manual {
            ensure!(
                self.last_manual_update
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(60)),
                "update check cooling down"
            );
            self.last_manual_update = Some(Instant::now());
        }
        self.snapshot.update.checking = true;
        self.publish();
        let tx = self.events.clone();
        std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                // This worker is detached, so a blocking send cannot deadlock
                // engine shutdown. Receiver closure releases it on service exit.
                let _ = tx.send(Event::Update(update::fetch().map_err(|e| e.to_string())));
            })?;
        Ok(())
    }
    fn call(&mut self, call: Call) {
        let id = call.request.id;
        let result = (|| -> Result<serde_json::Value> {
            match call.request.action {
                Action::Status | Action::Watch => Ok(serde_json::to_value(&self.snapshot)?),
                Action::Interfaces => {
                    self.refresh_adapters()?;
                    Ok(serde_json::to_value(&self.adapters)?)
                }
                Action::GetConfig => Ok(serde_json::to_value(&self.config)?),
                Action::Authenticate => {
                    ensure!(!self.suspended, "系统休眠中");
                    self.manual_disconnect = false;
                    self.preserve_manual_disconnect = false;
                    self.recovering_resume = false;
                    self.retries = 0;
                    if let Err(e) = self.authenticate() {
                        self.status(State::Failed, e.to_string());
                        return Err(e);
                    }
                    Ok(serde_json::to_value(&self.snapshot)?)
                }
                Action::Logoff => {
                    self.logoff()?;
                    Ok(serde_json::to_value(&self.snapshot)?)
                }
                Action::SetConfig { config } => {
                    config.validate()?;
                    config::save(&self.paths, &config)?;
                    #[cfg(windows)]
                    if config.service_mode != self.config.service_mode {
                        platform::configure_service(config.service_mode)?;
                    }
                    self.apply_config(config)?;
                    Ok(serde_json::json!({"saved":true}))
                }
                Action::ReloadConfig => {
                    let config = config::load(&self.paths)?;
                    self.apply_config(config)?;
                    Ok(serde_json::json!({"reloaded":true}))
                }
                Action::CheckUpdate => {
                    self.start_update(true)?;
                    Ok(serde_json::json!({"checking":true}))
                }
            }
        })();
        match result {
            Ok(data) => {
                let _ = call.reply.try_send(Response::success(id, data));
                if let Some(updates) = call.updates {
                    self.subscribers
                        .retain(|s| s.alive.load(std::sync::atomic::Ordering::Relaxed));
                    self.subscribers.push(updates);
                }
            }
            Err(e) => {
                let _ = call.reply.try_send(Response::error(id, e));
            }
        }
    }
    fn apply_config(&mut self, config: Config) -> Result<()> {
        let credentials_changed = self.config.username != config.username
            || self.config.password != config.password
            || self.config.iface != config.iface
            || self.config.iface_mode != config.iface_mode;
        let enabled = !self.config.auto_auth && config.auto_auth;
        self.config = config;
        if !self.config.auto_auth {
            self.retry = None;
            self.recovering_resume = false;
        }
        if credentials_changed {
            self.invalidate();
            self.status(State::Idle, "认证配置已更改");
        }
        if (credentials_changed || (enabled && self.snapshot.state != State::Authenticated))
            && self.config.auto_auth
            && !self.manual_disconnect
            && !self.suspended
        {
            if self.recovering_resume {
                self.retry = Some(Instant::now() + Duration::from_secs(2));
            } else if let Err(e) = self.authenticate() {
                self.status(State::Failed, e.to_string());
            }
        }
        Ok(())
    }
    fn logoff(&mut self) -> Result<()> {
        self.manual_disconnect = true;
        self.preserve_manual_disconnect |= self.suspended || self.recovering_resume;
        self.recovering_resume = false;
        self.retry = None;
        self.candidates.clear();
        let sent = if let (Some(session), Some(adapter)) = (&self.session, &self.snapshot.adapter) {
            session.capture.send(&protocol::control(adapter.mac, 2))
        } else {
            Ok(())
        };
        self.invalidate();
        self.status(State::Idle, "已断开");
        sent
    }
    fn tick(&mut self) {
        if self.stop.pending.load(Ordering::Acquire) {
            return;
        }
        self.drain_power();
        self.drain_network();
        if self.suspended {
            return;
        }
        let now = Instant::now();
        if self.reconcile_deadline <= now {
            self.reload_changed_config();
            if let Err(e) = self.network_changed() {
                self.failed(&e.to_string());
            }
            self.reconcile_deadline = Instant::now() + RECONCILE_INTERVAL;
        }
        if self.handshake_deadline.is_some_and(|d| d <= now) {
            self.failed("认证超时");
        }
        if self.start_deadline.is_some_and(|d| d <= now) {
            if let (Some(session), Some(adapter)) = (&self.session, &self.snapshot.adapter) {
                if let Err(e) = session.capture.send(&protocol::control(adapter.mac, 1)) {
                    self.failed(&e.to_string());
                    return;
                }
            }
            self.start_deadline = Some(now + Duration::from_secs(1));
        }
        if self.retry.is_some_and(|d| d <= now) {
            self.retry = None;
            if let Err(e) = self.authenticate() {
                self.failed(&e.to_string());
            }
        }
        if self.update_deadline <= now && !self.snapshot.update.checking {
            if let Err(e) = self.start_update(false) {
                eprintln!("update: {e}");
                self.update_deadline = now + Duration::from_secs(1800);
            }
        }
    }
    fn timeout(&self) -> Option<Duration> {
        if self.suspended {
            return None;
        }
        [
            self.retry,
            self.handshake_deadline,
            self.start_deadline,
            Some(self.reconcile_deadline),
            (!self.snapshot.update.checking).then_some(self.update_deadline),
        ]
        .into_iter()
        .flatten()
        .min()
        .map(|d| d.saturating_duration_since(Instant::now()))
    }
    pub fn handle(&mut self, event: Event) -> bool {
        if self.stop.take() {
            self.invalidate();
            self.status(State::Stopped, "服务已停止");
            return false;
        }
        if !matches!(event, Event::Stop) {
            self.drain_power();
            if !matches!(event, Event::NetworkChanged) {
                self.drain_network();
            }
        }
        match event {
            Event::Stop => {
                self.invalidate();
                self.status(State::Stopped, "服务已停止");
                return false;
            }
            Event::Packet(g, b) => {
                if let Err(e) = self.packet(g, &b) {
                    self.failed(&e.to_string());
                }
            }
            Event::CaptureError(g, e) if g == self.generation => self.failed(&e),
            Event::CaptureError(..) => {}
            Event::PowerChanged => {}
            Event::NetworkChanged => {
                self.network.take();
                if let Err(e) = self.network_changed() {
                    self.failed(&e.to_string());
                }
            }
            Event::Connectivity(g, success) if g == self.generation => {
                self.probe = None;
                if success {
                    self.recovering_resume = false;
                    self.candidates.clear();
                    self.retries = 0;
                    self.retry = None;
                    self.startup = Instant::now();
                    self.startup_retry = 0;
                    if let Some(adapter) = &self.snapshot.adapter {
                        self.cache = Cache {
                            iface: adapter.name.clone(),
                            last_success_mac: protocol::mac_text(adapter.mac),
                            manual_selector: self.config.iface.clone(),
                        };
                        if let Err(e) = config::atomic_write(
                            &self.paths.state.join("service_cache.json"),
                            &self.cache,
                        ) {
                            eprintln!("cache: {e}");
                        }
                    }
                    let _ = self.refresh_adapters();
                    if let Some(active) = &self.snapshot.adapter {
                        if let Some(a) = self.adapters.iter().find(|a| a.id == active.id) {
                            self.snapshot.adapter = Some(a.clone());
                        }
                    }
                    self.snapshot.state = State::Authenticated;
                    crate::diagnostics::record(&self.paths, "authenticated; Internet reachable");
                    self.snapshot.message = "已认证，互联网连通".into();
                    self.snapshot.internet = true;
                    self.snapshot.authenticated_at = Some(config::now());
                    self.publish();
                } else {
                    self.candidates.clear();
                    self.failed("EAP 成功但无法访问互联网");
                }
            }
            Event::Connectivity(..) => {}
            Event::Update(result) => {
                let now = config::now();
                match result {
                    Ok(mut update) => {
                        update.next_check_at = now + 86400 + (std::process::id() as u64 % 1800);
                        self.snapshot.update = update;
                    }
                    Err(e) => {
                        let u = &mut self.snapshot.update;
                        u.failure_count = u.failure_count.saturating_add(1);
                        u.checked_at = now;
                        u.next_check_at = now + update::failure_delay(u.failure_count);
                        u.error = e;
                        u.checking = false;
                    }
                }
                self.update_deadline = Instant::now()
                    + Duration::from_secs(self.snapshot.update.next_check_at.saturating_sub(now));
                if let Err(e) = config::atomic_write(
                    &self.paths.state.join("update_state.json"),
                    &self.snapshot.update,
                ) {
                    eprintln!("update state: {e}");
                }
                self.publish();
            }
            Event::Call(call) => self.call(call),
        }
        true
    }
    pub fn run(&mut self, rx: Receiver<Event>) {
        if self.stop.pending.load(Ordering::Acquire) {
            self.handle(Event::Stop);
            return;
        }
        self.drain_power();
        if !self.suspended && !self.recovering_resume {
            self.start();
        }
        loop {
            let received = match self.timeout() {
                Some(t) => rx.recv_timeout(t),
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(event) => {
                    if !self.handle(event) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn engine() -> Engine {
        let (tx, _) = mpsc::sync_channel(8);
        let dir = std::env::temp_dir().join(format!(
            "netauth-engine-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        Engine::new(
            Paths {
                config: dir.join("missing.json"),
                state: dir.clone(),
                endpoint: dir.join("socket"),
            },
            tx,
        )
        .unwrap()
    }
    #[test]
    fn network_signals_are_latched_and_coalesced_on_a_full_queue() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Event::PowerChanged).unwrap();
        let signal = Signal::new(tx, || Event::NetworkChanged);
        signal.notify();
        signal.notify();
        assert!(signal.take());
        assert!(!signal.take());
        assert!(matches!(rx.recv().unwrap(), Event::PowerChanged));
        for _ in 0..100 {
            signal.notify();
        }
        assert!(matches!(rx.recv().unwrap(), Event::NetworkChanged));
        assert!(rx.try_recv().is_err());
        assert!(signal.take());
    }
    #[test]
    fn stop_request_has_priority_even_when_its_wakeup_cannot_be_queued() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut e = Engine::new(engine().paths, tx.clone()).unwrap();
        tx.send(Event::Packet(0, vec![])).unwrap();
        e.stop_notifier().notify();
        assert!(!e.handle(rx.recv().unwrap()));
        assert_eq!(e.snapshot.state, State::Stopped);
    }
    #[test]
    fn startup_config_read_failure_can_recover_without_gui() {
        let initial = engine();
        config::save(&initial.paths, &initial.config).unwrap();
        std::fs::write(&initial.paths.config, b"{ incomplete").unwrap();
        let mut e = Engine::new(initial.paths.clone(), initial.events.clone()).unwrap();
        assert!(e.config_error.is_some());
        let config = Config {
            username: "recovered-user".into(),
            auto_auth: false,
            ..Config::default()
        };
        config::save(&e.paths, &config).unwrap();
        e.reload_changed_config();
        assert_eq!(e.config, config);
        assert!(e.config_error.is_none());
        assert!(e.config_version.is_some());
        std::fs::remove_dir_all(&e.paths.state).unwrap();
    }
    #[test]
    fn bad_or_missing_config_keeps_online_credentials_and_valid_reads_do_not_rewrite() {
        let mut e = online();
        let config = Config {
            username: "known-user".into(),
            password: "password".into(),
            auto_auth: false,
            ..Config::default()
        };
        config::save(&e.paths, &config).unwrap();
        e.config = config.clone();
        e.config_version = Engine::file_version(&e.paths);
        std::fs::write(&e.paths.config, [0xff, 0xff]).unwrap();
        e.reload_changed_config();
        assert_eq!(e.config, config);
        assert_eq!(e.snapshot.state, State::Authenticated);
        assert!(e.config_error.is_some());
        std::fs::remove_file(&e.paths.config).unwrap();
        e.reload_changed_config();
        assert_eq!(e.config, config);
        assert_eq!(e.snapshot.state, State::Authenticated);
        config::save(&e.paths, &config).unwrap();
        let before = std::fs::read(&e.paths.config).unwrap();
        e.reload_changed_config();
        assert_eq!(before, std::fs::read(&e.paths.config).unwrap());
        assert!(e.config_error.is_none());
        assert_eq!(e.snapshot.state, State::Authenticated);
        std::fs::remove_dir_all(&e.paths.state).unwrap();
    }
    #[test]
    fn stale_results_and_manual_disconnect() {
        let mut e = engine();
        e.manual_disconnect = true;
        e.generation = 12;
        e.handle(Event::Connectivity(11, true));
        assert_eq!(e.snapshot.state, State::Idle);
        e.handle(Event::CaptureError(11, "old".into()));
        assert_eq!(e.snapshot.state, State::Idle);
        e.config.auto_auth = false;
        e.retry = Some(Instant::now());
        e.apply_config(e.config.clone()).unwrap();
        assert!(e.retry.is_none());
    }
    #[test]
    fn candidate_order_and_manual_selection() {
        let mut e = engine();
        e.adapters = vec![
            Adapter {
                name: "new".into(),
                id: "new".into(),
                ethernet: true,
                up: true,
                mac: [1; 6],
                score: 60,
                ..Adapter::default()
            },
            Adapter {
                name: "saved".into(),
                id: "saved".into(),
                ethernet: true,
                up: true,
                mac: [2; 6],
                score: 40,
                ..Adapter::default()
            },
        ];
        e.cache.last_success_mac = protocol::mac_text([2; 6]);
        assert_eq!(e.select()[0].name, "saved");
        e.config.iface_mode = "manual".into();
        e.config.iface = "new".into();
        assert_eq!(e.select().len(), 1);
        assert_eq!(e.select()[0].name, "new");
    }
    #[test]
    fn retry_exhaustion_and_no_auto_auth() {
        let mut e = engine();
        e.startup = Instant::now();
        e.retries = 5;
        e.failed("rejected");
        assert_eq!(e.snapshot.state, State::Failed);
        assert!(e.retry.is_none());
        e.retries = 0;
        e.config.auto_auth = false;
        e.failed("rejected");
        assert!(e.retry.is_none());
    }
    #[test]
    fn slow_subscriber_is_dropped() {
        let mut e = engine();
        let (tx, _rx) = mpsc::sync_channel(1);
        e.subscribers.push(Subscription {
            updates: tx,
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        });
        e.publish();
        e.publish();
        assert!(e.subscribers.is_empty());
    }
    fn online() -> Engine {
        let mut e = engine();
        e.snapshot.state = State::Authenticated;
        e.snapshot.internet = true;
        e.snapshot.authenticated_at = Some(1);
        e.snapshot.adapter = Some(Adapter {
            mac: [1; 6],
            ..Adapter::default()
        });
        e.peer = Some([2; 6]);
        e.username = b"user".to_vec();
        e.password = b"password".to_vec();
        e
    }
    fn server_packet(code: u8, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut raw = protocol::response([2; 6], [1; 6], 7, kind, data).unwrap();
        raw[18] = code;
        if code >= 3 {
            raw.truncate(22);
            raw[16..18].copy_from_slice(&4u16.to_be_bytes());
            raw[20..22].copy_from_slice(&4u16.to_be_bytes());
        }
        raw
    }
    #[test]
    fn repeated_renewal_preserves_status_and_rejects_other_peers() {
        let mut e = online();
        let mut challenge = vec![16];
        challenge.extend(0..16);
        for _ in 0..3 {
            e.packet(e.generation, &server_packet(1, 1, &[])).unwrap();
            e.packet(e.generation, &server_packet(1, 4, &challenge))
                .unwrap();
            assert!(e.handshake_deadline.is_none());
            e.packet(e.generation, &server_packet(3, 0, &[])).unwrap();
            assert!(e.handshake_deadline.is_none());
            assert_eq!(e.snapshot.state, State::Authenticated);
        }
        let mut spoof = server_packet(4, 0, &[]);
        spoof[6..12].copy_from_slice(&[3; 6]);
        e.packet(e.generation, &spoof).unwrap();
        assert_eq!(e.snapshot.state, State::Authenticated);
        e.packet(e.generation, &server_packet(4, 0, &[])).unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_some());
    }
    #[test]
    fn manual_disconnect_invalidates_pending_success() {
        let mut e = online();
        let old = e.generation;
        e.logoff().unwrap();
        e.handle(Event::Connectivity(old, true));
        assert!(e.manual_disconnect);
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(!e.snapshot.internet);
        assert!(e.retry.is_none());
    }
    #[test]
    fn online_probes_without_success_do_not_expire_the_connection() {
        let mut e = online();
        let generation = e.generation;
        let authenticated_at = e.snapshot.authenticated_at;
        let mut challenge = vec![16];
        challenge.extend(0..16);
        for _ in 0..3 {
            e.packet(generation, &server_packet(1, 1, &[])).unwrap();
            e.packet(generation, &server_packet(1, 4, &challenge))
                .unwrap();
            // The initial handshake timer would force a disconnect here even
            // though an online probe does not necessarily receive EAP-Success.
            assert!(e.handshake_deadline.is_none());
            e.tick();
            assert_eq!(e.snapshot.state, State::Authenticated);
            assert_eq!(e.snapshot.authenticated_at, authenticated_at);
            assert_eq!(e.generation, generation);
            assert!(e.retry.is_none());
        }
    }
    #[test]
    fn online_authenticator_can_change_only_through_a_valid_request() {
        let mut e = online();
        let generation = e.generation;
        let mut raw = server_packet(1, 99, &[]);
        raw[6..12].copy_from_slice(&[4; 6]);
        e.packet(generation, &raw).unwrap();
        assert_eq!(e.peer, Some([2; 6]));
        raw = server_packet(1, 4, &[16, 1]);
        raw[6..12].copy_from_slice(&[4; 6]);
        e.packet(generation, &raw).unwrap();
        assert_eq!(e.peer, Some([2; 6]));
        raw = server_packet(1, 1, &[]);
        raw[6..12].copy_from_slice(&[4; 6]);
        e.packet(generation, &raw).unwrap();
        assert_eq!(e.peer, Some([4; 6]));
        e.packet(generation, &server_packet(4, 0, &[])).unwrap();
        assert_eq!(e.snapshot.state, State::Authenticated);
        assert_eq!(e.generation, generation);
        raw = server_packet(4, 0, &[]);
        raw[6..12].copy_from_slice(&[4; 6]);
        e.packet(generation, &raw).unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_some());
    }
    #[test]
    fn automatic_retry_errors_keep_the_retry_schedule_without_gui() {
        let mut e = online();
        e.snapshot.state = State::Authenticating;
        e.startup = Instant::now();
        e.failed("capture temporarily unavailable");
        assert_eq!(e.retries, 1);
        e.retry = Some(Instant::now());
        // Missing configured credentials exercise authenticate's early-error
        // path without opening a real capture handle on the test host.
        e.tick();
        assert_eq!(e.snapshot.state, State::Idle);
        assert_eq!(e.retries, 2);
        assert!(e.retry.is_some());
        assert!(e.session.is_none());
        e.logoff().unwrap();
        e.tick();
        assert!(e.retry.is_none());
    }
    #[test]
    fn lost_established_session_recovers_promptly_without_stale_candidates() {
        let mut e = online();
        e.startup = Instant::now();
        e.retries = 5;
        e.candidates.push(Adapter::default());
        e.failed("EAP session lost");
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.candidates.is_empty());
        assert_eq!(e.retries, 0);
        assert_eq!(e.startup_retry, 1);
        assert!(e.retry.unwrap() <= Instant::now() + Duration::from_secs(3));
    }
    #[test]
    fn reachability_failure_prevents_authenticated_status() {
        let mut e = online();
        e.snapshot.state = State::Authenticating;
        e.handle(Event::Connectivity(e.generation, false));
        assert!(!e.snapshot.internet);
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_some());
    }
    #[test]
    fn manual_adapter_can_be_renamed_more_than_once() {
        let mut e = engine();
        e.config.iface_mode = "manual".into();
        e.config.iface = "original".into();
        e.cache = Cache {
            iface: "previous-name".into(),
            last_success_mac: protocol::mac_text([2; 6]),
            manual_selector: "original".into(),
        };
        e.adapters = vec![Adapter {
            name: "current-name".into(),
            id: "id".into(),
            mac: [2; 6],
            up: true,
            ethernet: true,
            ..Adapter::default()
        }];
        assert_eq!(e.select()[0].name, "current-name");
        e.config.iface = "different-adapter".into();
        assert!(e.select().is_empty());
    }
    #[test]
    fn incomplete_saved_credentials_report_failure_without_rejecting_settings() {
        let mut e = engine();
        let config = Config {
            username: "user".into(),
            ..Config::default()
        };
        e.apply_config(config).unwrap();
        assert_eq!(e.config.username, "user");
        assert_eq!(e.snapshot.state, State::Failed);
    }
    #[test]
    fn no_adapters_waits_without_spending_authentication_retries() {
        let mut e = engine();
        e.next_candidate().unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_none());
        assert!(e.start_deadline.is_none());
    }
    #[test]
    fn power_notifications_survive_a_full_queue_and_coalesce() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Event::NetworkChanged).unwrap();
        let power = PowerNotifier {
            pending: AtomicU8::new(0),
            events: tx,
        };
        power.notify(PowerEvent::Suspend);
        power.notify(PowerEvent::Resume);
        assert!(matches!(power.take(), Some(PowerEvent::Resume)));
        assert!(power.take().is_none());
        rx.recv().unwrap();
        power.notify(PowerEvent::Resume);
        power.notify(PowerEvent::Suspend);
        assert!(matches!(power.take(), Some(PowerEvent::Suspend)));
    }
    #[test]
    fn suspend_invalidates_online_state_and_blocks_old_work() {
        let mut e = online();
        let old = e.generation;
        e.retry = Some(Instant::now());
        e.handshake_deadline = Some(Instant::now());
        e.power.notify(PowerEvent::Suspend);
        // The latch is drained even when the wake marker is behind old traffic.
        e.handle(Event::Connectivity(old, true));
        e.handle(Event::Packet(old, server_packet(1, 1, &[])));
        e.handle(Event::CaptureError(old, "stale capture".into()));
        e.tick();
        assert!(e.suspended);
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(!e.snapshot.internet);
        assert!(e.snapshot.authenticated_at.is_none());
        assert!(e.snapshot.adapter.is_none());
        assert!(e.retry.is_none());
        assert!(e.timeout().is_none());
        assert!(e.authenticate().is_err());
        let mut config = e.config.clone();
        config.username = "new-user".into();
        config.password = "password".into();
        e.apply_config(config).unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_none());
    }
    #[test]
    fn resume_without_suspend_resets_session_and_restarts_grace() {
        let mut e = online();
        let old = e.generation;
        e.retries = 5;
        e.startup = Instant::now();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::Connectivity(old, true));
        assert!(!e.snapshot.internet);
        assert!(e.snapshot.adapter.is_none());
        assert!(!e.suspended);
        assert!(e.recovering_resume);
        assert_eq!(e.retries, 0);
        assert!(e.startup > Instant::now() + Duration::from_secs(110));
        assert!(e.retry.unwrap() > Instant::now() + Duration::from_secs(1));
        // A temporary capture/driver error must retain recovery, rather than
        // terminating it until the GUI sends another Authenticate command.
        e.failed("capture driver not ready");
        assert!(e.retry.is_some());
        e.retry = Some(Instant::now());
        // Missing credentials deliberately exercise an early authenticate error.
        e.tick();
        assert!(e.retry.is_some());
        assert_eq!(e.snapshot.state, State::Idle);
    }
    #[test]
    fn resume_adapter_wait_is_bounded_and_online_has_no_retry_timer() {
        let mut e = engine();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.retry = None;
        e.next_candidate().unwrap();
        assert!(e.retry.is_some());
        e.startup = Instant::now();
        e.retry = None;
        e.next_candidate().unwrap();
        assert!(e.retry.is_none());
        assert!(!e.recovering_resume);
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.handle(Event::Connectivity(e.generation, true));
        assert!(!e.recovering_resume);
        assert!(e.retry.is_none());
        assert!(e.handshake_deadline.is_none());
        assert!(e.start_deadline.is_none());
    }
    #[test]
    fn resume_respects_manual_disconnect_and_auto_auth_setting() {
        let mut e = online();
        e.logoff().unwrap();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        assert!(e.manual_disconnect);
        assert!(e.preserve_manual_disconnect);
        assert!(e.retry.is_none());
        e = online();
        e.config.auto_auth = false;
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        assert!(!e.recovering_resume);
        assert!(e.retry.is_none());
        assert!(!e.snapshot.internet);
    }
    #[test]
    fn logoff_or_disabling_auto_auth_cancels_resume_recovery() {
        let mut e = engine();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.logoff().unwrap();
        assert!(e.retry.is_none());
        assert!(!e.recovering_resume);
        assert!(e.preserve_manual_disconnect);
        e.manual_disconnect = false;
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        let mut config = e.config.clone();
        config.auto_auth = false;
        e.apply_config(config).unwrap();
        assert!(e.retry.is_none());
        assert!(!e.recovering_resume);
    }
}
