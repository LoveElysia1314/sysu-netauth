fn main() {
    #[cfg(windows)]
    {
        if std::env::args().any(|a| a == "--install") {
            if let Err(e) = netauth::platform::install_service() {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            return;
        }
        if !std::env::args().any(|a| a == "--console") {
            if let Err(e) = netauth::platform::run_service_dispatcher() {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            return;
        }
    }
    let mut paths = netauth::config::Paths::default();
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(i) = args.iter().position(|a| a == "--data-dir") {
        let Some(dir) = args.get(i + 1) else {
            eprintln!("--data-dir requires a path");
            std::process::exit(2);
        };
        let dir = std::path::PathBuf::from(dir);
        paths.config = dir.join("config.json");
        paths.state = dir.clone();
        #[cfg(unix)]
        {
            paths.endpoint = dir.join("control.sock");
        }
    }
    if let Err(e) = netauth::service::run(paths) {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
