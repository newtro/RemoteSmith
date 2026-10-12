//! The sidecar's view of the world and the presence state machine (DEF-17, §6.2).
//!
//! Pure: every transition takes the current time explicitly so tests are deterministic.

use crate::protocol::{Peer, Presence, Reason, Status};

/// State of the IPC link to the fork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Not yet attempted, or attempting.
    Connecting,
    /// Handshake done.
    Up {
        protocol: u32,
        server_version: String,
    },
    /// Fork unreachable / link died. `reason` is shown to the user.
    Down { reason: String },
    /// Fork reachable but no common protocol version (DEF-4).
    Mismatch { server_min: u32, server_max: u32 },
}

impl Link {
    pub fn is_up(&self) -> bool {
        matches!(self, Link::Up { .. })
    }

    /// Why presence is unknown, for display; `None` when the link is up.
    pub fn unknown_reason(&self) -> Option<String> {
        match self {
            Link::Up { .. } => None,
            Link::Connecting => Some("connecting to RemoteSmith".into()),
            Link::Down { reason } => Some(reason.clone()),
            Link::Mismatch {
                server_min,
                server_max,
            } => Some(format!(
                "version mismatch (app speaks {server_min}–{server_max}, tray {}–{})",
                crate::protocol::MIN_VERSION,
                crate::protocol::MAX_VERSION
            )),
        }
    }
}

/// Outcome of the last connect attempt made from the tray for a peer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectState {
    #[default]
    Idle,
    Pending,
    Started,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerView {
    pub id: String,
    pub alias: Option<String>,
    /// Presence as the tray shows it (Unknown when the link is not up).
    pub presence: Presence,
    /// Best known last-seen time (Unix s): from the fork, or when we last saw it reachable.
    pub last_seen: Option<u64>,
    pub connect: ConnectState,
}

impl PeerView {
    pub fn name(&self) -> String {
        match &self.alias {
            Some(a) if !a.is_empty() => a.clone(),
            _ => self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub link: Link,
    /// Latest fork status; `None` unless the link is up and a status has arrived.
    pub status: Option<Status>,
    /// Peers, cached across link drops so the menu keeps listing them (as unknown).
    pub peers: Vec<PeerView>,
    /// True once a peer list has been received on the current link.
    pub peers_fresh: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            link: Link::Connecting,
            status: None,
            peers: Vec::new(),
            peers_fresh: false,
        }
    }
}

/// Inputs to the state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum StateEvent {
    Connecting,
    LinkUp {
        protocol: u32,
        server_version: String,
    },
    LinkDown {
        reason: String,
    },
    VersionMismatch {
        server_min: u32,
        server_max: u32,
    },
    Status(Box<Status>),
    Peers(Vec<Peer>),
    /// The user clicked Connect; the client is about to send (or cannot send) the request.
    ConnectRequested {
        peer_id: String,
    },
    ConnectStarted {
        peer_id: String,
    },
    ConnectFailed {
        peer_id: String,
        reason: String,
    },
}

impl AppState {
    pub fn apply(&mut self, ev: StateEvent, now: u64) {
        match ev {
            StateEvent::Connecting => self.go_unknown(Link::Connecting),
            StateEvent::LinkDown { reason } => self.go_unknown(Link::Down { reason }),
            StateEvent::VersionMismatch {
                server_min,
                server_max,
            } => self.go_unknown(Link::Mismatch {
                server_min,
                server_max,
            }),
            StateEvent::LinkUp {
                protocol,
                server_version,
            } => {
                self.link = Link::Up {
                    protocol,
                    server_version,
                };
                self.peers_fresh = false;
                for p in &mut self.peers {
                    p.presence = Presence::Checking;
                }
            }
            StateEvent::Status(s) => {
                if self.link.is_up() {
                    self.status = Some(*s);
                }
            }
            StateEvent::Peers(list) => {
                if !self.link.is_up() {
                    return;
                }
                let old = std::mem::take(&mut self.peers);
                self.peers = list
                    .into_iter()
                    .map(|p| {
                        let prev = old.iter().find(|o| o.id == p.id);
                        let mut last_seen = prev.and_then(|o| o.last_seen);
                        match &p.presence {
                            Presence::Reachable { .. } => last_seen = Some(now),
                            Presence::Unavailable {
                                last_seen: Some(ls),
                                ..
                            } => last_seen = Some(last_seen.map_or(*ls, |l| l.max(*ls))),
                            _ => {}
                        }
                        PeerView {
                            id: p.id,
                            alias: p.alias,
                            presence: p.presence,
                            last_seen,
                            connect: prev.map(|o| o.connect.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                self.peers_fresh = true;
            }
            StateEvent::ConnectRequested { peer_id } => {
                let reason = self.link.unknown_reason();
                if let Some(p) = self.peer_mut(&peer_id) {
                    p.connect = match reason {
                        None => ConnectState::Pending,
                        Some(r) => ConnectState::Failed(format!("RemoteSmith not reachable: {r}")),
                    };
                }
            }
            StateEvent::ConnectStarted { peer_id } => {
                if let Some(p) = self.peer_mut(&peer_id) {
                    p.connect = ConnectState::Started;
                }
            }
            StateEvent::ConnectFailed { peer_id, reason } => {
                if let Some(p) = self.peer_mut(&peer_id) {
                    p.connect = ConnectState::Failed(reason);
                }
            }
        }
    }

    fn peer_mut(&mut self, id: &str) -> Option<&mut PeerView> {
        self.peers.iter_mut().find(|p| p.id == id)
    }

    fn go_unknown(&mut self, link: Link) {
        self.link = link;
        self.status = None;
        self.peers_fresh = false;
        for p in &mut self.peers {
            p.presence = Presence::Unknown;
            if p.connect == ConnectState::Pending {
                p.connect = ConnectState::Failed("lost connection to RemoteSmith".into());
            }
        }
    }

    /// True when the tray should show the onboarding item instead of peers.
    pub fn needs_onboarding(&self) -> bool {
        self.link.is_up() && self.peers_fresh && self.peers.is_empty()
    }
}

/// Human text for a connect failure reported by the fork.
pub fn connect_failure_text(reason: Option<&Reason>, detail: Option<&str>) -> String {
    let base = reason.map(|r| r.label()).unwrap_or_else(|| "failed".into());
    match detail {
        Some(d) if !d.is_empty() => format!("{base} ({d})"),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Route;

    fn peer(id: &str, presence: Presence) -> Peer {
        Peer {
            id: id.into(),
            alias: Some(format!("alias-{id}")),
            platform: None,
            presence,
        }
    }

    fn up(s: &mut AppState) {
        s.apply(
            StateEvent::LinkUp {
                protocol: 1,
                server_version: "x".into(),
            },
            0,
        );
    }

    #[test]
    fn starts_unknown_and_connecting() {
        let s = AppState::default();
        assert_eq!(s.link, Link::Connecting);
        assert!(s.link.unknown_reason().is_some());
        assert!(!s.needs_onboarding());
    }

    #[test]
    fn link_up_then_peers_then_down() {
        let mut s = AppState::default();
        up(&mut s);
        assert!(!s.needs_onboarding(), "no onboarding before the first list");
        s.apply(
            StateEvent::Peers(vec![peer("1", Presence::Reachable { route: Route::Lan })]),
            100,
        );
        assert_eq!(
            s.peers[0].presence,
            Presence::Reachable { route: Route::Lan }
        );
        assert_eq!(s.peers[0].last_seen, Some(100));
        s.apply(
            StateEvent::LinkDown {
                reason: "app not running".into(),
            },
            200,
        );
        assert_eq!(s.peers.len(), 1, "peers cached across link drop");
        assert_eq!(s.peers[0].presence, Presence::Unknown);
        assert_eq!(s.peers[0].last_seen, Some(100));
        assert_eq!(s.status, None);
        // Events arriving while down are ignored.
        s.apply(StateEvent::Peers(vec![]), 210);
        assert_eq!(s.peers.len(), 1);
        // Back up: checking until a fresh list arrives.
        up(&mut s);
        assert_eq!(s.peers[0].presence, Presence::Checking);
    }

    #[test]
    fn unavailable_keeps_latest_last_seen() {
        let mut s = AppState::default();
        up(&mut s);
        s.apply(
            StateEvent::Peers(vec![peer("1", Presence::Reachable { route: Route::Id })]),
            500,
        );
        s.apply(
            StateEvent::Peers(vec![peer(
                "1",
                Presence::Unavailable {
                    reason: Reason::Offline,
                    detail: None,
                    last_seen: Some(400),
                },
            )]),
            600,
        );
        assert_eq!(s.peers[0].last_seen, Some(500));
        s.apply(
            StateEvent::Peers(vec![peer(
                "1",
                Presence::Unavailable {
                    reason: Reason::Offline,
                    detail: None,
                    last_seen: Some(550),
                },
            )]),
            700,
        );
        assert_eq!(s.peers[0].last_seen, Some(550));
    }

    #[test]
    fn mismatch_is_unknown_with_reason() {
        let mut s = AppState::default();
        up(&mut s);
        s.apply(StateEvent::Peers(vec![peer("1", Presence::Checking)]), 0);
        s.apply(
            StateEvent::VersionMismatch {
                server_min: 2,
                server_max: 3,
            },
            0,
        );
        assert_eq!(s.peers[0].presence, Presence::Unknown);
        let r = s.link.unknown_reason().unwrap();
        assert!(r.contains("version mismatch") && r.contains("2–3"), "{r}");
        s.apply(StateEvent::Status(Box::default()), 0);
        assert_eq!(s.status, None, "status ignored while mismatched");
    }

    #[test]
    fn connect_lifecycle() {
        let mut s = AppState::default();
        up(&mut s);
        s.apply(
            StateEvent::Peers(vec![peer("1", Presence::Reachable { route: Route::Lan })]),
            0,
        );
        s.apply(
            StateEvent::ConnectRequested {
                peer_id: "1".into(),
            },
            0,
        );
        assert_eq!(s.peers[0].connect, ConnectState::Pending);
        s.apply(
            StateEvent::ConnectStarted {
                peer_id: "1".into(),
            },
            0,
        );
        assert_eq!(s.peers[0].connect, ConnectState::Started);
        s.apply(
            StateEvent::ConnectFailed {
                peer_id: "1".into(),
                reason: "auth failed".into(),
            },
            0,
        );
        assert_eq!(
            s.peers[0].connect,
            ConnectState::Failed("auth failed".into())
        );
        // Connect state survives a peers refresh.
        s.apply(StateEvent::Peers(vec![peer("1", Presence::Checking)]), 0);
        assert_eq!(
            s.peers[0].connect,
            ConnectState::Failed("auth failed".into())
        );
        // Pending connect fails when the link drops.
        s.apply(
            StateEvent::ConnectRequested {
                peer_id: "1".into(),
            },
            0,
        );
        s.apply(StateEvent::LinkDown { reason: "x".into() }, 0);
        assert!(
            matches!(s.peers[0].connect, ConnectState::Failed(ref r) if r.contains("lost connection"))
        );
        // Connect while down fails immediately with a reason (AC-21: Connect reports why).
        s.apply(
            StateEvent::ConnectRequested {
                peer_id: "1".into(),
            },
            0,
        );
        assert!(
            matches!(s.peers[0].connect, ConnectState::Failed(ref r) if r.contains("not reachable"))
        );
    }

    #[test]
    fn empty_list_triggers_onboarding() {
        let mut s = AppState::default();
        up(&mut s);
        s.apply(StateEvent::Peers(vec![]), 0);
        assert!(s.needs_onboarding());
        s.apply(StateEvent::LinkDown { reason: "x".into() }, 0);
        assert!(!s.needs_onboarding());
    }

    #[test]
    fn failure_text() {
        assert_eq!(
            connect_failure_text(Some(&Reason::AuthFailed), None),
            "auth failed"
        );
        assert_eq!(connect_failure_text(None, Some("boom")), "failed (boom)");
    }
}
