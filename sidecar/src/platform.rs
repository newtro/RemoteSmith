//! Small OS helpers: opening the RemoteSmith app, single-instance lock.

use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::process::Command;

pub const BUNDLE_ID: &str = "ai.johnnycode.remotesmith";

fn spawn_detached(mut cmd: Command) -> io::Result<()> {
    let mut child = cmd.spawn()?;
    // Reap in the background so no zombie is left behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Bring up the main RemoteSmith window.
pub fn open_app() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("/usr/bin/open");
        c.args(["-b", BUNDLE_ID]);
        spawn_detached(c)
    }
    #[cfg(windows)]
    {
        let exe = windows_app_path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "RemoteSmith.exe not found"))?;
        spawn_detached(Command::new(exe))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        spawn_detached(Command::new("remotesmith"))
    }
}

/// `RemoteSmith.exe` next to the tray binary (installed layout), else under Program Files.
#[cfg(windows)]
fn windows_app_path() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()?
        .parent()?
        .join("RemoteSmith.exe");
    if beside.exists() {
        return Some(beside);
    }
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    let p = pf.join("RemoteSmith").join("RemoteSmith.exe");
    p.exists().then_some(p)
}

/// Hold an exclusive lock so only one tray runs per user. Keep the returned file alive.
pub fn single_instance_lock() -> io::Result<Option<File>> {
    let dir = crate::logging::log_dir();
    std::fs::create_dir_all(&dir)?;
    let path: PathBuf = dir.join("tray.lock");
    let f = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}
