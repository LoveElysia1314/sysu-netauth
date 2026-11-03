#![cfg(target_os = "macos")]
use netauth::{
    config::{self, Config, Paths},
    ipc::{self, Action},
    platform,
};
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires root; runs only against private state and loopback"]
fn service_ipc_and_sigterm_shutdown() {
    assert!(platform::is_admin());
    let directory =
        std::env::temp_dir().join(format!("netauth-macos-service-{}", std::process::id()));
    config::private_dir(&directory).unwrap();
    let paths = Paths {
        config: directory.join("config.json"),
        state: directory.clone(),
        endpoint: directory.join("control.sock"),
    };
    config::save(
        &paths,
        &Config {
            auto_auth: false,
            ..Config::default()
        },
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_sysu_netauth_service"))
        .args(["--data-dir", directory.to_str().unwrap()])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !paths.endpoint.exists() {
        assert!(
            running.0.try_wait().unwrap().is_none(),
            "service exited during startup"
        );
        assert!(
            Instant::now() < deadline,
            "service did not create IPC endpoint"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        ipc::request(&paths, Action::Status).unwrap().data["state"],
        "idle"
    );
    assert_eq!(
        ipc::request(&paths, Action::GetConfig).unwrap().data["auto_auth"],
        false
    );
    assert!(platform::ping(
        std::net::Ipv4Addr::LOCALHOST,
        &platform::Cancel::new().unwrap()
    ));
    unsafe {
        libc::kill(running.0.id() as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = running.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "SIGTERM did not stop service");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!paths.endpoint.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires root and an available Ethernet adapter; never transmits frames"]
fn bpf_capture_is_cancellable() {
    assert!(platform::is_admin());
    let Some(adapter) = platform::adapters()
        .unwrap()
        .into_iter()
        .find(|a| a.candidate())
    else {
        eprintln!("No Ethernet adapter; BPF capture check skipped");
        return;
    };
    let capture = platform::Capture::open(&adapter).unwrap();
    let stop = Arc::new(platform::Cancel::new().unwrap());
    let reader_stop = stop.clone();
    let (finished, completion) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        while capture.receive(&reader_stop).unwrap().is_some() {}
        finished.send(()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));
    stop.set();
    completion
        .recv_timeout(Duration::from_secs(5))
        .expect("BPF read did not cancel");
    reader.join().unwrap();
}
