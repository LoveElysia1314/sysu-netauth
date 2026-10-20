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
        eprintln!("The native desktop UI is Windows-only. Use sysu_netauth --help.");
        std::process::exit(1);
    }
}
