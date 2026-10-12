//! IPC client: connects to the fork, negotiates, subscribes, keeps the link alive, reconnects
//! with backoff, and reduces everything into an [`AppState`] snapshot (docs/IPC.md §4–§10).
//!
//! All I/O runs on its own thread / runtime. The UI only ever receives finished snapshots through
//! the `sink` callback and sends [`Command`]s through an unbounded channel, so it never blocks.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::logging::redact;
use crate::protocol::{
    self, decode, encode, read_line_bounded, Envelope, ErrorCode, LineRead, Request, ServerMessage,
    Topic,
};
use crate::state::{connect_failure_text, AppState, StateEvent};
use crate::transport;

/// Commands from the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Connect(String),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub endpoint: String,
    pub min_version: u32,
    pub max_version: u32,
    pub connect_timeout: Duration,
    pub hello_timeout: Duration,
    pub request_timeout: Duration,
    pub ping_interval: Duration,
    pub stale_after: Duration,
    pub write_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    pub mismatch_retry: Duration,
    pub max_malformed: u32,
    pub max_line: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoint: transport::default_endpoint(),
            min_version: protocol::MIN_VERSION,
            max_version: protocol::MAX_VERSION,
            connect_timeout: Duration::from_secs(2),
            hello_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(5),
            stale_after: Duration::from_secs(12),
            write_timeout: Duration::from_secs(5),
            backoff_min: Duration::from_millis(250),
            backoff_max: Duration::from_secs(5),
            mismatch_retry: Duration::from_secs(30),
            max_malformed: 20,
            max_line: protocol::MAX_LINE,
        }
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Spawn the client on its own thread. Returns the command sender and the thread handle.
pub fn spawn<F>(
    cfg: ClientConfig,
    sink: F,
) -> (mpsc::UnboundedSender<Command>, std::thread::JoinHandle<()>)
where
    F: FnMut(&AppState) + Send + 'static,
{
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = std::thread::Builder::new()
        .name("remotesmith-ipc".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("cannot start IPC runtime: {e}");
                    return;
                }
            };
            rt.block_on(run(cfg, rx, sink));
        })
        .expect("spawn IPC thread");
    (tx, handle)
}

struct Reducer<F: FnMut(&AppState)> {
    state: AppState,
    sink: F,
}

impl<F: FnMut(&AppState)> Reducer<F> {
    fn apply(&mut self, ev: StateEvent) {
        let before = self.state.clone();
        self.state.apply(ev, unix_now());
        if self.state != before {
            (self.sink)(&self.state);
        }
    }
}

enum End {
    Shutdown,
    Mismatch,
    Failed { reason: String, handshake_ok: bool },
}

/// Run the client until [`Command::Shutdown`] or the command channel closes.
pub async fn run<F>(cfg: ClientConfig, mut cmds: mpsc::UnboundedReceiver<Command>, sink: F)
where
    F: FnMut(&AppState),
{
    let mut r = Reducer {
        state: AppState::default(),
        sink,
    };
    (r.sink)(&r.state);
    let mut backoff = cfg.backoff_min;
    loop {
        let wait = match session(&cfg, &mut cmds, &mut r).await {
            End::Shutdown => return,
            End::Mismatch => cfg.mismatch_retry,
            End::Failed {
                reason,
                handshake_ok,
            } => {
                log::info!("IPC link down: {reason}");
                r.apply(StateEvent::LinkDown { reason });
                if handshake_ok {
                    backoff = cfg.backoff_min;
                }
                let w = backoff;
                backoff = (backoff * 2).min(cfg.backoff_max);
                w
            }
        };
        // Back off, still answering the UI (Connect while down fails with a reason, AC-21).
        let deadline = Instant::now() + wait;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                cmd = cmds.recv() => match cmd {
                    None | Some(Command::Shutdown) => return,
                    Some(Command::Connect(peer_id)) => r.apply(StateEvent::ConnectRequested { peer_id }),
                },
            }
        }
    }
}

fn describe_connect_error(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound | ConnectionRefused => "RemoteSmith app not running".into(),
        TimedOut => "RemoteSmith app not responding".into(),
        _ => format!("cannot reach RemoteSmith ({e})"),
    }
}

enum Pending {
    Hello,
    Subscribe,
    Ping,
    Connect(String),
}

async fn session<F: FnMut(&AppState)>(
    cfg: &ClientConfig,
    cmds: &mut mpsc::UnboundedReceiver<Command>,
    r: &mut Reducer<F>,
) -> End {
    let fail = |reason: String, handshake_ok: bool| End::Failed {
        reason,
        handshake_ok,
    };
    let stream =
        match tokio::time::timeout(cfg.connect_timeout, transport::connect(&cfg.endpoint)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return fail(describe_connect_error(&e), false),
            Err(_) => return fail("RemoteSmith app not responding".into(), false),
        };
    let (rd, mut wr) = tokio::io::split(stream);

    // Dedicated reader task: read_line_bounded is not cancel-safe, so it never sits in a select!.
    let (line_tx, mut lines) = mpsc::channel::<LineRead>(64);
    let max_line = cfg.max_line;
    let reader = tokio::spawn(async move {
        let mut rd = BufReader::with_capacity(64 * 1024, rd);
        loop {
            let res = read_line_bounded(&mut rd, max_line).await;
            let eof = !matches!(res, Ok(LineRead::Line(_)) | Ok(LineRead::TooLong));
            if line_tx.send(res.unwrap_or(LineRead::Eof)).await.is_err() || eof {
                return;
            }
        }
    });
    let _abort = AbortOnDrop(reader);

    let mut next_id: u64 = 1;
    let mut pending: HashMap<u64, (Pending, Instant)> = HashMap::new();
    let mut malformed = 0u32;

    macro_rules! send {
        ($v:expr, $req:expr, $kind:expr) => {{
            let id = next_id;
            next_id += 1;
            let line = encode($v, Some(id), $req);
            match tokio::time::timeout(cfg.write_timeout, wr.write_all(line.as_bytes())).await {
                Ok(Ok(())) => {
                    pending.insert(id, ($kind, Instant::now()));
                    Ok(id)
                }
                Ok(Err(e)) => Err(format!("write failed: {e}")),
                Err(_) => Err("write timed out".to_string()),
            }
        }};
    }

    // --- hello (frozen shape, always v=1) ---
    let hello = Request::Hello {
        min_version: cfg.min_version,
        max_version: cfg.max_version,
        client: "remotesmith-tray".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
    };
    let hello_id = match send!(1, hello, Pending::Hello) {
        Ok(id) => id,
        Err(e) => return fail(e, false),
    };
    let hello_deadline = Instant::now() + cfg.hello_timeout;
    let version = loop {
        let line = tokio::select! {
            l = lines.recv() => l,
            _ = tokio::time::sleep_until(hello_deadline) => return fail("RemoteSmith did not answer hello".into(), false),
        };
        let bytes = match line {
            None | Some(LineRead::Eof) => {
                return fail("RemoteSmith closed the connection".into(), false)
            }
            Some(LineRead::TooLong) => {
                malformed += 1;
                if malformed >= cfg.max_malformed {
                    return fail("too many malformed messages from RemoteSmith".into(), false);
                }
                continue;
            }
            Some(LineRead::Line(b)) => b,
        };
        match decode::<ServerMessage>(&bytes) {
            Ok(Envelope {
                id: Some(id), body, ..
            }) if id == hello_id => match body {
                ServerMessage::HelloOk {
                    version,
                    min_version,
                    max_version,
                    server_version,
                    ..
                } => {
                    if version < cfg.min_version || version > cfg.max_version {
                        log::warn!("server chose v{version} outside our range");
                        r.apply(StateEvent::VersionMismatch {
                            server_min: min_version,
                            server_max: max_version,
                        });
                        return End::Mismatch;
                    }
                    pending.remove(&hello_id);
                    log::info!("IPC up: protocol v{version}, server {server_version}");
                    r.apply(StateEvent::LinkUp {
                        protocol: version,
                        server_version,
                    });
                    break version;
                }
                ServerMessage::Error { error } if error.code == ErrorCode::VersionMismatch => {
                    log::warn!("version mismatch: {}", redact(&error.message));
                    r.apply(StateEvent::VersionMismatch {
                        server_min: error.min_version.unwrap_or(0),
                        server_max: error.max_version.unwrap_or(0),
                    });
                    return End::Mismatch;
                }
                other => {
                    return fail(
                        format!("unexpected hello reply: {}", short(&format!("{other:?}"))),
                        false,
                    )
                }
            },
            Ok(_) => {} // events before hello_ok are not expected; ignore
            Err((e, _)) => {
                malformed += 1;
                log::warn!(
                    "malformed line during hello ({e:?}): {}",
                    short(&String::from_utf8_lossy(&bytes))
                );
            }
        }
        if malformed >= cfg.max_malformed {
            return fail("too many malformed messages from RemoteSmith".into(), false);
        }
    };
    malformed = 0;

    if let Err(e) = send!(
        version,
        Request::Subscribe {
            topics: vec![Topic::Status, Topic::Peers]
        },
        Pending::Subscribe
    ) {
        return fail(e, true);
    }

    let mut last_rx = Instant::now();
    let mut last_ping = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            line = lines.recv() => {
                let bytes = match line {
                    None | Some(LineRead::Eof) => return fail("RemoteSmith closed the connection".into(), true),
                    Some(LineRead::TooLong) => {
                        last_rx = Instant::now();
                        malformed += 1;
                        log::warn!("discarded over-long line from RemoteSmith");
                        if malformed >= cfg.max_malformed {
                            return fail("too many malformed messages from RemoteSmith".into(), true);
                        }
                        continue;
                    }
                    Some(LineRead::Line(b)) => b,
                };
                last_rx = Instant::now();
                let env = match decode::<ServerMessage>(&bytes) {
                    Ok(env) if env.v == version => env,
                    Ok(env) => {
                        malformed += 1;
                        log::warn!("ignoring message with v={} (negotiated {version})", env.v);
                        if malformed >= cfg.max_malformed {
                            return fail("too many malformed messages from RemoteSmith".into(), true);
                        }
                        continue;
                    }
                    Err((e, _)) => {
                        malformed += 1;
                        log::warn!("malformed line ({e:?}): {}", short(&String::from_utf8_lossy(&bytes)));
                        if malformed >= cfg.max_malformed {
                            return fail("too many malformed messages from RemoteSmith".into(), true);
                        }
                        continue;
                    }
                };
                malformed = 0;
                if let Some(end) = handle_message(env, &mut pending, r) {
                    return end;
                }
            }
            cmd = cmds.recv() => match cmd {
                None | Some(Command::Shutdown) => return End::Shutdown,
                Some(Command::Connect(peer_id)) => {
                    r.apply(StateEvent::ConnectRequested { peer_id: peer_id.clone() });
                    if let Err(e) = send!(version, Request::Connect { peer_id: peer_id.clone() }, Pending::Connect(peer_id.clone())) {
                        r.apply(StateEvent::ConnectFailed { peer_id, reason: "could not send request to RemoteSmith".into() });
                        return fail(e, true);
                    }
                }
            },
            _ = tick.tick() => {
                let now = Instant::now();
                if now.duration_since(last_rx) >= cfg.stale_after {
                    return fail("RemoteSmith stopped responding".into(), true);
                }
                if now.duration_since(last_ping) >= cfg.ping_interval {
                    last_ping = now;
                    if let Err(e) = send!(version, Request::Ping, Pending::Ping) {
                        return fail(e, true);
                    }
                }
                let expired: Vec<u64> = pending
                    .iter()
                    .filter(|(_, (_, at))| now.duration_since(*at) >= cfg.request_timeout)
                    .map(|(id, _)| *id)
                    .collect();
                for id in expired {
                    match pending.remove(&id) {
                        Some((Pending::Connect(peer_id), _)) => r.apply(StateEvent::ConnectFailed {
                            peer_id,
                            reason: "no response from RemoteSmith".into(),
                        }),
                        Some((Pending::Subscribe, _)) => return fail("RemoteSmith did not accept subscribe".into(), true),
                        _ => {} // ping: staleness covers it
                    }
                }
            }
        }
    }
}

fn handle_message<F: FnMut(&AppState)>(
    env: Envelope<ServerMessage>,
    pending: &mut HashMap<u64, (Pending, Instant)>,
    r: &mut Reducer<F>,
) -> Option<End> {
    if let Some(id) = env.id {
        let Some((kind, _)) = pending.remove(&id) else {
            log::debug!("response for unknown id {id} ignored");
            return None;
        };
        match (kind, env.body) {
            (Pending::Connect(peer_id), ServerMessage::ConnectStarted { .. }) => {
                r.apply(StateEvent::ConnectStarted { peer_id })
            }
            (Pending::Connect(peer_id), ServerMessage::Error { error }) => {
                let reason = if error.message.is_empty() {
                    format!("{:?}", error.code)
                } else {
                    error.message
                };
                r.apply(StateEvent::ConnectFailed {
                    peer_id,
                    reason: redact(&reason),
                })
            }
            (Pending::Subscribe, ServerMessage::Error { error }) => {
                return Some(End::Failed {
                    reason: format!("RemoteSmith refused subscribe: {}", redact(&error.message)),
                    handshake_ok: true,
                })
            }
            (_, ServerMessage::Status { status }) => r.apply(StateEvent::Status(status)),
            (_, ServerMessage::Peers { peers }) => r.apply(StateEvent::Peers(peers)),
            _ => {}
        }
        return None;
    }
    match env.body {
        ServerMessage::Status { status } => r.apply(StateEvent::Status(status)),
        ServerMessage::Peers { peers } => r.apply(StateEvent::Peers(peers)),
        ServerMessage::ConnectResult {
            peer_id,
            ok,
            reason,
            detail,
        } => {
            if ok {
                r.apply(StateEvent::ConnectStarted { peer_id })
            } else {
                let text = connect_failure_text(reason.as_ref(), detail.as_deref());
                r.apply(StateEvent::ConnectFailed {
                    peer_id,
                    reason: text,
                })
            }
        }
        ServerMessage::Error { error } => {
            log::warn!("server error: {:?} {}", error.code, redact(&error.message))
        }
        _ => {}
    }
    None
}

fn short(s: &str) -> String {
    let red = redact(s);
    if red.chars().count() > 120 {
        let mut t: String = red.chars().take(120).collect();
        t.push('…');
        t
    } else {
        red
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
