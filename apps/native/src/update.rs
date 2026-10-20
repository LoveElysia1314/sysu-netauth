use crate::config::{VERSION, now};
use anyhow::{Result, bail, ensure};
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
    let newer = version(latest)? > version(VERSION)?;
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
    let mut errors = Vec::new();
    for (name, url, host) in SOURCES {
        match crate::platform::https_get(url, &[*host], 65536).and_then(|bytes| parse(&bytes, name))
        {
            Ok(update) => return Ok(update),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    bail!("{}", errors.join("; "))
}
pub fn failure_delay(count: u32) -> u64 {
    [1800, 7200, 21600, 86400][count.saturating_sub(1).min(3) as usize]
}
#[cfg(test)]
mod tests {
    use super::*;
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
    fn manifest_validation() {
        assert!(parse(br#"{"schema_version":1,"version":"0.8.0","release_url":"https://github.com/a/b/releases","summary":"hello"}"#,"test").unwrap().available);
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
