//! Runs in a private user/network namespace: no campus network or root login needed.
#![cfg(target_os = "linux")]
use netauth::{
    config::{self, Config, Paths},
    engine::{Engine, Event, PowerEvent, Snapshot, State},
    ipc::{self, Action, Call, Request, Response, Subscription},
    platform::{self, Session},
    protocol,
};
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let _ = self.0.wait();
    }
}

struct EngineGuard {
    events: mpsc::SyncSender<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for EngineGuard {
    fn drop(&mut self) {
        let _ = self.events.send(Event::Stop);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn wait_state(rx: &mpsc::Receiver<Option<Snapshot>>, state: State) -> Snapshot {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let s = rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .expect("state transition timed out")
            .unwrap();
        if s.state == state {
            return s;
        }
    }
}
fn recv_start(rx: &mpsc::Receiver<Event>, client: [u8; 6]) {
    let expected = protocol::control(client, 1);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Event::Packet(_, bytes) = rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .expect("service did not initiate authentication")
        {
            if bytes.starts_with(&expected) {
                return;
            }
        }
    }
}
fn engine_request(tx: &mpsc::SyncSender<Event>, action: Action) -> Response {
    let (reply, rx) = mpsc::sync_channel(1);
    tx.send(Event::Call(Call {
        request: Request {
            version: 1,
            id: 1,
            action,
        },
        reply,
        updates: None,
    }))
    .unwrap();
    let response = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(response.ok, "{:?}", response.error);
    response
}

#[test]
#[ignore = "requires user/network namespaces and iproute2; run with --ignored --nocapture"]
fn isolated_resume_reauthenticates_without_gui_or_link_change() {
    if std::env::var_os("NETAUTH_IN_NAMESPACE").is_none() {
        let status = Command::new("unshare")
            .args(["--user", "--map-root-user", "--net"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_resume_reauthenticates_without_gui_or_link_change",
                "--ignored",
                "--nocapture",
            ])
            .env("NETAUTH_IN_NAMESPACE", "1")
            .env("NETAUTH_TEST_VETH", "1")
            .status()
            .unwrap();
        assert!(status.success(), "isolated resume test failed");
        return;
    }
    ip(&["link", "set", "lo", "up"]);
    ip(&[
        "link", "add", "nactest0", "type", "veth", "peer", "name", "napeer0",
    ]);
    ip(&["addr", "add", "10.10.0.2/24", "dev", "nactest0"]);
    ip(&["addr", "add", "119.29.29.29/32", "dev", "napeer0"]);
    ip(&["link", "set", "nactest0", "up"]);
    ip(&["link", "set", "napeer0", "up"]);
    ip(&["route", "add", "119.29.29.29/32", "dev", "nactest0"]);
    let dir = std::env::temp_dir().join(format!("netauth-resume-{}", std::process::id()));
    config::private_dir(&dir).unwrap();
    let paths = Paths {
        config: dir.join("config.json"),
        state: dir.clone(),
        endpoint: dir.join("control.sock"),
    };
    config::save(
        &paths,
        &Config {
            username: "test-user".into(),
            password: "密码".into(),
            iface_mode: "manual".into(),
            iface: "nactest0".into(),
            ..Config::default()
        },
    )
    .unwrap();
    let adapters = platform::adapters().unwrap();
    let server = adapters.iter().find(|a| a.name == "napeer0").unwrap();
    let client = adapters.iter().find(|a| a.name == "nactest0").unwrap();
    let (capture_tx, capture_rx) = mpsc::sync_channel(128);
    let session = Session::open(server, 1, capture_tx).unwrap();
    let (tx, rx) = mpsc::sync_channel(256);
    let mut engine = Engine::new(paths, tx.clone()).unwrap();
    let power = engine.power_notifier();
    let (updates_tx, updates_rx) = mpsc::sync_channel(32);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    tx.send(Event::Call(Call {
        request: Request {
            version: 1,
            id: 1,
            action: Action::Watch,
        },
        reply: reply_tx,
        updates: Some(Subscription {
            updates: updates_tx,
            alive: Arc::new(AtomicBool::new(true)),
        }),
    }))
    .unwrap();
    let running = EngineGuard {
        events: tx.clone(),
        thread: Some(std::thread::spawn(move || engine.run(rx))),
    };
    assert!(reply_rx.recv_timeout(Duration::from_secs(5)).unwrap().ok);
    let mut challenge = vec![16];
    challenge.extend(0..16);
    // Initial authentication, then a normal sleep/wake, then a wake whose
    // suspend event was missed. No GUI or Authenticate IPC command is used.
    for round in 0..3 {
        if round > 0 {
            // Remove old control traffic before checking for a fresh Start.
            while capture_rx.try_recv().is_ok() {}
            if round == 1 {
                power.notify(PowerEvent::Suspend);
                let s = wait_state(&updates_rx, State::Idle);
                assert!(!s.internet);
                assert!(s.adapter.is_none());
            }
            assert!(
                platform::adapters()
                    .unwrap()
                    .iter()
                    .any(|a| { a.name == "nactest0" && a.candidate() })
            );
            power.notify(PowerEvent::Resume);
            let s = wait_state(&updates_rx, State::Idle);
            assert!(!s.internet);
            // Simulate unrelated address/route notifications with unchanged link.
            tx.send(Event::NetworkChanged).unwrap();
        }
        recv_start(&capture_rx, client.mac);
        session
            .capture
            .send(&request(server.mac, client.mac, 7, 1, &[]))
            .unwrap();
        let identity = recv(&capture_rx, 2, Some(1));
        assert_eq!(protocol::parse(&identity).unwrap().data, b"test-user");
        let md5_request = request(server.mac, client.mac, 8, 4, &challenge);
        session.capture.send(&md5_request).unwrap();
        let md5 = recv(&capture_rx, 2, Some(4));
        let expected = protocol::answer(
            client.mac,
            server.mac,
            &protocol::parse(&md5_request).unwrap(),
            b"test-user",
            &protocol::gbk("密码").unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(md5, expected);
        session
            .capture
            .send(&success(server.mac, client.mac, 8))
            .unwrap();
        assert!(wait_state(&updates_rx, State::Authenticated).internet);
    }
    // A deliberate logoff must survive the artificial down/up edge caused by
    // powering an adapter back on. Observe down before sleep, then up on wake.
    engine_request(&tx, Action::Logoff);
    wait_state(&updates_rx, State::Idle);
    ip(&["link", "set", "nactest0", "down"]);
    tx.send(Event::NetworkChanged).unwrap();
    wait_state(&updates_rx, State::Idle);
    power.notify(PowerEvent::Suspend);
    wait_state(&updates_rx, State::Idle);
    ip(&["link", "set", "nactest0", "up"]);
    power.notify(PowerEvent::Resume);
    wait_state(&updates_rx, State::Idle);
    tx.send(Event::NetworkChanged).unwrap();
    assert_eq!(engine_request(&tx, Action::Status).data["state"], "idle");
    let end = Instant::now() + Duration::from_secs(3);
    while let Ok(event) = capture_rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        if let Event::Packet(_, bytes) = event {
            assert!(
                !bytes.starts_with(&protocol::control(client.mac, 1)),
                "resume ignored manual disconnect"
            );
        }
    }
    drop(running);
    std::fs::remove_dir_all(dir).unwrap();
}
fn ip(args: &[&str]) {
    assert!(
        Command::new("ip").args(args).status().unwrap().success(),
        "ip {args:?}"
    );
}
fn recv(rx: &mpsc::Receiver<Event>, code: u8, kind: Option<u8>) -> Vec<u8> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Ok(Event::Packet(_, bytes)) =
            rx.recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            if let Some(p) = protocol::parse(&bytes) {
                if p.code == code && p.kind == kind {
                    return bytes;
                }
            }
        }
    }
    panic!("no EAP packet code={code} kind={kind:?}");
}
fn request(src: [u8; 6], dst: [u8; 6], id: u8, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut frame = protocol::response(src, dst, id, kind, data).unwrap();
    frame[18] = 1;
    frame
}
fn success(src: [u8; 6], dst: [u8; 6], id: u8) -> Vec<u8> {
    let mut frame = protocol::response(src, dst, id, 1, &[]).unwrap();
    frame.truncate(22);
    frame[16..18].copy_from_slice(&4u16.to_be_bytes());
    frame[18] = 3;
    frame[20..22].copy_from_slice(&4u16.to_be_bytes());
    frame
}

#[test]
#[ignore = "requires user/network namespaces and iproute2; run with --ignored --nocapture"]
fn isolated_service_authentication_and_renewal() {
    if std::env::var_os("NETAUTH_IN_NAMESPACE").is_none() {
        let status = Command::new("unshare")
            .args(["--user", "--map-root-user", "--net"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_service_authentication_and_renewal",
                "--ignored",
                "--nocapture",
            ])
            .env("NETAUTH_IN_NAMESPACE", "1")
            .status()
            .unwrap();
        assert!(status.success(), "isolated integration test failed");
        return;
    }
    ip(&["link", "set", "lo", "up"]);
    ip(&[
        "link", "add", "nactest0", "type", "veth", "peer", "name", "napeer0",
    ]);
    ip(&["addr", "add", "10.10.0.2/24", "dev", "nactest0"]);
    ip(&["addr", "add", "10.10.0.1/24", "dev", "napeer0"]);
    ip(&["addr", "add", "119.29.29.29/32", "dev", "napeer0"]);
    ip(&["link", "set", "nactest0", "up"]);
    ip(&["link", "set", "napeer0", "up"]);
    ip(&["route", "add", "119.29.29.29/32", "dev", "nactest0"]);
    let dir = std::env::temp_dir().join(format!("netauth-integration-{}", std::process::id()));
    config::private_dir(&dir).unwrap();
    let paths = Paths {
        config: dir.join("config.json"),
        state: dir.clone(),
        endpoint: dir.join("control.sock"),
    };
    let cfg = Config {
        username: "test-user".into(),
        password: "密码".into(),
        iface_mode: "manual".into(),
        iface: "nactest0".into(),
        ..Config::default()
    };
    config::save(&paths, &cfg).unwrap();
    let config_before = std::fs::read(&paths.config).unwrap();
    let adapters = platform::adapters().unwrap();
    let server = adapters.iter().find(|a| a.name == "napeer0").unwrap();
    let client = adapters.iter().find(|a| a.name == "nactest0").unwrap();
    let (tx, rx) = mpsc::sync_channel(128);
    let session = Session::open(server, 1, tx).unwrap();
    let service = Command::new(env!("CARGO_BIN_EXE_sysu_netauth_service"))
        .args(["--data-dir", dir.to_str().unwrap()])
        .env("NETAUTH_TEST_VETH", "1")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(service);
    for _ in 0..100 {
        if paths.endpoint.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(paths.endpoint.exists());
    let mut watch = ipc::connect(&paths).unwrap();
    ipc::write_frame(
        &mut watch,
        &Request {
            version: 1,
            id: 7,
            action: Action::Watch,
        },
    )
    .unwrap();
    let first: Response = ipc::read_frame(&mut watch).unwrap();
    assert!(first.ok);
    // Reject unsupported IPC versions without executing their action.
    let mut bad = ipc::connect(&paths).unwrap();
    ipc::write_frame(
        &mut bad,
        &Request {
            version: 99,
            id: 4,
            action: Action::Logoff,
        },
    )
    .unwrap();
    assert!(!ipc::read_frame::<Response>(&mut bad).unwrap().ok);
    session
        .capture
        .send(&request(server.mac, client.mac, 7, 1, &[]))
        .unwrap();
    let identity = recv(&rx, 2, Some(1));
    assert_eq!(protocol::parse(&identity).unwrap().data, b"test-user");
    let mut challenge = vec![16];
    challenge.extend(0..16);
    session
        .capture
        .send(&request(server.mac, client.mac, 8, 4, &challenge))
        .unwrap();
    let md5 = recv(&rx, 2, Some(4));
    let p = protocol::parse(&md5).unwrap();
    assert_eq!(p.data.len(), 26);
    let input = request(server.mac, client.mac, 8, 4, &challenge);
    let expected = protocol::answer(
        client.mac,
        server.mac,
        &protocol::parse(&input).unwrap(),
        b"test-user",
        &protocol::gbk("密码").unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(md5, expected);
    session
        .capture
        .send(&success(server.mac, client.mac, 8))
        .unwrap();
    // Renewal requests must be answered during the five-second Internet check.
    session
        .capture
        .send(&request(server.mac, client.mac, 9, 1, &[]))
        .unwrap();
    recv(&rx, 2, Some(1));
    session
        .capture
        .send(&request(server.mac, client.mac, 10, 4, &challenge))
        .unwrap();
    recv(&rx, 2, Some(4));
    session
        .capture
        .send(&success(server.mac, client.mac, 10))
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut authenticated = false;
    while Instant::now() < end {
        let s: netauth::engine::Snapshot = ipc::read_frame(&mut watch).unwrap();
        if s.state == State::Authenticated {
            assert!(s.internet);
            authenticated = true;
            break;
        }
    }
    assert!(authenticated, "reachability check did not complete");
    // Multiple online renewal handshakes reuse the existing capture session.
    for id in 11..14 {
        session
            .capture
            .send(&request(server.mac, client.mac, id, 1, &[]))
            .unwrap();
        recv(&rx, 2, Some(1));
        session
            .capture
            .send(&request(server.mac, client.mac, id, 4, &challenge))
            .unwrap();
        recv(&rx, 2, Some(4));
        session
            .capture
            .send(&success(server.mac, client.mac, id))
            .unwrap();
    }
    let response = ipc::request(&paths, Action::Status).unwrap();
    assert_eq!(response.data["state"], "authenticated");
    assert_eq!(config_before, std::fs::read(&paths.config).unwrap());
    // Repeatedly reopening a watcher must release slots even without state changes.
    for id in 20..52 {
        let mut transient = ipc::connect(&paths).unwrap();
        ipc::write_frame(
            &mut transient,
            &Request {
                version: 1,
                id,
                action: Action::Watch,
            },
        )
        .unwrap();
        assert!(ipc::read_frame::<Response>(&mut transient).unwrap().ok);
        drop(transient);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(ipc::request(&paths, Action::Status).is_ok());
    if let Some(seconds) = std::env::var_os("NETAUTH_MEASURE_SECONDS") {
        let measure = std::path::Path::new(env!("CARGO_BIN_EXE_sysu_netauth_service"))
            .parent()
            .unwrap()
            .join("examples/measure_linux");
        let status = Command::new(measure)
            .arg(child.0.id().to_string())
            .arg(seconds)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let direct = Command::new(env!("CARGO_BIN_EXE_sysu_netauth"))
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "logoff",
            "--iface",
            "nactest0",
        ])
        .output()
        .unwrap();
    assert!(!direct.status.success());
    assert!(String::from_utf8_lossy(&direct.stderr).contains("another service"));
    ipc::request(&paths, Action::Logoff).unwrap();
    let s = ipc::request(&paths, Action::Status).unwrap();
    assert_eq!(s.data["state"], "idle");
    // Down/up invalidates the old session and makes authentication available again.
    ip(&["link", "set", "nactest0", "down"]);
    std::thread::sleep(Duration::from_millis(100));
    ip(&["link", "set", "nactest0", "up"]);
    std::thread::sleep(Duration::from_millis(150));
    assert!(ipc::request(&paths, Action::Status).is_ok());
    let result = Command::new(env!("CARGO_BIN_EXE_sysu_netauth"))
        .args(["--data-dir", dir.to_str().unwrap(), "--json", "status"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(serde_json::from_slice::<serde_json::Value>(&result.stdout).is_ok());
    assert!(!dir.join("status.json").exists());
    assert!(!dir.join("commands").exists());
    unsafe {
        libc::kill(child.0.id() as i32, libc::SIGTERM);
    }
    assert!(child.0.wait().unwrap().success());
    std::fs::remove_dir_all(dir).unwrap();
}
