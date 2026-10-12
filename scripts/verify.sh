#!/usr/bin/env bash
# Fast verification gate for RemoteSmith. Runs every check whose inputs exist.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f sidecar/Cargo.toml ]]; then
  echo "== sidecar: fmt, clippy, test"
  (cd sidecar && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)
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
