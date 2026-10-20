use crate::config::{Paths, now};
use std::{
    fs::{self, OpenOptions},
    io::Write,
};

/// Transition-only log: bounded disk use and no background flushing timer.
pub fn record(paths: &Paths, message: &str) {
    let path = paths.state.join("service.log");
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(&paths.state)?;
        if fs::metadata(&path).is_ok_and(|m| m.len() > 512 * 1024) {
            let rotated = paths.state.join("service.log.1");
            if rotated.exists() {
                fs::remove_file(&rotated)?;
            }
            fs::rename(&path, rotated)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        writeln!(file, "{} {}", now(), message.replace(['\r', '\n'], " "))?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("diagnostic log: {e}");
    }
}
