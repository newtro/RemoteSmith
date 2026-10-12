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
