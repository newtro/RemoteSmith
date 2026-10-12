# shellcheck shell=bash
# Shared settings for scripts/bootstrap-mac.sh and scripts/build-mac.sh.
# Every version here mirrors vendor/rustdesk/.github/workflows/flutter-build.yml (macOS job,
# aarch64-apple-darwin) and bridge.yml at RustDesk 1.5.0. Line numbers refer to those files.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUSTDESK_DIR="$REPO_ROOT/vendor/rustdesk"

RS_CACHE="${REMOTESMITH_CACHE:-$HOME/.cache/remotesmith}"

# flutter-build.yml:28 FLUTTER_VERSION
FLUTTER_VERSION="3.24.5"
# bridge.yml:25 the bridge is generated with Flutter 3.22.3, not FLUTTER_VERSION
BRIDGE_FLUTTER_VERSION="3.22.3"
# flutter-build.yml:23 MAC_RUST_VERSION (cidre needs 1.81)
MAC_RUST_VERSION="1.81"
# bridge.yml:11 RUST_VERSION used to install the bridge codegen tools
BRIDGE_RUST_VERSION="1.75"
# bridge.yml:9-10
CARGO_EXPAND_VERSION="1.0.95"
FLUTTER_RUST_BRIDGE_VERSION="1.80.1"
# flutter-build.yml:45 VCPKG_COMMIT_ID
VCPKG_COMMIT_ID="9e593bb18ea69cc5095e012465dcd675a822ed0d"
# flutter-build.yml:831 NASM 2.16.03 from nasm.us (not brew's NASM 3.x)
NASM_VERSION="2.16.03"
# flutter-build.yml:762-767 aarch64 job
VCPKG_TRIPLET="arm64-osx"
MIN_MACOS_VERSION="12.3"   # flutter-build.yml:914

FLUTTER_ROOT_DIR="$RS_CACHE/flutter-$FLUTTER_VERSION"
BRIDGE_FLUTTER_ROOT_DIR="$RS_CACHE/flutter-$BRIDGE_FLUTTER_VERSION"
TOOLS_DIR="$RS_CACHE/tools"            # nasm, cargo-installed codegen tools (bin/)
export VCPKG_ROOT="$RS_CACHE/vcpkg"
export VCPKG_DEFAULT_BINARY_CACHE="$RS_CACHE/vcpkg-archives"
export VCPKG_DISABLE_METRICS=1
CARGO_TARGET_CACHE="$RS_CACHE/target-mac"
# One lock for everything that writes the shared cache: bootstrap-mac.sh and build-mac.sh in any
# worktree. Name kept from the first version so an older script still honours it.
BUILD_LOCK="$RS_CACHE/build-mac.lock"

export PATH="$TOOLS_DIR/bin:$PATH"
export COCOAPODS_DISABLE_STATS=true

log() { printf '\n== [%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# --- shared-cache lock ---------------------------------------------------------------------------
# shlock(1) creates the lock file atomically with our pid and clears it when that pid is dead.
# Callers install their own EXIT trap that calls release_cache_lock.
CACHE_LOCK_HELD=0
acquire_cache_lock() { # purpose
  mkdir -p "$RS_CACHE"
  until shlock -f "$BUILD_LOCK" -p $$; do
    echo "waiting for $BUILD_LOCK: held by pid $(cat "$BUILD_LOCK" 2>/dev/null) ($(cat "$BUILD_LOCK.where" 2>/dev/null)); retrying in 30s"
    sleep 30
  done
  CACHE_LOCK_HELD=1
  echo "$1: $REPO_ROOT" > "$BUILD_LOCK.where"
}
release_cache_lock() {
  if ((CACHE_LOCK_HELD)) && [[ "$(cat "$BUILD_LOCK" 2>/dev/null)" == "$$" ]]; then
    rm -f "$BUILD_LOCK" "$BUILD_LOCK.where"
  fi
  CACHE_LOCK_HELD=0
}

# --- vcpkg manifest install ----------------------------------------------------------------------
# As flutter-build.yml:885-899 does it: run from the repo root (vcpkg.json, overlay ports in
# res/vcpkg) into $VCPKG_ROOT/installed. One addition: scripts/vcpkg-triplets/arm64-osx.cmake pins
# the deployment target to MIN_MACOS_VERSION (see that file). Idempotent: an unchanged manifest is
# a no-op; built packages are cached in $VCPKG_DEFAULT_BINARY_CACHE. Caller must hold the lock.
vcpkg_install() {
  ((CACHE_LOCK_HELD)) || die "internal: vcpkg_install without the cache lock"
  [[ -x "$VCPKG_ROOT/vcpkg" ]] || die "vcpkg missing; run scripts/bootstrap-mac.sh"
  log "vcpkg install ($VCPKG_TRIPLET) for $RUSTDESK_DIR/vcpkg.json"
  if ! (cd "$RUSTDESK_DIR" && "$VCPKG_ROOT/vcpkg" install --triplet "$VCPKG_TRIPLET" \
          --x-install-root="$VCPKG_ROOT/installed" --overlay-triplets="$REPO_ROOT/scripts/vcpkg-triplets"); then
    find "$VCPKG_ROOT/buildtrees" -name '*.log' -newer "$VCPKG_ROOT/vcpkg" -print 2>/dev/null | tail -20 >&2
    die "vcpkg install failed; logs listed above"
  fi
  [[ -d "$VCPKG_ROOT/installed/$VCPKG_TRIPLET/include/vpx" ]] || die "vcpkg finished but libvpx headers are missing"
}
