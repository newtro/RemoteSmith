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
  - `apply-patches.sh` refuses to run over uncommitted edits, or over commits (on `remotesmith` or a detached HEAD) whose exact patch text is not a file in `patches/`. Exact means sha256 of the export output, message and whitespace included, not `git patch-id`; `--force` discards them (it prints the old branch tip so it can be recovered).
  - `apply-patches.sh --check` test-applies everything in throwaway `git worktree`s and leaves the submodules untouched. `REMOTESMITH_PATCHES_DIR=<dir>` points either script's checks at another patch tree with the same `rustdesk/` and `hbb_common/` layout.
  - `apply-patches.sh` initialises a missing submodule only; it never updates an existing checkout before the guard has looked at it. The guard also refuses merge commits above the pin.
  - Never commit a change to the `libs/hbb_common` gitlink inside rustdesk; hbb_common changes go in `patches/hbb_common`. `export-patches.sh` rejects such commits.
- **Commit messages and numbering:** each patch commit's subject is `remotesmith(NNNN): <topic>`, where NNNN is the patch number from your unit's range below; the body says which plan item it serves (DEC/DEF/AC IDs). `export-patches.sh` names the file `NNNN-<topic-slug>.patch`, so re-exporting never renumbers, and `git am` keeps the subject, so the number survives every apply/export round trip. A new commit carries its number from the start; to change a number, reword the commit. Export rejects commits without a number, duplicate numbers and empty commits, and test-applies the new set in number order before writing anything. `--dry-run` shows what would change.
  - **Other units' patches.** `apply-patches.sh` records what it applied in `remotesmith-applied` inside each submodule's git dir, and export reconciles `patches/` against that record:
    - A patch file not on your branch that you last applied unchanged is one you dropped, so export removes it.
    - Any other file not on your branch is kept and test-applied together with yours. Usually it is another unit's patch pulled from main. `--prune` removes it anyway.
    - Export refuses to overwrite a patch file that changed since your last apply (someone updated it on main), because writing your branch's copy would revert their change. Re-apply and redo your commits on top, or pass `--force`.
  - Both scripts ignore your global/system git config when writing or applying patches, so output is byte-stable across machines; `.gitattributes` keeps `patches/**` out of line-ending conversion.
  - `apply-patches.sh --check` (and so `verify.sh`) fails if a pin points at a `remotesmith(NNNN)` commit, which is what happens if the patched submodule gets `git add`ed in the superproject. Don't stage `vendor/rustdesk` unless you are bumping the upstream pin.
- **Patch number ranges, one per work unit,** so parallel units never collide:

  | Range | Unit |
  |---|---|
  | 0100–0199 | branding, no default servers, hardened defaults, first-run provisioning (DEC-15, DEF-2, DEC-10) |
  | 0200–0299 | key-mapping contract and escape hotkey (DEC-22) |
  | 0300–0399 | peer identity / TOFU, LAN-first timeout, reconnect rules (DEF-3, DEF-7, DEF-8) |
  | 0400–0499 | RemoteSmith IPC endpoint and presence data (DEF-4, DEF-17) |
  | 0500–0599 | session indicator, readiness reporting hooks (§6.2a, §6.3) |
  | 0600–0699 | release packaging: bundled sidecar autostart and its removal on uninstall, macOS and Windows (DEC-14, DEC-20, AC-39) |
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
- **LLVM on macOS:** upstream's mac job does not use 15.0.6 (that is Windows/Linux); it `brew install`s llvm, which clang-sys never finds on arm64, so bindgen uses Xcode's libclang. We skip brew llvm.

### Local macOS build

From a fresh clone:

```sh
git submodule update --init --recursive --depth 1   # upstream RustDesk 1.5.0 + hbb_common at their pins
scripts/apply-patches.sh     # branch `remotesmith` in both submodules, one commit per patches/ file
scripts/bootstrap-mac.sh     # idempotent; Flutter 3.24.5 (+3.22.3 for the bridge), Rust 1.81/1.75 toolchains,
                             # NASM 2.16.03, cargo-expand + flutter_rust_bridge_codegen, vcpkg + arm64-osx packages
scripts/build-mac.sh         # -> build/mac/RemoteSmith.app (+ build/mac/build-*.log)
scripts/verify.sh            # sidecar, patch, fork Rust + Flutter unit tests, lint, AC-33 check
```

- **`build-mac.sh` builds only the exact patch stack.** Before touching anything it checks that `vendor/rustdesk` and `libs/hbb_common` are each on branch `remotesmith` with exactly one commit per file in `patches/rustdesk` / `patches/hbb_common`, in order, each byte-identical to its patch file (the same sha256-of-export identity `apply-patches.sh` uses). Otherwise it lists what is wrong and stops:
  - a fresh or stale checkout (submodules at the upstream pins, a patch missing, a `patches/` file changed since the last apply): run `scripts/apply-patches.sh`;
  - new commits on top of the stack: run `scripts/export-patches.sh` first, so the build is reproducible from `patches/`;
  - a detached HEAD, a reworded commit, or a superproject gitlink staged at a patch commit is also refused.
  - Uncommitted edits and untracked files in the submodules are allowed (the normal edit-build loop) but printed as a warning, since they are not in `patches/`. An empty `patches/rustdesk` or `patches/hbb_common` is refused too (that would be a stock build).
- `scripts/build-mac.sh --stock` deliberately builds unpatched upstream RustDesk (`RustDesk.app`, upstream defaults). It requires both submodules at their pinned commits; `git submodule update --init --recursive --force` gets back there and `apply-patches.sh` restores the fork afterwards.

- `scripts/lib/mac-env.sh` holds every version, each with the upstream workflow line it mirrors. Everything lands in `~/.cache/remotesmith/`; your default rustup toolchain is not touched (the build sets `RUSTUP_TOOLCHAIN`).
- `bootstrap-mac.sh` and `build-mac.sh` share one machine-wide lock (`~/.cache/remotesmith/build-mac.lock`, via `shlock`) for everything that writes the shared cache. Parallel worktrees queue rather than collide.
- `vendor/rustdesk/target` is a symlink to the shared `~/.cache/remotesmith/target-mac`, because build.py and the Xcode project hard-code `../../target/release`. The build also exports `CARGO_TARGET_DIR` to the same directory and checks it with `cargo metadata`. It refuses to run if the environment sets `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` or `CARGO_BUILD_TARGET`, or if a cargo config sets `[build] target`/`target-dir`, since any of those would make build.py package stale binaries.
- The build makes CI's in-place edits (MIN_MACOS_VERSION 12.3 in build.py, Podfile, Cargo.toml and project.pbxproj; the bridge's pubspec.yaml edit). Tools also rewrite `Podfile.lock` and `pubspec.lock`.
  - It refuses to start if any of those files has uncommitted changes.
  - Afterwards it restores a file only if its content still equals what the build wrote. A file edited by anything else during the build is left alone with a warning.
- The flutter_rust_bridge files are regenerated when the rustdesk HEAD or sources change, like CI's `generate-bridge` job (Flutter 3.22.3, codegen 1.80.1). One difference: cargo-expand runs on Rust 1.81 here, because the crate is expanded for macOS, where cidre needs 1.81. `--skip-bridge` reuses the existing files.
- **Differences from the macos-14 runner, all because this Mac has Xcode 27 and no Rosetta:**
  - NASM 2.16.03 is built from source (CI's prebuilt binary is x86_64).
  - Flutter's x86_64 `gen_snapshot_arm64` is swapped for the native arm64 one from the same engine build (`darwin-arm64-release`).
  - `FLUTTER_XCODE_MACOSX_DEPLOYMENT_TARGET=12.3` is passed, because Xcode 27 rejects the pods' 10.13/10.14 targets.
  - The output copy is re-signed ad hoc **without** the hardened runtime: Xcode's ad-hoc + hardened-runtime app fails library validation on FlutterMacOS.framework. CI's unsigned artifact has the same problem until it is Developer ID signed.
- vcpkg uses `scripts/vcpkg-triplets/arm64-osx.cmake`: the stock triplet plus `VCPKG_OSX_DEPLOYMENT_TARGET 12.3`. Without it the static libs come out as minos 27.0 (the runner's come out as 14.x), inside an app that declares 12.3.
- None of this needs a patch to upstream sources.
- **Windows features:** `--flutter --hwcodec`.
- **Shared build caches** live on the internal disk under `~/.cache/remotesmith/`: vcpkg installed tree, cargo target, Flutter SDK. `/Volumes/Cache` has limited free space.
- **Local Mac builds are heavy:** run one full build at a time. Parallel work should use `cargo check` against the shared target dir, where cargo's lock serializes them safely.

## Signing (macOS)

There is no local Developer ID cert. Signing follows the iSmith pattern (`/Volumes/Cache/repos/iSmith/Tools/release.sh`):
- `xcodebuild archive` of the Flutter macOS Runner;
- `-exportArchive` with `method=developer-id` and `teamID=232A77467G`, using the cloud-managed cert from the Apple ID signed in to Xcode;
- `-exportNotarizedApp`, then `stapler validate`.

Local dev builds sign with the developer's own `Apple Development` certificate (configured locally, never committed), so TCC permissions persist across rebuilds. CI produces unsigned Mac artifacts; signing and notarization happen locally in `scripts/release-mac.sh`, because the cloud cert needs the Xcode account.

`scripts/release-mac.sh` does the whole release: `build-mac.sh --archive` (build.py, then `xcodebuild archive` of the Runner with the team's Apple Development identity, inside the build lock while build.py's outputs and CI edits are in place), the export and notarization above, verification (every Mach-O Developer ID + hardened runtime + timestamp, `spctl`, `stapler validate`) and `build/release/<version>/RemoteSmith-<version>-arm64.dmg` plus a zip. The dmg itself is unsigned (the Developer ID key is only reachable through Xcode's export, which signs app bundles); the app inside is stapled. Entitlements are upstream's `Release.entitlements`, unchanged.

**Bundle extras** (`scripts/lib/mac-bundle.sh`, used by `build-mac.sh`, the release archive and CI): upstream's `service` binary in `Contents/MacOS` (build.py copies it in after `flutter build`, so an archive lacks it), and the sidecar as an `LSUIElement` login-item helper, `Contents/Library/LoginItems/RemoteSmith Tray.app` (bundle id `ai.johnnycode.remotesmith.tray`). Patch 0601 registers the helper with `SMAppService.loginItem` when the main window of an installed copy (/Applications or ~/Applications, not translocated) starts and the system has no record of it, launches it if it is not running, and replaces a tray left running from before an upgrade. The user turns it off in System Settings > General > Login Items, which is respected. Patch 0603: uninstalling the service unregisters the login item, removes a standalone LaunchAgent, quits the tray and sets `RemoteSmithTrayRemoved` (app defaults) until the service is installed again. The bundled copy refuses `--install-autostart`, which stays for standalone sidecar builds.

**Windows:** CI builds `remotesmith-tray.exe` (sidecar toolchain, `x86_64-pc-windows-msvc`) into the portable folder, so it is in the portable zip, the self-extracting exe and the MSI. Patch 0602 gives the MSI an HKLM `Run` value `RemoteSmithTray`, starts the tray for the installing user after install, and stops it before removal. Patch 0603 does the same for RemoteSmith's own installer (HKLM `Run` at install; upgrade stops the tray; uninstall stops it and deletes the HKLM and HKCU values). Portable use: `remotesmith-tray.exe --install-autostart` (HKCU `Run`); it refuses when an installer already registered the tray.

Bundle id: `ai.johnnycode.remotesmith`. App name: `RemoteSmith`.

## Server

- **Host:** a per-deployment DNS name (`RS_HOST`) on a Lightsail VM, Cloudflare DNS-only. The name, addresses, ssh target and account names live only in the gitignored `deploy/server/site.env` (template: `site.env.example`); `install.sh` renders the hbbs unit from `remotesmith-hbbs.service.in`, and `scripts/check-private.sh` (run by `scripts/verify.sh`) keeps them out of the repo (AC-33, DEC-15).
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
- CI runs the same suites before it builds anything (see [CI.md](CI.md)): sidecar fmt/clippy/test on macOS and Windows, the fork's whole `cargo test --lib` with the Mac features, and the RemoteSmith Flutter tests. The build jobs depend on them, so no artifact comes from a red suite.
- Two-machine ACs: `docs/test-runs/checklist.md`. The Windows side needs Scott's PC; CI builds the Windows artifact.
