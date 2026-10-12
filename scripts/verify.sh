#!/usr/bin/env bash
# Fast verification gate for RemoteSmith. Runs every check whose inputs exist.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f sidecar/Cargo.toml ]]; then
  # sidecar/rust-toolchain.toml pins the sidecar's own toolchain (not vendor/rustdesk's).
  echo "== sidecar: toolchain $(cd sidecar && rustc --version)"
  echo "== sidecar: fmt, clippy, test"
  (cd sidecar && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
  if (cd sidecar && rustup target list --installed 2>/dev/null | grep -q '^x86_64-pc-windows-msvc$'); then
    echo "== sidecar: clippy for Windows (x86_64-pc-windows-msvc)"
    (cd sidecar && cargo clippy --locked --target x86_64-pc-windows-msvc --all-targets -- -D warnings)
  else
    echo "== sidecar: Windows target not installed; skipping (rustup target add x86_64-pc-windows-msvc)"
  fi
fi
if [[ -x scripts/apply-patches.sh ]]; then
  echo "== patches apply cleanly"
  scripts/apply-patches.sh --check
fi
# Unit tests carried inside RustDesk patches (remotesmith(0201): keyboard::rs_keymap), run when the
# submodule has the patches applied (scripts/apply-patches.sh) and the Mac build env exists
# (scripts/bootstrap-mac.sh, plus the bridge files a scripts/build-mac.sh run generates).
rs_pin="$(git ls-tree HEAD vendor/rustdesk | awk '{print $3}')"
if [[ "$(uname)" == Darwin && -f vendor/rustdesk/src/bridge_generated.rs ]] \
   && rustup toolchain list 2>/dev/null | grep -q '^1\.81' \
   && git -C vendor/rustdesk log --format=%s "$rs_pin..HEAD" 2>/dev/null | grep -q '^remotesmith(0201)'; then
  echo "== rustdesk patch unit tests (keyboard::rs_keymap)"
  (
    # shellcheck disable=SC1091  # sourced relative to the repo root
    source scripts/lib/mac-env.sh
    cd vendor/rustdesk
    RUSTUP_TOOLCHAIN="$MAC_RUST_VERSION" CARGO_TARGET_DIR="$CARGO_TARGET_CACHE" \
      cargo test --lib --features flutter,hwcodec,unix-file-copy-paste,screencapturekit rs_keymap
  )
else
  echo "== rustdesk patch unit tests: skipped (needs apply-patches.sh, the Mac build env and the generated bridge)"
fi
# The fork's RemoteSmith unit tests (patch 0400: IPC endpoint, shared fixture, readiness logic).
# They need the patched checkout and the bootstrapped Mac toolchain (scripts/bootstrap-mac.sh)
# plus the generated bridge files from a scripts/build-mac.sh run; skipped otherwise.
if [[ "$(uname -s)" == Darwin && -f vendor/rustdesk/src/remotesmith/mod.rs \
      && -f vendor/rustdesk/src/bridge_generated.rs && -L vendor/rustdesk/target ]]; then
  echo "== fork: RemoteSmith unit tests (cargo test --lib remotesmith, Rust 1.81)"
  (
    # shellcheck source=scripts/lib/mac-env.sh
    source scripts/lib/mac-env.sh
    cd vendor/rustdesk
    RUSTUP_TOOLCHAIN="$MAC_RUST_VERSION" cargo test --locked --lib \
      --features flutter,hwcodec,unix-file-copy-paste,screencapturekit remotesmith
  )
else
  echo "== fork: RemoteSmith unit tests skipped (needs scripts/apply-patches.sh + scripts/build-mac.sh first)"
fi
# The fork's RemoteSmith Flutter tests (patch 0502), with the pinned Flutter from bootstrap-mac.sh.
if compgen -G "vendor/rustdesk/flutter/test/remotesmith_*_test.dart" >/dev/null \
   && [[ -x "${REMOTESMITH_CACHE:-$HOME/.cache/remotesmith}/flutter-3.24.5/bin/flutter" ]]; then
  echo "== fork: RemoteSmith Flutter tests"
  (cd vendor/rustdesk/flutter && "${REMOTESMITH_CACHE:-$HOME/.cache/remotesmith}/flutter-3.24.5/bin/flutter" \
     --suppress-analytics test test/remotesmith_*_test.dart)
else
  echo "== fork: RemoteSmith Flutter tests skipped (needs the patched checkout and scripts/bootstrap-mac.sh)"
fi
for f in deploy/server/*.sh scripts/*.sh; do
  [[ -f "$f" ]] && bash -n "$f"
done
if compgen -G ".github/workflows/*.yml" >/dev/null; then
  if command -v actionlint >/dev/null; then
    echo "== actionlint"
    actionlint .github/workflows/*.yml
  else
    echo "== actionlint not installed; skipping workflow lint (brew install actionlint)"
  fi
fi
if [[ -f deploy/server/install.sh ]]; then
  echo "== deploy/server: shellcheck, probe.py syntax, install.sh dry run"
  if command -v shellcheck >/dev/null 2>&1; then
    shellcheck deploy/server/*.sh
  else
    echo "shellcheck not installed (brew install shellcheck); skipping"
  fi
  python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read(), sys.argv[1])' deploy/server/probe.py
  bash deploy/server/install.sh --dry-run --arch amd64 >/dev/null
  bash deploy/server/firewall.sh --dry-run >/dev/null
fi
echo "verify: OK"
