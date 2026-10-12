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
                other => return fail(format!("unexpected hello reply: {}", other.kind()), false),
            },
            Ok(_) => {} // events before hello_ok are not expected; ignore
            Err((e, _)) => {
                malformed += 1;
                log::warn!("malformed line during hello: {}", e.describe(bytes.len()));
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
                        log::warn!("malformed line: {}", e.describe(bytes.len()));
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
        let Some((kind, _)) = pending.get(&id) else {
            log::debug!("response for unknown id {id} ignored");
            return None;
        };
        // Validate the reply type against what was asked before consuming the pending entry.
        let expected = matches!(
            (kind, &env.body),
            (Pending::Connect(_), ServerMessage::ConnectStarted { .. })
                | (Pending::Subscribe, ServerMessage::Subscribed { .. })
                | (Pending::Ping, ServerMessage::Pong)
                | (Pending::Hello, ServerMessage::HelloOk { .. })
                | (_, ServerMessage::Error { .. })
        );
        let (kind, _) = pending.remove(&id)?;
        if !expected {
            log::warn!("unexpected {} reply to request {id}", env.body.kind());
            return match kind {
                Pending::Connect(peer_id) => {
                    r.apply(StateEvent::ConnectFailed {
                        peer_id,
                        reason: "unexpected reply from RemoteSmith".into(),
                    });
                    None
                }
                Pending::Subscribe => Some(End::Failed {
                    reason: "unexpected reply to subscribe from RemoteSmith".into(),
                    handshake_ok: true,
                }),
                Pending::Ping | Pending::Hello => None,
            };
        }
        match (kind, env.body) {
            (Pending::Connect(peer_id), ServerMessage::ConnectStarted { .. }) => {
                r.apply(StateEvent::ConnectStarted { peer_id })
            }
            (Pending::Connect(peer_id), ServerMessage::Error { error }) => {
                let reason = if error.message.is_empty() {
                    error.code.label()
                } else {
                    redact(&error.message)
                };
                r.apply(StateEvent::ConnectFailed { peer_id, reason })
            }
            (Pending::Subscribe, ServerMessage::Error { error }) => {
                return Some(End::Failed {
                    reason: format!("RemoteSmith refused subscribe: {}", redact(&error.message)),
                    handshake_ok: true,
                })
            }
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
            log::warn!(
                "server error: {} {}",
                error.code.label(),
                redact(&error.message)
            )
        }
        _ => {}
    }
    None
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ErrorBody, Peer, Presence, Route};
    use crate::state::{ConnectState, StateEvent};

    fn reducer() -> Reducer<impl FnMut(&AppState)> {
        let mut r = Reducer {
            state: AppState::default(),
            sink: |_: &AppState| {},
        };
        r.apply(StateEvent::LinkUp {
            protocol: 1,
            server_version: "t".into(),
        });
        r.apply(StateEvent::Peers(vec![Peer {
            id: "1".into(),
            alias: None,
            platform: None,
            presence: Presence::Reachable { route: Route::Lan },
        }]));
        r
    }

    fn reply(id: u64, body: ServerMessage) -> Envelope<ServerMessage> {
        Envelope::new(1, Some(id), body)
    }

    #[test]
    fn wrong_reply_type_fails_the_connect_instead_of_hanging() {
        let mut r = reducer();
        let mut pending = HashMap::new();
        r.apply(StateEvent::ConnectRequested {
            peer_id: "1".into(),
        });
        pending.insert(7, (Pending::Connect("1".into()), Instant::now()));
        assert!(handle_message(reply(7, ServerMessage::Pong), &mut pending, &mut r).is_none());
        assert!(pending.is_empty());
        assert_eq!(
            r.state.peers[0].connect,
            ConnectState::Failed("unexpected reply from RemoteSmith".into())
        );
    }

    #[test]
    fn matching_replies_are_applied() {
        let mut r = reducer();
        let mut pending = HashMap::new();
        pending.insert(1, (Pending::Connect("1".into()), Instant::now()));
        handle_message(
            reply(
                1,
                ServerMessage::ConnectStarted {
                    peer_id: "1".into(),
                },
            ),
            &mut pending,
            &mut r,
        );
        assert_eq!(r.state.peers[0].connect, ConnectState::Started);
        pending.insert(2, (Pending::Connect("1".into()), Instant::now()));
        let err = ErrorBody::new(ErrorCode::PeerUnavailable, "");
        handle_message(
            reply(2, ServerMessage::Error { error: err }),
            &mut pending,
            &mut r,
        );
        assert_eq!(
            r.state.peers[0].connect,
            ConnectState::Failed("peer_unavailable".into())
        );
        // A pong for a ping is fine; a stray id is ignored.
        pending.insert(3, (Pending::Ping, Instant::now()));
        assert!(handle_message(reply(3, ServerMessage::Pong), &mut pending, &mut r).is_none());
        assert!(handle_message(reply(99, ServerMessage::Pong), &mut pending, &mut r).is_none());
        assert!(pending.is_empty());
    }

    #[test]
    fn wrong_reply_to_subscribe_drops_the_link() {
        let mut r = reducer();
        let mut pending = HashMap::new();
        pending.insert(2, (Pending::Subscribe, Instant::now()));
        let end = handle_message(reply(2, ServerMessage::Pong), &mut pending, &mut r);
        assert!(matches!(end, Some(End::Failed { .. })));
    }
}
