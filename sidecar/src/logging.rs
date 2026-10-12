//! File logging to the per-OS log directory, with redaction applied to every line.
//!
//! No secret ever crosses the IPC (docs/IPC.md §1), so redaction is defence in depth: anything
//! that looks like `password=…`, `"token":"…"`, `key: …` etc. is masked before it reaches disk.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

fn redact_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // key names (optionally quoted) followed by : or = and a value (quoted or bare).
        regex::Regex::new(
            r#"(?i)("?(?:password|passwd|pass|pwd|secret|token|api[_-]?key|private[_-]?key|credential|auth)"?\s*[:=]\s*)("(?:[^"\\]|\\.)*"|[^\s,;}&]+)"#,
        )
        .expect("valid redaction regex")
    })
}

/// Mask secret-looking values in `s`.
pub fn redact(s: &str) -> String {
    redact_re()
        .replace_all(s, |c: &regex::Captures| {
            let quoted = c[2].starts_with('"');
            format!(
                "{}{}",
                &c[1],
                if quoted {
                    "\"[REDACTED]\""
                } else {
                    "[REDACTED]"
                }
            )
        })
        .into_owned()
}

/// Per-OS log directory: `~/Library/Logs/RemoteSmith` on macOS,
/// `%LOCALAPPDATA%\RemoteSmith\logs` on Windows, XDG state/data dir elsewhere.
pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = dirs::home_dir() {
            return h.join("Library/Logs/RemoteSmith");
        }
    }
    #[cfg(windows)]
    {
        if let Some(d) = dirs::data_local_dir() {
            return d.join("RemoteSmith").join("logs");
        }
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("RemoteSmith")
        .join("logs")
}

struct FileLogger {
    file: Mutex<File>,
    level: log::LevelFilter,
    echo_stderr: bool,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level
    }

    fn log(&self, rec: &log::Record) {
        if !self.enabled(rec.metadata()) {
            return;
        }
        let ts = crate::client::unix_now();
        let line = redact(&format!(
            "{ts} {} {}: {}\n",
            rec.level(),
            rec.target(),
            rec.args()
        ));
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
        }
        if self.echo_stderr {
            eprint!("{line}");
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

fn open_rotated(path: &Path) -> std::io::Result<File> {
    if let Ok(m) = std::fs::metadata(path) {
        if m.len() > MAX_LOG_BYTES {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Install the file logger. Returns the log file path. Falls back to stderr-only on failure.
pub fn init(name: &str, echo_stderr: bool) -> Option<PathBuf> {
    let level = match std::env::var("REMOTESMITH_LOG").as_deref() {
        Ok("debug") => log::LevelFilter::Debug,
        Ok("trace") => log::LevelFilter::Trace,
        Ok("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Info,
    };
    let dir = log_dir();
    let path = dir.join(format!("{name}.log"));
    let file = std::fs::create_dir_all(&dir).and_then(|_| open_rotated(&path));
    match file {
        Ok(file) => {
            let logger = FileLogger {
                file: Mutex::new(file),
                level,
                echo_stderr,
            };
            if log::set_boxed_logger(Box::new(logger)).is_ok() {
                log::set_max_level(level);
            }
            Some(path)
        }
        Err(e) => {
            eprintln!("remotesmith: cannot open log {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_shapes() {
        assert_eq!(
            redact(r#"{"password":"hunter2","id":"1"}"#),
            r#"{"password":"[REDACTED]","id":"1"}"#
        );
        assert_eq!(redact("token=abc123 next"), "token=[REDACTED] next");
        assert_eq!(redact("Api-Key: sk-live-xyz"), "Api-Key: [REDACTED]");
        assert_eq!(
            redact(r#""private_key" : "a\"b""#),
            r#""private_key" : "[REDACTED]""#
        );
        assert_eq!(
            redact("peer 123 reachable via LAN"),
            "peer 123 reachable via LAN"
        );
        // Fingerprints and ids are public and stay readable.
        assert_eq!(
            redact(r#"{"key_fingerprint":"SHA256:ab"}"#),
            r#"{"key_fingerprint":"SHA256:ab"}"#
        );
    }

    #[test]
    fn log_dir_is_per_os() {
        let d = log_dir();
        assert!(d.to_string_lossy().contains("RemoteSmith"));
        #[cfg(target_os = "macos")]
        assert!(d.ends_with("Library/Logs/RemoteSmith"));
    }

    #[test]
    fn rotation_moves_big_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.log");
        std::fs::write(&p, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();
        let mut f = open_rotated(&p).unwrap();
        f.write_all(b"new\n").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new\n");
        assert!(dir.path().join("t.log.1").exists());
    }
}
