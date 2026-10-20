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
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

pub enum Event {
    Packet(u64, Vec<u8>),
    CaptureError(u64, String),
    NetworkChanged,
    Connectivity(u64, bool),
    Update(Result<Update, String>),
    Call(Call),
    Stop,
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
    renewing: bool,
    subscribers: Vec<Subscription>,
    update_deadline: Instant,
    last_manual_update: Option<Instant>,
}
impl Engine {
    pub fn paths(&self) -> &Paths {
        &self.paths
    }
    pub fn new(paths: Paths, events: SyncSender<Event>) -> Result<Self> {
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
            events,
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
            renewing: false,
            subscribers: Vec::new(),
            update_deadline: now + Duration::from_secs(delay),
            last_manual_update: None,
        })
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
        self.renewing = false;
    }
    pub fn start(&mut self) {
        if let Err(e) = self.refresh_adapters() {
            self.status(State::Failed, e.to_string());
            return;
        }
        if self.config.auto_auth {
            if let Err(e) = self.authenticate() {
                self.status(State::Failed, e.to_string());
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
        self.retry = None;
        self.invalidate();
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
        self.invalidate();
        if !self.candidates.is_empty() {
            if let Err(e) = self.next_candidate() {
                self.status(State::Failed, e.to_string());
            }
            return;
        }
        if self.manual_disconnect || !self.config.auto_auth {
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
            || (packet.destination != adapter.mac && packet.destination != protocol::PAE)
        {
            return Ok(());
        }
        if let Some(peer) = self.peer {
            if peer != packet.source {
                return Ok(());
            }
        } else if packet.code == 1 {
            self.peer = Some(packet.source);
        } else {
            return Ok(());
        }
        if packet.code == 1 {
            let destination = if self.snapshot.state == State::Authenticated || self.probe.is_some()
            {
                protocol::PAE
            } else {
                packet.source
            };
            if let Some(response) = protocol::answer(
                adapter.mac,
                destination,
                &packet,
                &self.username,
                &self.password,
            )? {
                if let Some(session) = &self.session {
                    session.capture.send(&response)?;
                }
                self.start_deadline = None;
                if packet.kind == Some(4) && self.snapshot.state == State::Authenticated {
                    self.renewing = true;
                    self.handshake_deadline = Some(Instant::now() + Duration::from_secs(10));
                }
            }
        } else if packet.code == 3 {
            self.handshake_deadline = None;
            self.start_deadline = None;
            if self.snapshot.state == State::Authenticated {
                self.renewing = false;
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
                            let _ = tx.try_send(Event::Connectivity(generation, true));
                            return;
                        }
                    }
                    if round < 2 && !platform::cancellable_wait(&cancel, Duration::from_secs(10)) {
                        return;
                    }
                }
                let _ = tx.try_send(Event::Connectivity(generation, false));
            })?;
        self.probe = Some(Probe {
            stop,
            thread: Some(thread),
        });
        Ok(())
    }
    fn network_changed(&mut self) -> Result<()> {
        let previous_media = self.adapters.iter().any(Adapter::candidate);
        self.refresh_adapters()?;
        let current_media = self.adapters.iter().any(Adapter::candidate);
        if !previous_media && current_media {
            self.manual_disconnect = false;
            self.retries = 0;
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
                return Ok(());
            }
            self.invalidate();
            self.snapshot.adapter = None;
            self.retry = None;
            self.status(State::Idle, "网卡已断开");
        }
        if self.config.auto_auth && !self.manual_disconnect && self.session.is_none() {
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
                let _ = tx.try_send(Event::Update(update::fetch().map_err(|e| e.to_string())));
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
                    self.manual_disconnect = false;
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
        }
        if credentials_changed {
            self.invalidate();
            self.status(State::Idle, "认证配置已更改");
        }
        if (credentials_changed || (enabled && self.snapshot.state != State::Authenticated))
            && self.config.auto_auth
            && !self.manual_disconnect
        {
            if let Err(e) = self.authenticate() {
                self.status(State::Failed, e.to_string());
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
        let now = Instant::now();
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
                self.status(State::Failed, e.to_string());
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
        [
            self.retry,
            self.handshake_deadline,
            self.start_deadline,
            (!self.snapshot.update.checking).then_some(self.update_deadline),
        ]
        .into_iter()
        .flatten()
        .min()
        .map(|d| d.saturating_duration_since(Instant::now()))
    }
    pub fn handle(&mut self, event: Event) -> bool {
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
            Event::NetworkChanged => {
                if let Err(e) = self.network_changed() {
                    self.status(State::Failed, e.to_string());
                }
            }
            Event::Connectivity(g, success) if g == self.generation => {
                self.probe = None;
                if success {
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
        self.start();
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
            assert!(e.handshake_deadline.is_some());
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
    fn no_adapters_waits_for_notifications_without_retry_polling() {
        let mut e = engine();
        e.next_candidate().unwrap();
        assert_eq!(e.snapshot.state, State::Idle);
        assert!(e.retry.is_none());
        assert!(e.start_deadline.is_none());
    }
}
