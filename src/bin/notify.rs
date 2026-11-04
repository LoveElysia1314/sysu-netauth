#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    {
        let result = if std::env::args().any(|a| a == "--install") {
            netauth::platform::set_notification_task(true)
        } else if std::env::args().any(|a| a == "--uninstall") {
            netauth::platform::set_notification_task(false)
        } else {
            netauth::notify::run()
        };
        if let Err(error) = result {
            netauth::diagnostics::record(
                &netauth::config::Paths::default(),
                &format!("notification assistant: {error}"),
            );
            std::process::exit(1);
        }
    }
}
