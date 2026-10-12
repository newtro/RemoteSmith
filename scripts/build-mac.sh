#!/usr/bin/env bash
# Build the Flutter macOS app (arm64, unsigned) from vendor/rustdesk the way upstream CI does.
# Output: build/mac/RustDesk.app (+ build/mac/build-*.log). Run scripts/bootstrap-mac.sh first.
#
# Mirrors, at RustDesk 1.5.0:
#   bridge.yml:86-104            generate the flutter_rust_bridge files (CI does this on Linux)
#   flutter-build.yml:885-899    vcpkg manifest install
#   flutter-build.yml:911-920    MIN_MACOS_VERSION=12.3 edits, then
#                                ./build.py --flutter --hwcodec --unix-file-copy-paste --screencapturekit
#
# Usage: scripts/build-mac.sh [--skip-bridge]
#   --skip-bridge   reuse the existing generated bridge files even if the sources changed
#
# The cargo target dir is shared across worktrees at ~/.cache/remotesmith/target-mac (build.py and
# the Xcode project hard-code ../../target/release, so vendor/rustdesk/target is a symlink to it).
# Only one full build runs at a time machine-wide (lock at ~/.cache/remotesmith/build-mac.lock).
set -euo pipefail
# shellcheck source=lib/mac-env.sh
source "$(dirname "$0")/lib/mac-env.sh"

skip_bridge=0
for arg in "$@"; do
  case "$arg" in
    --skip-bridge) skip_bridge=1 ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

[[ -f "$RUSTDESK_DIR/build.py" ]] || die "vendor/rustdesk is not checked out; run: git submodule update --init --recursive --depth 1"
[[ -f "$RUSTDESK_DIR/libs/hbb_common/Cargo.toml" ]] || die "libs/hbb_common is not checked out; run: git submodule update --init --recursive --depth 1"
[[ -f "$FLUTTER_ROOT_DIR/.remotesmith-ok" && -f "$BRIDGE_FLUTTER_ROOT_DIR/.remotesmith-ok" && -x "$VCPKG_ROOT/vcpkg" \
   && -x "$TOOLS_DIR/bin/flutter_rust_bridge_codegen" ]] || die "toolchain incomplete; run scripts/bootstrap-mac.sh"
[[ "$(lipo -archs "$FLUTTER_ROOT_DIR/bin/cache/artifacts/engine/darwin-x64-release/gen_snapshot_arm64" 2>/dev/null)" == arm64 ]] \
  || die "Flutter's gen_snapshot_arm64 is not the native arm64 build (Flutter re-downloaded its cache?); run scripts/bootstrap-mac.sh"

OUT_DIR="$REPO_ROOT/build/mac"
mkdir -p "$OUT_DIR" "$CARGO_TARGET_CACHE"
LOG="$OUT_DIR/build-$(date +%Y%m%d-%H%M%S).log"
exec > >(tee "$LOG") 2>&1
start_ts=$(date +%s)
echo "log: $LOG"

# --- machine-wide lock, and restoring the files the build edits ----------------------------------
# Files CI edits in place before build.py (flutter-build.yml:913-918), the bridge step's pubspec
# edit (bridge.yml:93), and lock files the tools rewrite (pod install -> Podfile.lock, pub get ->
# pubspec.lock). CI's checkout is throwaway; ours is not, so they are put back afterwards. Each time
# this script (or a tool it ran) writes one, its hash is recorded; at the end a file is restored
# only if it still has exactly that content. Anything else (an edit made while the build ran) is
# left alone with a warning.
CI_EDITED=(build.py flutter/macos/Podfile Cargo.toml flutter/macos/Runner.xcodeproj/project.pbxproj)
TOOL_WRITTEN=(flutter/macos/Podfile.lock flutter/pubspec.lock)
BRIDGE_EDITED=(flutter/pubspec.yaml)
ALL_EDITED=("${CI_EDITED[@]}" "${TOOL_WRITTEN[@]}" "${BRIDGE_EDITED[@]}")
STATE_DIR="$OUT_DIR/.restore.$$"
key() { tr '/' '%' <<<"$1"; }
remember() { # files written by us -> record their current content
  local f
  for f in "$@"; do shasum -a 256 "$RUSTDESK_DIR/$f" | cut -d' ' -f1 >"$STATE_DIR/$(key "$f")"; done
}
safe_restore() { # files
  local f k
  for f in "$@"; do
    k="$STATE_DIR/$(key "$f")"
    if git -C "$RUSTDESK_DIR" diff --quiet HEAD -- "$f"; then rm -f "$k"; continue; fi
    if [[ -f "$k" && "$(shasum -a 256 "$RUSTDESK_DIR/$f" | cut -d' ' -f1)" == "$(cat "$k")" ]]; then
      git -C "$RUSTDESK_DIR" checkout -- "$f"
    else
      echo "warning: vendor/rustdesk/$f changed during the build (not by this script); left as is." \
           "If that was the build's own edit: git -C vendor/rustdesk checkout -- $f" >&2
    fi
    rm -f "$k"
  done
}
restore=0
cleanup() {
  ((restore)) && safe_restore "${ALL_EDITED[@]}"
  rm -rf "$STATE_DIR"
  release_cache_lock
}
trap cleanup EXIT
acquire_cache_lock build

dirty="$(git -C "$RUSTDESK_DIR" status --porcelain -- "${ALL_EDITED[@]}")"
[[ -z "$dirty" ]] || die "uncommitted changes in files the build edits and restores; commit them first:
$dirty"
rm -rf "$STATE_DIR"; mkdir -p "$STATE_DIR"
restore=1

# --- shared cargo target dir -------------------------------------------------------------------
# build.py and the Xcode project hard-code ../../target/release, so vendor/rustdesk/target is a
# symlink to the shared cache AND cargo is told the same directory explicitly. Anything that would
# make cargo write elsewhere (an inherited CARGO_TARGET_DIR / CARGO_BUILD_TARGET_DIR, or a build
# target triple, which moves output to target/<triple>/release) is rejected, since build.py would
# then package stale binaries from target/release.
target_link="$RUSTDESK_DIR/target"
if [[ -L "$target_link" ]]; then
  [[ "$(readlink "$target_link")" == "$CARGO_TARGET_CACHE" ]] || { rm "$target_link"; ln -s "$CARGO_TARGET_CACHE" "$target_link"; }
elif [[ -e "$target_link" ]]; then
  die "$target_link is a real directory; move or delete it so it can point at $CARGO_TARGET_CACHE"
else
  ln -s "$CARGO_TARGET_CACHE" "$target_link"
fi
for v in CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR; do
  val="${!v:-}"
  [[ -z "$val" || "$val" == "$CARGO_TARGET_CACHE" ]] \
    || die "$v=$val is set in your environment; build.py packages from vendor/rustdesk/target/release ($CARGO_TARGET_CACHE). Unset it."
done
[[ -z "${CARGO_BUILD_TARGET:-}" ]] || die "CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET would move cargo output out of target/release; unset it"
export CARGO_TARGET_DIR="$CARGO_TARGET_CACHE"
unset CARGO_BUILD_TARGET_DIR
# Cargo config files cargo would read for this crate: a [build] target or target-dir there.
d="$RUSTDESK_DIR"
cfgs=("${CARGO_HOME:-$HOME/.cargo}/config" "${CARGO_HOME:-$HOME/.cargo}/config.toml")
while [[ "$d" != / ]]; do cfgs+=("$d/.cargo/config" "$d/.cargo/config.toml"); d="$(dirname "$d")"; done
for c in "${cfgs[@]}"; do
  [[ -f "$c" ]] || continue
  if awk '/^[[:space:]]*\[/{sec=$0} sec ~ /^[[:space:]]*\[build\]/ && /^[[:space:]]*(target|target-dir)[[:space:]]*=/{found=1} END{exit !found}' "$c"; then
    die "$c sets [build] target or target-dir, which would move cargo output away from target/release; remove it for this build"
  fi
done

# --- toolchain ---------------------------------------------------------------------------------
export RUSTUP_TOOLCHAIN="$MAC_RUST_VERSION"   # flutter-build.yml:857-862; default toolchain untouched
log "versions"
xcodebuild -version | sed -n 1p
clang --version | sed -n 1p
rustup -V 2>/dev/null | sed -n 1p
rustc -V
cargo -V

# --- vcpkg -------------------------------------------------------------------------------------
vcpkg_install

# --- flutter_rust_bridge -----------------------------------------------------------------------
# bridge.yml:86-104. CI generates these on Linux with Flutter 3.22.3 (extended_text pinned down to
# 13.0.0 for that Dart) and hands them to the mac job as an artifact. Here cargo-expand has to
# compile the crate for macOS, where cidre needs Rust 1.81, so the expansion runs on 1.81 while the
# codegen tools themselves were built with 1.75 as in CI.
cd "$RUSTDESK_DIR"
bridge_outputs=(src/bridge_generated.rs src/bridge_generated.io.rs flutter/lib/generated_bridge.dart
                flutter/lib/generated_bridge.freezed.dart flutter/macos/Runner/bridge_generated.h flutter/ios/Runner/bridge_generated.h)
bridge_key="$( { git rev-parse HEAD; git diff HEAD -- src libs flutter/pubspec.yaml Cargo.toml Cargo.lock
                 git ls-files --others --exclude-standard -- src | xargs -I{} shasum -a 256 {}
                 git -C libs/hbb_common rev-parse HEAD; git -C libs/hbb_common diff HEAD
                 echo "$FLUTTER_RUST_BRIDGE_VERSION $MAC_RUST_VERSION $BRIDGE_FLUTTER_VERSION"; } | shasum -a 256 | cut -d' ' -f1)"
bridge_stamp="$OUT_DIR/.bridge-stamp"
have_outputs=1
for f in "${bridge_outputs[@]}"; do [[ -f "$f" ]] || have_outputs=0; done
if ((have_outputs)) && { ((skip_bridge)) || [[ "$(cat "$bridge_stamp" 2>/dev/null)" == "$bridge_key" ]]; }; then
  log "bridge: up to date"
else
  log "bridge: generating with Flutter $BRIDGE_FLUTTER_VERSION + flutter_rust_bridge_codegen $FLUTTER_RUST_BRIDGE_VERSION"
  (
    export PATH="$BRIDGE_FLUTTER_ROOT_DIR/bin:$PATH"
    sed -i '' -e 's/extended_text: 14.0.0/extended_text: 13.0.0/g' flutter/pubspec.yaml   # bridge.yml:93
    remember flutter/pubspec.yaml
    (cd flutter && flutter --suppress-analytics pub get)
    remember flutter/pubspec.lock
    flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs --dart-output ./flutter/lib/generated_bridge.dart \
      --c-output ./flutter/macos/Runner/bridge_generated.h
    cp ./flutter/macos/Runner/bridge_generated.h ./flutter/ios/Runner/bridge_generated.h
  )
  safe_restore flutter/pubspec.yaml flutter/pubspec.lock
  rm -rf flutter/.dart_tool   # resolved against 3.22.3; let 3.24.5 resolve its own
  for f in "${bridge_outputs[@]}"; do [[ -f "$f" ]] || die "bridge generation did not produce $f"; done
  echo "$bridge_key" > "$bridge_stamp"
fi

# --- build.py ----------------------------------------------------------------------------------
# flutter-build.yml:913-918, aarch64-apple-darwin branch.
sed -i '' -e "s/MACOSX_DEPLOYMENT_TARGET\=[0-9]*.[0-9]*/MACOSX_DEPLOYMENT_TARGET=${MIN_MACOS_VERSION}/" build.py
sed -i '' -e "s/platform :osx, '.*'/platform :osx, '${MIN_MACOS_VERSION}'/" flutter/macos/Podfile
sed -i '' -e "s/osx_minimum_system_version = \"[0-9]*.[0-9]*\"/osx_minimum_system_version = \"${MIN_MACOS_VERSION}\"/" Cargo.toml
sed -i '' -e "s/MACOSX_DEPLOYMENT_TARGET = [0-9]*.[0-9]*;/MACOSX_DEPLOYMENT_TARGET = ${MIN_MACOS_VERSION};/" flutter/macos/Runner.xcodeproj/project.pbxproj
remember "${CI_EDITED[@]}"

export PATH="$FLUTTER_ROOT_DIR/bin:$PATH"
# Xcode 27 rejects deployment targets below 12.0, and the pod targets keep their podspecs'
# 10.13/10.14 (the Podfile's `platform :osx` does not change them). flutter forwards FLUTTER_XCODE_*
# variables to xcodebuild as command-line build settings, which apply to every target including
# the Pods project, so this raises all of them to CI's MIN_MACOS_VERSION without a source change.
# (On the macos-14 runner's Xcode these were only warnings.)
export FLUTTER_XCODE_MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS_VERSION"
log "build.py --flutter --hwcodec --unix-file-copy-paste --screencapturekit (Flutter $(flutter --suppress-analytics --version 2>/dev/null | sed -n 1p | awk '{print $2}'))"
# Pre-flight: cargo itself must agree on the target directory.
meta_dir="$(cargo metadata --format-version 1 --no-deps --offline 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' || true)"
[[ "$(cd "$meta_dir" 2>/dev/null && pwd -P)" == "$(cd "$CARGO_TARGET_CACHE" && pwd -P)" ]] \
  || die "cargo would build into '${meta_dir:-?}', not $CARGO_TARGET_CACHE"
rc=0
python3 ./build.py --flutter --hwcodec --unix-file-copy-paste --screencapturekit || rc=$?   # flutter-build.yml:765,920
# pod install and flutter's pub get rewrite these; record what they wrote so it can be undone.
remember "${TOOL_WRITTEN[@]}"
changed_by_tools="$(for f in "${CI_EDITED[@]}"; do
  [[ "$(shasum -a 256 "$f" | cut -d' ' -f1)" == "$(cat "$STATE_DIR/$(key "$f")")" ]] || echo "$f"; done)"
[[ -z "$changed_by_tools" ]] || echo "note: changed during build.py (will be left as is): $changed_by_tools"
((rc == 0)) || die "build.py failed (exit $rc)"

app_src="$RUSTDESK_DIR/flutter/build/macos/Build/Products/Release/RustDesk.app"
[[ -x "$app_src/Contents/MacOS/RustDesk" ]] || die "build.py finished but $app_src is missing"
rm -rf "$OUT_DIR/RustDesk.app"
ditto "$app_src" "$OUT_DIR/RustDesk.app"
# Make the local copy launchable. Xcode leaves the app ad-hoc signed WITH the hardened runtime, and
# then library validation refuses the ad-hoc FlutterMacOS.framework ("different Team IDs"), so the
# app dies in dyld. That is the same "unsigned" app CI uploads; CI only launches it after signing
# everything with a Developer ID (.github/scripts/sign-macos-app.sh). Locally we re-sign the copy
# ad hoc without the hardened runtime, keeping upstream's Release.entitlements. Real signing
# (Apple Development / Developer ID) is scripts/release-mac.sh's job.
codesign --force --deep --sign - --entitlements "$RUSTDESK_DIR/flutter/macos/Runner/Release.entitlements" "$OUT_DIR/RustDesk.app"
codesign --verify --deep --strict "$OUT_DIR/RustDesk.app"

elapsed=$(( $(date +%s) - start_ts ))
log "done in $((elapsed / 60))m$((elapsed % 60))s"
echo "app:    $OUT_DIR/RustDesk.app ($(du -sh "$OUT_DIR/RustDesk.app" | cut -f1))"
echo "arch:   $(lipo -archs "$OUT_DIR/RustDesk.app/Contents/MacOS/RustDesk")"
echo "target: $CARGO_TARGET_CACHE ($(du -sh "$CARGO_TARGET_CACHE" | cut -f1))"
