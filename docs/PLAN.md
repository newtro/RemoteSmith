# RemoteSmith — Plan

**Status:** approved brainstorm output, 2026-10-12. **Owner:** Scott.
**IDs:** `FLOW-n` (flows), `CON-n` (concepts), `DEC-n` (decisions), `DEF-n` (technical defaults), `AC-n` (acceptance criteria), `ASM-n` (assumptions). These IDs carry from the brainstorm draft, so gaps in the numbering are intentional: they belong to features that were cut.

---

## 1. Vision

RemoteSmith is a two-way remote desktop between Scott's Mac and his Windows PC. Either machine can host a session or control the other. It is a thin, renamed fork of **RustDesk** plus a small **Rust sidecar**, with a self-hosted rendezvous/relay server on Scott's Lightsail VM.

The reason to build it rather than use stock tools is **agent control**: in the end, Claude and other agents should be able to drive either machine over MCP. v1 is still **human-first**. It has to be a daily-driver remote desktop before any agent work starts (DEC-2).

## 2. Goals and non-goals

**Goals**
- G1 — A daily driver: Scott works a full day through RemoteSmith in each direction without walking over (DEC-3, AC-34).
- G2 — Dev and office quality: sharp text in editors, terminals, browsers and Unity. Text clarity matters more than frame rate (DEC-4).
- G3 — v1 must-haves: screen, input, clipboard, file transfer, multi-monitor and audio, in both directions (CON-5).
- G4 — Primarily on the home LAN, with an occasional internet path. The LAN path must work when the Lightsail VM is down (DEC-11).
- G5 — Reachable in any OS state: logged in, locked, at the login screen, or after an unattended reboot (DEC-8).
- G6 — Personal use now, a public open-source repo from day one (DEC-15).
- G7 — Agent control over MCP, with an explicit human Take over (v3; DEC-17a, DEC-18).

**Non-goals**
- Gaming, video, or 60fps+ streaming.
- Mobile, Linux or web clients.
- Accounts, multi-tenancy, or RustDesk Pro server features.
- Pre-boot disk unlock (FileVault/BitLocker PIN).
- Headless or virtual displays (deferred).
- Things cut during the brainstorm:
  - Wake-on-LAN (DEC-13: both machines are always on).
  - Watch & notify (DEC-17).
  - Remote app windows (DEC-19).
- Expansion ideas that weren't selected (DEC-21): continue-on-other-OS, a cross-platform rehearsal bench, teach-by-demo recipes, and upstreaming the MCP server ourselves.

## 3. Actors and core flows

**Actors**
- **Scott, as controller.** Drives Mac→Windows or Windows→Mac.
- **Host service.** The RustDesk-fork service: a Windows service, or on macOS a root LaunchDaemon plus a per-user server process. It captures the screen and audio and injects input.
- **Sidecar.** A Rust tray/menu-bar app on each machine. It provides presence and connect (v1) and manages Deskflow (v2). Human remote-desktop features never depend on it (DEC-5).
- **Rendezvous/relay server.** hbbs/hbbr on the shared Lightsail VM.
- **Agent (v3).** Claude Code or another MCP client on the controlling machine.
- **Maintainer.** Scott: rebasing on upstream, building, signing and releasing.
- **Future open-source user.** Installs from GitHub and points it at their own server.

**Core flows** (FLOW-11, -12 and -14 were removed with the cut features)

| ID | Category | Flow | Phase |
|----|----------|------|-------|
| FLOW-1 | setup | Install on the Mac and the PC. Grant macOS permissions (Accessibility, Screen Recording, Input Monitoring, Screen & System Audio Recording). Install the service. Run the readiness check. | v1 |
| FLOW-2 | setup | First run: enter or import the server host and key (or choose direct-LAN only), set the permanent password, and enroll the other machine as a peer by confirming its ID and key fingerprint once (§7). | v1 |
| FLOW-3 | ops | Deploy and upgrade hbbs/hbbr on Lightsail (Docker, firewall, DNS). | v1 |
| FLOW-4 | primary | Connect on the LAN (direct IP first) and run a session: view, input with key translation, raw toggle, escape hotkey. | v1 |
| FLOW-5 | primary | Connect from outside the LAN through Lightsail (hole-punch, falling back to relay). | v1 |
| FLOW-6 | primary | Multi-monitor: pick or switch displays, or view all; 1:1 or scale-to-fit. | v1 |
| FLOW-7 | primary | Clipboard sync: text, images, files. | v1 |
| FLOW-8 | primary | File transfer. | v1 |
| FLOW-9 | primary | Audio forwarding. | v1 |
| FLOW-13 | primary | Presence tray: the other machine's state and route, plus one-click Connect. | v1 |
| FLOW-15 | recovery | Reconnect after a network drop, a host reboot (to the login screen), or a lock. | v1 |
| FLOW-16 | recovery | The sidecar crashes or is a mismatched version; human features are unaffected. | v1 |
| FLOW-17 | recovery | Lightsail is down: the LAN still works, and the internet path reports the problem clearly. | v1 |
| FLOW-18 | lifecycle | Rebase the fork on an upstream release, rebuild, sign, and update installs. | v1+ |
| FLOW-19 | lifecycle | Forget a peer, change the password, uninstall. | v1 |
| FLOW-21 | lifecycle | Keep the public repo compliant: AGPL, renamed, no personal infrastructure. | v1 |
| FLOW-22 | setup | Desk-mode setup: install the bundled Deskflow, grant its macOS permissions, install the elevated Windows daemon, confirm TLS fingerprints, pick the primary machine and layout. | v2 |
| FLOW-10 | primary | Desk mode: Deskflow edge-crossing between the two physical desks. | v2 |
| FLOW-20 | primary | An agent drives the other machine over MCP. Lifecycle: list peers → connect or select a session → request control (local approval) → act → release. Scott can Take over at any time. Control is lost on disconnect, takeover or denial (§7). | v3 |

## 4. Architecture overview

```
 Mac                                         Windows PC
┌──────────────────────────────┐            ┌──────────────────────────────┐
│ RemoteSmith (RustDesk fork)  │◀──LAN─────▶│ RemoteSmith (RustDesk fork)  │
│  client UI (Flutter)         │ direct TCP │  client UI (Flutter)         │
│  host service (LaunchDaemon) │   21118    │  host service (Win service)  │
│  [v3] MCP patch 127.0.0.1    │            │  [v3] MCP patch 127.0.0.1    │
│        ▲ versioned IPC       │            │        ▲ versioned IPC       │
│ Sidecar (Rust tray)          │            │ Sidecar (Rust tray)          │
│  presence/connect            │            │  presence/connect            │
│  [v2] Deskflow supervisor ───┼──TLS 24800─┼─▶ [v2] Deskflow              │
└──────────────┬───────────────┘            └───────────────┬──────────────┘
               │      off-LAN: rendezvous + hole-punch/relay│
               └──────────▶ Lightsail VM: hbbs + hbbr ◀─────┘
```

**Thin fork (DEC-1, DEC-5).** RustDesk carries every human feature. RemoteSmith's changes are a small patch set, rebased onto each upstream stable release:
1. **Branding.** Rename `APP_NAME` and the bundle and installer metadata, and replace the logo. Then audit every `is_custom_client()` path, because renaming switches on upstream's custom-client behaviour.
2. **No default servers.** Empty or replace `RENDEZVOUS_SERVERS` and `RS_PUB_KEY` in `libs/hbb_common`, so builds never contact rustdesk.com (DEF-2).
3. **Hardened defaults.** Set through default or hard settings:
   - `verification-method=use-permanent-password` and `approve-mode=password`, which disables the temporary password.
   - `direct-server=Y` and `enable-lan-discovery=Y`.
4. **Key-translation contract (DEC-22).** Applied wherever upstream mapping falls short.
5. **Peer identity check (DEF-3)** and **LAN-first timeout (DEF-8)**, if upstream doesn't already behave this way.
6. **A small versioned local IPC endpoint (DEF-4).** It exposes peers, session state, connect requests and the inbound-session state the sidecar needs.
7. **[v3]** Upstream PR #16470 (the built-in MCP server), carried as one patch and dropped once it merges upstream (DEC-18).

Every patch should be offered upstream when it is generally useful.

**Sidecar (DEC-20).** A Rust tray/menu-bar binary built on `tray-icon`, `muda` and `tao`. It owns only its extras: presence, connect shortcuts, and (v2) Deskflow supervision.
- It reads connection state over the fork's IPC and never parses upstream config files.
- Credentials stay with the fork.
- On a version mismatch it shows "unknown" and degrades gracefully (DEF-4).

**Server (DEC-9).** rustdesk-server OSS 1.1.16 running in Docker on the shared Lightsail VM, deployed with the `aws-vm` skill.
- Firewall: TCP 21115–21117 and UDP 21116 only (DEF-14). TCP 21114 is Pro-only and 21118–21119 are the WebSocket ports; leave all three closed.
- hbbr runs with `-k _` so the relay also enforces the key. hbbs enforces it by default.
- DNS: a subdomain on an existing domain, set as a Cloudflare DNS-only record (not proxied).
- Back up `id_ed25519` and `id_ed25519.pub`.

**Desk mode (v2; DEC-16, DEC-23).** Deskflow is bundled, re-signed and notarized with Scott's Developer ID, and its source is linked to satisfy GPL-2.0.
- The sidecar writes `Deskflow.conf`: the layout, the primary machine as the Deskflow server, the modifier remap that matches DEC-22, and TLS with fingerprint checking.
- It runs `deskflow-core` as a supervised child process.
- Deskflow is **suspended whenever an inbound remote session is active on either machine.** Input that RustDesk injects on a Deskflow server is otherwise treated as real input and can trigger screen switches.
- If the sidecar dies, Deskflow stops too: fail closed (DEF-18).
- If the sidecar is alive but session state is **unknown** (IPC down, stale, or a version mismatch), Deskflow is also suspended.
- Every suspension first releases any keys and buttons Deskflow is holding, including mid-drag.
- **Setup prerequisites (FLOW-22):**
  - macOS: Accessibility must be granted to both `Deskflow.app` and the `deskflow` core binary, plus Local Network permission. The readiness check reports each one, and desk mode stays off with a named reason when any is missing.
  - Windows: Deskflow runs as the `deskflow-daemon` service with elevate on, so it works at the login screen and on UAC prompts. The sidecar controls it through the daemon instead of spawning the core directly, and supervision watches the daemon's core process.
  - Deskflow's own login-screen support is a bonus only. Desk mode's acceptance criteria cover logged-in desktops.

## 5. Key decisions

| ID | Decision | Alternatives rejected | Why |
|----|----------|----------------------|-----|
| DEC-1 | Build on a RustDesk fork | From scratch; a wrapper over RDP and VNC | It already covers every must-have on both OSes; AGPL suits open-sourcing |
| DEC-2 | Human-first v1, agents in v3 | Agents first; both at once | It has to be a good remote desktop first |
| DEC-3 | Success means a daily driver | "Works reliably"; "agent-ready foundation" | It's a self-set bar with no incumbent tool to replace |
| DEC-4 | Dev/office workload; text clarity over fps | Gaming; quick admin | Matches Scott's work |
| DEC-5 | Thin fork plus sidecar; human features never depend on the sidecar | Deep fork; stock RustDesk plus sidecar; only basic control survives a sidecar failure | Easy rebases; a sidecar fault can't lock Scott out |
| DEC-6 / DEC-22 | Cmd↔Ctrl translation (§6.1); raw toggle; escape hotkey; app switching goes to the remote while input is captured | Raw keys; per-app profiles; local-only Cmd+Tab; translating only listed shortcuts | Muscle memory works on both sides, and a terminal Ctrl+C still interrupts |
| DEC-7 | Native 1:1 by default, with scale-to-fit; never change the host's layout | Match host resolution to the client window; always use a virtual display | Sharp text, and the physical desk stays untouched |
| DEC-8 | Reachable in any OS state; no pre-boot unlock on either machine | Logged-in only | Unattended recovery |
| DEC-9 | Self-hosted hbbs/hbbr on Lightsail | Tailscale; both | Scott's choice; no third-party VPN |
| DEC-10 | RustDesk default auth (ID plus permanent password), hardened (DEF-13/14) | Paired devices only; approve each session | Keeps the fork thin; per-session approval breaks unattended use |
| DEC-11 | Direct IP plus LAN discovery first, Lightsail as fallback; DHCP reservations | Always route through Lightsail | Lower LAN latency, and no dependency on the VM at home |
| DEC-12 | v1 = core plus presence tray; v2 = desk mode; v3 = agent MCP | Everything in v1; core only | Fastest route to a daily driver |
| DEC-13 | Wake-on-LAN cut | Keep the PC awake with LAN-only WoL; a router or always-on relay; the Mac as relay | Both machines are always on |
| DEC-14 | Developer ID signing and notarization for every Mac build | Self-signed; decide later | TCC permissions survive upgrades; clean installs for open-source users |
| DEC-15 | Public repo from day one; renamed; no personal infrastructure committed | Private until ready | AGPL-friendly |
| DEC-16 | Desk mode by bundling Deskflow | Build it into the sidecar | Mature, active project with TLS and modifier remapping |
| DEC-17 | Watch & notify cut | Pixel, OCR, window-event or vision triggers | Scott's call |
| DEC-17a / DEC-24 | Explicit Take over button. Exclusive ownership applies only between agent and remote controller; physical input at the host always works | Human input wins instantly; shared input; locking local input during agent control | Matches the PR #16470 model; it's Scott's own machine |
| DEC-18 | MCP by carrying upstream PR #16470 as a patch. If it can't be rebased, the fallback is to reimplement an equivalent MCP *inside the fork*, keeping the same tools and ownership model | A sidecar-local MCP; deciding at v3 | The plugin framework was removed upstream (PR #15854), so a sidecar can't reach capture or input |
| DEC-19 | Remote app windows cut | Region crop; true per-window capture | Scott's call |
| DEC-20 | Sidecar is a Rust tray app | Tauri; native Swift plus C# | Same language as the fork; one codebase |
| DEC-23 | Desk mode has one primary input machine, chosen in the tray | Whichever input is touched drives | This is what Deskflow natively supports |

## 6. Behaviour details

### 6.1 Key mapping contract (DEC-22)

Rules for a captured session with translation on:

| Mac → Windows | Windows → Mac |
|---|---|
| Cmd → Ctrl | Ctrl → Cmd |
| Ctrl → Ctrl | Win → Ctrl |
| Option → Alt | Alt → Option |
| Cmd+Tab → Alt+Tab | Alt+Tab → Cmd+Tab |
| Win key: session menu or raw mode | — |

- Result: Mac Ctrl+C interrupts a process on Windows. On the Mac, Win+C from Windows interrupts and Ctrl+C copies.
- Raw mode sends physical keys unchanged.
- **Escape hotkey:** **Ctrl+Option+Cmd+Esc** on a Mac controller, **Ctrl+Alt+Win+Esc** on a Windows controller (the same physical keys).
  - It is always consumed locally and never forwarded, in both translated and raw mode, and it takes precedence over all mapping.
  - It releases capture so Scott can switch local apps. The session stays connected.
- All held modifiers are released, on the host and locally, whenever capture or the mapping mode changes.

### 6.2 Presence states (DEF-17)

- **unknown:** the sidecar can't reach the fork's IPC.
- **checking**
- **reachable:** shown with the route, LAN or ID.
- **unavailable:** shown with a reason (offline with last-seen time, auth failed, host missing permissions).
- Connect stays available and shows the reason when an attempt fails.
- An empty peer list shows onboarding instead.

### 6.2a Host-side session indicator

While any inbound session is active, human or agent, the host shows RustDesk's connection window or indicator plus a tray badge. The badge names the controlling peer and whether an agent holds control. It clears when the last session ends (AC-60).

### 6.3 Readiness check (DEF-1)

The installer and tray report each capability separately:
- each macOS permission;
- the service running;
- the power setting (never sleeps);
- no pre-boot unlock (ASM-1);
- server registration.

"Unattended ready" is shown only after a real cold-boot-to-login-screen connection succeeds. The check never changes security settings by itself.

## 7. Edge cases and failure handling

- **Peer identity (DEF-3).** A peer is identified by its stable RustDesk ID plus the host key fingerprint. IP addresses are only hints. On a mismatch the client stops before sending the saved password (AC-37).
- **Peer enrollment, trust on first use.** The first connection to a new peer shows its ID and key fingerprint. Scott checks them against the host's own tray, which displays the same values, then confirms. Only then are the fingerprint and credential saved.
  - **Replacing a host** (reinstall or new machine): Scott chooses "Forget peer" and enrolls the new one. No automatic re-pairing happens (AC-43).
- **LAN-first fallback (DEF-8).** The LAN attempt gets 3 seconds, then the ID route is tried. Cancel aborts every attempt in flight, and only one session is accepted (AC-38).
- **Reconnection (DEF-7):**
  - When the connection drops, the host releases all injected keys and buttons, and stale queued input is dropped.
  - Transient failures auto-retry for up to 60 seconds, with a Cancel button.
  - Auth failures are never retried, and a session Scott closed is never reopened (AC-24, AC-41).
- **Display topology (DEF-5).** When a display is added or removed mid-session, the active pointer is released and the view moves to a remaining display. With no displays, an explicit state is shown. Coordinates map from source pixels, including negative offsets and mixed DPI (AC-15, AC-16).
- **File transfer (DEF-6):**
  - **Existing files:** never silently overwritten. Scott is prompted to overwrite, skip, keep both or apply to all.
  - **Case collisions:** a name that differs only by case from something already at the destination counts as a conflict and is prompted, because Mac and Windows filesystems are case-insensitive by default.
  - **Unsupported names** (characters invalid on Windows, reserved names like `CON`, over-long paths): skipped with a per-item reason. Nothing is renamed silently.
  - **Symlinks:** not followed. A symlink is reported as "skipped (symlink)".
  - **Staging:** each file goes to a hidden temp file at the destination and is renamed once its size and hash match. Stale staging files from a crash are deleted when the next transfer to that folder starts.
  - Zero-byte files and empty folders are kept.
  - Results are reported per item.
  - Disk full, a disconnect or an app crash leaves completed files in place and lists the incomplete ones. A restart is offered (AC-18, AC-48).
- **Mutual control (DEF-9).** Mac→Windows and Windows→Mac can run at the same time; the client shows a one-line "mutual control active" notice.
  - The client refuses to open a nested session that would end up viewing itself.
  - **Clipboard:** echoes are removed by tracking origin plus a content hash. Each change carries a Lamport counter and the origin peer ID.
    - The highest counter wins; on a tie, the lower peer ID wins.
    - Wall clocks aren't used, so clock skew can't matter. Both sides end up with the same value.
  - **Audio:** played-back remote audio is never re-captured. When both directions are active, the peer with the lower ID decides which direction carries audio: the session whose window most recently gained focus, as reported to it. Its decision is authoritative, so at most one direction is ever active.
  - **Connections:** duplicate connect requests to the same peer are coalesced into one session.
- **Clipboard limits (DEF-10).** The RemoteSmith contract:
  - Text up to 1 MB and images up to 20 MB sync within 1 s on the LAN.
  - Anything larger isn't synced; a notice says "too large — use file transfer".
  - Files copy through upstream's file-clipboard path.
  - These caps are mandatory. If upstream's limits at the pinned version are tighter, a fork patch raises them.
  - A file transfer must not stall interactive typing (AC-14).
- **Sidecar failure (DEF-4, DEF-18).** Human features carry on (AC-25). Deskflow stops within 5 seconds (AC-36).
- **Lightsail down.** The LAN still works (AC-8). Off-LAN connections fail with "server unreachable" within 10 s, not a hang (AC-45).
- **Reconnect expiry.** After 60 s of failed retries, or when Scott presses Cancel, the session window shows "Disconnected — Reconnect". Queued input is discarded and nothing is replayed (AC-46).
- **Agent control lifecycle (v3).** Control is bound to one authenticated connection and one grant ID.
  - The grant ends on release, Take over, disconnect or reconnect, when Scott denies the approval prompt, and when MCP is disabled or its token rotated. Disabling MCP or rotating the token leaves the desktop session connected.
  - **Every** grant-ending transition cancels queued agent actions, interrupts gestures in progress, and releases held keys and buttons.
  - After the grant ends, every queued or new agent input is rejected with "control lost". The agent has to call `request_control` again and get a fresh approval.
  - Only one agent grant can exist per connection; a second request gets "busy".
  - Switching the target session never carries a grant across.
  - Nothing is replayed after reconnecting (AC-32, AC-49, AC-50).
- **Missing macOS permission.** An explicit error names the permission, rather than showing a black screen (AC-2). Upstream issue #16402 (black frames on macOS 15+) is covered by upstream's fallback capturer (#16464), which we verify.
- **Upgrades (DEF-12).**
  - **Config:** versioned, with the schema version stored. Before migrating, the installer backs up the full persistent state (ID, keys, peers, credentials, options, sidecar config) to a timestamped folder.
  - **During sessions:** the installer won't upgrade during an active inbound session or transfer unless forced. Forcing ends the transfer and reports it as incomplete.
  - **Rollback:** reinstall the previous kept installer, then restore the backup. A newer schema is never loaded by an older build; it refuses and offers the restore.
  - **Fork–sidecar compatibility:** they declare a compatible IPC version range. Outside that range the sidecar shows "unknown" and suspends Deskflow (AC-25, AC-47).
  - **Server:** before upgrading hbbs/hbbr, back up `data/` (the keys). Pin the image tag rather than `latest`, and keep the previous tag for rollback. After the upgrade, existing clients register without being re-keyed (AC-44).
- **Port collisions.** The direct port 21118 and MCP port 21119 match hbbs/hbbr's WebSocket ports. That only matters if the server and a client share a host; it never happens in this setup, but it's documented for open-source users.

## 8. Entity lifecycle

| Entity | Owner / store | Create | Update | Delete | Notes |
|---|---|---|---|---|---|
| Host config (password, server host and key, direct-server, options) | Fork; upstream's encrypted config | First run (FLOW-2) | Settings | Uninstall, unless kept | Changing the password ends existing sessions (AC-39) |
| Peer record (ID, key fingerprint, LAN hint, alias) | Fork | Saving a peer | On each connect (LAN hint) | Forget peer, which also deletes the stored credential | IPs are hints only |
| Saved credential | Fork; OS-protected store (Keychain or Credential Manager, or upstream's encrypted store) | Saving a peer | Password change | Forget peer | Never logged (AC-42) |
| Sidecar config (autostart, Deskflow layout and primary) | Sidecar | First run | Tray settings | Uninstall | No credentials |
| Deskflow config, TLS cert and fingerprints | Sidecar-generated (v2) | Enabling desk mode | Layout change | Disabling desk mode | Fingerprints confirmed once |
| Session (displays, input mode, transfers) | Fork; in memory | Connect | — | Disconnect | Held input released on teardown |
| MCP token (v3) | Fork; OS-protected store (Keychain or Credential Manager) | Enabling MCP | "Rotate token" button (the old token stops working immediately) | Disabling MCP deletes it | Loopback only, never logged (AC-31, AC-42) |
| Server keys (`id_ed25519`) | Lightsail volume | First start | Never (rotating means re-keying clients) | — | Backed up |

## 9. Security

- **Advisories, by component.** Rebase promptly when new ones appear.
  - **Client:** CVE-2026-57850 (a restricted session could send full-control messages; fixed in 1.4.9) and CVE-2026-2490 (Windows file-transfer link-following). The base is ≥1.5.0, which includes both fixes.
  - **March 2026 batch** (CVE-2026-30783/-30789/-30790/-30792/-30793/-30794/-30796/-30797): mostly Pro server, policy/config sync, address book and TLS handling. Most don't apply to the OSS server with no Pro console. Sources disagree on fixed versions (1.4.6 vs "through 1.4.8"), so before v1 ships, confirm each fix against 1.5.x source or the release notes.
  - **Server:** OSS 1.1.16 fixes the UDP punch-hole amplification issue.
- Use a long random permanent password stored in Scott's password manager. Keep the temporary password disabled, and rely on RustDesk's backoff after failed attempts (AC-10).
- Server key enforcement on both hbbs and hbbr (AC-5). Only the needed ports are open.
- Direct port 21118 listens only on LAN interfaces (not utun or other VPN interfaces). An OS firewall rule also limits it to the local subnet (DEF-14, AC-51).
- Deskflow (v2): TLS on, with peer fingerprint checking. An unknown fingerprint is rejected (AC-53).
- Logs hold redacted session IDs, route changes, failure reasons and component versions. They never hold passwords, tokens, clipboard contents or screenshots (DEF-13, DEF-16).
- The public repo contains nothing personal. Server host and key exist only in local config (AC-33, AC-40).
- v3 MCP is off by default, loopback-only, needs a bearer token, and needs a local click to grant control. Auto-approve stays off (AC-31).

## 10. Tech stack (verified 2026-10-12)

| Component | Version | Notes |
|---|---|---|
| RustDesk (upstream base) | 1.5.0 (2026-09-30); 1.5.1 imminent | AGPL-3.0. Follow `.github/workflows/flutter-build.yml`, not the outdated docs |
| Toolchain pins (from upstream CI) | Flutter 3.24.5; Rust 1.75 (1.81 on macOS); LLVM 15.0.6; pinned vcpkg commit; flutter_rust_bridge 1.80 | Pins must match exactly |
| Mac build flags | `--flutter --hwcodec --unix-file-copy-paste --screencapturekit` | ScreenCaptureKit captures system audio on Apple Silicon |
| Windows build flags | `--flutter --hwcodec` | MSVC, LLVM with `LIBCLANG_PATH`, x64-windows-static triplet |
| rustdesk-server (hbbs/hbbr) | 1.1.16 (2026-07-20) | Docker, `network_mode: host` |
| Deskflow (v2) | 1.27.0 (2026-10-01) | GPL-2.0. macOS 26 bugs open (#10068, #10136, #10137, #9451); pin and re-test |
| Upstream PR #16470 (v3) | Open; head `e11601e15` at time of research | +5091/−85; no protocol change; Rust 1.75 |
| Sidecar crates | tray-icon 0.26.1, muda 0.21.2, tao 0.37.1 | macOS tray must run on the main thread |
| Targets | macOS 26+ on Apple Silicon; Windows 11 x64 | |
| Signing | Apple Developer ID plus notarization (Mac); Windows unsigned for personal v1 | Windows signing is an open question for public releases |

## 11. Phases and sequencing

**v1: daily driver**
1. Fork setup. Create the public GitHub repo `RemoteSmith` and add RustDesk as a submodule or fork at 1.5.x. Reproduce upstream's CI build on macOS (Apple Silicon) and Windows *before* changing anything. This is the gate for everything after it.
2. Branding and the no-default-servers patches (patches 1–2), plus the `is_custom_client()` audit.
3. Mac signing and notarization in CI with the Developer ID (DEC-14).
4. Deploy hbbs/hbbr on Lightsail (FLOW-3), confirm the key is enforced, and set up DNS.
5. Hardened defaults and first-run provisioning (patch 3, FLOW-2, DEF-2).
6. Key-mapping contract (patch 4). Peer identity check and LAN timeout (patch 5).
7. IPC endpoint (patch 6), then the sidecar presence tray (FLOW-13).
8. Readiness check (§6.3). Reboot and lock recovery tests (FLOW-15).
9. Full acceptance matrix in both directions (§13), then the two-day daily-driver trial (AC-34).

**v2: desk mode.** Depends on the v1 IPC (inbound-session state) and the sidecar.
- Desk-mode setup and readiness (FLOW-22): bundle, re-sign and notarize Deskflow; permission checks; the Windows elevated daemon.
- Write its config, supervise it, and suspend it during inbound sessions or when session state is unknown (FLOW-10).

**v3: agent control.** Depends on v1.
- Rebase PR #16470 onto the fork, or drop the patch if it has merged upstream.
- Wire Claude Code to the MCP endpoint and verify Take over (FLOW-20).

**Ongoing:** rebase on every upstream stable release (FLOW-18). The release gate is the full applicable AC matrix (DEF-15).

## 12. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Upstream builds are fragile (strict version pins) | Reproduce upstream CI before touching anything; pin toolchains in our own CI |
| Rebase conflicts grow over time | Keep patches small and isolated; offer them upstream; log rebase effort per release |
| macOS TCC permissions or capture regressions on new macOS versions (#16402 black frames) | Developer ID signing; readiness check; test after every macOS update |
| PR #16470 is rejected or changes shape | Carry it as a patch regardless. If it can't be rebased, reimplement an equivalent MCP inside the fork (ASM-5, DEC-18) |
| Deskflow instability on macOS 26 | Pin a known-good version; it's a v2 extra that fails closed |
| RustDesk-injected input interacting with the Deskflow server | Suspend Deskflow during inbound sessions (DEF-18, AC-28) |
| Relay bandwidth on the shared VM | Relay only carries off-LAN sessions (~100 KB/s for office work); watch the Lightsail transfer allowance (ASM-4) |
| `is_custom_client()` paths behave unexpectedly after the rename | Audit all 19 call sites in step 2 |

## 13. Acceptance criteria

**Setup (FLOW-1/2)**
- **AC-1** Given a fresh Mac (macOS 26+, Apple Silicon) and Windows 11 PC, when RemoteSmith is installed from the signed and notarized release and the guided permission steps are done, then:
  - the host service runs on both machines and survives a reboot;
  - each machine shows the other as a saved peer.
- **AC-2** Given a missing macOS permission, when a session to the Mac is attempted, then the Mac's tray names the missing permission and the controller sees an explicit error, not a black screen.
- **AC-3** Given an upgrade signed with the same Developer ID, when it is installed, then no macOS permission needs to be granted again.
- **AC-40** Given a fresh public build, when it first launches, then:
  - no rendezvous server is configured;
  - it asks for the server host and key, or offers direct-LAN only;
  - it never contacts rustdesk.com servers.

**Server (FLOW-3)**
- **AC-4** Given hbbs/hbbr 1.1.16 with key enforcement, when a client with the right host and key registers, then it gets an ID and shows as online.
- **AC-5** Given a client with the wrong key or no key, when it tries to register or relay, then it is refused (both hbbs and hbbr).
- **AC-6** Given RemoteSmith clients running, then they never contact RustDesk's public servers.

**Sessions and routes (FLOW-4/5/17)**
- **AC-7** Given both machines on the LAN, when Scott connects from either side, then the direct LAN route is used and the client shows "LAN".
- **AC-8** Given the Lightsail VM is down, when Scott connects on the LAN, then the session succeeds.
- **AC-9** Given the controller is off the LAN, when Scott connects to the home machine, then it goes through Lightsail and the client shows "P2P" or "Relay". With hole-punching blocked (UDP 21116 to the peer blocked or `force-always-relay=Y`), the session succeeds over **Relay** in both directions.
- **AC-45** Given the Lightsail VM is down and the controller is off the LAN, when Scott connects, then "server unreachable" appears within 10 s.
- **AC-46** Given a reconnect in progress, when Scott presses Cancel, or 60 s of retries fail, then the window shows "Disconnected — Reconnect", queued input is discarded, and nothing is replayed after a later manual reconnect.
- **AC-58** Given a connect attempt:
  - Cancel during it, followed by late LAN or ID results, opens no session.
  - When the LAN and ID routes both succeed, exactly one session opens.
  - Clicking Connect five times quickly opens one session.
  - An attempt to open a session that would view itself is refused with a reason.
  - Opposing sessions in both directions show the "mutual control active" notice.
- **AC-43** Given a first connection to a new peer, then the client shows the peer's ID and key fingerprint, and saves the credential only after Scott confirms it against the host's tray. Given "Forget peer" followed by re-enrollment, the new fingerprint is accepted.
- **AC-51** Given a connection attempt to direct port 21118 from outside the local subnet, or over a non-LAN interface, then it is refused.
- **AC-10** Given a wrong password, when a connection is attempted, then it is refused and backoff applies. The temporary password is not accepted.
- **AC-38** Given a stale LAN address but a working Lightsail route, then the LAN attempt gives up within 3 seconds and the ID route connects.
- **AC-37** Given a saved peer whose LAN IP now belongs to a different machine, then the client detects the ID or key mismatch and stops before sending the saved password.

**Keyboard (DEC-22)**
- **AC-11** Mac→Windows: Cmd+C and Cmd+V copy and paste on Windows. Windows→Mac: Ctrl+C reaches the Mac as Cmd+C.
- **AC-12** In raw mode, Cmd+C on the Mac reaches Windows as Win+C.
- **AC-13** Ctrl+Option+Cmd+Esc on a Mac controller, or Ctrl+Alt+Win+Esc on a Windows controller, releases local input capture immediately in both translated and raw mode. The chord never reaches the host, no modifier stays held on the host, and the session stays connected.
- **AC-59** Toggling between translated and raw mode, or gaining or releasing capture, while Cmd, Ctrl, Option or Shift is held leaves no modifier held on either machine.
- **AC-35** In a captured Mac→Windows session:
  - Cmd+Tab switches Windows apps.
  - Ctrl+C in a remote terminal interrupts the running process.

  Windows→Mac: Win+C in a Mac terminal interrupts, and Ctrl+C copies.

**Quality and display (FLOW-6)**
- **AC-14** On a LAN 1:1 session to a 4K display, 12pt editor text is legible with no artifacts within 1 s of stillness.
  - **Measurement:** a 240 fps camera films the controller's keyboard and screen together, and latency is counted from key-down to the glyph appearing, over 100 keystrokes.
  - **Recorded per run:** codec (default H.265 hardware when available, otherwise VP9), image quality preset, fps cap, and transfer throttling. File transfer runs at lower priority than interactive input and video, with a throttle cap (default 80% of link speed).
  - **Relay sessions** are tracked separately. The target there is usability (p95 under 200 ms), not the LAN thresholds.
  - **Pass:** p95 under 50 ms on an idle LAN, and p95 under 100 ms on the LAN while a 2 GB file transfer runs.
- **AC-15** Switching displays or choosing "all" needs no reconnect. The host's monitor arrangement is unchanged after the session. When a display is removed mid-session, the view falls back to a remaining one and input is released.
- **AC-52** Given a host with mixed-DPI monitors and one at negative coordinates (left of or above the primary), when Scott clicks a 10 px target on each, then the click lands on the target. Given a host whose displays all go away (for example, every monitor unplugged), the client shows "No display available" instead of a frozen frame.
- **AC-16** Scale-to-fit shows the whole remote display. With it off, the view is 1:1 with scrolling.

**Clipboard, files and audio (FLOW-7/8/9)**
- **AC-17** On the LAN, text up to 1 MB or an image up to 20 MB copied on one side can be pasted on the other within 1 s. A larger item shows "too large — use file transfer" and is not synced. Files copied in Finder or Explorer can be pasted across. With sessions running in both directions, there is no clipboard echo loop, and simultaneous copies leave the same last-written value on both sides.
- **AC-18** File and folder transfers arrive byte-identical, with progress and cancel. Existing files are never silently overwritten, and cancelling or failing never leaves a partial file reported as complete. Against the §14 folder fixture:
  - the zero-byte file and the empty folder arrive;
  - the symlink is reported "skipped (symlink)";
  - the name collision and the case collision each trigger the conflict prompt;
  - the Windows-invalid name is skipped with a reason.
- **AC-48** Given a folder transfer interrupted by a full destination disk, or by a network disconnect, then completed files stay intact, incomplete items are listed, and Restart completes the rest. Given the app crashes mid-transfer, when the next transfer to that folder starts, then stale staging files are removed, completed files are intact, and the incomplete ones are listed for restart.
- **AC-54** Given sessions in both directions with audio on, including rapid competing focus changes on both machines, then at most one audio direction is active at any moment and there's no feedback loop.
- **AC-57** Given simultaneous copies on both machines, including with their clocks skewed by 5 minutes, then both clipboards converge on the same value.
- **AC-19** Host audio is heard on the controller, and mute stops it.

**Presence (FLOW-13)**
- **AC-20** With both machines running, the tray shows the other as reachable with its route, and one click opens a session.
- **AC-21** When the other machine is unreachable, the tray shows "unavailable" with a reason and last-seen time. Connect reports why it can't connect.

**Recovery (FLOW-15/16)**
- **AC-22** After an unattended reboot to the login screen, Scott can connect, log in and continue, on both Mac and Windows.
- **AC-23** When the host is locked, Scott sees the lock screen and can unlock it with the OS password.
- **AC-24** After a network drop of up to 60 seconds, the session reconnects automatically without the password being entered again. A session Scott closed himself does not reconnect.
- **AC-41** When a session drops while a key is held, the host releases all injected keys and buttons immediately.
- **AC-60** During an inbound session (human, or agent once v3 exists), the host shows the connection indicator and a tray badge naming the peer, plus "agent" when an agent holds control. Both clear within 2 s of the last session ending.
- **AC-25** With the sidecar killed or at a mismatched version, every human feature works from the client: screen, input, clipboard, files, audio, multi-monitor.
- **AC-55** Given a prerequisite fails (a missing permission, sleep enabled, or a pre-boot unlock detected), the readiness check names it and does not show "Unattended ready". "Unattended ready" appears only after a cold-boot-to-login-screen connection has succeeded.

**Upgrades**
- **AC-44** Given a hbbs/hbbr upgrade to a new pinned tag, then existing clients register without re-keying. Rolling back to the previous tag also works.
- **AC-47** Given a client upgrade:
  - after it, the ID, keys, peers, saved credentials and connectivity are unchanged;
  - with an older build reinstalled plus the backup restored, everything works as before the upgrade;
  - an older build that finds a newer config schema refuses to load it and offers the restore;
  - during an inbound session or a transfer, an ordinary upgrade is deferred with a message;
  - a forced upgrade ends the session, reports the transfer as incomplete, and leaves completed files intact.

**Lifecycle (FLOW-18/19/21)**
- **AC-26** After rebasing on a new upstream stable release:
  - CI produces a signed and notarized Mac build and a Windows build;
  - the full applicable AC matrix passes in both directions;
  - rebase effort is logged.
- **AC-39** Changing the host's password ends existing sessions, and the controller's saved credential stops working until it's updated. Forgetting a peer deletes its stored credential. Uninstalling removes services, listeners and autostart.
- **AC-33** The public repo has the AGPL license, upstream notices, the new name and logo, and build instructions that reproduce CI. It contains no personal hostnames, keys or passwords.
- **AC-42** No log contains passwords, MCP tokens, clipboard contents or screenshots.

**Desk mode, v2 (FLOW-10)**
- **AC-27** With desk mode on, the Mac as primary and the PC to its right: moving the mouse off the Mac's right edge drives Windows, with Mac keys translated per DEC-22. Moving back returns control.
- **AC-28** While an inbound remote session is active on either machine, desk mode is suspended. It resumes only when no inbound sessions remain and it is still enabled.
- **AC-29** If Deskflow dies, the tray shows desk mode off with an error, and remote desktop is unaffected.
- **AC-36** If the sidecar crashes while desk mode is on, Deskflow stops within 5 seconds. If the IPC is down or the versions are mismatched while the sidecar runs, Deskflow is suspended. Suspension during a drag or while a modifier is held leaves no key or button stuck on either machine.
- **AC-53** A Deskflow peer with an unknown TLS fingerprint is refused. With a macOS Deskflow permission missing, desk mode stays off and names the permission.
- **AC-56** Desk mode works with either machine chosen as primary. On Windows it keeps working across a UAC prompt on the logged-in desktop.

**Agent control, v3 (FLOW-20)**
- **AC-30** With MCP enabled and a valid token, Claude Code can:
  - list peers and connect;
  - request control (approved with a local click);
  - screenshot, click and type on the remote machine.

  A visible agent banner shows on the controller.
- **AC-31** With MCP disabled (the default), a wrong token, or a non-loopback origin, the request is refused.
- **AC-32** When Scott clicks Take over:
  - queued agent actions are cancelled and held keys released;
  - the agent gets a "control lost" result;
  - only Scott's input and the host's physical input reach the host.

  To act again, the agent must request control fresh.
- **AC-49** Given an agent grant, when the agent calls `release_control` (including mid-drag or while holding a modifier), Scott denies the approval prompt, or the connection drops and reconnects, then the grant ends and no key or button stays held on the host. Further agent input returns "control lost" until a fresh, approved `request_control`, and no earlier commands are replayed.
- **AC-50** During an active agent grant, the controller's ordinary input is suppressed (the banner offers Take over), while the host's own physical keyboard and mouse still work. A second `request_control` on the same connection gets "busy". Switching the agent's target session never carries the grant across.
- **AC-31b** Rotating the MCP token makes the old one fail immediately. Disabling MCP deletes the token from the OS credential store. Disabling MCP or rotating the token during an agent drag, or while it holds a modifier, ends the grant and releases all held input. Queued actions never run, the desktop session stays connected, and once MCP is re-enabled a fresh approved `request_control` is required.

**Daily-driver success (DEC-3)**
- **AC-34** With v1 on both machines, Scott works one full day through RemoteSmith in each direction. Any time he has to fall back to the physical machine is logged as a v1 bug, and v1 is done when a full day in each direction needs no fallback.

## 14. Validation strategy

- **Upstream tests.** Run RustDesk's own Rust and Flutter tests in CI on every build.
- **Patch tests.** Unit tests for each RemoteSmith patch:
  - the key-mapping table (AC-11/12/35);
  - the peer identity check (AC-37);
  - the LAN timeout (AC-38);
  - the IPC contract and version mismatch (AC-25).
- **Sidecar tests.** Unit tests for presence state transitions and the Deskflow supervisor (fail closed, suspend and resume), using a fake IPC.
- **Two-machine end-to-end runs.** The real Mac and PC, both directions, following a written checklist that maps one-to-one to the ACs in §13. Results are recorded per release in `docs/test-runs/`.
- **Fixtures:**
  - a 4K editor screen with 12pt text (AC-14);
  - clipboard items of 1 MB of text, a 20 MB image, and a 25 MB oversize image (AC-17);
  - a folder containing a zero-byte file, an empty directory, a symlink, an existing-name collision, a case-only collision and a Windows-invalid name; the expected outcomes are in AC-18;
  - a 2 GB file transfer while typing (AC-14, p95 under 100 ms);
  - a mixed-DPI, negative-offset monitor layout (AC-52).
- **Fault injection:**
  - stop the Lightsail containers (AC-8, AC-45);
  - force relay (AC-9);
  - Cancel during reconnect, and let retries expire (AC-46);
  - crash the app mid-transfer (AC-48);
  - drop the IPC or mismatch versions with desk mode on (AC-36);
  - kill the sidecar (AC-25, AC-36);
  - pull the network for 30 s and for 90 s (AC-24);
  - unattended reboot (AC-22);
  - revoke a macOS permission (AC-2);
  - wrong key or password (AC-5, AC-10).
- **Observability.** Logs record session IDs, route (LAN, P2P or Relay), failure reasons and component versions. On Lightsail, `docker logs` for hbbs and hbbr, plus the transfer metric.
- **Daily-driver trial.** AC-34, logged in `docs/test-runs/daily-driver.md`.

## 15. Assumptions register

| ID | Assumption | Confidence | If wrong | Verify |
|---|---|---|---|---|
| ASM-1 | Neither machine has pre-boot unlock (FileVault or BitLocker PIN) | High (stated) | Unattended reboot recovery fails | Readiness check at install |
| ASM-2 | Both machines are always on | High (stated) | Peer shows offline; there's no wake path | Power settings in the readiness check |
| ASM-3 | The Mac is Apple Silicon on macOS 26+ | High | System audio capture needs BlackHole | Install check |
| ASM-4 | The Lightsail VM has spare ports and bandwidth | Medium | Relay is laggy or blocked | `aws-vm` survey before FLOW-3 |
| ASM-5 | PR #16470 can still be rebased at v3 | Medium | Reimplement an equivalent MCP inside the fork | Check its status at the start of v3 |
| ASM-6 | Scott's Apple Developer ID can sign and notarize | High (stated) | Permissions reset on every upgrade | Find the cert in Keychain at step 3 |
| ASM-7 | Upstream's fallback capturer fixes black frames on macOS 26 | Medium | Mac hosting is broken | AC-1/AC-14 on the real Mac |

## 16. Open questions

- **OQ-1** Windows code signing for public releases (for example Azure Trusted Signing). It isn't needed for personal v1, so decide before announcing the open-source release.
- **OQ-2** Which existing domain to use for the server's DNS name. Pick one during FLOW-3.
- **OQ-3** Whether to ship Deskflow inside the installer or have the sidecar download a pinned, verified build on first use. Decide at the start of v2; both satisfy DEC-16.
