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
