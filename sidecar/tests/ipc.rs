//! End-to-end tests: the real IPC client against the reference fake fork (docs/IPC.md).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use remotesmith_tray::client::{self, ClientConfig, Command};
use remotesmith_tray::fake_fork::{bind, serve, ConnectLog, FakeConfig, Snapshot};
use remotesmith_tray::menu_model;
use remotesmith_tray::protocol::*;
use remotesmith_tray::state::{AppState, ConnectState, Link};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

struct Endpoint {
    path: String,
    _dir: Option<tempfile::TempDir>,
}

fn endpoint() -> Endpoint {
    #[cfg(unix)]
    {
        let dir = tempfile::Builder::new()
            .prefix("rs")
            .tempdir_in("/tmp")
            .unwrap();
        // Mirror the fork's private per-user IPC directory (mode 0700).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("ipc.sock").to_string_lossy().into_owned();
        Endpoint {
            path,
            _dir: Some(dir),
        }
    }
    #[cfg(windows)]
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = format!(
            r"\\.\pipe\RemoteSmith-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        );
        Endpoint { path, _dir: None }
    }
}

fn fast_cfg(endpoint: &str) -> ClientConfig {
    ClientConfig {
        endpoint: endpoint.to_string(),
        connect_timeout: Duration::from_millis(500),
        hello_timeout: Duration::from_millis(500),
        request_timeout: Duration::from_millis(500),
        ping_interval: Duration::from_millis(150),
        stale_after: Duration::from_millis(800),
        write_timeout: Duration::from_millis(500),
        backoff_min: Duration::from_millis(20),
        backoff_max: Duration::from_millis(200),
        mismatch_retry: Duration::from_millis(300),
        ..ClientConfig::default()
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        status: Status {
            app_version: "test".into(),
            my_id: Some("111".into()),
            service: ServiceStatus {
                running: true,
                detail: None,
            },
            ..Default::default()
        },
        peers: vec![
            Peer {
                id: "1".into(),
                alias: Some("PC".into()),
                platform: None,
                presence: Presence::Reachable { route: Route::Lan },
            },
            Peer {
                id: "2".into(),
                alias: Some("Old".into()),
                platform: None,
                presence: Presence::Unavailable {
                    reason: Reason::Offline,
                    detail: None,
                    last_seen: Some(1),
                },
            },
        ],
    }
}

struct Fork {
    tx: watch::Sender<Snapshot>,
    connects: ConnectLog,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn start_fork(ep: &str, cfg: FakeConfig, snap: Snapshot) -> Fork {
    let (tx, rx) = watch::channel(snap);
    let connects: ConnectLog = Arc::new(Mutex::new(Vec::new()));
    let listener = bind(ep).await.expect("bind");
    let task = tokio::spawn(serve(listener, cfg, rx, connects.clone()));
    Fork { tx, connects, task }
}

struct Client {
    cmds: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<AppState>,
    task: tokio::task::JoinHandle<()>,
}

fn start_client(cfg: ClientConfig) -> Client {
    let (stx, srx) = watch::channel(AppState::default());
    let (ctx, crx) = mpsc::unbounded_channel();
    let task = tokio::spawn(client::run(cfg, crx, move |s: &AppState| {
        let _ = stx.send(s.clone());
    }));
    Client {
        cmds: ctx,
        state: srx,
        task,
    }
}

impl Client {
    async fn wait_for(&mut self, what: &str, f: impl Fn(&AppState) -> bool) -> AppState {
        let r = match tokio::time::timeout(Duration::from_secs(5), self.state.wait_for(|s| f(s)))
            .await
        {
            Ok(Ok(s)) => Some(s.clone()),
            _ => None,
        };
        match r {
            Some(s) => s,
            None => panic!(
                "timed out waiting for {what}; last state: {:#?}",
                *self.state.borrow()
            ),
        }
    }

    async fn stop(self) {
        let _ = self.cmds.send(Command::Shutdown);
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .expect("client shuts down")
            .unwrap();
    }
}

fn reachable(s: &AppState) -> bool {
    s.link.is_up() && s.peers_fresh && s.status.is_some()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presence_status_and_connect() {
    let ep = endpoint();
    let fork = start_fork(
        &ep.path,
        FakeConfig {
            connect_result_after: Some(Duration::from_millis(50)),
            ..Default::default()
        },
        snapshot(),
    )
    .await;
    let mut c = start_client(fast_cfg(&ep.path));
    let s = c.wait_for("link up with peers", reachable).await;
    assert_eq!(
        s.peers[0].presence,
        Presence::Reachable { route: Route::Lan }
    );
    // AC-20: reachable + route + a Connect item.
    let m = menu_model::build(&s, client::unix_now());
    assert!(
        m.texts()
            .iter()
            .any(|t| t.contains("PC — reachable via LAN")),
        "{:#?}",
        m.texts()
    );

    c.cmds.send(Command::Connect("1".into())).unwrap();
    c.wait_for("connect started", |s| {
        s.peers[0].connect == ConnectState::Started
    })
    .await;
    assert_eq!(fork.connects.lock().unwrap().as_slice(), ["1"]);

    // AC-21: connect to an unavailable peer reports why.
    c.cmds.send(Command::Connect("2".into())).unwrap();
    let s = c
        .wait_for("connect failed", |s| {
            matches!(s.peers[1].connect, ConnectState::Failed(_))
        })
        .await;
    assert_eq!(s.peers[1].connect, ConnectState::Failed("offline".into()));
    let m = menu_model::build(&s, client::unix_now());
    assert!(m
        .texts()
        .iter()
        .any(|t| t.contains("Old — unavailable: offline, last seen")));
    assert!(m
        .texts()
        .iter()
        .any(|t| t.contains("Last connect failed: offline")));
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inbound_session_badge_appears_and_clears_quickly() {
    let ep = endpoint();
    let fork = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up", reachable).await;

    let mut snap = snapshot();
    snap.status.inbound_sessions = vec![InboundSession {
        conn_id: 1,
        peer_id: "1".into(),
        alias: Some("PC".into()),
        kind: SessionKind::Agent,
        since: None,
    }];
    fork.tx.send(snap).unwrap();
    let s = c
        .wait_for("session", |s| {
            s.status
                .as_ref()
                .is_some_and(|st| !st.inbound_sessions.is_empty())
        })
        .await;
    let m = menu_model::build(&s, 0);
    assert_eq!(m.badge.as_deref(), Some("● PC · agent"));

    // AC-60: clears within 2 s of the last session ending.
    let t0 = Instant::now();
    fork.tx.send(snapshot()).unwrap();
    let s = c
        .wait_for("session cleared", |s| {
            s.status
                .as_ref()
                .is_some_and(|st| st.inbound_sessions.is_empty())
        })
        .await;
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "took {:?}",
        t0.elapsed()
    );
    assert!(menu_model::build(&s, 0).badge.is_none());
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn version_mismatch_shows_unknown_and_sends_nothing() {
    // Start with a good fork so peers are cached, then "upgrade" it to an incompatible version.
    let ep = endpoint();
    let good = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up", reachable).await;
    good.task.abort();
    c.wait_for("down", |s| !s.link.is_up()).await;
    let fork = start_fork(
        &ep.path,
        FakeConfig {
            min_version: 2,
            max_version: 3,
            ..Default::default()
        },
        snapshot(),
    )
    .await;
    let s = c
        .wait_for("mismatch", |s| matches!(s.link, Link::Mismatch { .. }))
        .await;
    assert_eq!(
        s.link,
        Link::Mismatch {
            server_min: 2,
            server_max: 3
        }
    );
    assert!(s.status.is_none());
    assert!(s.peers.iter().all(|p| p.presence == Presence::Unknown));
    let m = menu_model::build(&s, 0);
    assert!(
        m.texts()
            .iter()
            .any(|t| t.contains("unknown — version mismatch (app speaks 2–3, tray 1–1)")),
        "{:#?}",
        m.texts()
    );
    assert!(!m.texts().iter().any(|t| t.contains("Unattended ready")));
    // Connect fails locally with the reason; nothing reaches the mismatched fork.
    c.cmds.send(Command::Connect("1".into())).unwrap();
    let s = c
        .wait_for("connect refused locally", |s| {
            matches!(&s.peers[0].connect, ConnectState::Failed(r) if r.contains("version mismatch"))
        })
        .await;
    assert!(matches!(s.link, Link::Mismatch { .. }));
    tokio::time::sleep(Duration::from_millis(700)).await; // spans at least one re-handshake
    assert!(fork.connects.lock().unwrap().is_empty());
    assert!(
        matches!(c.state.borrow().link, Link::Mismatch { .. }),
        "stays unknown across retries"
    );
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_restart_reconnects() {
    let ep = endpoint();
    let fork = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up", reachable).await;

    // Kill the fork: peers go unknown but stay listed (FLOW-16).
    fork.task.abort();
    let s = c
        .wait_for("down", |s| matches!(s.link, Link::Down { .. }))
        .await;
    assert_eq!(s.peers.len(), 2);
    assert!(s.peers.iter().all(|p| p.presence == Presence::Unknown));
    // Connect while down: fails with a reason, immediately.
    c.cmds.send(Command::Connect("1".into())).unwrap();
    c.wait_for(
        "connect failed while down",
        |s| matches!(&s.peers[0].connect, ConnectState::Failed(r) if r.contains("not reachable")),
    )
    .await;

    // Restart on the same endpoint with a changed peer list.
    let mut snap = snapshot();
    snap.peers.truncate(1);
    snap.peers[0].presence = Presence::Reachable { route: Route::Id };
    let fork2 = start_fork(&ep.path, FakeConfig::default(), snap).await;
    let s = c
        .wait_for("back up", |s| reachable(s) && s.peers.len() == 1)
        .await;
    assert_eq!(
        s.peers[0].presence,
        Presence::Reachable { route: Route::Id }
    );
    c.stop().await;
    fork2.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn survives_garbage_and_oversized_lines() {
    let ep = endpoint();
    let fork = start_fork(
        &ep.path,
        FakeConfig {
            garbage: true,
            ..Default::default()
        },
        snapshot(),
    )
    .await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up despite garbage", reachable).await;
    c.cmds.send(Command::Connect("1".into())).unwrap();
    c.wait_for("connect despite garbage", |s| {
        s.peers[0].connect == ConnectState::Started
    })
    .await;
    // Stays up across several ping rounds of garbage.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(c.state.borrow().link.is_up());
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hung_fork_is_detected_as_stale() {
    let ep = endpoint();
    let fork = start_fork(
        &ep.path,
        FakeConfig {
            silent_after_subscribe: true,
            ..Default::default()
        },
        snapshot(),
    )
    .await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up", reachable).await;
    let s = c
        .wait_for("stale -> down", |s| matches!(s.link, Link::Down { .. }))
        .await;
    assert_eq!(
        s.link,
        Link::Down {
            reason: "RemoteSmith stopped responding".into()
        }
    );
    assert!(s.peers.iter().all(|p| p.presence == Presence::Unknown));
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_ignoring_subscribe_times_out() {
    let ep = endpoint();
    let fork = start_fork(
        &ep.path,
        FakeConfig {
            silent_after_hello: true,
            ..Default::default()
        },
        snapshot(),
    )
    .await;
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("up", |s| s.link.is_up()).await;
    let s = c
        .wait_for("down", |s| matches!(s.link, Link::Down { .. }))
        .await;
    assert_eq!(
        s.link,
        Link::Down {
            reason: "RemoteSmith did not accept subscribe".into()
        }
    );
    c.stop().await;
    fork.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_fork_is_unknown_not_running() {
    let ep = endpoint();
    let mut c = start_client(fast_cfg(&ep.path));
    let s = c
        .wait_for("down", |s| matches!(s.link, Link::Down { .. }))
        .await;
    assert_eq!(
        s.link,
        Link::Down {
            reason: "RemoteSmith app not running".into()
        }
    );
    let m = menu_model::build(&s, 0);
    assert!(m.texts().iter().any(|t| t == "Readiness: unknown"));
    c.stop().await;
}

// ---- the reference server's own robustness (what the fork must also do; AC-25) ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_rejects_bad_input_and_keeps_serving() {
    let ep = endpoint();
    let fork = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
    let stream = remotesmith_tray::transport::connect(&ep.path)
        .await
        .unwrap();
    let (rd, mut wr) = tokio::io::split(stream);
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    async fn next(
        rd: &mut BufReader<tokio::io::ReadHalf<Box<dyn remotesmith_tray::transport::IpcStream>>>,
        line: &mut String,
    ) -> Envelope<ServerMessage> {
        line.clear();
        tokio::time::timeout(Duration::from_secs(2), rd.read_line(line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(line.trim_end()).unwrap()
    }
    // Request before hello.
    wr.write_all(b"{\"v\":1,\"id\":5,\"type\":\"list_peers\"}\n")
        .await
        .unwrap();
    let e = next(&mut rd, &mut line).await;
    assert_eq!(e.id, Some(5));
    assert!(
        matches!(e.body, ServerMessage::Error { error } if error.code == ErrorCode::HelloRequired)
    );
    // Garbage.
    wr.write_all(b"\xff\xfe garbage\n").await.unwrap();
    assert!(
        matches!(next(&mut rd, &mut line).await.body, ServerMessage::Error { error } if error.code == ErrorCode::BadRequest)
    );
    // Oversized line.
    let mut big = vec![b'{'; MAX_LINE + 100];
    big.push(b'\n');
    wr.write_all(&big).await.unwrap();
    assert!(
        matches!(next(&mut rd, &mut line).await.body, ServerMessage::Error { error } if error.code == ErrorCode::LineTooLong)
    );
    // Then a proper hello still works on the same connection.
    wr.write_all(
        encode(
            1,
            Some(1),
            Request::Hello {
                min_version: 1,
                max_version: 1,
                client: "t".into(),
                client_version: "0".into(),
            },
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    assert!(matches!(
        next(&mut rd, &mut line).await.body,
        ServerMessage::HelloOk { version: 1, .. }
    ));
    // A second hello and a wrong protocol version are both rejected.
    wr.write_all(b"{\"v\":1,\"id\":2,\"type\":\"hello\",\"min_version\":1,\"max_version\":1,\"client\":\"t\",\"client_version\":\"0\"}\n")
        .await
        .unwrap();
    assert!(
        matches!(next(&mut rd, &mut line).await.body, ServerMessage::Error { error } if error.code == ErrorCode::BadRequest)
    );
    wr.write_all(b"{\"v\":2,\"id\":3,\"type\":\"ping\"}\n")
        .await
        .unwrap();
    let e = next(&mut rd, &mut line).await;
    assert_eq!(e.id, Some(3));
    assert!(
        matches!(e.body, ServerMessage::Error { error } if error.code == ErrorCode::BadRequest)
    );
    // Unknown request type.
    wr.write_all(b"{\"v\":1,\"id\":7,\"type\":\"reboot\"}\n")
        .await
        .unwrap();
    let e = next(&mut rd, &mut line).await;
    assert_eq!(e.id, Some(7));
    assert!(
        matches!(e.body, ServerMessage::Error { error } if error.code == ErrorCode::UnknownType)
    );
    // Unknown peer.
    wr.write_all(
        encode(
            1,
            Some(8),
            Request::Connect {
                peer_id: "nope".into(),
            },
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    assert!(
        matches!(next(&mut rd, &mut line).await.body, ServerMessage::Error { error } if error.code == ErrorCode::PeerUnknown)
    );
    // Half a line then hang up: server must survive and serve a new client.
    wr.write_all(b"{\"v\":1,\"id\":9,\"ty").await.unwrap();
    drop(wr);
    drop(rd);
    let mut c = start_client(fast_cfg(&ep.path));
    c.wait_for("second client up", reachable).await;
    c.stop().await;
    fork.task.abort();
}

#[cfg(unix)]
mod unix_raw {
    use super::*;
    use tokio::net::UnixListener;

    /// A hostile server: answers hello with `reply`, then streams `after` lines forever.
    async fn hostile(
        path: &str,
        reply: String,
        after: Option<String>,
    ) -> tokio::task::JoinHandle<()> {
        let l = UnixListener::bind(path).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((s, _)) = l.accept().await else { return };
                let (reply, after) = (reply.clone(), after.clone());
                tokio::spawn(async move {
                    let (rd, mut wr) = tokio::io::split(s);
                    let mut rd = BufReader::new(rd);
                    let mut line = String::new();
                    if rd.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let id: serde_json::Value = serde_json::from_str(&line).unwrap();
                    let reply = reply.replace("ID", &id["id"].to_string());
                    let _ = wr.write_all(reply.as_bytes()).await;
                    if let Some(a) = after {
                        loop {
                            if wr.write_all(a.as_bytes()).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }
                    let mut sink = Vec::new();
                    let _ = tokio::io::AsyncReadExt::read_to_end(&mut rd, &mut sink).await;
                });
            }
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn server_choosing_unsupported_version_is_mismatch() {
        let ep = endpoint();
        let srv = hostile(
            &ep.path,
            "{\"v\":7,\"id\":ID,\"type\":\"hello_ok\",\"version\":7,\"min_version\":7,\"max_version\":9}\n".into(),
            None,
        )
        .await;
        let mut c = start_client(fast_cfg(&ep.path));
        let s = c
            .wait_for("mismatch", |s| matches!(s.link, Link::Mismatch { .. }))
            .await;
        assert_eq!(
            s.link,
            Link::Mismatch {
                server_min: 7,
                server_max: 9
            }
        );
        c.stop().await;
        srv.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endless_garbage_drops_link_and_retries() {
        let ep = endpoint();
        let srv = hostile(
            &ep.path,
            "{\"v\":1,\"id\":ID,\"type\":\"hello_ok\",\"version\":1,\"min_version\":1,\"max_version\":1}\n".into(),
            Some("garbage garbage\n".into()),
        )
        .await;
        let mut c = start_client(fast_cfg(&ep.path));
        c.wait_for("up", |s| s.link.is_up()).await;
        let s = c
            .wait_for("dropped for garbage", |s| {
                matches!(s.link, Link::Down { .. })
            })
            .await;
        assert_eq!(
            s.link,
            Link::Down {
                reason: "too many malformed messages from RemoteSmith".into()
            }
        );
        // ...and it retries (comes back up on the next handshake).
        c.wait_for("retried", |s| s.link.is_up()).await;
        c.stop().await;
        srv.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn non_socket_endpoint_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("not-a-socket");
        std::fs::write(&p, b"x").unwrap();
        let e = remotesmith_tray::transport::connect(p.to_str().unwrap())
            .await
            .err()
            .unwrap();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn socket_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let ep = endpoint();
        // A stale socket left by a crashed fork (std's listener never unlinks) is replaced.
        drop(std::os::unix::net::UnixListener::bind(&ep.path).unwrap());
        assert!(std::fs::symlink_metadata(&ep.path).is_ok());
        let fork = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
        let mode = std::fs::metadata(&ep.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A second server refuses to steal a live endpoint.
        assert_eq!(
            bind(&ep.path).await.err().unwrap().kind(),
            std::io::ErrorKind::AddrInUse
        );
        fork.task.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_directory_is_refused_by_both_sides() {
        use std::os::unix::fs::PermissionsExt;
        let ep = endpoint();
        let dir = std::path::Path::new(&ep.path)
            .parent()
            .unwrap()
            .to_path_buf();
        let fork = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
        // Someone loosens the directory: the client must refuse to trust what is inside.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = remotesmith_tray::transport::connect(&ep.path)
            .await
            .err()
            .unwrap();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        fork.task.abort();
        assert_eq!(
            bind(&ep.path).await.err().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn symlinked_socket_is_refused() {
        let ep = endpoint();
        let real = start_fork(&ep.path, FakeConfig::default(), snapshot()).await;
        let link = std::path::Path::new(&ep.path).with_file_name("link.sock");
        std::os::unix::fs::symlink(&ep.path, &link).unwrap();
        let e = remotesmith_tray::transport::connect(link.to_str().unwrap())
            .await
            .err()
            .unwrap();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        real.task.abort();
    }
}
