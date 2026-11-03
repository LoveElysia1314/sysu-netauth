use crate::{
    authentication::{self, Exchange},
    config::{self, Cache, Config, Paths},
    connectivity::Probe,
    ipc::{Action, Call, Response, Subscription},
    platform::{self, Adapter, Session},
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
    pub suspended: bool,
    pub manual_disconnect: bool,
    pub message: String,
    pub adapter: Option<Adapter>,
    pub adapters: Vec<Adapter>,
    pub authenticated_at: Option<u64>,
    pub internet: bool,
    pub update: Update,
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
    retry_attempt: u32,
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
        let update = config::read::<Update>(&paths.state.join("update_state.json"))
            .unwrap_or_default()
            .for_current_version(config::VERSION);
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
                suspended: false,
                manual_disconnect: false,
                message: error.unwrap_or_else(|| "待命".into()),
                adapter: None,
                adapters: Vec::new(),
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
            retry_attempt: 0,
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
        self.snapshot.suspended = self.suspended;
        self.snapshot.manual_disconnect = self.manual_disconnect;
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
        self.invalidate();
        self.candidates.clear();
        self.snapshot.adapter = None;
        self.retry = None;
        self.retry_attempt = 0;
        match event {
            PowerEvent::Suspend => {
                self.suspended = true;
                self.status(State::Idle, "系统休眠，等待恢复");
            }
            PowerEvent::Resume => {
                self.suspended = false;
                if self.config.auto_auth && !self.manual_disconnect {
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
        self.set_adapters(platform::adapters()?);
        Ok(())
    }
    fn set_adapters(&mut self, adapters: Vec<Adapter>) {
        if self.adapters != adapters {
            self.adapters = adapters;
            self.snapshot.adapters = self.adapters.clone();
            self.publish();
        }
    }
    fn select(&self) -> Vec<Adapter> {
        let mut available: Vec<_> = self
            .adapters
            .iter()
            .filter(|a| {
                a.candidate() && (self.config.iface_mode != "manual" || a.id == self.config.iface)
            })
            .cloned()
            .collect();
        available.sort_by_key(|a| {
            (
                std::cmp::Reverse(a.id == self.cache.last_success_id),
                std::cmp::Reverse(a.score),
            )
        });
        available
    }
    fn authenticate(&mut self) -> Result<()> {
        ensure!(!self.suspended, "系统休眠中");
        self.retry = None;
        self.invalidate();
        self.candidates.clear();
        self.config.validate_auth()?;
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
        if self.snapshot.state == State::Authenticated {
            self.retry_attempt = 0;
            self.candidates.clear();
        }
        self.retry = None;
        self.invalidate();
        if !self.candidates.is_empty() {
            match self.next_candidate() {
                Ok(()) => return,
                Err(error) => message = error.to_string(),
            }
        }
        if self.manual_disconnect
            || self.suspended
            || !self.config.auto_auth
            || self.config.validate_auth().is_err()
        {
            self.status(State::Failed, message);
            return;
        }
        let delay = 3u64
            .saturating_mul(1u64 << self.retry_attempt.min(12))
            .min(self.config.retry_interval);
        self.retry_attempt = self.retry_attempt.saturating_add(1);
        self.retry = Some(Instant::now() + Duration::from_secs(delay));
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
        let online = self.snapshot.state == State::Authenticated || self.probe.is_some();
        match authentication::receive(
            &mut self.peer,
            adapter.mac,
            &packet,
            &self.username,
            &self.password,
            online,
        )? {
            Exchange::Ignore => {}
            Exchange::Response {
                bytes,
                peer_changed,
            } => {
                if let Some(session) = &self.session {
                    session.capture.send(&bytes)?;
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
                self.start_deadline = None;
            }
            Exchange::Success => {
                self.handshake_deadline = None;
                self.start_deadline = None;
                if self.snapshot.state != State::Authenticated && self.probe.is_none() {
                    self.status(State::Authenticating, "已完成 EAP，验证互联网连通性");
                    self.probe = Some(Probe::start(self.events.clone(), self.generation)?);
                }
            }
            Exchange::Failure => self.failed("EAP 认证失败"),
        }
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
            self.retry_attempt = 0;
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
            && self.config.validate_auth().is_ok()
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
                Action::Preferences => {
                    let mut value = serde_json::to_value(&self.config)?;
                    value.as_object_mut().unwrap().remove("username");
                    value.as_object_mut().unwrap().remove("password");
                    Ok(value)
                }
                Action::Diagnostics => Ok(serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "state": self.snapshot.state,
                    "log": crate::diagnostics::read_log(&self.paths, &self.config)?
                })),
                Action::GetConfig => Ok(serde_json::to_value(&self.config)?),
                Action::Authenticate => {
                    self.connect()?;
                    Ok(serde_json::to_value(&self.snapshot)?)
                }
                Action::Logoff => {
                    self.logoff()?;
                    Ok(serde_json::to_value(&self.snapshot)?)
                }
                Action::SetConfig { config } => {
                    self.save_config(config, false)?;
                    Ok(serde_json::json!({"saved":true}))
                }
                Action::Connect { config } => {
                    ensure!(!self.suspended, "系统休眠中");
                    self.save_config(config, true)?;
                    Ok(serde_json::to_value(&self.snapshot)?)
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
    fn save_config(&mut self, config: Config, connect: bool) -> Result<()> {
        config.validate()?;
        config::save(&self.paths, &config)?;
        #[cfg(windows)]
        if config.service_mode != self.config.service_mode {
            platform::configure_service(config.service_mode)?;
        }
        if connect {
            // Apply settings without its automatic reconnect path, then start once.
            self.config = config;
            self.connect()
        } else {
            self.apply_config(config)
        }
    }
    fn connect(&mut self) -> Result<()> {
        ensure!(!self.suspended, "系统休眠中");
        self.manual_disconnect = false;
        self.retry_attempt = 0;
        self.authenticate()
            .inspect_err(|error| self.status(State::Failed, error.to_string()))
    }
    fn apply_config(&mut self, config: Config) -> Result<()> {
        let changed = self.config.username != config.username
            || self.config.password != config.password
            || self.config.iface != config.iface
            || self.config.iface_mode != config.iface_mode;
        let enabled = !self.config.auto_auth && config.auto_auth;
        self.config = config;
        if enabled {
            self.retry_attempt = 0;
        }
        if !self.config.auto_auth {
            self.retry = None;
        }
        if changed {
            self.invalidate();
            self.retry = None;
            self.retry_attempt = 0;
            self.status(State::Idle, "认证配置已更改");
        }
        if (changed || (enabled && self.snapshot.state != State::Authenticated))
            && self.config.auto_auth
            && !self.manual_disconnect
            && !self.suspended
        {
            if let Err(error) = self.authenticate() {
                self.failed(&error.to_string());
            }
        }
        Ok(())
    }
    fn logoff(&mut self) -> Result<()> {
        self.manual_disconnect = true;
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
                    self.candidates.clear();
                    self.retry_attempt = 0;
                    self.retry = None;
                    if let Some(adapter) = &self.snapshot.adapter {
                        self.cache = Cache {
                            last_success_id: adapter.id.clone(),
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
                    self.failed("EAP 成功但无法访问互联网");
                }
            }
            Event::Connectivity(..) => {}
            Event::Update(result) => {
                let now = config::now();
                match result {
                    Ok(mut update) => {
                        update.retain_newer_release(&self.snapshot.update);
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
        if !self.suspended && self.retry.is_none() {
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
    #[test]
    fn adapter_changes_reach_idle_watchers_without_reconnecting() {
        let mut e = engine();
        let (updates, rx) = mpsc::sync_channel(8);
        e.subscribers.push(Subscription {
            updates,
            alive: Arc::new(AtomicBool::new(true)),
        });
        let adapter = Adapter {
            id: "usb".into(),
            name: "USB Ethernet".into(),
            ethernet: true,
            up: true,
            ..Adapter::default()
        };
        e.set_adapters(vec![adapter.clone()]);
        assert_eq!(
            rx.recv().unwrap().unwrap().adapters.as_slice(),
            std::slice::from_ref(&adapter)
        );
        e.set_adapters(vec![adapter]);
        assert!(rx.try_recv().is_err());
        e.set_adapters(vec![]);
        assert!(rx.recv().unwrap().unwrap().adapters.is_empty());
        assert_eq!(e.snapshot.state, State::Idle);
    }
    #[test]
    fn preferences_exclude_credentials_and_preserve_internal_retry_settings() {
        let mut e = engine();
        e.config.username = "offline-user".into();
        e.config.password = "offline-password".into();
        e.config.retry_interval = 125;
        let (tx, rx) = mpsc::sync_channel(1);
        e.call(Call {
            request: crate::ipc::Request {
                version: 1,
                id: 42,
                action: Action::Preferences,
            },
            reply: tx,
            updates: None,
        });
        let response = rx.recv().unwrap();
        assert!(response.ok);
        assert_eq!(response.id, 42);
        assert_eq!(response.data["retry_interval"], 125);
        assert!(response.data.get("username").is_none());
        assert!(response.data.get("password").is_none());
    }
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
        e.cache.last_success_id = "saved".into();
        assert_eq!(e.select()[0].name, "saved");
        e.config.iface_mode = "manual".into();
        e.config.iface = "new".into();
        assert_eq!(e.select().len(), 1);
        assert_eq!(e.select()[0].name, "new");
    }
    #[test]
    fn retries_back_off_to_the_configured_cap_without_permanent_exhaustion() {
        let mut e = online();
        e.snapshot.state = State::Authenticating;
        for expected in [3, 6, 12, 24, 48, 60, 60, 60, 60, 60] {
            let before = Instant::now();
            e.failed("temporary network failure");
            let delay = e.retry.unwrap().duration_since(before);
            assert!(delay >= Duration::from_secs(expected));
            assert!(delay < Duration::from_secs(expected + 1));
        }
        e.config.auto_auth = false;
        e.failed("temporary network failure");
        assert!(e.retry.is_none());
        assert_eq!(e.snapshot.state, State::Failed);
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
        e.config.username = "user".into();
        e.config.password = "password".into();
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
        assert!(e.snapshot.manual_disconnect);
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
    fn local_configuration_errors_wait_for_correction_instead_of_retrying() {
        let mut e = online();
        e.config.password.clear();
        e.failed("missing password");
        assert_eq!(e.snapshot.state, State::Failed);
        assert!(e.retry.is_none());
        e.config.password = "password".into();
        e.failed("capture temporarily unavailable");
        assert!(e.retry.is_some());
        e.logoff().unwrap();
        assert!(e.retry.is_none());
    }
    #[test]
    fn lost_established_session_recovers_promptly_without_stale_candidates() {
        let mut e = online();
        e.retry_attempt = 100;
        e.candidates.push(Adapter::default());
        e.failed("EAP session lost");
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.candidates.is_empty());
        assert_eq!(e.retry_attempt, 1);
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
    fn manual_selection_uses_only_device_identity_across_renames() {
        let mut e = engine();
        e.config.iface_mode = "manual".into();
        e.config.iface = "device-id".into();
        e.adapters = vec![Adapter {
            id: "device-id".into(),
            name: "renamed twice".into(),
            mac: [2; 6],
            up: true,
            ethernet: true,
            ..Adapter::default()
        }];
        assert_eq!(e.select()[0].name, "renamed twice");
        e.adapters[0].id = "another-device".into();
        e.adapters[0].name = "device-id".into();
        e.cache.last_success_id = "another-device".into();
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
        assert!(e.snapshot.suspended);
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
    fn resume_without_suspend_invalidates_old_work_and_uses_the_same_retry_policy() {
        let mut e = online();
        let old = e.generation;
        e.retry_attempt = 100;
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::Connectivity(old, true));
        assert!(!e.snapshot.internet);
        assert!(e.snapshot.adapter.is_none());
        assert!(!e.suspended);
        assert_eq!(e.retry_attempt, 0);
        assert!(e.retry.unwrap() > Instant::now() + Duration::from_secs(1));
        e.failed("driver not ready");
        assert_eq!(e.retry_attempt, 1);
        assert!(e.retry.is_some());
    }
    #[test]
    fn missing_adapters_wait_for_network_events_and_online_has_no_retry_timer() {
        let mut e = engine();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.retry = None;
        e.next_candidate().unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_none());
        e.handle(Event::Connectivity(e.generation, true));
        assert!(e.retry.is_none());
        assert!(e.handshake_deadline.is_none());
        assert!(e.start_deadline.is_none());
    }
    #[test]
    fn manual_disconnect_survives_network_changes_and_resume() {
        let mut e = online();
        e.logoff().unwrap();
        e.adapters.clear();
        e.network_changed().unwrap();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.network_changed().unwrap();
        assert!(e.manual_disconnect);
        assert!(e.snapshot.manual_disconnect);
        assert!(e.retry.is_none());
        e = online();
        e.config.auto_auth = false;
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        assert!(e.retry.is_none());
        assert!(!e.snapshot.internet);
    }
    #[test]
    fn logoff_or_disabling_auto_auth_cancels_scheduled_recovery() {
        let mut e = online();
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        e.logoff().unwrap();
        assert!(e.retry.is_none());
        assert!(e.manual_disconnect);
        e.manual_disconnect = false;
        e.power.notify(PowerEvent::Resume);
        e.handle(Event::PowerChanged);
        let mut config = e.config.clone();
        config.auto_auth = false;
        e.apply_config(config).unwrap();
        assert!(e.retry.is_none());
    }
    #[test]
    fn saving_and_connecting_starts_exactly_one_authentication() {
        let mut e = engine();
        let config = Config {
            username: "example-user".into(),
            password: "example-password".into(),
            iface_mode: "manual".into(),
            iface: "nonexistent-test-device".into(),
            ..Config::default()
        };
        e.save_config(config.clone(), true).unwrap();
        assert_eq!(e.generation, 2);
        assert!(!e.manual_disconnect);
        assert_eq!(e.config, config);
        e.reload_changed_config();
        assert_eq!(e.generation, 2);
        e.logoff().unwrap();
        assert!(e.manual_disconnect);
        e.save_config(e.config.clone(), true).unwrap();
        assert!(!e.manual_disconnect);
        assert_eq!(e.generation, 5);
        std::fs::remove_dir_all(&e.paths.state).unwrap();
    }
}
