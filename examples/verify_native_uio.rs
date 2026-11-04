//! Administrator-only validation of the inbox driver. Secrets arrive on stdin;
//! output contains packet metadata and results only. No credential files.
#[cfg(windows)]
fn run() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use netauth::{
        authentication,
        native_eap::{Credentials, wipe},
        platform, protocol,
    };
    use std::{
        io::{self, Read},
        sync::Arc,
        time::{Duration, Instant},
    };
    ensure!(
        platform::is_admin(),
        "Run the native driver checks as administrator"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args
        .first()
        .context("Use --io-check GUID, --logoff GUID, or --live GUID seconds")?;
    let id = args.get(1).context("Ethernet adapter GUID required")?;
    let adapter = platform::adapters()?
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(id))
        .context("Ethernet adapter not found")?;
    if mode == "--io-check" {
        for _ in 0..20 {
            let capture = platform::native_uio::Capture::open(&adapter)?;
            let stop = Arc::new(platform::Cancel::new()?);
            let timer = stop.clone();
            let started = Instant::now();
            let worker = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                timer.set();
            });
            while capture.receive(&stop)?.is_some() {}
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("cancellation timer failed"))?;
            ensure!(
                started.elapsed() < Duration::from_secs(2),
                "native read did not cancel promptly"
            );
        }
        println!("PASS: 20 native adapter open/read/cancel/close cycles");
        return Ok(());
    }
    let capture = platform::native_uio::Capture::open(&adapter)?;
    if mode == "--logoff" {
        capture.send(&protocol::control(adapter.mac, 2))?;
        println!("PASS: native EAPOL logoff sent");
        return Ok(());
    }
    ensure!(mode == "--live", "Unknown native verification mode");
    let seconds: u64 = args.get(2).context("duration required")?.parse()?;
    ensure!(
        (1..=600).contains(&seconds),
        "test duration must be 1..600 seconds"
    );
    #[derive(serde::Deserialize)]
    struct Input {
        username: String,
        password: String,
    }
    let mut input = String::new();
    io::stdin().take(8193).read_to_string(&mut input)?;
    let parsed = serde_json::from_str::<Input>(&input);
    unsafe {
        wipe(input.as_bytes_mut());
    }
    let mut data = parsed.context("Invalid stdin credentials")?;
    let credentials = Credentials::new(&data.username, &data.password);
    unsafe {
        wipe(data.password.as_bytes_mut());
    }
    let credentials = credentials?;
    println!("PASS: inbox NDISUIO driver opened the selected Ethernet adapter");
    let stop = Arc::new(platform::Cancel::new()?);
    let timer = stop.clone();
    let worker = std::thread::spawn(move || {
        platform::cancellable_wait(&timer, Duration::from_secs(seconds));
        timer.set();
    });
    let started = Instant::now();
    let mut peer = None;
    let mut online = false;
    let mut renewals = 0;
    capture.send(&protocol::control(adapter.mac, 2))?;
    std::thread::sleep(Duration::from_secs(2));
    capture.send(&protocol::control(adapter.mac, 1))?;
    let exchange = (|| -> anyhow::Result<()> {
        while let Some(bytes) = capture.receive(&stop)? {
            let Some(packet) = protocol::parse(&bytes) else {
                continue;
            };
            if !authentication::addressed_to(&packet, adapter.mac)
                || (peer.is_some_and(|p| p != packet.source) && !(online && packet.code == 1))
            {
                continue;
            }
            match packet.code {
                1 => {
                    let destination = if online { protocol::PAE } else { packet.source };
                    if let Some(response) = credentials.answer(adapter.mac, destination, &packet)? {
                        capture.send(&response)?;
                        if online {
                            renewals += 1;
                        }
                        println!(
                            "RESPONSE elapsed={}s kind={} id={} online={online}",
                            started.elapsed().as_secs(),
                            packet.kind.unwrap_or(0),
                            packet.id
                        );
                        peer = Some(packet.source);
                    }
                }
                3 if peer == Some(packet.source) => {
                    online = true;
                    println!("AUTHENTICATED elapsed={}s", started.elapsed().as_secs());
                }
                4 if peer == Some(packet.source) => {
                    anyhow::bail!("Authenticator rejected the native exchange")
                }
                _ => {}
            }
        }
        ensure!(
            online && renewals > 0,
            "Native transport did not authenticate and handle a subsequent request"
        );
        Ok(())
    })();
    stop.set();
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("test timer failed"))?;
    exchange?;
    println!(
        "PASS: native EAPOL authentication and {renewals} online responses over {} seconds",
        started.elapsed().as_secs()
    );
    Ok(())
}
fn main() {
    #[cfg(windows)]
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    eprintln!("Native UIO verification requires Windows");
}
