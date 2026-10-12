//! Pure menu model built from [`AppState`]; the OS tray only renders it (FLOW-13, §6.2, §6.2a).

use crate::protocol::{Presence, SessionKind};
use crate::readiness::{self, Outcome};
use crate::state::{AppState, ConnectState, PeerView};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Connect(String),
    OpenApp,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Informational, disabled row.
    Label(String),
    Action {
        text: String,
        action: Action,
        enabled: bool,
    },
    Submenu {
        text: String,
        items: Vec<Item>,
    },
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub tooltip: String,
    /// Short text next to the menu-bar icon (macOS) while inbound sessions exist (AC-60).
    pub badge: Option<String>,
    /// Icon variant: true while someone is controlling this machine.
    pub active: bool,
    pub items: Vec<Item>,
}

impl MenuModel {
    /// Every row's text, depth-first (for tests and logs).
    pub fn texts(&self) -> Vec<String> {
        fn walk(items: &[Item], out: &mut Vec<String>) {
            for i in items {
                match i {
                    Item::Label(t) => out.push(t.clone()),
                    Item::Action { text, .. } => out.push(text.clone()),
                    Item::Submenu { text, items } => {
                        out.push(text.clone());
                        walk(items, out);
                    }
                    Item::Separator => out.push("---".into()),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut out);
        out
    }
}

/// "3 min ago" style relative time.
pub fn ago(then: u64, now: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86_400),
    }
}

fn presence_text(p: &PeerView, unknown_reason: Option<&str>, now: u64) -> String {
    let seen = p.last_seen.map(|t| format!(", last seen {}", ago(t, now)));
    match &p.presence {
        Presence::Unknown => match unknown_reason {
            Some(r) => format!("unknown ({r})"),
            None => "unknown".into(),
        },
        Presence::Checking => "checking…".into(),
        Presence::Reachable { route } => format!("reachable via {}", route.label()),
        Presence::Unavailable {
            reason,
            detail,
            last_seen,
        } => {
            let mut s = format!("unavailable: {}", reason.label());
            if let Some(d) = detail.as_deref().filter(|d| !d.is_empty()) {
                s.push_str(&format!(" ({d})"));
            }
            match last_seen {
                Some(t) => s.push_str(&format!(", last seen {}", ago(*t, now))),
                None => {
                    if let Some(seen) = seen {
                        s.push_str(&seen)
                    }
                }
            }
            s
        }
    }
}

fn dot(p: &Presence) -> &'static str {
    match p {
        Presence::Reachable { .. } => "●",
        Presence::Unavailable { .. } => "○",
        _ => "◌",
    }
}

pub fn build(state: &AppState, now: u64) -> MenuModel {
    let unknown = state.link.unknown_reason();
    let mut items = Vec::new();

    // Host-side session badge (§6.2a, AC-60).
    let sessions = state
        .status
        .as_ref()
        .map(|s| s.inbound_sessions.as_slice())
        .unwrap_or_default();
    let mut badge = None;
    if !sessions.is_empty() {
        let name = |s: &crate::protocol::InboundSession| {
            s.alias
                .clone()
                .filter(|a| !a.is_empty())
                .or_else(|| Some(s.peer_id.clone()).filter(|p| !p.is_empty()))
                .unwrap_or_else(|| "unknown peer".into())
        };
        let any_agent = sessions.iter().any(|s| s.kind == SessionKind::Agent);
        for s in sessions {
            let agent = if s.kind == SessionKind::Agent {
                " — agent in control"
            } else {
                ""
            };
            let id = if s.peer_id.is_empty() {
                String::new()
            } else {
                format!(" ({})", s.peer_id)
            };
            items.push(Item::Label(format!("Connected: {}{id}{agent}", name(s))));
        }
        let mut b = format!("● {}", name(&sessions[0]));
        if sessions.len() > 1 {
            b.push_str(&format!(" +{}", sessions.len() - 1));
        }
        if any_agent {
            b.push_str(" · agent");
        }
        badge = Some(b);
        items.push(Item::Separator);
    }

    if let Some(r) = &unknown {
        items.push(Item::Label(format!("RemoteSmith: unknown — {r}")));
    }

    // Peers (FLOW-13) or onboarding (§6.2).
    if state.needs_onboarding() {
        items.push(Item::Label("No peers yet".into()));
        items.push(Item::Action {
            text: "Add a peer in RemoteSmith…".into(),
            action: Action::OpenApp,
            enabled: true,
        });
    } else if state.peers.is_empty() {
        items.push(Item::Label(
            if unknown.is_some() {
                "Peers: unknown"
            } else {
                "Peers: checking…"
            }
            .into(),
        ));
    } else {
        for p in &state.peers {
            items.push(Item::Label(format!(
                "{} {} — {}",
                dot(&p.presence),
                p.name(),
                presence_text(p, unknown.as_deref(), now)
            )));
            let (text, enabled) = match &p.connect {
                ConnectState::Pending => (format!("Connecting to {}…", p.name()), false),
                _ => (format!("Connect to {}", p.name()), true),
            };
            items.push(Item::Action {
                text,
                action: Action::Connect(p.id.clone()),
                enabled,
            });
            if let ConnectState::Failed(r) = &p.connect {
                items.push(Item::Label(format!("    Last connect failed: {r}")));
            }
        }
    }
    items.push(Item::Separator);

    // Readiness (§6.3, AC-55).
    let r = readiness::evaluate(state);
    let failing = r.failing();
    let (title, sub) = match (&r.checks, &r.unknown_reason) {
        (None, reason) => (
            "Readiness: unknown".to_string(),
            vec![Item::Label(
                reason.clone().unwrap_or_else(|| "unknown".into()),
            )],
        ),
        (Some(checks), _) => {
            let title = if r.unattended_ready {
                "Readiness: Unattended ready".to_string()
            } else {
                if failing.is_empty() {
                    "Readiness: awaiting confirmation".to_string()
                } else {
                    format!("Readiness: {} to fix", failing.len())
                }
            };
            let mut sub: Vec<Item> = failing
                .iter()
                .map(|c| {
                    let (mark, why) = match &c.outcome {
                        Outcome::Fail(w) => ("✗", w.as_str()),
                        Outcome::Unknown(w) => ("?", w.as_str()),
                        Outcome::Pass => ("✓", ""),
                    };
                    Item::Label(format!("{mark} {} — {why}", c.label))
                })
                .collect();
            if !failing.is_empty() {
                sub.push(Item::Separator);
            }
            let passing = checks.iter().filter(|c| c.outcome == Outcome::Pass).count();
            sub.push(Item::Label(format!(
                "{passing} of {} checks pass",
                checks.len()
            )));
            if !r.unattended_ready && failing.is_empty() {
                sub.push(Item::Label(
                    "Waiting for RemoteSmith to confirm unattended readiness".into(),
                ));
            }
            (title, sub)
        }
    };
    items.push(Item::Submenu {
        text: title,
        items: sub,
    });

    // This machine's identity, for TOFU checks from the other side (PLAN §7).
    if let Some(st) = &state.status {
        let mut me = Vec::new();
        if let Some(id) = &st.my_id {
            me.push(Item::Label(format!("ID: {id}")));
        }
        if let Some(fp) = &st.key_fingerprint {
            me.push(Item::Label(format!("Key: {fp}")));
        }
        if !me.is_empty() {
            items.push(Item::Submenu {
                text: "This machine".into(),
                items: me,
            });
        }
    }

    items.push(Item::Separator);
    items.push(Item::Action {
        text: "Open RemoteSmith".into(),
        action: Action::OpenApp,
        enabled: true,
    });
    items.push(Item::Action {
        text: "Quit RemoteSmith Tray".into(),
        action: Action::Quit,
        enabled: true,
    });

    let tooltip = match (&badge, &unknown) {
        (Some(b), _) => format!("RemoteSmith — {}", b.trim_start_matches("● ")),
        (None, Some(r)) => format!("RemoteSmith — unknown ({r})"),
        (None, None) => {
            let reachable = state
                .peers
                .iter()
                .filter(|p| matches!(p.presence, Presence::Reachable { .. }))
                .count();
            format!(
                "RemoteSmith — {reachable} of {} peers reachable",
                state.peers.len()
            )
        }
    };
    MenuModel {
        tooltip,
        active: badge.is_some(),
        badge,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::*;
    use crate::readiness::passing_status;
    use crate::state::StateEvent;

    fn up_state() -> AppState {
        let mut s = AppState::default();
        s.apply(
            StateEvent::LinkUp {
                protocol: 1,
                server_version: "x".into(),
            },
            0,
        );
        s.apply(StateEvent::Status(Box::new(passing_status())), 0);
        s
    }

    fn peer(id: &str, alias: &str, presence: Presence) -> Peer {
        Peer {
            id: id.into(),
            alias: Some(alias.into()),
            platform: None,
            presence,
        }
    }

    fn has(m: &MenuModel, needle: &str) -> bool {
        m.texts().iter().any(|t| t.contains(needle))
    }

    #[test]
    fn reachable_peer_shows_route_and_connect() {
        // AC-20
        let mut s = up_state();
        s.apply(
            StateEvent::Peers(vec![peer(
                "9",
                "Scott-PC",
                Presence::Reachable { route: Route::Lan },
            )]),
            0,
        );
        let m = build(&s, 0);
        assert!(
            has(&m, "● Scott-PC — reachable via LAN"),
            "{:#?}",
            m.texts()
        );
        assert!(m.items.contains(&Item::Action {
            text: "Connect to Scott-PC".into(),
            action: Action::Connect("9".into()),
            enabled: true
        }));
        assert!(has(&m, "Readiness: Unattended ready"));
        assert!(m.badge.is_none() && !m.active);
        assert_eq!(m.tooltip, "RemoteSmith — 1 of 1 peers reachable");
    }

    #[test]
    fn unavailable_peer_shows_reason_and_last_seen() {
        // AC-21
        let mut s = up_state();
        s.apply(
            StateEvent::Peers(vec![peer(
                "9",
                "Scott-PC",
                Presence::Unavailable {
                    reason: Reason::Offline,
                    detail: None,
                    last_seen: Some(1000),
                },
            )]),
            1300,
        );
        let m = build(&s, 1300);
        assert!(
            has(&m, "○ Scott-PC — unavailable: offline, last seen 5 min ago"),
            "{:#?}",
            m.texts()
        );
        // Connect stays available.
        assert!(m.items.iter().any(|i| matches!(
            i,
            Item::Action {
                action: Action::Connect(_),
                enabled: true,
                ..
            }
        )));
        s.apply(
            StateEvent::ConnectFailed {
                peer_id: "9".into(),
                reason: "offline".into(),
            },
            0,
        );
        assert!(has(&build(&s, 1300), "Last connect failed: offline"));
    }

    #[test]
    fn id_route_and_checking() {
        let mut s = up_state();
        s.apply(
            StateEvent::Peers(vec![
                peer("1", "A", Presence::Reachable { route: Route::Id }),
                peer("2", "B", Presence::Checking),
            ]),
            0,
        );
        let m = build(&s, 0);
        assert!(has(&m, "A — reachable via ID"));
        assert!(has(&m, "◌ B — checking…"));
    }

    #[test]
    fn unknown_when_link_down_or_mismatch() {
        let mut s = up_state();
        s.apply(
            StateEvent::Peers(vec![peer(
                "1",
                "A",
                Presence::Reachable { route: Route::Lan },
            )]),
            0,
        );
        s.apply(
            StateEvent::VersionMismatch {
                server_min: 2,
                server_max: 2,
            },
            0,
        );
        let m = build(&s, 0);
        assert!(
            has(&m, "RemoteSmith: unknown — version mismatch"),
            "{:#?}",
            m.texts()
        );
        assert!(has(&m, "◌ A — unknown (version mismatch"));
        assert!(has(&m, "Readiness: unknown"));
        assert!(!has(&m, "Unattended ready"));
        assert!(m.tooltip.contains("unknown"));
    }

    #[test]
    fn empty_peer_list_shows_onboarding() {
        let mut s = up_state();
        s.apply(StateEvent::Peers(vec![]), 0);
        let m = build(&s, 0);
        assert!(has(&m, "No peers yet"));
        assert!(m.items.iter().any(|i| matches!(i, Item::Action { action: Action::OpenApp, text, .. } if text.starts_with("Add a peer"))));
        assert!(!m.items.iter().any(|i| matches!(
            i,
            Item::Action {
                action: Action::Connect(_),
                ..
            }
        )));
    }

    #[test]
    fn inbound_session_badge_and_agent_marker() {
        // AC-60 / §6.2a
        let mut s = up_state();
        let mut st = passing_status();
        st.inbound_sessions = vec![InboundSession {
            conn_id: 1,
            peer_id: "9".into(),
            alias: Some("Scott-PC".into()),
            kind: SessionKind::Human,
            since: None,
        }];
        s.apply(StateEvent::Status(Box::new(st.clone())), 0);
        let m = build(&s, 0);
        assert_eq!(m.badge.as_deref(), Some("● Scott-PC"));
        assert!(m.active);
        assert!(has(&m, "Connected: Scott-PC (9)"));
        assert!(!has(&m, "agent"));

        st.inbound_sessions[0].kind = SessionKind::Agent;
        st.inbound_sessions.push(InboundSession {
            conn_id: 2,
            peer_id: "8".into(),
            alias: None,
            kind: SessionKind::Human,
            since: None,
        });
        s.apply(StateEvent::Status(Box::new(st.clone())), 0);
        let m = build(&s, 0);
        assert_eq!(m.badge.as_deref(), Some("● Scott-PC +1 · agent"));
        assert!(has(&m, "Connected: Scott-PC (9) — agent in control"));
        assert!(has(&m, "Connected: 8 (8)"));

        // Clears when the last session ends.
        st.inbound_sessions.clear();
        s.apply(StateEvent::Status(Box::new(st)), 0);
        let m = build(&s, 0);
        assert!(m.badge.is_none() && !m.active);
        assert!(!has(&m, "Connected:"));
    }

    #[test]
    fn readiness_submenu_lists_each_failure() {
        let mut s = up_state();
        let mut st = passing_status();
        st.permissions[0].state = PermState::Missing;
        st.power.sleep_disabled = Some(false);
        s.apply(StateEvent::Status(Box::new(st)), 0);
        let m = build(&s, 0);
        let sub = m
            .items
            .iter()
            .find_map(|i| match i {
                Item::Submenu { text, items } if text.starts_with("Readiness") => {
                    Some((text.clone(), items.clone()))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(sub.0, "Readiness: 2 to fix");
        let texts: Vec<_> = sub
            .1
            .iter()
            .filter_map(|i| {
                if let Item::Label(t) = i {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            texts.contains(&"✗ Accessibility permission — missing".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"✗ Never sleeps — sleep is enabled".to_string()));
        assert!(!has(&m, "Unattended ready"));
    }

    #[test]
    fn identity_and_fixed_items() {
        let s = up_state();
        let m = build(&s, 0);
        assert!(has(&m, "ID: 111222333"));
        assert!(has(&m, "Key: SHA256:ab:cd"));
        let n = m.items.len();
        assert_eq!(
            m.items[n - 2],
            Item::Action {
                text: "Open RemoteSmith".into(),
                action: Action::OpenApp,
                enabled: true
            }
        );
        assert_eq!(
            m.items[n - 1],
            Item::Action {
                text: "Quit RemoteSmith Tray".into(),
                action: Action::Quit,
                enabled: true
            }
        );
    }

    #[test]
    fn pending_connect_disables_item() {
        let mut s = up_state();
        s.apply(
            StateEvent::Peers(vec![peer(
                "1",
                "A",
                Presence::Reachable { route: Route::Lan },
            )]),
            0,
        );
        s.apply(
            StateEvent::ConnectRequested {
                peer_id: "1".into(),
            },
            0,
        );
        let m = build(&s, 0);
        assert!(m.items.contains(&Item::Action {
            text: "Connecting to A…".into(),
            action: Action::Connect("1".into()),
            enabled: false
        }));
    }

    #[test]
    fn ago_formats() {
        assert_eq!(ago(100, 100), "just now");
        assert_eq!(ago(0, 7200), "2 h ago");
        assert_eq!(ago(0, 3 * 86_400), "3 d ago");
        assert_eq!(ago(500, 100), "just now");
    }
}
