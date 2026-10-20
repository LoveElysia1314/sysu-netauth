//! Sample an existing service PID. Run in release mode outside the measured process.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use std::{
        fs,
        time::{Duration, Instant},
    };
    let mut args = std::env::args().skip(1);
    let pid = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: measure_linux PID [seconds, default 600]"))?;
    let seconds: u64 = args.next().unwrap_or_else(|| "600".into()).parse()?;
    fn stat(pid: &str) -> anyhow::Result<(u64, u64)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields: Vec<_> = stat
            .rsplit_once(')')
            .ok_or_else(|| anyhow::anyhow!("invalid proc stat"))?
            .1
            .split_whitespace()
            .collect();
        Ok((
            fields[11].parse::<u64>()? + fields[12].parse::<u64>()?,
            fields[21].parse()?,
        ))
    }
    fn field(pid: &str, name: &str, key: &str) -> anyhow::Result<u64> {
        Ok(fs::read_to_string(format!("/proc/{pid}/{name}"))?
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or("0")
            .split_whitespace()
            .next()
            .unwrap_or("0")
            .parse()?)
    }
    let start = stat(&pid)?;
    let switches = field(&pid, "status", "voluntary_ctxt_switches:")?;
    let writes = field(&pid, "io", "write_bytes:")?;
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds));
    let end = stat(&pid)?;
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"pid":pid,"seconds":started.elapsed().as_secs_f64(),"cpu_percent_one_core":(end.0-start.0) as f64/ticks/started.elapsed().as_secs_f64()*100.0,"rss_bytes":end.1*page,"voluntary_context_switches_main_thread":field(&pid,"status","voluntary_ctxt_switches:")?.saturating_sub(switches),"disk_write_bytes":field(&pid,"io","write_bytes:")?.saturating_sub(writes)})
        )?
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Use apps/native/scripts/measure-windows.ps1 on Windows");
}
