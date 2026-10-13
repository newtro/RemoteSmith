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
   && git -C vendor/rustdesk log --format=%s "$rs_pin..HEAD" 2>/dev/null | grep '^remotesmith(0201)' >/dev/null; then
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
  echo "== fork: RemoteSmith unit tests (cargo test --lib remotesmith + transfer handler tests, Rust 1.81)"
  (
    # shellcheck source=scripts/lib/mac-env.sh
    source scripts/lib/mac-env.sh
    cd vendor/rustdesk
    RUSTUP_TOOLCHAIN="$MAC_RUST_VERSION" cargo test --locked --lib \
      --features flutter,hwcodec,unix-file-copy-paste,screencapturekit \
      -- remotesmith client::io_loop::tests upload_is_verified_before_done
  )
else
  echo "== fork: RemoteSmith unit tests skipped (needs scripts/apply-patches.sh + scripts/build-mac.sh first)"
fi
# File-transfer staging/finalize tests carried by patch 0801 (crate `base`, no bridge needed).
if [[ "$(uname -s)" == Darwin && -f vendor/rustdesk/libs/base/Cargo.toml ]] \
   && git -C vendor/rustdesk log --format=%s "$rs_pin..HEAD" 2>/dev/null | grep '^remotesmith(0801)' >/dev/null \
   && rustup toolchain list 2>/dev/null | grep -q '^1\.81'; then
  echo "== fork: file-transfer tests (cargo test -p base --lib fs::, Rust 1.81)"
  (
    # shellcheck source=scripts/lib/mac-env.sh
    source scripts/lib/mac-env.sh
    cd vendor/rustdesk
    RUSTUP_TOOLCHAIN="$MAC_RUST_VERSION" CARGO_TARGET_DIR="$CARGO_TARGET_CACHE" \
      cargo test --locked -p base --lib fs::
  )
else
  echo "== fork: file-transfer tests skipped (needs scripts/apply-patches.sh and the Rust 1.81 toolchain)"
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
if [[ -d patches ]]; then
  # DEC-15 / AC-33 / AC-40: the client patches must not carry a personal server host or key, and
  # must not reintroduce RustDesk's public servers. (Bundle id ai.johnnycode.remotesmith is fine.)
  echo "== patches: no personal infrastructure, no RustDesk public servers"
  # Added lines only, ignoring comments that name what was removed.
  if grep -rnE '^\+.*(johnnycode\.ai|rs-ny\.rustdesk\.com|admin\.rustdesk\.com|api\.rustdesk\.com)' patches/ \
       | grep -vE '^[^:]+:[0-9]+:\+[[:space:]]*(//|#)'; then
    echo "verify: a patch adds a personal hostname or a RustDesk public server (lines above)" >&2
    exit 1
  fi
fi
for f in scripts/branding/*.py; do
  [[ -f "$f" ]] && python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read(), sys.argv[1])' "$f"
done
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
  echo "== deploy/server: shellcheck, probe.py syntax, unit render, install.sh dry run"
  if command -v shellcheck >/dev/null 2>&1; then
    shellcheck deploy/server/*.sh scripts/check-private.sh
  else
    echo "shellcheck not installed (brew install shellcheck); skipping"
  fi
  python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read(), sys.argv[1])' deploy/server/probe.py
  # A documentation host (RFC 2606), so the result doesn't depend on a local site.env.
  units=$(mktemp -d)
  bash deploy/server/install.sh --render-units "$units" --host rs.example.com >/dev/null
  grep -qx 'ExecStart=/opt/remotesmith-server/bin/hbbs --port 21116 --relay-servers rs.example.com:21117 --key _' \
    "$units/remotesmith-hbbs.service"
  grep -qx 'ExecStartPre=/opt/remotesmith-server/libexec/wait-resolve.sh rs.example.com 60' "$units/remotesmith-hbbs.service"
  cmp -s "$units/remotesmith-hbbr.service" deploy/server/remotesmith-hbbr.service
  rm -rf "$units"
  bash deploy/server/install.sh --dry-run --arch amd64 --host rs.example.com >/dev/null
  # --host over a site.env that holds only RS_HOST (staging rewrites it; regression:
  # grep -v matched nothing and set -e ended the run silently).
  kit=$(mktemp -d)
  cp deploy/server/install.sh deploy/server/site-env.sh deploy/server/keycheck.sh deploy/server/wait-resolve.sh \
    deploy/server/remotesmith-hbbs.service.in deploy/server/remotesmith-hbbr.service "$kit/"
  printf 'RS_HOST=rs.example.net\n' > "$kit/site.env"
  out=$(bash "$kit/install.sh" --dry-run --arch amd64 --host rs.example.com 2>&1) \
    || { printf '%s\n' "$out" | tail -5; echo "install.sh --host over a RS_HOST-only site.env failed" >&2; exit 1; }
  grep -q 'server name rs.example.com (from --host)' <<<"$out"
  rm -rf "$kit"
  bash deploy/server/firewall.sh --dry-run >/dev/null
fi
echo "== no personal infrastructure in the repo (AC-33)"
scripts/check-private.sh
echo "verify: OK"
