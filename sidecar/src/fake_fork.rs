//! Reference / fake implementation of the fork side of docs/IPC.md.
//!
//! Used by the integration tests and by the `fake-fork` dev binary for manual demos. It follows
//! every server-side MUST in the spec (hello first, version negotiation, per-connection isolation,
//! malformed-input handling, peer-uid check, 0600 socket / user-only pipe DACL), so the fork unit
//! can port it into `vendor/rustdesk/src/` almost verbatim.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Semaphore};

use crate::protocol::{
    decode, encode, negotiate, read_line_bounded, DecodeError, Envelope, ErrorBody, ErrorCode,
    LineRead, Peer, Presence, Request, ServerMessage, Status, Topic, MAX_LINE,
};

/// What the fake fork currently "knows". Publish changes through the `watch::Sender`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub status: Status,
    pub peers: Vec<Peer>,
}

#[derive(Debug, Clone)]
pub struct FakeConfig {
    pub min_version: u32,
    pub max_version: u32,
    pub server_version: String,
    /// Send garbage, an over-long line and an unknown event type before every real reply.
    pub garbage: bool,
    /// Never answer anything after hello_ok (simulates a hung fork).
    pub silent_after_hello: bool,
    /// Answer hello and subscribe (with the snapshot), then never send anything again.
    pub silent_after_subscribe: bool,
    /// Push `connect_result{ok:true}` this long after `connect_started`.
    pub connect_result_after: Option<Duration>,
    pub hello_deadline: Duration,
    pub max_clients: usize,
    pub max_malformed: u32,
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            min_version: 1,
            max_version: 1,
            server_version: "fake-fork".into(),
            garbage: false,
            silent_after_hello: false,
            silent_after_subscribe: false,
            connect_result_after: None,
            hello_deadline: Duration::from_secs(5),
            max_clients: 8,
            max_malformed: 20,
        }
    }
}

/// Peer ids received via `connect`, in order.
pub type ConnectLog = Arc<Mutex<Vec<String>>>;

// ---------------------------------------------------------------- listener (per OS)

#[cfg(unix)]
pub struct Listener {
    inner: tokio::net::UnixListener,
    path: std::path::PathBuf,
    /// (dev, ino) of the socket we bound, so Drop never unlinks a successor's socket.
    ident: (u64, u64),
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::symlink_metadata(&self.path) {
            if (m.dev(), m.ino()) == self.ident {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

/// Bind the endpoint (docs/IPC.md §2): the parent directory must be private to this user
/// (created 0700 if missing), a stale socket of ours is replaced, a live one is never stolen.
#[cfg(unix)]
pub async fn bind(endpoint: &str) -> io::Result<Listener> {
    use crate::transport::{check_private_dir, euid};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let path = std::path::PathBuf::from(endpoint);
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "endpoint has no parent"))?;
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    check_private_dir(dir)?;
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.uid() != euid() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "endpoint owned by another user",
            ));
        }
        if tokio::net::UnixStream::connect(&path).await.is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another server is listening",
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let inner = tokio::net::UnixListener::bind(&path)?;
    // The 0700 directory already keeps other users out; 0600 is belt and braces.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let m = std::fs::symlink_metadata(&path)?;
    Ok(Listener {
        inner,
        path,
        ident: (m.dev(), m.ino()),
    })
}

#[cfg(unix)]
impl Listener {
    async fn accept(&mut self) -> io::Result<tokio::net::UnixStream> {
        loop {
            let (s, _) = self.inner.accept().await?;
            // Peer-credential check: only our own user may talk to us.
            match s.peer_cred() {
                Ok(c) if c.uid() == crate::transport::euid() => return Ok(s),
                Ok(c) => log::warn!("rejected IPC client with uid {}", c.uid()),
                Err(e) => log::warn!("rejected IPC client: no peer credentials ({e})"),
            }
        }
    }
}

#[cfg(windows)]
pub struct Listener {
    name: String,
    next: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
fn create_pipe(
    name: &str,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    static SA: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let sa = match SA.get() {
        Some(p) => *p,
        None => {
            let p = crate::winsec::user_only_security_attributes()? as *mut _ as usize;
            *SA.get_or_init(|| p)
        }
    };
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: `sa` points to a leaked, valid SECURITY_ATTRIBUTES.
    unsafe { opts.create_with_security_attributes_raw(name, sa as *mut std::ffi::c_void) }
}

#[cfg(windows)]
pub async fn bind(endpoint: &str) -> io::Result<Listener> {
    Ok(Listener {
        name: endpoint.to_string(),
        next: create_pipe(endpoint, true)?,
    })
}

#[cfg(windows)]
impl Listener {
    async fn accept(&mut self) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        if let Err(e) = self.next.connect().await {
            // e.g. ERROR_NO_DATA: the client left before we connected. Start a fresh instance.
            self.next = create_pipe(&self.name, false)?;
            return Err(e);
        }
        let fresh = create_pipe(&self.name, false)?;
        Ok(std::mem::replace(&mut self.next, fresh))
    }
}

// ---------------------------------------------------------------- serving

/// Serve forever on `listener`. Each connection runs in its own task; one misbehaving client
/// can never affect another (DEC-5, AC-25).
pub async fn serve(
    mut listener: Listener,
    cfg: FakeConfig,
    snap: watch::Receiver<Snapshot>,
    connects: ConnectLog,
) -> io::Result<()> {
    let slots = Arc::new(Semaphore::new(cfg.max_clients));
    // Connections live in a JoinSet so dropping `serve` (fork exit) closes every one of them.
    let mut conns = tokio::task::JoinSet::new();
    loop {
        let stream = tokio::select! {
            s = listener.accept() => match s {
                Ok(s) => s,
                Err(e) => {
                    // Transient (EMFILE, ECONNABORTED, a client that vanished): never let one
                    // failed accept take the endpoint down for good.
                    log::warn!("IPC accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            Some(_) = conns.join_next(), if !conns.is_empty() => continue,
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(stream); // over the client cap: close immediately
            continue;
        };
        let (cfg, snap, connects) = (cfg.clone(), snap.clone(), connects.clone());
        conns.spawn(async move {
            if let Err(e) = handle_conn(stream, cfg, snap, connects).await {
                log::debug!("IPC client ended: {e}");
            }
            drop(permit);
        });
    }
}

async fn write_line<W: AsyncWrite + Unpin>(w: &mut W, line: &str) -> io::Result<()> {
    match tokio::time::timeout(Duration::from_secs(5), w.write_all(line.as_bytes())).await {
        Ok(r) => r,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "client not reading",
        )),
    }
}

async fn handle_conn<S>(
    stream: S,
    cfg: FakeConfig,
    mut snap: watch::Receiver<Snapshot>,
    connects: ConnectLog,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (rd, mut wr) = tokio::io::split(stream);
    let (tx, mut lines) = mpsc::channel::<LineRead>(16);
    let reader = tokio::spawn(async move {
        let mut rd = BufReader::new(rd);
        loop {
            let r = read_line_bounded(&mut rd, MAX_LINE)
                .await
                .unwrap_or(LineRead::Eof);
            let eof = r == LineRead::Eof;
            if tx.send(r).await.is_err() || eof {
                return;
            }
        }
    });
    struct Abort(tokio::task::JoinHandle<()>);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _abort = Abort(reader);

    let mut version: Option<u32> = None;
    let mut topics: Vec<Topic> = Vec::new();
    let mut malformed = 0u32;
    let mut silenced = false;
    let hello_deadline = tokio::time::Instant::now() + cfg.hello_deadline;
    let (delayed_tx, mut delayed) = mpsc::unbounded_channel::<String>();

    loop {
        tokio::select! {
            line = lines.recv() => {
                let v = version.unwrap_or(1);
                let bytes = match line {
                    None | Some(LineRead::Eof) => return Ok(()),
                    Some(LineRead::TooLong) => {
                        malformed += 1;
                        write_line(&mut wr, &encode(v, None, err(ErrorCode::LineTooLong, "line exceeds 1 MiB"))).await?;
                        if malformed >= cfg.max_malformed { return Ok(()); }
                        continue;
                    }
                    Some(LineRead::Line(b)) => b,
                };
                let env = match decode::<Request>(&bytes) {
                    Ok(e) => e,
                    Err((e, id)) => {
                        malformed += 1;
                        let code = if e == DecodeError::TooLong { ErrorCode::LineTooLong } else { ErrorCode::BadRequest };
                        write_line(&mut wr, &encode(v, id, err(code, "malformed request"))).await?;
                        if malformed >= cfg.max_malformed { return Ok(()); }
                        continue;
                    }
                };
                if silenced || (version.is_some() && cfg.silent_after_hello) {
                    continue;
                }
                if let Some(v) = version {
                    if env.v != v {
                        malformed += 1;
                        write_line(&mut wr, &encode(v, env.id, err(ErrorCode::BadRequest, format!("negotiated v{v}, got v{}", env.v)))).await?;
                        if malformed >= cfg.max_malformed { return Ok(()); }
                        continue;
                    }
                }
                if cfg.garbage {
                    write_line(&mut wr, "this is not json\n").await?;
                    write_line(&mut wr, &format!("{}\n", "x".repeat(MAX_LINE + 10))).await?;
                    write_line(&mut wr, &format!("{{\"v\":{v},\"type\":\"from_the_future\",\"x\":1}}\n")).await?;
                    write_line(&mut wr, "{\"v\":1,\"id\":999999,\"type\":\"pong\"}\n").await?;
                }
                let id = env.id;
                match (version, env.body) {
                    (Some(v), Request::Hello { .. }) => {
                        write_line(&mut wr, &encode(v, id, err(ErrorCode::BadRequest, "already negotiated"))).await?;
                    }
                    (None, Request::Hello { min_version, max_version, .. }) => {
                        match negotiate(min_version, max_version, cfg.min_version, cfg.max_version) {
                            Some(chosen) => {
                                version = Some(chosen);
                                write_line(&mut wr, &encode(chosen, id, ServerMessage::HelloOk {
                                    version: chosen,
                                    min_version: cfg.min_version,
                                    max_version: cfg.max_version,
                                    server: "RemoteSmith".into(),
                                    server_version: cfg.server_version.clone(),
                                })).await?;
                            }
                            None => {
                                let mut e = ErrorBody::new(ErrorCode::VersionMismatch, format!(
                                    "server speaks {}-{}, client {min_version}-{max_version}", cfg.min_version, cfg.max_version));
                                e.min_version = Some(cfg.min_version);
                                e.max_version = Some(cfg.max_version);
                                write_line(&mut wr, &encode(1, id, ServerMessage::Error { error: e })).await?;
                                return Ok(());
                            }
                        }
                    }
                    (None, _) => {
                        write_line(&mut wr, &encode(1, id, err(ErrorCode::HelloRequired, "send hello first"))).await?;
                    }
                    (Some(v), Request::GetStatus) => {
                        let status = snap.borrow().status.clone();
                        write_line(&mut wr, &encode(v, id, ServerMessage::Status { status: Box::new(status) })).await?;
                    }
                    (Some(v), Request::ListPeers) => {
                        let peers = snap.borrow().peers.clone();
                        write_line(&mut wr, &encode(v, id, ServerMessage::Peers { peers })).await?;
                    }
                    (Some(v), Request::Ping) => write_line(&mut wr, &encode(v, id, ServerMessage::Pong)).await?,
                    (Some(v), Request::Subscribe { topics: t }) => {
                        topics = t.into_iter().filter(|t| *t != Topic::Unknown).collect();
                        write_line(&mut wr, &encode(v, id, ServerMessage::Subscribed { topics: topics.clone() })).await?;
                        let s = snap.borrow_and_update().clone();
                        push(&mut wr, v, &topics, &s).await?;
                        silenced = cfg.silent_after_subscribe;
                    }
                    (Some(v), Request::Connect { peer_id }) => {
                        connects.lock().map(|mut c| c.push(peer_id.clone())).ok();
                        let peer = snap.borrow().peers.iter().find(|p| p.id == peer_id).cloned();
                        let reply = match peer {
                            None => err(ErrorCode::PeerUnknown, format!("no peer {peer_id}")),
                            Some(Peer { presence: Presence::Unavailable { reason, .. }, .. }) => {
                                err(ErrorCode::PeerUnavailable, reason.label())
                            }
                            Some(_) => {
                                if let Some(after) = cfg.connect_result_after {
                                    let tx = delayed_tx.clone();
                                    let line = encode(v, None, ServerMessage::ConnectResult {
                                        peer_id: peer_id.clone(), ok: true, reason: None, detail: None });
                                    tokio::spawn(async move {
                                        tokio::time::sleep(after).await;
                                        let _ = tx.send(line);
                                    });
                                }
                                ServerMessage::ConnectStarted { peer_id }
                            }
                        };
                        write_line(&mut wr, &encode(v, id, reply)).await?;
                    }
                    (Some(v), Request::Unknown) => {
                        write_line(&mut wr, &encode(v, id, err(ErrorCode::UnknownType, "unknown request type"))).await?;
                    }
                }
            }
            changed = snap.changed(), if version.is_some() && !topics.is_empty() && !cfg.silent_after_hello && !silenced => {
                if changed.is_err() { return Ok(()); }
                let s = snap.borrow_and_update().clone();
                push(&mut wr, version.unwrap_or(1), &topics, &s).await?;
            }
            Some(line) = delayed.recv() => write_line(&mut wr, &line).await?,
            _ = tokio::time::sleep_until(hello_deadline), if version.is_none() => return Ok(()),
        }
    }
}

fn err(code: ErrorCode, msg: impl Into<String>) -> ServerMessage {
    ServerMessage::Error {
        error: ErrorBody::new(code, msg),
    }
}

async fn push<W: AsyncWrite + Unpin>(
    w: &mut W,
    v: u32,
    topics: &[Topic],
    s: &Snapshot,
) -> io::Result<()> {
    if topics.contains(&Topic::Status) {
        write_line(
            w,
            &encode(
                v,
                None,
                ServerMessage::Status {
                    status: Box::new(s.status.clone()),
                },
            ),
        )
        .await?;
    }
    if topics.contains(&Topic::Peers) {
        write_line(
            w,
            &encode(
                v,
                None,
                ServerMessage::Peers {
                    peers: s.peers.clone(),
                },
            ),
        )
        .await?;
    }
    Ok(())
}

/// Convenience used by tests: the `Envelope` type re-exported for raw-client assertions.
pub type ServerEnvelope = Envelope<ServerMessage>;
