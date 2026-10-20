use crate::{config::Paths, engine::Engine, ipc, platform};
use anyhow::{Result, ensure};

pub fn run(paths: Paths) -> Result<()> {
    ensure!(
        platform::is_admin(),
        "service requires administrator/root privileges"
    );
    let _guard = platform::instance_lock(&paths)?;
    #[cfg(unix)]
    {
        crate::config::private_dir(&paths.state)?;
        if let Some(parent) = paths.config.parent() {
            crate::config::private_dir(parent)?;
        }
    }
    #[cfg(windows)]
    {
        platform::secure_directory(&paths.state)?;
        if let Some(parent) = paths.config.parent() {
            platform::secure_directory(parent)?;
        }
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(256);
    #[cfg(unix)]
    platform::signal_thread(tx.clone())?;
    #[cfg(windows)]
    platform::register_stop_sender(tx.clone());
    let _server = ipc::start(&paths, tx.clone())?;
    let _monitor = platform::monitor(tx.clone())?;
    let mut engine = Engine::new(paths, tx)?;
    crate::diagnostics::record(engine.paths(), "service starting");
    #[cfg(windows)]
    platform::service_ready();
    engine.run(rx);
    crate::diagnostics::record(engine.paths(), "service stopped");
    Ok(())
}
