#!/usr/bin/env bash
# Fast verification gate for RemoteSmith. Runs every check whose inputs exist.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f sidecar/Cargo.toml ]]; then
  echo "== sidecar: fmt, clippy, test"
  (cd sidecar && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)
  if rustup target list --installed 2>/dev/null | grep -q '^x86_64-pc-windows-msvc$'; then
    echo "== sidecar: clippy for Windows (x86_64-pc-windows-msvc)"
    (cd sidecar && cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings)
  else
    echo "== sidecar: Windows target not installed; skipping (rustup target add x86_64-pc-windows-msvc)"
  fi
fi
if [[ -x scripts/apply-patches.sh ]]; then
  echo "== patches apply cleanly"
  scripts/apply-patches.sh --check
fi
for f in deploy/server/*.sh scripts/*.sh; do
  [[ -f "$f" ]] && bash -n "$f"
done
echo "verify: OK"
