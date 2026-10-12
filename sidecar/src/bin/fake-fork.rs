//! `fake-fork` — dev stand-in for the RemoteSmith fork's IPC endpoint (docs/IPC.md).
//!
//! Scenarios: demo (default, cycles states every few seconds), static, empty, mismatch, garbage.

use std::time::Duration;

use remotesmith_tray::client::unix_now;
use remotesmith_tray::fake_fork::{bind, serve, FakeConfig, Snapshot};
use remotesmith_tray::protocol::*;
use remotesmith_tray::transport;

fn demo_status(inbound: Vec<InboundSession>, cold_boot: bool) -> Status {
    Status {
        app_version: "1.5.0-rs.1 (fake)".into(),
        platform: if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Macos
        },
        my_id: Some("111 222 333".into()),
        key_fingerprint: Some("SHA256:3f:9a:c2:71:0b:de".into()),
        service: ServiceStatus {
            running: true,
            detail: None,
        },
        permissions: if cfg!(windows) {
            vec![]
        } else {
            vec![
                PermissionStatus {
                    name: Permission::Accessibility,
                    state: PermState::Granted,
                    detail: None,
                },
                PermissionStatus {
                    name: Permission::ScreenRecording,
                    state: PermState::Granted,
                    detail: None,
                },
                PermissionStatus {
                    name: Permission::InputMonitoring,
                    state: PermState::Missing,
                    detail: None,
                },
                PermissionStatus {
                    name: Permission::SystemAudioRecording,
                    state: PermState::Granted,
                    detail: None,
                },
            ]
        },
        power: PowerStatus {
            sleep_disabled: Some(true),
            detail: None,
        },
        preboot_unlock: PrebootStatus {
            detected: Some(false),
            detail: None,
        },
        server: ServerStatus {
            state: RegState::Registered,
            host: Some("rs.johnnycode.ai".into()),
            detail: None,
        },
        unattended: UnattendedStatus {
            ready: false,
            cold_boot_verified_at: cold_boot.then(unix_now),
        },
        inbound_sessions: inbound,
    }
}

fn demo_peers(step: u64) -> Vec<Peer> {
    let now = unix_now();
    let pc = match step % 3 {
        0 => Presence::Reachable { route: Route::Lan },
        1 => Presence::Reachable { route: Route::Id },
        _ => Presence::Checking,
    };
    vec![
        Peer {
            id: "987654321".into(),
            alias: Some("Scott-PC".into()),
            platform: Some(Platform::Windows),
            presence: pc,
        },
        Peer {
            id: "555000111".into(),
            alias: Some("Office-PC".into()),
            platform: Some(Platform::Windows),
            presence: Presence::Unavailable {
                reason: Reason::Offline,
                detail: None,
                last_seen: Some(now - 7 * 60),
            },
        },
    ]
}

fn session(kind: SessionKind) -> InboundSession {
    InboundSession {
        conn_id: 1,
        peer_id: "987654321".into(),
        alias: Some("Scott-PC".into()),
        kind,
        since: Some(unix_now()),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut scenario = "demo".to_string();
    let mut endpoint = transport::default_endpoint();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--scenario" => scenario = args.next().unwrap_or_default(),
            "--endpoint" => endpoint = args.next().unwrap_or_default(),
            "-h" | "--help" => {
                println!(
                    "fake-fork [--scenario demo|static|empty|mismatch|garbage] [--endpoint <path>]"
                );
                return;
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let mut cfg = FakeConfig {
        connect_result_after: Some(Duration::from_secs(1)),
        ..Default::default()
    };
    match scenario.as_str() {
        "mismatch" => {
            cfg.min_version = 2;
            cfg.max_version = 3;
        }
        "garbage" => cfg.garbage = true,
        "demo" | "static" | "empty" => {}
        s => {
            eprintln!("unknown scenario {s}");
            std::process::exit(2);
        }
    }
    let initial = if scenario == "empty" {
        Snapshot {
            status: demo_status(vec![], false),
            peers: vec![],
        }
    } else {
        Snapshot {
            status: demo_status(vec![session(SessionKind::Human)], false),
            peers: demo_peers(0),
        }
    };
    let (tx, rx) = tokio::sync::watch::channel(initial);
    let listener = match bind(&endpoint).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("fake-fork: cannot bind {endpoint}: {e}");
            std::process::exit(1);
        }
    };
    println!("fake-fork: scenario {scenario}, listening on {endpoint}");
    let server = tokio::spawn(serve(listener, cfg, rx, Default::default()));
    if scenario == "demo" {
        tokio::spawn(async move {
            let mut step = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(4)).await;
                step += 1;
                let inbound = match step % 4 {
                    0 => vec![session(SessionKind::Human)],
                    1 => vec![session(SessionKind::Agent)],
                    _ => vec![],
                };
                let _ = tx.send(Snapshot {
                    status: demo_status(inbound, step % 5 == 4),
                    peers: demo_peers(step),
                });
            }
        });
    }
    tokio::select! {
        r = server => if let Ok(Err(e)) = r { eprintln!("fake-fork: {e}") },
        _ = tokio::signal::ctrl_c() => {}
    }
}
