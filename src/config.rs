use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const VERSION: &str = env!("NETAUTH_APP_VERSION");
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
    pub auto_update_check: bool,
    pub update_notify: bool,
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
            desktop_notify: false,
            auto_update_check: true,
            update_notify: true,
        }
    }
}
impl Config {
    pub fn validate_auth(&self) -> Result<()> {
        self.validate()?;
        ensure!(
            !self.username.is_empty() && !self.password.is_empty(),
            "需要配置 NetID 和密码"
        );
        ensure!(
            self.iface_mode != "manual" || !self.iface.is_empty(),
            "请选择网卡"
        );
        Ok(())
    }
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
    pub last_success_id: String,
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
        #[cfg(target_os = "linux")]
        {
            Self {
                config: "/etc/sysu-netauth/config.json".into(),
                state: "/var/lib/sysu-netauth".into(),
                endpoint: "/run/sysu-netauth/control.sock".into(),
            }
        }
        #[cfg(target_os = "macos")]
        {
            let base = PathBuf::from("/Library/Application Support/SYSUNetAuth");
            Self {
                config: base.join("config.json"),
                state: base,
                endpoint: "/var/run/sysu-netauth/control.sock".into(),
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

/// Reading configuration never rewrites settings or creates migration files.
pub fn load(paths: &Paths) -> Result<Config> {
    if !paths.config.exists() {
        return Ok(Config::default());
    }
    let config: Config = read(&paths.config)?;
    config.validate()?;
    Ok(config)
}
pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    config.validate()?;
    atomic_write(&paths.config, config)
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_preferences_default_on_and_preserve_explicit_opt_out() {
        let initial: super::Config = serde_json::from_str("{}").unwrap();
        assert!(initial.auto_update_check && initial.update_notify);
        let opted_out: super::Config =
            serde_json::from_str(r#"{"auto_update_check":false,"update_notify":false}"#).unwrap();
        let restored: super::Config =
            serde_json::from_value(serde_json::to_value(opted_out).unwrap()).unwrap();
        assert!(!restored.auto_update_check && !restored.update_notify);
    }
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
    fn settings_round_trip_without_rewriting_or_side_effects() {
        let dir = std::env::temp_dir().join(format!("netauth-config-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = Paths {
            config: dir.join("config.json"),
            state: dir.clone(),
            endpoint: dir.join("socket"),
        };
        let config = Config {
            username: "user".into(),
            password: "secret".into(),
            ..Config::default()
        };
        save(&p, &config).unwrap();
        let before = fs::read(&p.config).unwrap();
        assert_eq!(load(&p).unwrap(), config);
        assert_eq!(before, fs::read(&p.config).unwrap());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::write(&p.config, br#"{"retry_interval":false}"#).unwrap();
        assert!(load(&p).is_err());
        fs::write(&p.config, br#"{"retry_interval":1}"#).unwrap();
        assert!(load(&p).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

/// Immediate GUI options. Account and adapter drafts cannot enter this request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreferencesPatch {
    pub auto_auth: Option<bool>,
    pub launch_gui_on_login: Option<bool>,
    pub hide_window_on_login: Option<bool>,
    pub desktop_notify: Option<bool>,
    pub auto_update_check: Option<bool>,
    pub update_notify: Option<bool>,
}
impl PreferencesPatch {
    pub fn apply(self, config: &mut Config) {
        if let Some(v) = self.auto_auth {
            config.auto_auth = v;
        }
        if let Some(v) = self.launch_gui_on_login {
            config.launch_gui_on_login = v;
        }
        if let Some(v) = self.hide_window_on_login {
            config.hide_window_on_login = v;
        }
        if let Some(v) = self.desktop_notify {
            config.desktop_notify = v;
        }
        if let Some(v) = self.auto_update_check {
            config.auto_update_check = v;
        }
        if let Some(v) = self.update_notify {
            config.update_notify = v;
        }
    }
}
