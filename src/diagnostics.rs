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

/// Read only the fixed service log, bounded to the last 32 KiB. Never export config.
pub fn read_log(paths: &Paths, config: &crate::config::Config) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(paths.state.join("service.log")) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e),
    };
    let size = file.metadata()?.len();
    let start = size.saturating_sub(32768);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(32768).read_to_end(&mut bytes)?;
    if start > 0 {
        if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            bytes.drain(..=end);
        } else {
            bytes.clear();
        }
    }
    let mut log = String::from_utf8_lossy(&bytes).into_owned();
    // Longest first, so overlapping credentials cannot leave a partial secret.
    let mut secrets = [&config.username, &config.password];
    secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for secret in secrets {
        if !secret.is_empty() {
            log = log.replace(secret.as_str(), "[redacted]");
        }
    }
    log.retain(|c| !c.is_control() || c == '\n' || c == '\t');
    // JSON escaping can double the length; retain space within the 64 KiB IPC frame.
    if log.len() > 28 * 1024 {
        let mut start = log.len() - 28 * 1024;
        while !log.is_char_boundary(start) {
            start += 1;
        }
        // Drop old records, keeping the most recent text even after redaction
        // expands it. Prefer a complete first line when another line follows.
        if let Some(end) = log[start..].find('\n')
            && start + end + 1 < log.len()
        {
            start += end + 1;
        }
        log.drain(..start);
    }
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_export_is_bounded_and_excludes_credentials_and_config() {
        let root = std::env::temp_dir().join(format!("netauth-diagnostics-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            state: root.clone(),
            config: root.join("config.json"),
            endpoint: root.join("socket"),
        };
        let config = crate::config::Config {
            username: "offline-user".into(),
            password: "offline-password".into(),
            ..Default::default()
        };
        fs::write(&paths.config, "must not be read").unwrap();
        assert_eq!(read_log(&paths, &config).unwrap(), "");
        fs::write(
            root.join("service.log"),
            format!("{}\noffline-user offline-password\n", "x".repeat(40000)),
        )
        .unwrap();
        let output = read_log(&paths, &config).unwrap();
        assert_eq!(output, "[redacted] [redacted]\n");
        for (history, config) in [
            ("旧记录：互联网连通\n".repeat(2000), config),
            (
                "x\n".repeat(16000),
                crate::config::Config {
                    password: "x".into(),
                    ..Default::default()
                },
            ),
        ] {
            let latest = "1793752947 最新认证成功；互联网连通\n";
            fs::write(root.join("service.log"), format!("{history}{latest}")).unwrap();
            let output = read_log(&paths, &config).unwrap();
            assert!(
                output.ends_with(latest),
                "newest record must survive export"
            );
            assert!(output.len() <= 28 * 1024);
            let encoded = serde_json::to_vec(&serde_json::json!({"log": output})).unwrap();
            assert!(encoded.len() < crate::ipc::MAX_MESSAGE);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
