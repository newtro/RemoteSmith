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
echo "verify: OK"
