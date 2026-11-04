//! Read-only network smoke check. Never reads credentials or contacts the authentication service.
use anyhow::Result;

fn main() -> Result<()> {
    let bytes = netauth::platform::https_get(
        netauth::update::SOURCES[0].1,
        &["gitee.com", "raw.giteeusercontent.com"],
        65536,
    )?;
    let update = netauth::update::parse(&bytes, "gitee")?;
    println!(
        "PASS: Gitee HTTPS redirects and stable manifest ({})",
        update.latest_version
    );
    let update = netauth::update::fetch()?;
    println!(
        "PASS: update feed selection ({} / {})",
        update.source, update.latest_version
    );
    Ok(())
}
