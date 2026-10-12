//! Local endpoint naming and connection setup (docs/IPC.md §2).

use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

/// Environment variable that overrides the endpoint path on both sides (tests, dev tools).
pub const ENV_IPC_PATH: &str = "REMOTESMITH_IPC_PATH";

/// File name of the socket inside the fork's per-user IPC directory.
pub const SOCKET_NAME: &str = "remotesmith-ipc.sock";

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
pub fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// `/tmp/RemoteSmith-<uid>/remotesmith-ipc.sock`: inside the per-user 0700 directory the fork
/// already creates for its own IPC (hbb_common `ipc_parent_dir_for_uid`, APP_NAME RemoteSmith).
#[cfg(unix)]
fn platform_endpoint() -> String {
    format!("/tmp/RemoteSmith-{}/{SOCKET_NAME}", euid())
}

#[cfg(windows)]
fn platform_endpoint() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".into());
    let user: String = user.chars().filter(|c| *c != '\\' && *c != '/').collect();
    format!(r"\\.\pipe\RemoteSmith-ipc-{user}")
}

/// Check that `dir` is a real directory (not a symlink) owned by us with no group/other access,
/// so no other user can replace entries in it.
#[cfg(unix)]
pub fn check_private_dir(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a directory", dir.display()),
        ));
    }
    if meta.uid() != euid() || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be owned by uid {} with mode 0700 (is uid {}, mode {:o}); refusing",
                dir.display(),
                euid(),
                meta.uid(),
                meta.mode() & 0o7777
            ),
        ));
    }
    Ok(())
}

/// Connect as a client. Unix: the parent directory must be private to us, the endpoint must be
/// a socket (not a symlink), and the peer credentials of the connected server must be our uid.
#[cfg(unix)]
pub async fn connect(endpoint: &str) -> io::Result<Box<dyn IpcStream>> {
    use std::os::unix::fs::FileTypeExt;
    let path = std::path::Path::new(endpoint);
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "endpoint is not a socket",
        ));
    }
    if let Some(dir) = path.parent() {
        check_private_dir(dir)?;
    }
    let s = tokio::net::UnixStream::connect(path).await?;
    // Authoritative check, immune to any path race: who is actually on the other end?
    let cred = s.peer_cred()?;
    if cred.uid() != euid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "IPC server runs as uid {} not {}; refusing",
                cred.uid(),
                euid()
            ),
        ));
    }
    Ok(Box::new(s))
}

/// Connect as a client. Windows: after opening, the pipe's server process must run as our user
/// (another process could have created the pipe name first).
#[cfg(windows)]
pub async fn connect(endpoint: &str) -> io::Result<Box<dyn IpcStream>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    loop {
        match ClientOptions::new().open(endpoint) {
            Ok(c) => {
                crate::winsec::check_pipe_server_is_us(&c)?;
                return Ok(Box::new(c));
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 10 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
