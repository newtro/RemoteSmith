# RemoteSmith fork ↔ sidecar IPC — protocol v1

This is the contract between the RemoteSmith fork (the server: the per-user RustDesk UI/server
process, patch range 0400–0499) and the sidecar tray app `remotesmith-tray` (the client).
It serves DEF-4, DEF-17, FLOW-13, FLOW-16, §6.2, §6.2a, §6.3 and AC-20, AC-21, AC-25, AC-55, AC-60
in [PLAN.md](PLAN.md). The reference client lives in `sidecar/src/` and a reference server
(`fake-fork`) in `sidecar/src/fake_fork.rs`; the Rust types in `sidecar/src/protocol.rs` are the
normative serde definitions of everything below.

Keywords MUST, SHOULD and MAY are used in the RFC 2119 sense.

## 1. Principles

1. **The fork never depends on the sidecar (DEC-5, AC-25).** The endpoint runs on its own task.
   Nothing the sidecar does or fails to do — never connecting, connecting and going silent,
   crashing mid-line, sending garbage, sending a 2 GB line, speaking another version — may block,
   slow or crash any human feature. Every per-connection failure closes only that connection.
2. **Credentials never cross the IPC.** No message carries a password, permanent or temporary
   password, session token, MCP token, private key, or saved peer credential. `connect` names a
   peer; the fork uses its own saved credential. The fork MUST ignore any extra field on `connect`
   (it never reads credentials from IPC). Public data is fine: peer IDs, aliases, the host's
   public-key fingerprint.
3. **The sidecar never parses upstream config files.** Everything it shows comes from this protocol.
4. **Plain serde-JSON.** Newline-delimited UTF-8 JSON objects; no other framing, compression or
   binary encoding, so the fork side needs nothing beyond `serde`, `serde_json` and `tokio`
   (all already in RustDesk).

## 2. Transport

| | macOS | Windows |
|---|---|---|
| Endpoint | Unix domain stream socket `/tmp/RemoteSmith-<uid>/remotesmith-ipc.sock` (`<uid>` = numeric effective uid of the logged-in user, e.g. `501`). The directory is the per-user IPC directory the fork already uses for its own IPC (hbb_common `ipc_parent_dir_for_uid`, created mode `0700` by `ensure_secure_ipc_parent_dir`) | Named pipe `\\.\pipe\RemoteSmith-ipc-<username>` (`<username>` = `%USERNAME%`), byte mode |
| Who listens | the user's fork process (the UI/`--server` process running as that user, not the root service) | a fork process running **with the logged-in user's own token** in that user's session — on RustDesk that is the `--tray` / UI process, which relays what it needs from the SYSTEM `--server` / service over RustDesk's existing internal IPC. The pipe MUST NOT be created by a SYSTEM-token process (the service or a SYSTEM-launched `--server`): the client's server-SID check rejects such a listener by design, and its DACL would be built from the wrong SID. The DACL SID is the session user's SID, i.e. the token user of the listening process |
| Server: endpoint permissions | the directory MUST be a real directory (not a symlink) owned by the user with no group/other access (`mode & 077 == 0`); create it `0700` if missing, refuse to bind otherwise. Socket file mode `0600` | DACL grants `GENERIC_ALL` to the current user's SID only (SDDL `D:P(A;;GA;;;<user SID>)`); `PIPE_REJECT_REMOTE_CLIENTS`; first instance created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so if the name already exists (someone squatted it) the fork's create fails and it logs an error rather than sharing the name |
| Server: client check | after `accept`, the server MUST check the peer credentials (`getpeereid` / tokio `peer_cred()`) and close any client whose uid is not its own | the DACL keeps other users out |
| Stale endpoint | before binding, the server removes an existing socket only if it is owned by the same uid and a connect to it fails; a live one is never stolen. On exit it unlinks the socket only if it is still the inode it bound | n/a (pipes vanish with their last handle) |
| Client checks | `lstat` the socket (must be a socket, not a symlink); the parent directory must pass the same private-directory check; after connecting, the server's peer credentials MUST equal the client's effective uid. The last check is authoritative and immune to path races | after opening, `GetNamedPipeServerProcessId` → that process's token user SID MUST equal the client's SID; otherwise disconnect. This defeats a process that created the pipe name before the fork |
| Concurrency | the server SHOULD accept at least 4 and MAY cap at 8 simultaneous clients; extra clients are closed immediately. A failed `accept` is logged and retried after a short pause; it never ends the endpoint | same; a new pipe instance is created for each connection, and after a failed `ConnectNamedPipe` (e.g. `ERROR_NO_DATA`) a fresh instance replaces it |

The `<uid>` / `<username>` suffix keeps two logged-in users (fast user switching) from colliding.
For tests and the dev tool, both sides honour the environment variable `REMOTESMITH_IPC_PATH`,
which replaces the whole endpoint path (socket path or `\\.\pipe\...` name); the
private-directory rules still apply to an overridden socket path.

## 3. Framing and limits

- Each message is one JSON object on one line, UTF-8, terminated by `\n` (a preceding `\r` is
  tolerated and stripped). No embedded newlines (serde_json never emits them in compact mode).
- **Max line length: 1 MiB (1 048 576 bytes) including the `\n`.** A receiver MUST NOT buffer
  more than that: it discards bytes until the next `\n` and treats the line as malformed
  (`line_too_long`). Senders MUST NOT emit longer lines (truncate peer lists instead; 1 MiB is
  thousands of peers).
- Every message has `"v"`: the protocol version the sender is speaking. `hello` is always sent
  with `"v": 1` (see §4). After negotiation, both sides send the negotiated version.
- `"id"` (unsigned 64-bit integer) is chosen by the client on each request. A server message
  **with** `id` is the response to that request; a server message **without** `id` is a pushed
  event. Every request gets exactly one response (success type or `error`).
- `"type"` (string, snake_case) names the message.
- Unknown fields MUST be ignored by both sides.
- **List shapes.** The top-level `peers` array of a `peers` message is required and must be an
  array; if it is non-empty but no element is readable, the message is rejected. `inbound_sessions`
  may be absent or `null` (no sessions), but any other non-array value rejects the whole `status`
  message, so a malformed list can never silently clear the session badge.
- **Leniency inside objects.** Fields marked *required* below (`id` of a peer, `name` of a
  permission, the top-level `status` / `peers` of their messages, `version`/`min_version`/
  `max_version` of `hello_ok`, `code` of an error) must be present with the right type, or the
  element / message is rejected. For every other field, an absent value, JSON `null`, a wrong
  JSON type or an unrecognised shape is treated as that field's default (shown in brackets in §7
  and §8) rather than rejecting the whole message, and an array element that cannot be read is
  dropped while the rest of the array is kept. A receiver must never keep showing stale state
  because one field of a fresh message was odd.

## 4. Version negotiation

Current protocol version: **1**. Supported range of this sidecar: **1–1**.

The first client message MUST be `hello`; the server answers anything else with
`error{code:"hello_required"}`. `hello`, `hello_ok` and `error` are **frozen**: their shape never
changes in any future version, so any two builds can always negotiate.

```json
{"v":1,"id":1,"type":"hello","min_version":1,"max_version":1,"client":"remotesmith-tray","client_version":"0.1.0"}
```

Server rule: `chosen = min(client.max, server.max)`; if `chosen < max(client.min, server.min)`
the ranges do not overlap.

- Overlap → `hello_ok` with `"v": chosen`:
  ```json
  {"v":1,"id":1,"type":"hello_ok","version":1,"min_version":1,"max_version":1,"server":"RemoteSmith","server_version":"1.5.0-rs.1"}
  ```
- No overlap → `error`, then the server closes the connection:
  ```json
  {"v":1,"id":1,"type":"error","error":{"code":"version_mismatch","message":"server speaks 2-3, client 1-1","min_version":2,"max_version":3}}
  ```

The client independently checks `hello_ok.version` is inside its own range (it MUST NOT trust a
buggy server's choice). On mismatch from either side the client shows every peer and the readiness
summary as **unknown** with the reason "version mismatch (app speaks a–b, tray x–y)", sends nothing
further on that connection, closes it, and re-tries the handshake every 30 s (the app may be
upgraded under it). It never falls back to guessing (AC-25, AC-47, DEF-4).

The server MUST close a connection that has not completed `hello` within **5 s**.

## 5. Requests (client → server)

| type | fields | success response |
|---|---|---|
| `hello` | `min_version` u32, `max_version` u32, `client` string, `client_version` string | `hello_ok` |
| `get_status` | — | `status` |
| `list_peers` | — | `peers` |
| `connect` | `peer_id` string | `connect_started` |
| `subscribe` | `topics`: array of `"status"` \| `"peers"` | `subscribed`, then immediately one `status` and/or `peers` **event** (no id) per topic with the current snapshot |
| `ping` | — | `pong` |

Examples:

```json
{"v":1,"id":2,"type":"subscribe","topics":["status","peers"]}
{"v":1,"id":3,"type":"connect","peer_id":"123456789"}
{"v":1,"id":4,"type":"ping"}
```

`connect` asks the fork to open a normal outgoing session window to that peer exactly as if Scott
had clicked it in the fork (LAN-first per DEF-8, saved credential, TOFU prompts, retry rules all
belong to the fork). The response only says whether the attempt was started:
- `connect_started{peer_id}` — the fork has a session window for it (a duplicate request for a peer
  that already has a session or attempt in flight focuses it and still answers `connect_started`;
  DEF-9 coalescing).
- `error` with `peer_unknown` (not in the fork's peer list), `peer_unavailable` (fork already knows
  it cannot connect; `message` says why), or `internal`.

A Connect click made while the link is not up fails immediately in the tray with the reason
(nothing is queued for later). One made while the handshake is in progress is sent right after
`hello_ok`, within the 5 s request timeout.

The final outcome MAY be pushed later as a `connect_result` event. Failures are always also shown
in the fork's own session window — the sidecar is never the only place an error appears.

## 6. Server messages

### 6.1 Responses

| type | fields |
|---|---|
| `hello_ok` | `version` u32 (chosen), `min_version` u32, `max_version` u32, `server` string, `server_version` string |
| `status` | `status`: Status (§7) |
| `peers` | `peers`: array of Peer (§8) |
| `connect_started` | `peer_id` string |
| `subscribed` | `topics` array |
| `pong` | — |
| `error` | `error`: Error (§6.3) |

### 6.2 Pushed events (no `id`)

| type | when |
|---|---|
| `status` | whenever any Status field changes, to clients subscribed to `status` |
| `peers` | whenever the peer list or any presence changes, to clients subscribed to `peers` |
| `connect_result` | MAY be sent when an attempt started via IPC finishes: `peer_id` string, `ok` bool, `reason` Reason (§8.2, when `ok` is false), `detail` string? |

Events always carry the **full** current object (no deltas), so a client that missed one only
needs the next. The server SHOULD coalesce bursts (at most ~10 events/s per topic) but MUST deliver
the latest state within **500 ms** of a change. In particular an inbound session starting or
ending MUST be pushed within 500 ms so the tray badge clears within 2 s (AC-60).

### 6.3 Error shape

```json
{"v":1,"id":3,"type":"error","error":{"code":"peer_unknown","message":"no peer 987654321"}}
```

`error.code` (string): `version_mismatch` (also carries `min_version`, `max_version` of the
server), `hello_required`, `bad_request` (unparseable JSON or wrong field types), `line_too_long`,
`unknown_type`, `peer_unknown`, `peer_unavailable`, `busy`, `internal`. `error.message` is a short
human-readable string with no secrets. When the offending line had no readable `id`, the error is
sent without `id`. New codes MAY be added; clients treat unknown codes as generic failures.

## 7. Status object

All fields are always present in v1 output; receivers MUST tolerate absent fields (defaults in
brackets) so additive changes stay compatible. Timestamps are integer **Unix seconds, UTC**.

```json
{
  "app_version": "1.5.0-rs.1",
  "platform": "macos",
  "my_id": "123456789",
  "key_fingerprint": "SHA256:3f:9a:…",
  "service": {"running": true, "detail": null},
  "permissions": [
    {"name": "accessibility",          "state": "granted"},
    {"name": "screen_recording",       "state": "missing"},
    {"name": "input_monitoring",       "state": "granted"},
    {"name": "system_audio_recording", "state": "unknown", "detail": "not yet requested"}
  ],
  "power": {"sleep_disabled": true, "detail": null},
  "preboot_unlock": {"detected": false, "detail": null},
  "server": {"state": "registered", "host": "rs.example.com", "detail": null},
  "unattended": {"ready": false, "cold_boot_verified_at": null},
  "inbound_sessions": [
    {"conn_id": 7, "peer_id": "987654321", "alias": "Scott-PC", "kind": "human", "since": 1791820800}
  ]
}
```

| field | type | meaning |
|---|---|---|
| `app_version` | string [""] | fork build version |
| `platform` | `"macos"` \| `"windows"` \| other string [`"other"`] | |
| `my_id` | string? | this machine's RustDesk ID (shown in tray for TOFU checks, §7 of PLAN) |
| `key_fingerprint` | string? | fingerprint of this host's **public** key, same format the client's TOFU prompt shows |
| `service.running` | bool [false] | the RemoteSmith service (macOS LaunchDaemon / Windows service) is running |
| `permissions[]` | array [[]] | macOS TCC state; empty on Windows. On macOS the fork MUST report all four names below; the sidecar treats any of the four that is missing from the list as `unknown` (failing). `name` is required. `name`: `accessibility`, `screen_recording`, `input_monitoring`, `system_audio_recording`, or another string. `state`: `granted` \| `missing` \| `unknown` (other strings → `unknown`). `detail` string? |
| `power.sleep_disabled` | bool? | true when the machine is set never to sleep on AC power; null = could not determine |
| `preboot_unlock.detected` | bool? | true when a pre-boot unlock would block unattended reboot (FileVault on macOS without auto-unlock, BitLocker with PIN on Windows; ASM-1); null = unknown |
| `server.state` | `registered` \| `registering` \| `not_configured` \| `failed` \| other [`unknown`] | registration with hbbs; `host` string?, `detail` string? |
| `unattended.ready` | bool [false] | the fork's "Unattended ready" flag. The fork sets it **only** after it has recorded a successful inbound connection that arrived while the machine was at the login screen following a cold boot, with every other prerequisite passing at the time; it clears it whenever a prerequisite later fails |
| `unattended.cold_boot_verified_at` | u64? | when that cold-boot connection was recorded |
| `inbound_sessions[]` | array [[]] | every active **inbound** session (someone controlling this machine) |
| `.conn_id` | u64 | fork-local connection id, unique while the session lives |
| `.peer_id` | string [""] | controlling peer's ID (a session with an unreadable id is still shown, as "unknown peer", so the badge never silently disappears) |
| `.alias` | string? | its alias/hostname if known |
| `.kind` | `human` \| `agent` (other → `human`) | `agent` while an MCP agent holds the control grant on that connection (v3; always `human` in v1) |
| `.since` | u64? | session start |

**Readiness (§6.3, AC-55).** The sidecar derives one check per prerequisite: service running; each
of the four macOS permissions granted (on `platform` other than `macos`/`windows`, an extra
"Platform" check is unknown); sleep disabled; no pre-boot unlock; server registered; cold-boot
connection recorded. Any `false`, `missing` or `null`/`unknown` value is a failing (or unknown)
check, named in the tray. The tray shows **"Unattended ready"** only when `unattended.ready` is true
**and** every derived check passes — the sidecar re-checks rather than trusting the flag alone.
Neither side ever changes a security setting to make a check pass.

## 8. Peer object

```json
{"id": "987654321", "alias": "Scott-PC", "platform": "windows",
 "presence": {"state": "reachable", "route": "lan"}}
```

| field | type | meaning |
|---|---|---|
| `id` | string | RustDesk peer ID (stable identity, DEF-3) |
| `alias` | string? | display name |
| `platform` | string? | as in Status |
| `presence` | Presence | see below |

The list contains the fork's enrolled peers (the "other machine(s)"), never this machine itself.
An empty list means no peer is enrolled; the tray shows onboarding (§6.2 of PLAN).

### 8.1 Presence (DEF-17)

Internally tagged by `state`:

| state | extra fields | meaning |
|---|---|---|
| `checking` | — | the fork is probing (startup, after a network change) |
| `reachable` | `route`: `lan` \| `id` (other strings tolerated) | a connection would succeed by that route right now: `lan` = direct on the local network, `id` = via the rendezvous server (hole-punch or relay) |
| `unavailable` | `reason` Reason, `detail` string?, `last_seen` u64? | cannot connect, and why. `last_seen` = last time the peer was reachable by any route |
| `unknown` | — | reserved for the **sidecar**: it shows every peer as unknown while it cannot reach the fork or the versions mismatch. A server SHOULD NOT send it; a client treats it, and any unrecognised state, as unknown |

### 8.2 Reason

`offline`, `auth_failed`, `host_missing_permissions`, `server_unreachable`, `identity_mismatch`
(host key differs from the enrolled fingerprint, DEF-3), `not_enrolled`, `timeout`, `cancelled`,
or another string (displayed verbatim). Used by `unavailable` presence and `connect_result`.

## 9. Timeouts and liveness

| what | value | side |
|---|---|---|
| connect to endpoint | 2 s | client |
| `hello` → `hello_ok` | 3 s, else close and back off | client |
| hello must arrive | 5 s, else close | server |
| any other request → its response | 5 s; on timeout the request fails (a `connect` shows "no response from RemoteSmith") | client |
| keepalive | client sends `ping` every 5 s | client |
| stale link | no line received for 12 s → link considered dead: peers unknown, reconnect | client |
| idle client | server MAY close a client silent for 60 s (pings keep it alive) | server |
| reconnect backoff | 250 ms doubling to 5 s max; reset after a successful `hello_ok`; 30 s after a version mismatch | client |
| slow reader | the server MUST NOT let a client that stops reading block anything but that client's own task: either a bounded per-connection queue (e.g. 256 messages; drop the client when full) or a per-write timeout. The reference server uses the timeout | server |
| write | a single write blocked for 5 s → drop the client | both |

## 10. Malformed input

Receivers are hostile-input safe:

- **Server** — on an unparseable line (bad JSON, not an object, invalid UTF-8, wrong field types):
  reply `error{code:"bad_request"}` (with `id` if one was readable) and keep the connection. Oversized
  line: discard to the next newline, reply `line_too_long`. Unknown `type`: `unknown_type`.
  After 20 malformed lines on one connection, or at EOF mid-line, close that connection. Nothing
  here may panic or touch shared state.
- **Client** — ignores (and logs, truncated to 120 chars) unparseable lines, oversized lines,
  unknown message types, responses whose `id` matches no pending request, and events with unknown
  enum values (mapped to fallbacks). After 20 consecutive malformed lines it drops the connection and
  reconnects with backoff. It never panics and never blocks the UI thread: all I/O runs on its own
  thread, and the UI only receives finished state snapshots.

## 11. Compatibility rules

- **Within a version** changes are additive only: new optional fields, new enum string values,
  new request types, new event types, new error codes. Receivers MUST ignore unknown fields and
  unknown event types and map unknown enum values to the fallback documented above.
- **Breaking changes** (removing/renaming a field, changing a type or meaning) bump the version.
  The fork then supports a range (e.g. 1–2) and speaks whichever version `hello` selects, so an old
  sidecar keeps working until the fork drops the old version.
- `hello`, `hello_ok` and `error` never change shape.
- Each release states its range: fork in its `hello_ok`, sidecar in its `hello` and in
  `remotesmith-tray --version`.

## 12. Known gaps (deferred)

- Windows has no text next to the tray icon: the session badge appears as a different icon, the
  tooltip ("RemoteSmith — <peer> · agent") and the first menu rows. macOS shows the text in the
  menu bar.
- v2 adds Deskflow state (desk-mode topic) as an additive change within v1 or as v2.
