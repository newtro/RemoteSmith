# RemoteSmith — Implementation Guide

This is the working contract for everyone building v1, human or agent. What and why live in [PLAN.md](PLAN.md). This file covers where the code goes and how the pieces fit.

## Repo layout

```
docs/PLAN.md                 product plan (source of truth for behaviour and ACs)
docs/IMPLEMENTATION.md       this file
docs/IPC.md                  fork <-> sidecar local IPC protocol (versioned)
docs/test-runs/              two-machine AC checklist and run logs
vendor/rustdesk/             git submodule, pinned to upstream tag 1.5.0 (fada664df); hbb_common at 229b9045
patches/rustdesk/NNNN-*.patch    RemoteSmith patches for vendor/rustdesk, applied in order
patches/hbb_common/NNNN-*.patch  RemoteSmith patches for vendor/rustdesk/libs/hbb_common
scripts/apply-patches.sh     reset the submodules to their pinned commits and apply patches/ (idempotent)
scripts/export-patches.sh    turn the commits on top of the pinned base back into patches/ files
scripts/build-mac.sh         local macOS arm64 build, the same steps as CI
sidecar/                     Rust tray/menu-bar app (crate `remotesmith-tray`)
deploy/server/               hbbs/hbbr 1.1.16 deployment for the Lightsail VM (systemd, not Docker)
.github/workflows/           CI: apply patches, then build macOS arm64 and Windows x64
```

## Thin-fork patch workflow

- **The submodule never carries RemoteSmith commits upstream.** The patches in `patches/` are the fork.
- **To develop:** run `scripts/apply-patches.sh`, which creates branch `remotesmith` in each submodule with one commit per patch. Commit new work on top inside `vendor/rustdesk` (or `libs/hbb_common`), then run `scripts/export-patches.sh` to regenerate the `.patch` files.
- **Commit messages:** each patch commit's message is `remotesmith: <topic>` and says which plan item it serves (DEC/DEF/AC IDs).
- **Patch number ranges, one per work unit,** so parallel units never collide:

  | Range | Unit |
  |---|---|
  | 0100–0199 | branding, no default servers, hardened defaults, first-run provisioning (DEC-15, DEF-2, DEC-10) |
  | 0200–0299 | key-mapping contract and escape hotkey (DEC-22) |
  | 0300–0399 | peer identity / TOFU, LAN-first timeout, reconnect rules (DEF-3, DEF-7, DEF-8) |
  | 0400–0499 | RemoteSmith IPC endpoint and presence data (DEF-4, DEF-17) |
  | 0500–0599 | session indicator, readiness reporting hooks (§6.2a, §6.3) |
  | 0900–0999 | build and CI fixes needed to build at all (keep these minimal) |
- **Rebasing to a new upstream release:** bump the submodule pin, run `apply-patches.sh`, fix the patches that fail, then run `export-patches.sh`.

## Toolchain (pinned to match upstream CI at 1.5.0)

| Tool | Version |
|---|---|
| Flutter | 3.24.5 |
| Rust | 1.81 on macOS (1.75 on Windows, as upstream) |
| LLVM | 15.0.6 |
| vcpkg | commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`, triplets `arm64-osx` / `x64-windows-static` |
| flutter_rust_bridge | 1.80 |

- **The sidecar has its own toolchain.** The pins above are for `vendor/rustdesk` only. `sidecar/rust-toolchain.toml` pins Rust 1.98.1 (with rustfmt, clippy and the `x86_64-pc-windows-msvc` target), and `sidecar/Cargo.toml` declares `rust-version = "1.90"`, the floor set by tray-icon 0.26 / muda 0.21 (checked with `cargo +1.90.0 check`). `Cargo.lock` is format v4, which both read. rustup selects it automatically for any cargo command run inside `sidecar/`.
- **Mac features:** `--flutter --hwcodec --unix-file-copy-paste --screencapturekit`.
- **Windows features:** `--flutter --hwcodec`.
- **Shared build caches** live on the internal disk under `~/.cache/remotesmith/`: vcpkg installed tree, cargo target, Flutter SDK. `/Volumes/Cache` has limited free space.
- **Local Mac builds are heavy:** run one full build at a time. Parallel work should use `cargo check` against the shared target dir, where cargo's lock serializes them safely.

## Signing (macOS)

There is no local Developer ID cert. Signing follows the iSmith pattern (`/Volumes/Cache/repos/iSmith/Tools/release.sh`):
- `xcodebuild archive` of the Flutter macOS Runner;
- `-exportArchive` with `method=developer-id` and `teamID=232A77467G`, using the cloud-managed cert from the Apple ID signed in to Xcode;
- `-exportNotarizedApp`, then `stapler validate`.

Local dev builds sign with `Apple Development: scott@johnnycode.ai (52XDFWFDBZ)`, so TCC permissions persist across rebuilds. CI produces unsigned Mac artifacts; signing and notarization happen locally in `scripts/release-mac.sh`, because the cloud cert needs the Xcode account.

Bundle id: `ai.johnnycode.remotesmith`. App name: `RemoteSmith`.

## Server

- **Host:** `rs.johnnycode.ai` → Lightsail `vm` (98.93.243.79), Cloudflare DNS-only.
- **Runtime:** hbbs and hbbr 1.1.16 as native binaries under systemd. The VM has no Docker, and RAM is tight, so each unit has `MemoryMax=`.
- **hbbr:** runs with `-k _`.
- **Firewall:** Lightsail firewall plus ufw/iptables allow TCP 21115–21117 and UDP 21116 only.
- **Keys:** live in `/opt/remotesmith-server/data`, which is backed up.

The deviation from PLAN §4 (Docker → systemd) is deliberate. It matches the VM's conventions and saves memory.

## Fork ↔ sidecar IPC (summary; full spec in docs/IPC.md)

- **Transport:**
  - macOS: a Unix socket at `/tmp/RemoteSmith-<uid>/remotesmith-ipc.sock`, inside the per-user 0700 directory the fork already uses for its own IPC; socket mode 0600. Both sides check the peer uid.
  - Windows: a named pipe `\\.\pipe\RemoteSmith-ipc-<username>` with an ACL for the current user only.
- **Framing:** newline-delimited JSON. Every message has `"v": 1`. The server is the fork's per-user UI/server process; the client is the sidecar.
- **Version check:** the sidecar sends `hello` with its supported range, and the fork replies with its version. If the ranges don't overlap, the sidecar shows `unknown` and stops issuing requests (DEF-4).
- **Requests:** `hello`, `get_status`, `list_peers`, `connect {peer_id}`, `subscribe` (pushes `status` / `peers` events).
- **Credentials:** never cross the IPC.
- **Crash isolation:** the fork must keep working if the sidecar never connects, crashes or speaks garbage (AC-25).

## Verification

- `cargo test` and `cargo clippy -- -D warnings` in `sidecar/`.
- `scripts/apply-patches.sh` must apply cleanly onto the pinned submodules.
- `cargo check` of `vendor/rustdesk` with the Mac features (fast gate); `scripts/build-mac.sh` (full gate).
- Two-machine ACs: `docs/test-runs/checklist.md`. The Windows side needs Scott's PC; CI builds the Windows artifact.
