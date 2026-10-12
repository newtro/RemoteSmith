//! Local endpoint naming and connection setup (docs/IPC.md §2).

use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

/// Environment variable that overrides the endpoint path on both sides (tests, dev tools).
pub const ENV_IPC_PATH: &str = "REMOTESMITH_IPC_PATH";

/// A connected duplex byte stream.
pub trait IpcStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> IpcStream for T {}

/// The endpoint for the current user: socket path on Unix, pipe name on Windows.
pub fn default_endpoint() -> String {
    if let Ok(p) = std::env::var(ENV_IPC_PATH) {
        if !p.is_empty() {
            return p;
        }
    }
    platform_endpoint()
}

#[cfg(unix)]
fn platform_endpoint() -> String {
    // SAFETY: geteuid has no preconditions and cannot fail.
    let uid = unsafe { libc::geteuid() };
    format!("/tmp/RemoteSmith/remotesmith-ipc-{uid}.sock")
}

#[cfg(windows)]
fn platform_endpoint() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".into());
    let user: String = user.chars().filter(|c| *c != '\\' && *c != '/').collect();
    format!(r"\\.\pipe\RemoteSmith-ipc-{user}")
}

/// Connect to the endpoint as a client, enforcing the owner check on Unix.
#[cfg(unix)]
pub async fn connect(endpoint: &str) -> io::Result<Box<dyn IpcStream>> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = std::fs::metadata(endpoint)?;
    if !meta.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "endpoint is not a socket",
        ));
    }
    // SAFETY: as above.
    let me = unsafe { libc::geteuid() };
    if meta.uid() != me {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("endpoint owned by uid {} not {me}; refusing", meta.uid()),
        ));
    }
    let s = tokio::net::UnixStream::connect(endpoint).await?;
    Ok(Box::new(s))
}

#[cfg(windows)]
pub async fn connect(endpoint: &str) -> io::Result<Box<dyn IpcStream>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    loop {
        match ClientOptions::new().open(endpoint) {
            Ok(c) => return Ok(Box::new(c)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 10 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
