# CI

One GitHub Actions workflow, [`.github/workflows/build.yml`](../.github/workflows/build.yml). It applies the RemoteSmith patches onto `vendor/rustdesk` (RustDesk 1.5.0), runs the RemoteSmith test suites, and builds the macOS arm64 and Windows x64 Flutter apps.

## Triggers

- A push to `main` or any `unit/**` branch.
- Any pull request.
- Manually: **Actions → Build → Run workflow**, or `gh workflow run build.yml --ref <branch>`.

A newer run on the same ref cancels the older one (`concurrency` group per workflow and ref).

## Jobs

| Job | Runner | What it does |
|---|---|---|
| `ci-lint` | ubuntu-latest | `bash -n` on `scripts/*.sh` and `deploy/server/*.sh`. It also runs actionlint 1.7.12 (checksum-verified) on the workflows, with shellcheck on `run:` blocks, and `scripts/check-private.sh` (no public IP literals; AC-33). |
| `generate-bridge` | ubuntu-22.04 | flutter_rust_bridge 1.80.1 codegen with Flutter 3.22.3 and Rust 1.75, run on the **patched** `src/flutter_ffi.rs`. Uploads `bridge-artifact`. |
| `test-sidecar-macos` (`test: sidecar (macOS)`) | macos-14 | `cargo fmt --check`, `cargo clippy --locked --all-targets -D warnings` and `cargo test --locked` in `sidecar/`, on the toolchain `sidecar/rust-toolchain.toml` pins (not vendor/rustdesk's). |
| `test-sidecar-windows` (`test: sidecar (Windows)`) | windows-latest | Sidecar clippy and tests on Windows, natively (the named-pipe IPC code paths). |
| `test-fork-rust-macos` (`test: fork Rust unit tests (macOS)`) | macos-14 | Needs `generate-bridge`. The patched fork's whole `cargo test --locked --lib --features flutter,hwcodec,unix-file-copy-paste,screencapturekit` (upstream's tests plus every RemoteSmith module: keymap, identity/trust, reconnect, IPC, readiness, setup), then `cargo test -p hbb_common --lib -- --test-threads=1` (branding defaults, no default servers, LAN-only selection; the config tests share global state). Same Rust 1.81, vcpkg cache and bridge files as the mac build. |
| `test-fork-flutter-macos` (`test: fork Flutter tests (macOS)`) | macos-14 | Needs `generate-bridge`. `flutter test` on every `flutter/test/remotesmith_*_test.dart` of the patched tree, with Flutter 3.24.5 and upstream's Flutter patches. Fails if there are none. |
| `tests` (`tests passed (gate)`) | ubuntu-latest | Needs `ci-lint` and the four test jobs. Both build jobs need it, so when any suite fails the builds are skipped and no artifact is uploaded. |
| `build-RustDeskTempTopMostWindow (x64)` | windows-2022 | Builds `WindowInjection.dll`, which the Windows bundle uses for privacy mode. |
| `x86_64-pc-windows-msvc` | windows-2022 | The Windows Flutter app: portable folder, self-extracting exe and MSI. |
| `aarch64-apple-darwin` | macos-14 | The macOS Flutter app: unsigned `.dmg` and `.app.zip`. |

Every job that touches the source runs `bash scripts/apply-patches.sh` first. If that script doesn't exist yet, the job logs a "Patches skipped" notice and builds stock RustDesk.

The build jobs start only after the `tests` gate, so a run takes roughly the fork Rust test job (a debug build of the whole crate; longest on a cold cache) plus the build. That is the price of never producing artifacts from a red suite. The test jobs run in parallel with each other and with `build-RustDeskTempTopMostWindow`.

The upstream build runs about 35 min on macOS and 40 min on Windows. Job timeouts are 180–240 min. The first run on a branch takes longer, because the vcpkg and cargo caches are cold.

## Artifacts (kept 14 days)

| Artifact | Contents |
|---|---|
| `remotesmith-unsigned-macos-aarch64` | `remotesmith-<ver>-aarch64-unsigned.dmg` and `remotesmith-<ver>-aarch64-unsigned.app.zip`. The zip is made with `ditto`, which keeps the bundle symlinks that signing needs. |
| `remotesmith-unsigned-windows-x86_64` | The portable app folder. GitHub serves it as a zip: unzip it and run the exe. |
| `remotesmith-unsigned-windows-x86_64-installers` | `remotesmith-<ver>-x86_64.exe` (the self-extracting installer) and `remotesmith-<ver>-x86_64.msi`. |
| `bridge-artifact`, `topmostwindow-artifacts-x64` | Intermediate files passed between jobs. |

`<ver>` is read from `vendor/rustdesk/Cargo.toml` after the patches are applied. The app bundle name, the Windows exe name and the MSI product name are also read from the patched sources, so the branding patches don't need to edit the workflow.

## Signing

Nothing is signed in CI.
- **macOS:** signing and notarization happen locally in `scripts/release-mac.sh`. It uses the cloud-managed Developer ID certificate from the Apple ID signed in to Xcode (team `232A77467G`), and that certificate can't be exported to a CI runner. Unsigned CI builds are for smoke testing. A Mac you run them on needs Gatekeeper bypassed (`xattr -dr com.apple.quarantine`).
- **Windows:** the binaries are unsigned. SmartScreen warns on first run.

## Relation to upstream

The build jobs mirror the matching jobs in upstream's `vendor/rustdesk/.github/workflows/flutter-build.yml` (`build-for-macOS`, `build-for-windows-flutter`), `bridge.yml` and `third-party-RustDeskTempTopMostWindow.yml`, at the pinned commit.

- **Step names:** kept identical to upstream's, so a failing step can be found in upstream's file. Steps that exist only here start with `RemoteSmith:`.
- **Working directory:** upstream's steps assume RustDesk is the repo root. Here every job runs its shell steps in `vendor/rustdesk`.

The toolchain matches upstream:

| Tool | Version |
|---|---|
| Flutter | 3.24.5, plus upstream's dropdown patch, the macOS frames workaround, and on Windows the `rustdesk/engine` custom engine swap |
| Rust | 1.81 on macOS, 1.75 on Windows (upstream's `SCITER_RUST_VERSION`, which its Windows Flutter build uses) |
| LLVM | 15.0.6 on Windows. macOS uses `brew install llvm`, as upstream does. |
| vcpkg | commit `9e593bb18ea69cc5095e012465dcd675a822ed0d` |

### Intentional differences from upstream

1. **vcpkg binary cache.** Upstream sets `VCPKG_BINARY_SOURCES=clear;x-gha,readwrite`. vcpkg at this commit has removed the `x-gha` backend: upstream's own logs show the warning, and every port is built from source on every run. Here, vcpkg uses the `files` backend on `$RUNNER_TEMP/vcpkg-binary-cache`, which `actions/cache` saves and restores. The cache key covers the OS, the triplet, the vcpkg commit, and a hash of `vcpkg.json`, `res/vcpkg/**` and `res/vcpkg-triplets/**`. A prefix restore key reuses partial caches. For the same reason, upstream's "Export GitHub Actions cache environment variables" step is dropped: it only fed `x-gha`.
2. **Bridge generated on patched sources.** The bridge job applies the patches before codegen, so RemoteSmith FFI changes reach the generated bridge. Only the 3.22.3 `bridge-artifact` variant is built. Upstream's Flutter 3.44 variant exists only for Windows arm64.
3. **Only the two targets.** These are dropped:
   - matrix entries: x86_64 macOS, arm64 Windows;
   - arm64-only steps: "Force arm64 Dart SDK + engine", "Patch RustDesk sources for Flutter 3.44", "Set SODIUM_LIB_DIR";
   - other jobs: sbom, sciter, Linux, Android, iOS, web, flatpak, appimage.
4. **No signing or publishing.** These are dropped:
   - macOS: the cert/notarize import steps, rcodesign, "Codesign app and create signed dmg", "Rename rustdesk";
   - Windows: "Sign rustdesk files", "Sign rustdesk self-extracted file";
   - all "Publish Release" steps and `publish_unsigned`;
   - "Build pre-built MSI template" and its upload, which feed RustDesk's custom-client service.
5. **Always package.** Upstream's CI (`upload-artifact: false`) skips the dmg, the self-extracting exe and the MSI, and only its nightly builds them. This workflow always builds them. The portable folder is uploaded before the installer steps run, so it survives an installer failure. "Test cursor sizing" also always runs: upstream runs it only in non-upload CI.
6. **Names read from the sources.** Upstream hard-codes `RustDesk.app`, `rustdesk.exe` and the MSI default app name. This workflow reads:
   - `[package.metadata.bundle] name` from `Cargo.toml`, passed to MSI `preprocess.py --app-name`. For stock sources it is `RustDesk`, the same as upstream's default;
   - `BINARY_NAME` from `flutter/windows/CMakeLists.txt`;
   - the single `*.app` in the Release folder.

   Artifact files are named `remotesmith-*`.
7. **Small shell fixes so actionlint and shellcheck pass:** quoting around `$(which flutter)` and `$VCPKG_ROOT`, and the `$runner_res` quote. The behavior is the same.
8. **Pinned action versions:** upstream's SHAs everywhere except `actions/cache`. Upstream's `bridge.yml` uses a v3 SHA; this workflow uses v5.1.0. Swatinem/rust-cache gets `workspaces: vendor/rustdesk -> target`, and run-vcpkg gets `vcpkgJsonGlob: vendor/rustdesk/vcpkg.json`.

## Local checks

`scripts/verify.sh` runs the same suites locally when their inputs exist (the fork tests need `scripts/apply-patches.sh` and a `scripts/build-mac.sh` run first).

`scripts/verify.sh` runs `actionlint` on the workflows when it is installed (`brew install actionlint`). You can also run `ci-lint` locally with `act push -j ci-lint -P ubuntu-latest=catthehacker/ubuntu:act-latest --container-architecture linux/amd64 --container-daemon-socket -`. The macOS and Windows jobs can't run under `act`, which only runs Linux containers.
