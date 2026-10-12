//! Readiness check derived from the fork's status (§6.3, AC-55).

use crate::protocol::{PermState, Permission, PermissionStatus, Platform, RegState, Status};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(String),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub label: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    /// `None` when the fork is unreachable: nothing can be said.
    pub checks: Option<Vec<Check>>,
    pub unknown_reason: Option<String>,
    /// "Unattended ready": only when the fork's flag is set AND every check passes.
    pub unattended_ready: bool,
}

impl Readiness {
    pub fn failing(&self) -> Vec<&Check> {
        self.checks
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|c| c.outcome != Outcome::Pass)
            .collect()
    }
}

fn check(label: &str, outcome: Outcome) -> Check {
    Check {
        label: label.into(),
        outcome,
    }
}

fn with_detail(base: &str, detail: &Option<String>) -> String {
    match detail {
        Some(d) if !d.is_empty() => format!("{base}: {d}"),
        _ => base.to_string(),
    }
}

/// macOS permissions RemoteSmith needs (FLOW-1).
pub const REQUIRED_MACOS_PERMISSIONS: [Permission; 4] = [
    Permission::Accessibility,
    Permission::ScreenRecording,
    Permission::InputMonitoring,
    Permission::SystemAudioRecording,
];

pub fn checks_for(status: &Status) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(check(
        "Service running",
        if status.service.running {
            Outcome::Pass
        } else {
            Outcome::Fail(with_detail("service not running", &status.service.detail))
        },
    ));
    // The sidecar re-checks rather than trusting the fork: on macOS every required permission
    // must be reported granted; one the fork leaves out counts as unknown (AC-55).
    let mut perms: Vec<PermissionStatus> = status.permissions.clone();
    match status.platform {
        Platform::Macos => {
            for req in REQUIRED_MACOS_PERMISSIONS {
                if !perms.iter().any(|p| p.name == req) {
                    perms.push(PermissionStatus {
                        name: req,
                        state: PermState::Unknown,
                        detail: Some("not reported by RemoteSmith".into()),
                    });
                }
            }
        }
        Platform::Windows => {}
        Platform::Other => out.push(check(
            "Platform",
            Outcome::Unknown("RemoteSmith did not report its platform".into()),
        )),
    }
    for p in &perms {
        let label = format!("{} permission", p.name.display_name());
        let outcome = match p.state {
            PermState::Granted => Outcome::Pass,
            PermState::Missing => Outcome::Fail(with_detail("missing", &p.detail)),
            PermState::Unknown => Outcome::Unknown(with_detail("not determined", &p.detail)),
        };
        out.push(Check { label, outcome });
    }
    out.push(check(
        "Never sleeps",
        match status.power.sleep_disabled {
            Some(true) => Outcome::Pass,
            Some(false) => Outcome::Fail(with_detail("sleep is enabled", &status.power.detail)),
            None => Outcome::Unknown(with_detail("power setting unknown", &status.power.detail)),
        },
    ));
    out.push(check(
        "No pre-boot unlock",
        match status.preboot_unlock.detected {
            Some(false) => Outcome::Pass,
            Some(true) => Outcome::Fail(with_detail(
                "pre-boot unlock detected",
                &status.preboot_unlock.detail,
            )),
            None => Outcome::Unknown(with_detail(
                "pre-boot unlock unknown",
                &status.preboot_unlock.detail,
            )),
        },
    ));
    let host = status.server.host.as_deref().unwrap_or("server");
    out.push(check(
        "Server registration",
        match status.server.state {
            RegState::Registered => Outcome::Pass,
            RegState::Registering => Outcome::Unknown(format!("registering with {host}")),
            RegState::NotConfigured => Outcome::Fail("no server configured".into()),
            RegState::Failed => Outcome::Fail(with_detail(
                &format!("not registered with {host}"),
                &status.server.detail,
            )),
            RegState::Unknown => Outcome::Unknown("registration state unknown".into()),
        },
    ));
    out.push(check(
        "Cold-boot connection verified",
        if status.unattended.cold_boot_verified_at.is_some() {
            Outcome::Pass
        } else {
            Outcome::Fail("no cold-boot-to-login-screen connection recorded yet".into())
        },
    ));
    out
}

pub fn evaluate(state: &AppState) -> Readiness {
    match (&state.status, state.link.unknown_reason()) {
        (Some(status), None) => {
            let checks = checks_for(status);
            let all_pass = checks.iter().all(|c| c.outcome == Outcome::Pass);
            Readiness {
                unattended_ready: status.unattended.ready && all_pass,
                checks: Some(checks),
                unknown_reason: None,
            }
        }
        (_, reason) => Readiness {
            checks: None,
            unknown_reason: Some(reason.unwrap_or_else(|| "waiting for status".into())),
            unattended_ready: false,
        },
    }
}

#[cfg(test)]
pub(crate) fn passing_status() -> Status {
    use crate::protocol::*;
    Status {
        app_version: "1".into(),
        platform: Platform::Macos,
        my_id: Some("111222333".into()),
        key_fingerprint: Some("SHA256:ab:cd".into()),
        service: ServiceStatus {
            running: true,
            detail: None,
        },
        permissions: [
            Permission::Accessibility,
            Permission::ScreenRecording,
            Permission::InputMonitoring,
            Permission::SystemAudioRecording,
        ]
        .into_iter()
        .map(|name| PermissionStatus {
            name,
            state: PermState::Granted,
            detail: None,
        })
        .collect(),
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
            host: Some("rs.example.com".into()),
            detail: None,
        },
        unattended: UnattendedStatus {
            ready: true,
            cold_boot_verified_at: Some(1),
        },
        inbound_sessions: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::*;
    use crate::state::StateEvent;

    fn state_with(status: Status) -> AppState {
        let mut s = AppState::default();
        s.apply(
            StateEvent::LinkUp {
                protocol: 1,
                server_version: "x".into(),
            },
            0,
        );
        s.apply(StateEvent::Status(Box::new(status)), 0);
        s
    }

    #[test]
    fn all_pass_is_ready() {
        let r = evaluate(&state_with(passing_status()));
        assert!(r.unattended_ready);
        assert!(r.failing().is_empty());
    }

    #[test]
    fn each_prerequisite_failure_is_named_and_blocks_ready() {
        // AC-55: even if the fork's flag says ready, a failing prerequisite blocks it.
        type Mutator = fn(&mut Status);
        let cases: Vec<(Mutator, &str)> = vec![
            (|s| s.service.running = false, "Service running"),
            (
                |s| s.permissions[1].state = PermState::Missing,
                "Screen Recording permission",
            ),
            (
                |s| s.permissions[3].state = PermState::Unknown,
                "Screen & System Audio Recording permission",
            ),
            (|s| s.power.sleep_disabled = Some(false), "Never sleeps"),
            (|s| s.power.sleep_disabled = None, "Never sleeps"),
            (
                |s| s.preboot_unlock.detected = Some(true),
                "No pre-boot unlock",
            ),
            (|s| s.server.state = RegState::Failed, "Server registration"),
            (
                |s| s.unattended.cold_boot_verified_at = None,
                "Cold-boot connection verified",
            ),
        ];
        for (mutate, label) in cases {
            let mut st = passing_status();
            mutate(&mut st);
            let r = evaluate(&state_with(st));
            assert!(!r.unattended_ready, "{label} must block unattended ready");
            let failing: Vec<_> = r.failing().iter().map(|c| c.label.clone()).collect();
            assert_eq!(failing, vec![label.to_string()]);
        }
    }

    #[test]
    fn flag_unset_is_not_ready_even_if_all_pass() {
        let mut st = passing_status();
        st.unattended.ready = false;
        assert!(!evaluate(&state_with(st)).unattended_ready);
    }

    #[test]
    fn unknown_when_link_down() {
        let mut s = state_with(passing_status());
        s.apply(
            StateEvent::LinkDown {
                reason: "app not running".into(),
            },
            0,
        );
        let r = evaluate(&s);
        assert!(r.checks.is_none());
        assert!(!r.unattended_ready);
        assert_eq!(r.unknown_reason.as_deref(), Some("app not running"));
    }

    #[test]
    fn unreported_macos_permission_blocks_ready() {
        let mut st = passing_status();
        st.permissions
            .retain(|p| p.name != Permission::ScreenRecording);
        let r = evaluate(&state_with(st.clone()));
        assert!(!r.unattended_ready);
        let failing: Vec<_> = r.failing().iter().map(|c| c.label.clone()).collect();
        assert_eq!(failing, vec!["Screen Recording permission".to_string()]);
        st.permissions.clear();
        let r = evaluate(&state_with(st));
        assert_eq!(r.failing().len(), 4);
        assert!(!r.unattended_ready);
    }

    #[test]
    fn unknown_platform_blocks_ready() {
        let mut st = passing_status();
        st.platform = Platform::Other;
        let r = evaluate(&state_with(st));
        assert!(!r.unattended_ready);
        assert_eq!(r.failing()[0].label, "Platform");
    }

    #[test]
    fn windows_has_no_permission_checks() {
        let mut st = passing_status();
        st.platform = Platform::Windows;
        st.permissions.clear();
        let r = evaluate(&state_with(st));
        assert!(r.unattended_ready);
        assert!(r
            .checks
            .unwrap()
            .iter()
            .all(|c| !c.label.contains("permission")));
    }
}
