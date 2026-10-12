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
