//! Local, authenticated, bounded control protocol. No status files or heartbeats.
use crate::{
    config::{Config, Paths},
    engine::{Event, Snapshot},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    time::Duration,
};

pub const MAX_MESSAGE: usize = 65536;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Status,
    Watch,
    Interfaces,
    Authenticate,
    Logoff,
    GetConfig,
    SetConfig { config: Config },
    ReloadConfig,
    CheckUpdate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: u64,
    #[serde(flatten)]
    pub action: Action,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub data: Value,
}
impl Response {
    pub fn success(id: u64, data: impl Serialize) -> Self {
        Self {
            version: 1,
            id,
            ok: true,
            error: None,
            data: serde_json::to_value(data).unwrap_or(Value::Null),
        }
    }
    pub fn error(id: u64, error: impl ToString) -> Self {
        Self {
            version: 1,
            id,
            ok: false,
            error: Some(error.to_string()),
            data: Value::Null,
        }
    }
}
pub struct Call {
    pub request: Request,
    pub reply: SyncSender<Response>,
    pub updates: Option<Subscription>,
}
pub struct Subscription {
    pub updates: SyncSender<Option<Snapshot>>,
    pub alive: Arc<AtomicBool>,
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_MESSAGE, "IPC message too large");
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix)?;
    let len = u32::from_be_bytes(prefix) as usize;
    ensure!(len > 0 && len <= MAX_MESSAGE, "invalid IPC frame length");
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub use crate::platform::Pipe as Stream;

pub fn connect(paths: &Paths) -> Result<Stream> {
    #[cfg(unix)]
    {
        let stream = Stream::connect(&paths.endpoint)
            .context("service unavailable (start sysu_netauth_service)")?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(stream)
    }
    #[cfg(windows)]
    {
        Stream::connect(&paths.endpoint)
    }
}
pub fn request(paths: &Paths, action: Action) -> Result<Response> {
    let mut stream = connect(paths)?;
    write_frame(
        &mut stream,
        &Request {
            version: 1,
            id: 1,
            action,
        },
    )?;
    let response: Response = read_frame(&mut stream)?;
    ensure!(
        response.version == 1 && response.id == 1,
        "incompatible service response"
    );
    ensure!(
        response.ok,
        "{}",
        response.error.as_deref().unwrap_or("service error")
    );
    Ok(response)
}
pub fn watch(paths: &Paths, mut callback: impl FnMut(Snapshot) -> Result<()>) -> Result<()> {
    let mut stream = connect(paths)?;
    write_frame(
        &mut stream,
        &Request {
            version: 1,
            id: 1,
            action: Action::Watch,
        },
    )?;
    let first: Response = read_frame(&mut stream)?;
    ensure!(first.ok, "watch rejected");
    callback(serde_json::from_value(first.data)?)?;
    #[cfg(unix)]
    stream.set_read_timeout(None)?;
    #[cfg(windows)]
    stream.set_timeout(None);
    loop {
        let snapshot: Snapshot = read_frame(&mut stream)?;
        callback(snapshot)?;
    }
}

fn serve(mut stream: Stream, events: SyncSender<Event>) -> Result<()> {
    let request: Request = read_frame(&mut stream)?;
    if request.version != 1 {
        write_frame(
            &mut stream,
            &Response::error(request.id, "unsupported protocol version"),
        )?;
        return Ok(());
    }
    let watching = matches!(request.action, Action::Watch);
    let (tx, rx) = mpsc::sync_channel(1);
    let (updates, notifications) = mpsc::sync_channel(8);
    let alive = Arc::new(AtomicBool::new(true));
    events
        .try_send(Event::Call(Call {
            request,
            reply: tx,
            updates: watching.then(|| Subscription {
                updates: updates.clone(),
                alive: alive.clone(),
            }),
        }))
        .context("service busy")?;
    let response = rx.recv_timeout(Duration::from_secs(8))?;
    write_frame(&mut stream, &response)?;
    if watching && response.ok {
        // Detect EOF even when there are no state changes, without polling.
        // Shut down both halves before joining, so backpressure cannot strand a reader.
        let mut reader = stream.try_clone()?;
        #[cfg(unix)]
        reader.set_read_timeout(None)?;
        #[cfg(windows)]
        reader.set_timeout(None);
        let closed = alive.clone();
        let reader = std::thread::Builder::new()
            .name("watch-disconnect".into())
            .spawn(move || {
                let mut byte = [0];
                let _ = reader.read(&mut byte);
                closed.store(false, Ordering::Relaxed);
                let _ = updates.try_send(None);
            })?;
        let result = (|| -> Result<()> {
            while alive.load(Ordering::Relaxed) {
                match notifications.recv() {
                    Ok(Some(update)) => write_frame(&mut stream, &update)?,
                    _ => break,
                }
            }
            Ok(())
        })();
        #[cfg(unix)]
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        #[cfg(windows)]
        stream.disconnect();
        let _ = reader.join();
        result?;
    }
    Ok(())
}

pub fn start(paths: &Paths, events: SyncSender<Event>) -> Result<Server> {
    let listener = Listener::bind(paths)?;
    let active = Arc::new(AtomicUsize::new(0));
    let thread = std::thread::Builder::new()
        .name("control-listener".into())
        .spawn(move || {
            while let Ok(stream) = listener.accept() {
                if active.load(Ordering::Relaxed) >= 16 {
                    continue;
                }
                active.fetch_add(1, Ordering::Relaxed);
                let active = active.clone();
                let events = events.clone();
                let _ = std::thread::Builder::new()
                    .name("control-client".into())
                    .spawn(move || {
                        struct Count(Arc<AtomicUsize>);
                        impl Drop for Count {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                        let _count = Count(active);
                        let _ = serve(stream, events);
                    });
            }
        })?;
    Ok(Server {
        _thread: thread,
        #[cfg(unix)]
        endpoint: paths.endpoint.clone(),
    })
}
// The listener is process-lifetime; the service process exits after cleanup.
pub struct Server {
    _thread: std::thread::JoinHandle<()>,
    #[cfg(unix)]
    endpoint: std::path::PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.endpoint);
        }
    }
}

#[cfg(unix)]
struct Listener(std::os::unix::net::UnixListener);
#[cfg(unix)]
impl Listener {
    fn bind(paths: &Paths) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let parent = paths.endpoint.parent().context("socket parent")?;
        crate::config::private_dir(parent)?;
        if paths.endpoint.exists() {
            ensure!(
                std::os::unix::net::UnixStream::connect(&paths.endpoint).is_err(),
                "service already running"
            );
            std::fs::remove_file(&paths.endpoint)?;
        }
        let listener = std::os::unix::net::UnixListener::bind(&paths.endpoint)?;
        std::fs::set_permissions(&paths.endpoint, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self(listener))
    }
    fn accept(&self) -> Result<Stream> {
        use std::os::fd::AsRawFd;
        let (stream, _) = self.0.accept()?;
        #[cfg(target_os = "linux")]
        let authorized = {
            let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
            let mut size = std::mem::size_of_val(&cred) as u32;
            (unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut cred as *mut libc::ucred).cast(),
                    &mut size,
                )
            }) == 0
                && cred.uid == 0
        };
        #[cfg(target_os = "macos")]
        let authorized = {
            let mut uid = 1;
            let mut gid = 1;
            (unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) }) == 0 && uid == 0
        };
        ensure!(authorized, "unauthorized IPC peer");
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(stream)
    }
}
#[cfg(windows)]
use crate::platform::PipeListener as Listener;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_frames_and_limits() {
        struct Short(std::io::Cursor<Vec<u8>>);
        impl Read for Short {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                let n = b.len().min(1);
                self.0.read(&mut b[..n])
            }
        }
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Request {
                version: 1,
                id: 42,
                action: Action::Status,
            },
        )
        .unwrap();
        let r: Request = read_frame(&mut Short(std::io::Cursor::new(bytes))).unwrap();
        assert_eq!(r.id, 42);
        assert!(read_frame::<Request>(&mut &65537u32.to_be_bytes()[..]).is_err());
        assert!(read_frame::<Request>(&mut &0u32.to_be_bytes()[..]).is_err());
        assert!(read_frame::<Request>(&mut &[0, 0, 0, 8, b'{'][..]).is_err());
    }
}
