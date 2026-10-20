use anyhow::{Result, bail, ensure};
use clap::{Parser, Subcommand};
use netauth::{
    config::{self, Paths},
    engine::{Event, Snapshot, State},
    ipc::{self, Action},
    platform, protocol,
};
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "SYSU wired 802.1X client — native Rust edition")]
struct Args {
    #[arg(long, global = true)]
    json: bool,
    /// Override data location (Linux also isolates the IPC endpoint).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// List adapters using OS APIs, without requiring a running service.
    Interfaces,
    /// Send EAPOL-Start and wait for an authentication server.
    Probe {
        #[arg(short, long)]
        iface: String,
        #[arg(long, default_value_t = 10)]
        timeout: u64,
    },
    /// Request service authentication, or authenticate directly when --username is supplied.
    Authenticate {
        #[arg(short, long)]
        iface: Option<String>,
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short, long)]
        password: Option<String>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Disconnect via the service, or send a direct logoff on --iface.
    Logoff {
        #[arg(short, long)]
        iface: Option<String>,
    },
    Status,
    Watch,
    /// Show configuration (password redacted), import JSON, or set a password interactively.
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    CheckUpdate,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Import { file: PathBuf },
    Password,
}

fn paths(dir: Option<PathBuf>) -> Paths {
    let Some(dir) = dir else {
        return Paths::default();
    };
    #[cfg(windows)]
    let endpoint = PathBuf::from(r"\\.\pipe\SYSUNetAuth-v1");
    #[cfg(unix)]
    let endpoint = dir.join("control.sock");
    Paths {
        config: dir.join("config.json"),
        state: dir,
        endpoint,
    }
}
fn output(value: &impl serde::Serialize, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}
fn snapshot(s: &Snapshot, json: bool) -> Result<()> {
    if json {
        output(s, true)
    } else {
        println!("{:?}: {}", s.state, s.message);
        if let Some(a) = &s.adapter {
            println!(
                "{}  {}  IPv4={}  gateway={}  DNS={}",
                a.name,
                protocol::mac_text(a.mac),
                a.ipv4,
                a.gateway,
                a.dns
            );
        }
        Ok(())
    }
}
fn direct(
    paths: &Paths,
    iface: Option<String>,
    username: Option<String>,
    password: Option<String>,
    timeout: u64,
    probe: bool,
    json: bool,
) -> Result<()> {
    ensure!(
        platform::is_admin(),
        "direct packet access requires administrator/root privileges"
    );
    ensure!(
        (1..=3600).contains(&timeout),
        "timeout must be 1–3600 seconds"
    );
    let _guard = platform::instance_lock(paths)?;
    let mut adapters = platform::adapters()?;
    adapters.retain(|a| {
        a.candidate()
            && iface
                .as_ref()
                .is_none_or(|name| a.name == *name || a.id == *name)
    });
    ensure!(
        !adapters.is_empty(),
        "no matching connected Ethernet adapter"
    );
    let user = protocol::gbk(username.as_deref().unwrap_or_default())?;
    let password = if probe {
        Vec::new()
    } else {
        protocol::gbk(&match password {
            Some(p) => p,
            None => rpassword::prompt_password("Password: ")?,
        })?
    };
    let (tx, rx) = mpsc::sync_channel(256);
    #[cfg(unix)]
    platform::signal_thread(tx.clone())?;
    #[cfg(windows)]
    platform::register_stop_sender(tx.clone());
    let mut last = "no EAP server response".to_string();
    for adapter in adapters {
        let session = platform::Session::open(&adapter, 1, tx.clone())?;
        session.capture.send(&protocol::control(adapter.mac, 1))?;
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let mut next_start = Instant::now() + Duration::from_secs(1);
        let mut peer = None;
        let mut eap_success = false;
        let stop = std::sync::Arc::new(platform::Cancel::new()?);
        let mut checker = None;
        let mut failure = false;
        while Instant::now() < deadline {
            let now = Instant::now();
            let wait = deadline
                .min(if peer.is_none() { next_start } else { deadline })
                .saturating_duration_since(now);
            match rx.recv_timeout(wait) {
                Ok(Event::Packet(_, bytes)) => {
                    let Some(packet) = protocol::parse(&bytes) else {
                        continue;
                    };
                    if packet.source == adapter.mac
                        || (packet.destination != adapter.mac
                            && packet.destination != protocol::PAE)
                    {
                        continue;
                    }
                    if peer.is_some_and(|mac| mac != packet.source) {
                        continue;
                    }
                    if probe && packet.code == 1 {
                        output(
                            &serde_json::json!({"iface":adapter.name,"server":protocol::mac_text(packet.source),"eap_server":true}),
                            json,
                        )?;
                        return Ok(());
                    }
                    if packet.code == 1 {
                        peer = Some(packet.source);
                        if let Some(answer) =
                            protocol::answer(adapter.mac, packet.source, &packet, &user, &password)?
                        {
                            session.capture.send(&answer)?;
                        }
                    } else if packet.code == 4 {
                        last = "EAP authentication rejected".into();
                        failure = true;
                        break;
                    } else if packet.code == 3 && peer.is_some() && !eap_success {
                        eap_success = true;
                        let tx = tx.clone();
                        let cancel = stop.clone();
                        checker = Some(std::thread::spawn(move || {
                            if !platform::cancellable_wait(&cancel, Duration::from_secs(5)) {
                                return;
                            }
                            for round in 0..3 {
                                for ip in [[119, 29, 29, 29], [114, 114, 114, 114]] {
                                    if platform::ping(ip.into(), &cancel) {
                                        let _ = tx.try_send(Event::Connectivity(1, true));
                                        return;
                                    }
                                }
                                if round < 2
                                    && !platform::cancellable_wait(&cancel, Duration::from_secs(10))
                                {
                                    return;
                                }
                            }
                            let _ = tx.try_send(Event::Connectivity(1, false));
                        }));
                    }
                }
                Ok(Event::Connectivity(_, ok)) => {
                    stop.set();
                    if let Some(t) = checker.take() {
                        let _ = t.join();
                    }
                    if ok {
                        let s = Snapshot {
                            state: State::Authenticated,
                            message: "已认证，互联网连通".into(),
                            adapter: Some(adapter.clone()),
                            authenticated_at: Some(config::now()),
                            internet: true,
                            update: Default::default(),
                        };
                        snapshot(&s, json)?;
                        return Ok(());
                    }
                    last = "EAP succeeded but Internet is unreachable".into();
                    failure = true;
                    break;
                }
                Ok(Event::CaptureError(_, error)) => {
                    last = error;
                    failure = true;
                    break;
                }
                Ok(Event::Stop) => {
                    stop.set();
                    if let Some(t) = checker.take() {
                        let _ = t.join();
                    }
                    bail!("interrupted");
                }
                _ => {}
            }
            if peer.is_none() && Instant::now() >= next_start {
                session.capture.send(&protocol::control(adapter.mac, 1))?;
                next_start = Instant::now() + Duration::from_secs(1);
            }
        }
        stop.set();
        if let Some(t) = checker {
            let _ = t.join();
        }
        if !failure {
            last = "authentication timed out".into();
        }
    }
    bail!("{last}")
}
fn run() -> Result<()> {
    let args = Args::parse();
    let paths = paths(args.data_dir);
    match args.command {
        Command::Interfaces => output(&platform::adapters()?, args.json),
        Command::Probe { iface, timeout } => {
            direct(&paths, Some(iface), None, None, timeout, true, args.json)
        }
        Command::Authenticate {
            iface,
            username,
            password,
            timeout,
        } => {
            if username.is_some() {
                direct(&paths, iface, username, password, timeout, false, args.json)
            } else {
                ensure!(
                    iface.is_none() && password.is_none(),
                    "use config import to change service settings, or supply --username for direct authentication"
                );
                let response = ipc::request(&paths, Action::Authenticate)?;
                snapshot(&serde_json::from_value(response.data)?, args.json)
            }
        }
        Command::Logoff { iface } => {
            if let Some(iface) = iface {
                ensure!(
                    platform::is_admin(),
                    "direct logoff requires administrator/root"
                );
                let _guard = platform::instance_lock(&paths)?;
                let adapter = platform::adapters()?
                    .into_iter()
                    .find(|a| a.name == iface || a.id == iface)
                    .ok_or_else(|| anyhow::anyhow!("adapter not found"))?;
                let capture = platform::Capture::open(&adapter)?;
                capture.send(&protocol::control(adapter.mac, 2))?;
                output(&serde_json::json!({"logoff":true}), args.json)
            } else {
                let r = ipc::request(&paths, Action::Logoff)?;
                snapshot(&serde_json::from_value(r.data)?, args.json)
            }
        }
        Command::Status => {
            let response = ipc::request(&paths, Action::Status)?;
            snapshot(&serde_json::from_value(response.data)?, args.json)
        }
        Command::Watch => ipc::watch(&paths, |s| snapshot(&s, args.json)),
        Command::Config { command } => match command {
            Some(ConfigCommand::Import { file }) => {
                let config: config::Config = config::read(&file)?;
                #[cfg(windows)]
                let login = config.launch_gui_on_login;
                let response = ipc::request(&paths, Action::SetConfig { config })?;
                #[cfg(windows)]
                platform::set_login_task(login).map_err(|e| {
                    anyhow::anyhow!("configuration saved, but login task failed: {e}")
                })?;
                output(&response.data, args.json)
            }
            Some(ConfigCommand::Password) => {
                let mut config: config::Config =
                    serde_json::from_value(ipc::request(&paths, Action::GetConfig)?.data)?;
                config.password = rpassword::prompt_password("Password: ")?;
                output(
                    &ipc::request(&paths, Action::SetConfig { config })?.data,
                    args.json,
                )
            }
            None => {
                let mut config = ipc::request(&paths, Action::GetConfig)?.data;
                config["password"] = "********".into();
                output(&config, args.json)
            }
        },
        Command::CheckUpdate => output(&ipc::request(&paths, Action::CheckUpdate)?.data, args.json),
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("ERROR: {e:#}");
        std::process::exit(1);
    }
}
