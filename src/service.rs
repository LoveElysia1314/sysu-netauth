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
    let mut engine = Engine::new(paths, tx.clone())?;
    #[cfg(unix)]
    platform::signal_thread(engine.stop_notifier())?;
    #[cfg(windows)]
    platform::register_stop_notifier(engine.stop_notifier());
    let _server = ipc::start(engine.paths(), tx)?;
    // Native notifications are the fast path; periodic reconciliation keeps
    // startup/recovery working if registration is temporarily unavailable.
    let _monitor = match platform::monitor(engine.network_notifier(), engine.power_notifier()) {
        Ok(monitor) => Some(monitor),
        Err(e) => {
            crate::diagnostics::record(
                engine.paths(),
                &format!("network notifications unavailable; using reconciliation: {e}"),
            );
            None
        }
    };
    #[cfg(windows)]
    platform::register_power_notifier(engine.power_notifier());
    crate::diagnostics::record(engine.paths(), "service starting");
    #[cfg(windows)]
    platform::service_ready();
    engine.run(rx);
    crate::diagnostics::record(engine.paths(), "service stopped");
    Ok(())
}
