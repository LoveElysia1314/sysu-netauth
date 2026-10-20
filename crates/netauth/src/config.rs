use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub username: String,
    pub password: String,
    pub iface: String,
    pub iface_mode: String,
    pub auto_auth: bool,
    pub retry_interval: u64,
    pub service_mode: bool,
    pub launch_gui_on_login: bool,
    pub hide_window_on_login: bool,
    pub desktop_notify: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            iface: String::new(),
            iface_mode: "auto".into(),
            auto_auth: true,
            retry_interval: 60,
            service_mode: true,
            launch_gui_on_login: false,
            hide_window_on_login: true,
            desktop_notify: true,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["auto", "manual"].contains(&self.iface_mode.as_str()),
            "iface_mode must be auto or manual"
        );
        ensure!(
            (15..=3600).contains(&self.retry_interval),
            "retry_interval must be 15–3600 seconds"
        );
        ensure!(
            self.username.len() <= 512 && self.password.len() <= 1024 && self.iface.len() <= 512,
            "configuration value too long"
        );
        crate::protocol::gbk(&self.username)?;
        crate::protocol::gbk(&self.password)?;
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    pub iface: String,
    pub last_success_mac: String,
    pub manual_selector: String,
}

#[derive(Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub endpoint: PathBuf,
}
impl Default for Paths {
    fn default() -> Self {
        #[cfg(windows)]
        {
            let base = PathBuf::from(
                std::env::var_os("PROGRAMDATA").unwrap_or_else(|| r"C:\ProgramData".into()),
            )
            .join("SYSUNetAuth");
            Self {
                config: base.join("config.json"),
                state: base,
                endpoint: PathBuf::from(r"\\.\pipe\SYSUNetAuth-v1"),
            }
        }
        #[cfg(unix)]
        {
            Self {
                config: "/etc/sysu-netauth/config.json".into(),
                state: "/var/lib/sysu-netauth".into(),
                endpoint: "/run/sysu-netauth/control.sock".into(),
            }
        }
    }
}

pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(file.metadata()?.len() <= 65536, "data file exceeds 64 KiB");
    serde_json::from_reader(file).with_context(|| format!("parse {}", path.display()))
}
/// Do not chmod an arbitrary existing parent (e.g. --data-dir /tmp).
/// Require an already-private app directory or create one with private permissions.
#[cfg(unix)]
pub fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    ensure!(
        fs::metadata(path)?.permissions().mode() & 0o077 == 0,
        "data directory {} must have mode 0700",
        path.display()
    );
    Ok(())
}

pub fn atomic_write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().context("missing parent directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    #[cfg(windows)]
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".netauth-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        {
            fs::rename(&tmp, path)?;
            fs::File::open(parent)?.sync_all()?;
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let mut last = std::io::Error::other("replace failed");
            for attempt in 0..5 {
                if unsafe {
                    MoveFileExW(
                        crate::platform::wide(tmp.as_os_str()).as_ptr(),
                        crate::platform::wide(path.as_os_str()).as_ptr(),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                    )
                } != 0
                {
                    return Ok(());
                }
                last = std::io::Error::last_os_error();
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
            Err(last.into())
        }
        #[cfg(unix)]
        Ok(())
    })();
    let _ = fs::remove_file(&tmp);
    result
}

/// Import once without rewriting credentials during ordinary reads.
pub fn load(paths: &Paths) -> Result<Config> {
    if !paths.config.exists() {
        return Ok(Config::default());
    }
    let raw: serde_json::Value = read(&paths.config)?;
    if raw.get("native_schema").is_none() {
        let backup = paths.config.with_extension("json.python-backup");
        if !backup.exists() {
            fs::copy(&paths.config, &backup)?;
        }
        let defaults = serde_json::to_value(Config::default())?;
        let mut clean = defaults.clone();
        for (key, default) in defaults.as_object().context("config object")? {
            if let Some(value) = raw.get(key).filter(|v| match default {
                serde_json::Value::Bool(_) => v.is_boolean(),
                serde_json::Value::Number(_) => v.is_u64(),
                _ => v.is_string(),
            }) {
                clean[key] = value.clone();
            }
        }
        let mut config: Config = serde_json::from_value(clean)?;
        if !["auto", "manual"].contains(&config.iface_mode.as_str()) {
            config.iface_mode = "auto".into();
        }
        if !(15..=3600).contains(&config.retry_interval) {
            config.retry_interval = 60;
        }
        if !paths.state.join("service_cache.json").exists() {
            atomic_write(
                &paths.state.join("service_cache.json"),
                &Cache {
                    iface: config.iface.clone(),
                    manual_selector: config.iface.clone(),
                    last_success_mac: raw["last_success_mac"].as_str().unwrap_or_default().into(),
                },
            )?;
        }
        save(paths, &config)?;
        return Ok(config);
    }
    let config: Config = serde_json::from_value(raw)?;
    config.validate()?;
    Ok(config)
}
pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    config.validate()?;
    let mut value = serde_json::to_value(config)?;
    value["native_schema"] = 1.into();
    atomic_write(&paths.config, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn broad_existing_parent_is_rejected_without_chmod() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("netauth-permissions-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_dir(&dir).is_err());
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn migration_preserves_backup_and_cache() {
        let dir = std::env::temp_dir().join(format!("netauth-migration-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = Paths {
            config: dir.join("config.json"),
            state: dir.clone(),
            endpoint: dir.join("socket"),
        };
        fs::write(&p.config,r#"{"username":"user","password":"secret","retry_interval":false,"last_success_mac":"00:11:22:33:44:55"}"#).unwrap();
        let c = load(&p).unwrap();
        assert_eq!(c.retry_interval, 60);
        assert!(p.config.with_extension("json.python-backup").exists());
        assert_eq!(
            read::<Cache>(&dir.join("service_cache.json"))
                .unwrap()
                .last_success_mac,
            "00:11:22:33:44:55"
        );
        let before = fs::read(&p.config).unwrap();
        load(&p).unwrap();
        assert_eq!(before, fs::read(&p.config).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
}
