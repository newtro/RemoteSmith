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
scripts/bootstrap-mac.sh     idempotent macOS arm64 toolchain setup into ~/.cache/remotesmith/
scripts/build-mac.sh         local macOS arm64 build, the same steps as CI
scripts/verify.sh            fast verification gate
sidecar/                     Rust tray/menu-bar app (crate `remotesmith-tray`)
deploy/server/               hbbs/hbbr 1.1.16 deployment for the Lightsail VM (systemd, not Docker)
.github/workflows/           CI: apply patches, then build macOS arm64 and Windows x64
```

## Thin-fork patch workflow

- **The submodule never carries RemoteSmith commits upstream.** The patches in `patches/` are the fork.
- **Pins:** `vendor/rustdesk` is pinned by the superproject's gitlink (index), `libs/hbb_common` by the gitlink inside the pinned rustdesk commit. The scripts read both; nothing hardcodes a sha.
- **To develop:** run `scripts/apply-patches.sh`, which resets both submodules to their pins and creates branch `remotesmith` in each with one commit per patch (`git am`, applied in file-name order, committer date = author date so the same patches always give the same shas). Commit new work on top inside `vendor/rustdesk` (or `libs/hbb_common`), then run `scripts/export-patches.sh` to regenerate the `.patch` files.
  - `apply-patches.sh` refuses to run over uncommitted edits or over commits on `remotesmith` whose content (by `git patch-id`) is not in `patches/`; `--force` discards them (it prints the old branch tip so it can be recovered).
  - `apply-patches.sh --check` test-applies everything in throwaway `git worktree`s and leaves the submodules untouched. `REMOTESMITH_PATCHES_DIR=<dir>` points either script's checks at another patch tree with the same `rustdesk/` and `hbb_common/` layout.
  - Never commit a change to the `libs/hbb_common` gitlink inside rustdesk; hbb_common changes go in `patches/hbb_common`. `export-patches.sh` rejects such commits.
- **Commit messages and numbering:** each patch commit's subject is `remotesmith(NNNN): <topic>`, where NNNN is the patch number from your unit's range below; the body says which plan item it serves (DEC/DEF/AC IDs). `export-patches.sh` names the file `NNNN-<topic-slug>.patch`, so re-exporting never renumbers, and `git am` keeps the subject, so the number survives every apply/export round trip. A new commit carries its number from the start; to change a number, reword the commit. Export rejects commits without a number, duplicate numbers and empty commits, test-applies the new set in number order before writing anything, and deletes patch files whose number is no longer on the branch. `--dry-run` shows what would change.
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

- **Mac features:** `--flutter --hwcodec --unix-file-copy-paste --screencapturekit`.
- **LLVM on macOS:** upstream's mac job does not use 15.0.6 (that is Windows/Linux); it `brew install`s llvm, which clang-sys never finds on arm64, so bindgen uses Xcode's libclang. We skip brew llvm.

### Local macOS build

```sh
git submodule update --init --recursive --depth 1
scripts/bootstrap-mac.sh     # idempotent; Flutter 3.24.5 (+3.22.3 for the bridge), Rust 1.81/1.75 toolchains,
                             # NASM 2.16.03, cargo-expand + flutter_rust_bridge_codegen, vcpkg + arm64-osx packages
scripts/build-mac.sh         # -> build/mac/RustDesk.app (+ build/mac/build-*.log)
```

- `scripts/lib/mac-env.sh` holds every version, each with the upstream workflow line it mirrors. Everything lands in `~/.cache/remotesmith/`; your default rustup toolchain is not touched (the build sets `RUSTUP_TOOLCHAIN`).
- `build-mac.sh` takes a machine-wide lock (`~/.cache/remotesmith/build-mac.lock`). `vendor/rustdesk/target` is a symlink to the shared `~/.cache/remotesmith/target-mac`, because build.py and the Xcode project hard-code `../../target/release`.
- The build makes CI's in-place edits (MIN_MACOS_VERSION 12.3 in build.py, Podfile, Cargo.toml, project.pbxproj) and restores them afterwards, along with `Podfile.lock` and the bridge's pubspec edits. It refuses to start if those files have uncommitted changes.
- The flutter_rust_bridge files are regenerated when the rustdesk HEAD or sources change, like CI's `generate-bridge` job (Flutter 3.22.3, codegen 1.80.1). One difference: cargo-expand runs on Rust 1.81 here, because the crate is expanded for macOS, where cidre needs 1.81. `--skip-bridge` reuses the existing files.
- **Differences from the macos-14 runner, all because this Mac has Xcode 27 and no Rosetta:**
  - NASM 2.16.03 is built from source (CI's prebuilt binary is x86_64).
  - Flutter's x86_64 `gen_snapshot_arm64` is swapped for the native arm64 one from the same engine build (`darwin-arm64-release`).
  - `FLUTTER_XCODE_MACOSX_DEPLOYMENT_TARGET=12.3` is passed, because Xcode 27 rejects the pods' 10.13/10.14 targets.
  - The output copy is re-signed ad hoc **without** the hardened runtime: Xcode's ad-hoc + hardened-runtime app fails library validation on FlutterMacOS.framework. CI's unsigned artifact has the same problem until it is Developer ID signed.
- None of this needs a patch to upstream sources.
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
  - macOS: a Unix socket at `/tmp/RemoteSmith/remotesmith-ipc.sock`, mode 0600, owned by the user.
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
