use crate::config::{VERSION, now};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

pub const RELEASE_HOSTS: &[&str] = &["github.com", "gitee.com"];
pub const SOURCES: &[(&str, &str, &str)] = &[
    (
        "gitee",
        "https://gitee.com/LoveElysia1314/sysu-netauth/raw/main/updates/release.json",
        "gitee.com",
    ),
    (
        "github",
        "https://raw.githubusercontent.com/LoveElysia1314/sysu-netauth/main/updates/release.json",
        "raw.githubusercontent.com",
    ),
];
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Update {
    pub latest_version: String,
    pub release_url: String,
    pub summary: String,
    pub source: String,
    pub checked_at: u64,
    pub next_check_at: u64,
    pub failure_count: u32,
    pub error: String,
    pub available: bool,
    pub checking: bool,
}
impl Update {
    /// Availability belongs to the installed version, not the version that wrote the cache.
    pub fn for_current_version(mut self, current: &str) -> Self {
        self.checking = false;
        self.available = false;
        if !self.latest_version.is_empty() {
            let valid = available_for(&self.latest_version, current).and_then(|available| {
                ensure!(
                    safe_url(&self.release_url, RELEASE_HOSTS),
                    "untrusted cached release URL"
                );
                ensure!(
                    self.summary.chars().count() <= 500,
                    "cached summary too long"
                );
                Ok(available)
            });
            match valid {
                Ok(available) => self.available = available,
                Err(_) => return Self::default(),
            }
        }
        self
    }

    /// An outdated mirror must not replace a newer release already discovered.
    pub fn retain_newer_release(&mut self, previous: &Self) {
        let previous = previous.clone().for_current_version(VERSION);
        if version(&previous.latest_version).ok() > version(&self.latest_version).ok() {
            self.latest_version = previous.latest_version;
            self.release_url = previous.release_url;
            self.summary = previous.summary;
            self.source = previous.source;
            self.available = previous.available;
        }
    }
}
pub fn version(value: &str) -> Result<[u32; 3]> {
    let value = value.trim();
    let value = value.strip_prefix('v').unwrap_or(value);
    ensure!(
        value
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())),
        "invalid version"
    );
    let values: Vec<u32> = value
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        values.len() == 3 && values.iter().all(|v| *v <= 999999),
        "invalid version"
    );
    Ok([values[0], values[1], values[2]])
}
fn version_key(value: &str) -> Result<([u32; 3], u8, u32)> {
    let Some((core, suffix)) = value.split_once('-') else {
        return Ok((version(value)?, 3, 0));
    };
    let (stage, serial) = suffix
        .split_once('.')
        .ok_or_else(|| anyhow::anyhow!("invalid prerelease"))?;
    let rank = match stage {
        "alpha" => 0,
        "beta" => 1,
        "rc" => 2,
        _ => bail!("invalid prerelease stage"),
    };
    ensure!(
        !serial.is_empty() && serial.bytes().all(|b| b.is_ascii_digit()),
        "invalid prerelease number"
    );
    Ok((version(core)?, rank, serial.parse()?))
}
fn available_for(candidate: &str, current: &str) -> Result<bool> {
    // Candidate releases are always stable. A beta client can discover its final release.
    let stable = version(candidate)?;
    Ok((stable, 3, 0) > version_key(current)?)
}
pub fn safe_url(url: &str, hosts: &[&str]) -> bool {
    let Some(tail) = url.strip_prefix("https://") else {
        return false;
    };
    let host = tail.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.strip_suffix(":443").unwrap_or(host);
    !url.chars().any(|c| c.is_control() || c == '\\')
        && hosts.iter().any(|h| host.eq_ignore_ascii_case(h))
}
pub fn parse(bytes: &[u8], source: &str) -> Result<Update> {
    ensure!(bytes.len() <= 65536, "manifest too large");
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    ensure!(
        v["schema_version"] == 1 && v["channel"].as_str().unwrap_or("stable") == "stable",
        "unsupported manifest"
    );
    let latest = v["version"].as_str().unwrap_or_default();
    // The public feed stays stable-only; a prerelease client can still discover
    // the final release of its own version instead of rejecting its own version.
    let newer = available_for(latest, VERSION)?;
    let url = v["release_url"].as_str().unwrap_or_default();
    ensure!(safe_url(url, RELEASE_HOSTS), "untrusted release URL");
    let summary = v["summary"].as_str().unwrap_or_default();
    ensure!(summary.chars().count() <= 500, "summary too long");
    Ok(Update {
        latest_version: latest.trim_start_matches('v').into(),
        release_url: url.into(),
        summary: summary.into(),
        source: source.into(),
        available: newer,
        checked_at: now(),
        ..Update::default()
    })
}
pub fn fetch() -> Result<Update> {
    fetch_with(crate::platform::https_get)
}
fn fetch_with(fetcher: impl Fn(&str, &[&str], usize) -> Result<Vec<u8>> + Sync) -> Result<Update> {
    // Fetch both sources concurrently: a successful but stale mirror is not authoritative.
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = SOURCES
            .iter()
            .map(|&(name, url, host)| {
                let fetcher = &fetcher;
                scope.spawn(move || {
                    fetcher(url, &[host], 65536)
                        .and_then(|bytes| parse(&bytes, name))
                        .with_context(|| name.to_owned())
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("update source worker failed")))
            })
            .collect::<Vec<_>>()
    });
    let mut errors = Vec::new();
    let mut newest: Option<Update> = None;
    for result in results {
        match result {
            Ok(update) => {
                if newest.as_ref().is_none_or(|known| {
                    version(&update.latest_version).ok() > version(&known.latest_version).ok()
                }) {
                    newest = Some(update);
                }
            }
            Err(e) => errors.push(format!("{e:#}")),
        }
    }
    if let Some(update) = newest {
        return Ok(update);
    }
    bail!("{}", errors.join("; "))
}
pub fn failure_delay(count: u32) -> u64 {
    [1800, 7200, 21600, 86400][count.saturating_sub(1).min(3) as usize]
}
#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(release: &str, channel: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "channel": channel, "version": release,
            "release_url": format!("https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v{release}"),
            "summary": "release"
        })).unwrap()
    }

    #[test]
    fn beta_clients_only_offer_newer_stable_releases() {
        assert!(!available_for("0.6.3", "0.7.0-beta.2").unwrap());
        assert!(available_for("0.7.0", "0.7.0-beta.2").unwrap());
        assert!(!available_for("0.7.0", "0.7.0").unwrap());
        assert!(available_for("0.7.1", "0.7.0").unwrap());
        assert!(!available_for("0.7.0", "0.8.0-beta.1").unwrap());
        assert!(available_for("0.7.0-beta.3", "0.7.0-beta.2").is_err());
        assert!(parse(&manifest("0.7.0-beta.3", "stable"), "test").is_err());
        assert!(parse(&manifest("0.7.0", "beta"), "test").is_err());
    }

    #[test]
    fn installed_version_recalculates_cached_availability() {
        let cached = Update {
            latest_version: "0.7.0".into(),
            release_url: "https://github.com/a/b/releases".into(),
            checking: true,
            available: true,
            ..Update::default()
        };
        let current = cached.clone().for_current_version("0.7.0");
        assert!(!current.available && !current.checking);
        assert!(cached.clone().for_current_version("0.7.0-beta.2").available);
        assert!(!cached.clone().for_current_version("0.8.0-beta.1").available);
        assert!(
            Update {
                latest_version: "0.7.0-beta.3".into(),
                ..cached.clone()
            }
            .for_current_version("0.7.0-beta.2")
            .latest_version
            .is_empty()
        );
        assert!(
            Update {
                release_url: "https://evil.example/update".into(),
                ..cached
            }
            .for_current_version("0.7.0-beta.2")
            .latest_version
            .is_empty()
        );
    }

    #[test]
    fn outdated_mirror_does_not_hide_a_newer_source() {
        let update = fetch_with(|url, _, _| {
            Ok(manifest(
                if url.contains("gitee.com") {
                    "0.6.3"
                } else {
                    "0.7.0"
                },
                "stable",
            ))
        })
        .unwrap();
        assert_eq!(update.latest_version, "0.7.0");
        assert_eq!(update.source, "github");
        let update = fetch_with(|url, _, _| {
            Ok(manifest(
                if url.contains("gitee.com") {
                    "0.7.1"
                } else {
                    "0.7.0"
                },
                "stable",
            ))
        })
        .unwrap();
        assert_eq!(update.latest_version, "0.7.1");
        assert_eq!(update.source, "gitee");
    }

    #[test]
    fn one_unavailable_or_prerelease_source_does_not_break_stable_updates() {
        let update = fetch_with(|url, _, _| {
            if url.contains("gitee.com") {
                bail!("offline")
            }
            Ok(manifest("0.7.0", "stable"))
        })
        .unwrap();
        assert_eq!(update.source, "github");
        let update = fetch_with(|url, _, _| {
            Ok(manifest(
                if url.contains("gitee.com") {
                    "0.7.0-beta.3"
                } else {
                    "0.7.0"
                },
                "stable",
            ))
        })
        .unwrap();
        assert_eq!(update.source, "github");
        assert!(fetch_with(|_, _, _| bail!("offline")).is_err());
    }

    #[test]
    fn successful_but_older_feed_retains_the_known_newer_release() {
        let previous = parse(&manifest("0.7.1", "stable"), "github").unwrap();
        let mut incoming = parse(&manifest("0.6.3", "stable"), "gitee").unwrap();
        incoming.checked_at = 42;
        incoming.retain_newer_release(&previous);
        assert_eq!(incoming.latest_version, "0.7.1");
        assert_eq!(incoming.source, "github");
        assert_eq!(incoming.checked_at, 42);
    }
    #[test]
    fn urls_and_versions() {
        assert!(version("0.10.0").unwrap() > version("0.9.9").unwrap());
        assert!(version("1.2").is_err());
        assert!(version("vv1.2.3").is_err());
        assert!(version("+1.2.3").is_err());
        assert!(version("1.2.1000000").is_err());
        for url in [
            "http://github.com/x",
            "https://github.com.evil/x",
            "https://user@github.com/x",
            "https://github.com:80/x",
            "https://github.com\\@evil/x",
        ] {
            assert!(!safe_url(url, RELEASE_HOSTS), "{url}");
        }
        assert!(safe_url(
            "https://github.com:443/owner/repo/releases",
            RELEASE_HOSTS
        ));
    }
    #[test]
    fn prerelease_ordering() {
        let ordered = [
            "0.7.0-alpha.0",
            "0.7.0-alpha.1",
            "0.7.0-beta.0",
            "0.7.0-rc.1",
            "0.7.0",
            "0.7.1-alpha.0",
        ];
        for pair in ordered.windows(2) {
            assert!(version_key(pair[0]).unwrap() < version_key(pair[1]).unwrap());
        }
        assert!(version("0.7.0-alpha.0").is_err());
        assert!(version_key("0.7.0-unknown.1").is_err());
    }
    #[test]
    fn manifest_validation() {
        assert!(parse(br#"{"schema_version":1,"version":"999999.0.0","release_url":"https://github.com/a/b/releases","summary":"hello"}"#,"test").unwrap().available);
        assert!(
            parse(
                br#"{"schema_version":1,"version":"0.8.0","release_url":"https://evil.com"}"#,
                "test"
            )
            .is_err()
        );
        assert_eq!(
            (failure_delay(1), failure_delay(4), failure_delay(9)),
            (1800, 86400, 86400)
        );
    }
}
