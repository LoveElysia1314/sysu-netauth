#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    {
        if let Err(e) = netauth::gui::run() {
            netauth::platform::show_error(&format!("{e:#}"));
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "This desktop executable is for Windows. Use the macOS app or sysu_netauth --help."
        );
        std::process::exit(1);
    }
}
