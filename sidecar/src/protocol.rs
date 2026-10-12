//! Normative serde definitions of the fork <-> sidecar IPC protocol (docs/IPC.md).
//!
//! Everything here is plain serde-JSON with no exotic dependencies so the fork side can copy
//! these types verbatim into `vendor/rustdesk/src/`.

use serde::{Deserialize, Serialize};

/// Protocol version this build speaks natively.
pub const PROTOCOL_VERSION: u32 = 1;
/// Lowest protocol version this sidecar accepts.
pub const MIN_VERSION: u32 = 1;
/// Highest protocol version this sidecar accepts.
pub const MAX_VERSION: u32 = 1;
/// Maximum line length including the trailing newline (docs/IPC.md §3).
pub const MAX_LINE: usize = 1024 * 1024;

/// Wire envelope: `{"v":1,"id":7,"type":"...", ...body fields}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(v: u32, id: Option<u64>, body: T) -> Self {
        Self { v, id, body }
    }
}

/// Client -> server requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello {
        min_version: u32,
        max_version: u32,
        client: String,
        client_version: String,
    },
    GetStatus,
    ListPeers,
    Connect {
        peer_id: String,
    },
    Subscribe {
        topics: Vec<Topic>,
    },
    Ping,
    /// Any request type this build does not know.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Status,
    Peers,
    #[serde(other)]
    Unknown,
}

/// Server -> client messages (responses carry `id`, pushed events do not).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    HelloOk {
        version: u32,
        min_version: u32,
        max_version: u32,
        #[serde(default, deserialize_with = "lenient")]
        server: String,
        #[serde(default, deserialize_with = "lenient")]
        server_version: String,
    },
    Status {
        status: Box<Status>,
    },
    Peers {
        #[serde(deserialize_with = "peers_array")]
        peers: Vec<Peer>,
    },
    ConnectStarted {
        peer_id: String,
    },
    ConnectResult {
        peer_id: String,
        ok: bool,
        #[serde(
            default,
            deserialize_with = "lenient",
            skip_serializing_if = "Option::is_none"
        )]
        reason: Option<Reason>,
        #[serde(
            default,
            deserialize_with = "lenient",
            skip_serializing_if = "Option::is_none"
        )]
        detail: Option<String>,
    },
    Subscribed {
        #[serde(default, deserialize_with = "lenient_vec")]
        topics: Vec<Topic>,
    },
    Pong,
    Error {
        error: ErrorBody,
    },
    /// Any message type this build does not know; clients ignore it.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    #[serde(default, deserialize_with = "lenient")]
    pub message: String,
    /// Only on `version_mismatch`: the server's supported range.
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_version: Option<u32>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_version: Option<u32>,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            min_version: None,
            max_version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    VersionMismatch,
    HelloRequired,
    BadRequest,
    LineTooLong,
    UnknownType,
    PeerUnknown,
    PeerUnavailable,
    Busy,
    Internal,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Macos,
    Windows,
    #[default]
    #[serde(other)]
    Other,
}

/// The fork's status snapshot (docs/IPC.md §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Status {
    #[serde(deserialize_with = "lenient")]
    pub app_version: String,
    #[serde(deserialize_with = "lenient")]
    pub platform: Platform,
    #[serde(deserialize_with = "lenient")]
    pub my_id: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub key_fingerprint: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub service: ServiceStatus,
    #[serde(deserialize_with = "lenient_vec")]
    pub permissions: Vec<PermissionStatus>,
    #[serde(deserialize_with = "lenient")]
    pub power: PowerStatus,
    #[serde(deserialize_with = "lenient")]
    pub preboot_unlock: PrebootStatus,
    #[serde(deserialize_with = "lenient")]
    pub server: ServerStatus,
    #[serde(deserialize_with = "lenient")]
    pub unattended: UnattendedStatus,
    #[serde(deserialize_with = "sessions_array")]
    pub inbound_sessions: Vec<InboundSession>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServiceStatus {
    #[serde(deserialize_with = "lenient")]
    pub running: bool,
    #[serde(deserialize_with = "lenient")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionStatus {
    pub name: Permission,
    #[serde(default, deserialize_with = "lenient")]
    pub state: PermState,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Accessibility,
    ScreenRecording,
    InputMonitoring,
    SystemAudioRecording,
    #[serde(untagged)]
    Other(String),
}

impl Permission {
    pub fn display_name(&self) -> String {
        match self {
            Permission::Accessibility => "Accessibility".into(),
            Permission::ScreenRecording => "Screen Recording".into(),
            Permission::InputMonitoring => "Input Monitoring".into(),
            Permission::SystemAudioRecording => "Screen & System Audio Recording".into(),
            Permission::Other(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermState {
    Granted,
    Missing,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PowerStatus {
    #[serde(deserialize_with = "lenient")]
    pub sleep_disabled: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PrebootStatus {
    #[serde(deserialize_with = "lenient")]
    pub detected: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServerStatus {
    #[serde(deserialize_with = "lenient")]
    pub state: RegState,
    #[serde(deserialize_with = "lenient")]
    pub host: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegState {
    Registered,
    Registering,
    NotConfigured,
    Failed,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UnattendedStatus {
    #[serde(deserialize_with = "lenient")]
    pub ready: bool,
    #[serde(deserialize_with = "lenient")]
    pub cold_boot_verified_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundSession {
    #[serde(default, deserialize_with = "lenient")]
    pub conn_id: u64,
    #[serde(default, deserialize_with = "lenient")]
    pub peer_id: String,
    #[serde(default, deserialize_with = "lenient")]
    pub alias: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub kind: SessionKind,
    #[serde(default, deserialize_with = "lenient")]
    pub since: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Agent,
    #[default]
    #[serde(other)]
    Human,
}

/// An enrolled peer as the fork reports it (docs/IPC.md §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    #[serde(default, deserialize_with = "lenient")]
    pub alias: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub platform: Option<Platform>,
    #[serde(default, deserialize_with = "lenient")]
    pub presence: Presence,
}

/// Presence as reported on the wire (DEF-17). `Unknown` is the sidecar's own state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Presence {
    Checking,
    Reachable {
        #[serde(default, deserialize_with = "lenient")]
        route: Route,
    },
    Unavailable {
        #[serde(default, deserialize_with = "lenient")]
        reason: Reason,
        #[serde(
            default,
            deserialize_with = "lenient",
            skip_serializing_if = "Option::is_none"
        )]
        detail: Option<String>,
        #[serde(
            default,
            deserialize_with = "lenient",
            skip_serializing_if = "Option::is_none"
        )]
        last_seen: Option<u64>,
    },
    /// The sidecar's own state; also the fallback for any unrecognised state.
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Lan,
    Id,
    #[serde(untagged)]
    Other(String),
}

impl Route {
    pub fn label(&self) -> String {
        match self {
            Route::Lan => "LAN".into(),
            Route::Id => "ID".into(),
            Route::Other(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Offline,
    AuthFailed,
    HostMissingPermissions,
    ServerUnreachable,
    IdentityMismatch,
    NotEnrolled,
    Timeout,
    Cancelled,
    #[serde(untagged)]
    Other(String),
}

impl Reason {
    pub fn label(&self) -> String {
        match self {
            Reason::Offline => "offline".into(),
            Reason::AuthFailed => "auth failed".into(),
            Reason::HostMissingPermissions => "host missing permissions".into(),
            Reason::ServerUnreachable => "server unreachable".into(),
            Reason::IdentityMismatch => "host key mismatch".into(),
            Reason::NotEnrolled => "not enrolled".into(),
            Reason::Timeout => "timed out".into(),
            Reason::Cancelled => "cancelled".into(),
            Reason::Other(s) => s.clone(),
        }
    }
}

// ---- leniency (docs/IPC.md §3, §11) -------------------------------------------------------
// null, a wrong JSON type or an unrecognised shape in a defaulted field yields that field's
// default, and a bad element of a list is dropped, instead of rejecting the whole message
// (which would leave stale state on screen).

fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(T::deserialize(v).unwrap_or_default())
}

/// Top-level peer list: must be an array (anything else rejects the message, so a broken
/// list is never mistaken for "no peers" and onboarding); unreadable elements are dropped, but
/// a non-empty list with no readable element at all is rejected too.
fn peers_array<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    use serde::de::Error;
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(items) => {
            let n = items.len();
            let out: Vec<T> = items
                .into_iter()
                .filter_map(|e| T::deserialize(e).ok())
                .collect();
            if n > 0 && out.is_empty() {
                return Err(D::Error::custom(
                    "no readable peer in a non-empty peer list",
                ));
            }
            Ok(out)
        }
        _ => Err(D::Error::custom("peers must be an array")),
    }
}

/// Inbound sessions: null / absent means none; any other non-array rejects the whole status,
/// so a malformed list can never silently clear the session badge (AC-60).
fn sessions_array<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    use serde::de::Error;
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|e| T::deserialize(e).ok())
            .collect()),
        _ => Err(D::Error::custom("inbound_sessions must be an array")),
    }
}

fn lenient_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|e| T::deserialize(e).ok())
            .collect(),
        _ => Vec::new(),
    })
}

impl Default for Route {
    fn default() -> Self {
        Route::Other("unknown route".into())
    }
}

impl Default for Reason {
    fn default() -> Self {
        Reason::Other("unspecified".into())
    }
}

/// Outcome of the version negotiation rule in docs/IPC.md §4.
pub fn negotiate(
    client_min: u32,
    client_max: u32,
    server_min: u32,
    server_max: u32,
) -> Option<u32> {
    let chosen = client_max.min(server_max);
    if chosen < client_min.max(server_min) {
        None
    } else {
        Some(chosen)
    }
}

/// Serialize one message as a single line with trailing newline.
pub fn encode<T: Serialize>(v: u32, id: Option<u64>, body: T) -> String {
    // Serialization of these plain types cannot fail; fall back to an empty object defensively.
    let mut s = serde_json::to_string(&Envelope::new(v, id, body)).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

/// Why a received line could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooLong,
    NotUtf8,
    BadJson(String),
}

/// Parse one line (without its newline). Best-effort extracts `id` for error replies.
pub fn decode<T: for<'de> Deserialize<'de>>(
    line: &[u8],
) -> Result<Envelope<T>, (DecodeError, Option<u64>)> {
    if line.len() >= MAX_LINE {
        return Err((DecodeError::TooLong, None));
    }
    let text = std::str::from_utf8(line).map_err(|_| (DecodeError::NotUtf8, None))?;
    let text = text.strip_suffix('\r').unwrap_or(text);
    match serde_json::from_str::<Envelope<T>>(text) {
        Ok(env) => Ok(env),
        Err(e) => {
            let id = serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|v| v.get("id").and_then(|i| i.as_u64()));
            Err((DecodeError::BadJson(e.to_string()), id))
        }
    }
}

/// Result of reading one bounded line.
#[derive(Debug, PartialEq, Eq)]
pub enum LineRead {
    Line(Vec<u8>),
    /// A line exceeded [`MAX_LINE`]; it was discarded up to and including its newline.
    TooLong,
    Eof,
}

/// Read one `\n`-terminated line without ever buffering more than `max` bytes.
/// An unterminated tail at EOF is reported as `Eof` (a half-written line is never acted upon).
pub async fn read_line_bounded<R>(reader: &mut R, max: usize) -> std::io::Result<LineRead>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut buf: Vec<u8> = Vec::new();
    let mut overflow = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(LineRead::Eof);
        }
        if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
            if !overflow && buf.len() + pos < max {
                buf.extend_from_slice(&chunk[..pos]);
            } else {
                overflow = true;
            }
            reader.consume(pos + 1);
            return Ok(if overflow {
                LineRead::TooLong
            } else {
                LineRead::Line(buf)
            });
        }
        let n = chunk.len();
        if !overflow {
            if buf.len() + n >= max {
                overflow = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(chunk);
            }
        }
        reader.consume(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt<T>(v: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        serde_json::from_str(&serde_json::to_string(v).unwrap()).unwrap()
    }

    fn sample_status() -> Status {
        Status {
            app_version: "1.5.0-rs.1".into(),
            platform: Platform::Macos,
            my_id: Some("123456789".into()),
            key_fingerprint: Some("SHA256:aa:bb".into()),
            service: ServiceStatus {
                running: true,
                detail: None,
            },
            permissions: vec![
                PermissionStatus {
                    name: Permission::Accessibility,
                    state: PermState::Granted,
                    detail: None,
                },
                PermissionStatus {
                    name: Permission::Other("full_disk_access".into()),
                    state: PermState::Missing,
                    detail: Some("x".into()),
                },
            ],
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
                ready: true,
                cold_boot_verified_at: Some(1_791_820_800),
            },
            inbound_sessions: vec![InboundSession {
                conn_id: 7,
                peer_id: "987".into(),
                alias: Some("Scott-PC".into()),
                kind: SessionKind::Agent,
                since: Some(1),
            }],
        }
    }

    #[test]
    fn requests_round_trip() {
        let reqs = vec![
            Request::Hello {
                min_version: 1,
                max_version: 1,
                client: "t".into(),
                client_version: "0".into(),
            },
            Request::GetStatus,
            Request::ListPeers,
            Request::Connect {
                peer_id: "123".into(),
            },
            Request::Subscribe {
                topics: vec![Topic::Status, Topic::Peers],
            },
            Request::Ping,
        ];
        for (i, r) in reqs.into_iter().enumerate() {
            let line = encode(1, Some(i as u64), r.clone());
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            let env: Envelope<Request> = decode(line.trim_end().as_bytes()).unwrap();
            assert_eq!(env.v, 1);
            assert_eq!(env.id, Some(i as u64));
            assert_eq!(env.body, r);
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let msgs = vec![
            ServerMessage::HelloOk {
                version: 1,
                min_version: 1,
                max_version: 1,
                server: "RemoteSmith".into(),
                server_version: "1.5.0".into(),
            },
            ServerMessage::Status {
                status: Box::new(sample_status()),
            },
            ServerMessage::Peers {
                peers: vec![
                    Peer {
                        id: "1".into(),
                        alias: Some("PC".into()),
                        platform: Some(Platform::Windows),
                        presence: Presence::Reachable { route: Route::Lan },
                    },
                    Peer {
                        id: "2".into(),
                        alias: None,
                        platform: None,
                        presence: Presence::Unavailable {
                            reason: Reason::Other("weird".into()),
                            detail: Some("d".into()),
                            last_seen: Some(5),
                        },
                    },
                    Peer {
                        id: "3".into(),
                        alias: None,
                        platform: None,
                        presence: Presence::Checking,
                    },
                ],
            },
            ServerMessage::ConnectStarted {
                peer_id: "1".into(),
            },
            ServerMessage::ConnectResult {
                peer_id: "1".into(),
                ok: false,
                reason: Some(Reason::AuthFailed),
                detail: None,
            },
            ServerMessage::Subscribed {
                topics: vec![Topic::Peers],
            },
            ServerMessage::Pong,
            ServerMessage::Error {
                error: ErrorBody {
                    code: ErrorCode::VersionMismatch,
                    message: "m".into(),
                    min_version: Some(2),
                    max_version: Some(3),
                },
            },
        ];
        for m in msgs {
            let line = encode(1, None, m.clone());
            let env: Envelope<ServerMessage> = decode(line.trim_end().as_bytes()).unwrap();
            assert_eq!(env.id, None);
            assert_eq!(env.body, m);
        }
        assert_eq!(rt(&sample_status()), sample_status());
    }

    #[test]
    fn documented_examples_parse() {
        let hello = r#"{"v":1,"id":1,"type":"hello_ok","version":1,"min_version":1,"max_version":1,"server":"RemoteSmith","server_version":"1.5.0-rs.1"}"#;
        let env: Envelope<ServerMessage> = decode(hello.as_bytes()).unwrap();
        assert!(matches!(
            env.body,
            ServerMessage::HelloOk { version: 1, .. }
        ));
        let err = r#"{"v":1,"id":3,"type":"error","error":{"code":"peer_unknown","message":"no peer 987654321"}}"#;
        let env: Envelope<ServerMessage> = decode(err.as_bytes()).unwrap();
        assert_eq!(env.id, Some(3));
        match env.body {
            ServerMessage::Error { error } => assert_eq!(error.code, ErrorCode::PeerUnknown),
            other => panic!("{other:?}"),
        }
        let peer = r#"{"v":1,"type":"peers","peers":[{"id":"987654321","alias":"Scott-PC","platform":"windows","presence":{"state":"reachable","route":"lan"}}]}"#;
        let env: Envelope<ServerMessage> = decode(peer.as_bytes()).unwrap();
        assert!(matches!(env.body, ServerMessage::Peers { .. }));
    }

    #[test]
    fn unknown_values_map_to_fallbacks() {
        let line = r#"{"v":1,"type":"peers","extra":true,"peers":[
            {"id":"1","presence":{"state":"teleporting"}},
            {"id":"2","presence":{"state":"reachable","route":"carrier_pigeon"}},
            {"id":"3"}]}"#
            .replace('\n', "");
        let env: Envelope<ServerMessage> = decode(line.as_bytes()).unwrap();
        let ServerMessage::Peers { peers } = env.body else {
            panic!()
        };
        assert_eq!(peers[0].presence, Presence::Unknown);
        assert_eq!(
            peers[1].presence,
            Presence::Reachable {
                route: Route::Other("carrier_pigeon".into())
            }
        );
        assert_eq!(peers[2].presence, Presence::Unknown);

        let st = r#"{"v":1,"type":"status","status":{"platform":"beos","permissions":[{"name":"accessibility","state":"maybe"}],"server":{"state":"exploded"},"inbound_sessions":[{"peer_id":"9","kind":"robot"}]}}"#;
        let env: Envelope<ServerMessage> = decode(st.as_bytes()).unwrap();
        let ServerMessage::Status { status } = env.body else {
            panic!()
        };
        assert_eq!(status.platform, Platform::Other);
        assert_eq!(status.permissions[0].state, PermState::Unknown);
        assert_eq!(status.server.state, RegState::Unknown);
        assert_eq!(status.inbound_sessions[0].kind, SessionKind::Human);
        assert!(!status.unattended.ready);

        let unk = r#"{"v":1,"type":"desk_mode","on":true}"#;
        let env: Envelope<ServerMessage> = decode(unk.as_bytes()).unwrap();
        assert_eq!(env.body, ServerMessage::Unknown);
        let unk_req = r#"{"v":1,"id":4,"type":"format_disk"}"#;
        let env: Envelope<Request> = decode(unk_req.as_bytes()).unwrap();
        assert_eq!(env.body, Request::Unknown);
        assert_eq!(env.id, Some(4));
    }

    #[test]
    fn malformed_lines_are_errors_with_best_effort_id() {
        assert!(matches!(
            decode::<Request>(b"not json"),
            Err((DecodeError::BadJson(_), None))
        ));
        assert!(matches!(
            decode::<Request>(b"[1,2]"),
            Err((DecodeError::BadJson(_), None))
        ));
        assert!(matches!(
            decode::<Request>(&[0xff, 0xfe]),
            Err((DecodeError::NotUtf8, None))
        ));
        // Right shape but wrong field type: id still recovered for the error reply.
        assert!(matches!(
            decode::<Request>(br#"{"v":1,"id":9,"type":"connect","peer_id":5}"#),
            Err((DecodeError::BadJson(_), Some(9)))
        ));
        // Missing v.
        assert!(decode::<Request>(br#"{"id":1,"type":"ping"}"#).is_err());
        let big = vec![b'a'; MAX_LINE];
        assert!(matches!(
            decode::<Request>(&big),
            Err((DecodeError::TooLong, None))
        ));
        // CRLF tolerated.
        assert!(decode::<Request>(b"{\"v\":1,\"id\":1,\"type\":\"ping\"}\r").is_ok());
    }

    #[test]
    fn credentials_are_not_representable_on_connect() {
        // A client cannot smuggle a password through `connect`; extra fields are dropped.
        let env: Envelope<Request> =
            decode(br#"{"v":1,"id":2,"type":"connect","peer_id":"1","password":"hunter2"}"#)
                .unwrap();
        let re = encode(1, env.id, env.body);
        assert!(!re.contains("hunter2"));
    }

    #[test]
    fn nulls_wrong_types_and_bad_elements_do_not_reject_the_message() {
        let st = r#"{"v":1,"type":"status","status":{"app_version":null,"platform":null,"service":null,
            "permissions":[{"name":"accessibility","state":null},{"state":"granted"},7,{"name":"screen_recording","state":"granted"}],
            "power":{"sleep_disabled":"yes"},"server":null,"unattended":{"ready":null},
            "inbound_sessions":[{"conn_id":null,"kind":null,"peer_id":"9"},{"alias":"x"},"junk"]}}"#
            .replace('\n', "");
        let env: Envelope<ServerMessage> = decode(st.as_bytes()).unwrap();
        let ServerMessage::Status { status } = env.body else {
            panic!()
        };
        assert_eq!(status.platform, Platform::Other);
        assert!(!status.service.running);
        // Element without a name is dropped; others kept.
        assert_eq!(status.permissions.len(), 2);
        assert_eq!(status.permissions[0].state, PermState::Unknown);
        assert_eq!(status.power.sleep_disabled, None);
        assert_eq!(status.server.state, RegState::Unknown);
        assert!(!status.unattended.ready);
        // Sessions stay visible even when malformed (badge must not silently clear).
        assert_eq!(status.inbound_sessions.len(), 2);
        assert_eq!(status.inbound_sessions[0].peer_id, "9");
        assert_eq!(status.inbound_sessions[1].peer_id, "");

        let peers = r#"{"v":1,"type":"peers","peers":[{"id":"1","presence":null},{"id":"2","alias":5,
            "presence":{"state":"reachable"}},{"id":"3","presence":{"state":"unavailable","last_seen":"x"}},{"alias":"no id"}]}"#
            .replace('\n', "");
        let env: Envelope<ServerMessage> = decode(peers.as_bytes()).unwrap();
        let ServerMessage::Peers { peers } = env.body else {
            panic!()
        };
        assert_eq!(peers.len(), 3);
        assert_eq!(peers[0].presence, Presence::Unknown);
        assert_eq!(peers[1].alias, None);
        assert_eq!(
            peers[1].presence,
            Presence::Reachable {
                route: Route::Other("unknown route".into())
            }
        );
        assert_eq!(
            peers[2].presence,
            Presence::Unavailable {
                reason: Reason::Other("unspecified".into()),
                detail: None,
                last_seen: None
            }
        );

        let e = r#"{"v":1,"id":1,"type":"error","error":{"code":"version_mismatch","message":null,"min_version":2,"max_version":3}}"#;
        let env: Envelope<ServerMessage> = decode(e.as_bytes()).unwrap();
        let ServerMessage::Error { error } = env.body else {
            panic!()
        };
        assert_eq!(error.code, ErrorCode::VersionMismatch);
        assert_eq!((error.min_version, error.max_version), (Some(2), Some(3)));

        let h = r#"{"v":1,"id":1,"type":"hello_ok","version":1,"min_version":1,"max_version":1,"server":null,"server_version":null}"#;
        assert!(decode::<ServerMessage>(h.as_bytes()).is_ok());
    }

    #[test]
    fn list_shape_errors_reject_instead_of_clearing() {
        for bad in [
            r#"{"v":1,"type":"peers","peers":{"id":"1"}}"#,
            r#"{"v":1,"type":"peers","peers":null}"#,
            r#"{"v":1,"type":"peers","peers":[5,{"alias":"x"}]}"#,
            r#"{"v":1,"type":"status","status":{"inbound_sessions":{"peer_id":"9"}}}"#,
            r#"{"v":1,"type":"status","status":{"inbound_sessions":"none"}}"#,
        ] {
            assert!(decode::<ServerMessage>(bad.as_bytes()).is_err(), "{bad}");
        }
        for ok in [
            r#"{"v":1,"type":"peers","peers":[]}"#,
            r#"{"v":1,"type":"peers","peers":[5,{"id":"1"}]}"#,
            r#"{"v":1,"type":"status","status":{"inbound_sessions":null}}"#,
            r#"{"v":1,"type":"status","status":{}}"#,
        ] {
            assert!(decode::<ServerMessage>(ok.as_bytes()).is_ok(), "{ok}");
        }
    }

    #[test]
    fn negotiation_rule() {
        assert_eq!(negotiate(1, 1, 1, 1), Some(1));
        assert_eq!(negotiate(1, 3, 2, 5), Some(3));
        assert_eq!(negotiate(1, 1, 2, 3), None);
        assert_eq!(negotiate(4, 5, 1, 3), None);
        assert_eq!(negotiate(1, 2, 2, 2), Some(2));
    }

    #[tokio::test]
    async fn bounded_reader_handles_oversize_and_eof() {
        let mut data = Vec::new();
        data.extend_from_slice(b"short\n");
        data.extend(std::iter::repeat_n(b'x', 50));
        data.extend_from_slice(b"\nafter\npartial");
        let mut r = tokio::io::BufReader::with_capacity(8, &data[..]);
        assert_eq!(
            read_line_bounded(&mut r, 20).await.unwrap(),
            LineRead::Line(b"short".to_vec())
        );
        assert_eq!(
            read_line_bounded(&mut r, 20).await.unwrap(),
            LineRead::TooLong
        );
        assert_eq!(
            read_line_bounded(&mut r, 20).await.unwrap(),
            LineRead::Line(b"after".to_vec())
        );
        assert_eq!(read_line_bounded(&mut r, 20).await.unwrap(), LineRead::Eof);
    }

    #[tokio::test]
    async fn bounded_reader_exact_boundary() {
        // max=6: a 5-byte line fits (5 + newline = 6 bytes would be >= max? line len < max).
        let data = b"12345\n123456\n".to_vec();
        let mut r = tokio::io::BufReader::with_capacity(3, &data[..]);
        assert_eq!(
            read_line_bounded(&mut r, 6).await.unwrap(),
            LineRead::Line(b"12345".to_vec())
        );
        assert_eq!(
            read_line_bounded(&mut r, 6).await.unwrap(),
            LineRead::TooLong
        );
    }
}
