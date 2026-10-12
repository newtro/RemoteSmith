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
BUILD_LOCK="$RS_CACHE/build-mac.lock"

export PATH="$TOOLS_DIR/bin:$PATH"
export COCOAPODS_DISABLE_STATS=true

log() { printf '\n== [%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
