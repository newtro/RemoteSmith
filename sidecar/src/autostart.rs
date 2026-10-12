//! Login autostart registration: a per-user LaunchAgent on macOS, the HKCU Run key on Windows.

use std::io;
use std::path::{Path, PathBuf};

pub const LAUNCH_AGENT_LABEL: &str = "ai.johnnycode.remotesmith.tray";
pub const RUN_VALUE_NAME: &str = "RemoteSmithTray";

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// LaunchAgent plist that starts the tray at login and restarts it if it crashes, but not after
/// a clean Quit (KeepAlive.SuccessfulExit = false).
pub fn launch_agent_plist(exe: &Path, log_dir: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let log = xml_escape(&log_dir.join("tray.launchd.log").to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LAUNCH_AGENT_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ThrottleInterval</key>
	<integer>10</integer>
	<key>ProcessType</key>
	<string>Interactive</string>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#
    )
}

/// `~/Library/LaunchAgents/<label>.plist` under `home`.
pub fn launch_agent_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// Write the LaunchAgent plist under `home`. Takes effect at next login (we do not
/// `launchctl bootstrap` here, so installing never starts a second tray instance).
pub fn install_launch_agent(home: &Path, exe: &Path, log_dir: &Path) -> io::Result<PathBuf> {
    let path = launch_agent_path(home);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("plist.tmp");
    std::fs::write(&tmp, launch_agent_plist(exe, log_dir))?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn uninstall_launch_agent(home: &Path) -> io::Result<bool> {
    let path = launch_agent_path(home);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The Run-key command line for `exe` (quoted so paths with spaces work).
pub fn run_key_command(exe: &Path) -> String {
    format!("\"{}\"", exe.to_string_lossy())
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn install_run_key(exe: &Path) -> io::Result<()> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    key.set_value(RUN_VALUE_NAME, &run_key_command(exe))
}

#[cfg(windows)]
pub fn uninstall_run_key() -> io::Result<bool> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE) {
        Ok(k) => k,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    match key.delete_value(RUN_VALUE_NAME) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// CLI entry: install for the current user and OS. Returns a message for stdout.
pub fn install() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        let p = install_launch_agent(&home, &exe, &crate::logging::log_dir())?;
        Ok(format!(
            "installed LaunchAgent {} (starts at next login)",
            p.display()
        ))
    }
    #[cfg(windows)]
    {
        install_run_key(&exe)?;
        Ok(format!(
            r"installed HKCU\{RUN_KEY}\{RUN_VALUE_NAME} = {}",
            run_key_command(&exe)
        ))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = exe;
        Err(io::Error::other(
            "autostart is only supported on macOS and Windows",
        ))
    }
}

pub fn uninstall() -> io::Result<String> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        // Unload it too, or a loaded agent keeps relaunching the tray until logout.
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let _ = std::process::Command::new("/bin/launchctl")
            .arg("bootout")
            .arg(format!("gui/{uid}/{LAUNCH_AGENT_LABEL}"))
            .stderr(std::process::Stdio::null())
            .status();
        Ok(if uninstall_launch_agent(&home)? {
            "removed LaunchAgent"
        } else {
            "LaunchAgent was not installed"
        }
        .into())
    }
    #[cfg(windows)]
    {
        Ok(if uninstall_run_key()? {
            "removed Run key value"
        } else {
            "Run key value was not present"
        }
        .into())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err(io::Error::other(
            "autostart is only supported on macOS and Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contents() {
        let p = launch_agent_plist(
            Path::new("/Applications/Remote Smith & Co.app/x"),
            Path::new("/L"),
        );
        assert!(p.contains("<string>ai.johnnycode.remotesmith.tray</string>"));
        assert!(p.contains("<string>/Applications/Remote Smith &amp; Co.app/x</string>"));
        assert!(p.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert!(p.contains("<string>/L/tray.launchd.log</string>"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plist_is_valid_per_plutil() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            install_launch_agent(dir.path(), Path::new("/usr/bin/true"), dir.path()).unwrap();
        let out = std::process::Command::new("plutil")
            .arg("-lint")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = install_launch_agent(dir.path(), Path::new("/x/tray"), dir.path()).unwrap();
        assert!(path.ends_with("Library/LaunchAgents/ai.johnnycode.remotesmith.tray.plist"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("/x/tray"));
        // Idempotent re-install.
        install_launch_agent(dir.path(), Path::new("/y/tray"), dir.path()).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("/y/tray"));
        assert!(uninstall_launch_agent(dir.path()).unwrap());
        assert!(!uninstall_launch_agent(dir.path()).unwrap());
    }

    #[test]
    fn run_key_quotes_path() {
        assert_eq!(
            run_key_command(Path::new(
                r"C:\Program Files\RemoteSmith\remotesmith-tray.exe"
            )),
            r#""C:\Program Files\RemoteSmith\remotesmith-tray.exe""#
        );
    }
}
