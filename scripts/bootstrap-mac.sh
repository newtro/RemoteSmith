#!/usr/bin/env bash
# Idempotent macOS arm64 toolchain setup for building vendor/rustdesk the way upstream CI does.
# Everything heavy goes under ~/.cache/remotesmith/ (override with REMOTESMITH_CACHE).
# The user's default rustup toolchain is never changed; build-mac.sh selects toolchains per command.
#
# Mirrors, at RustDesk 1.5.0:
#   vendor/rustdesk/.github/workflows/flutter-build.yml  job build-for-macOS, aarch64-apple-darwin
#   vendor/rustdesk/.github/workflows/bridge.yml         job generate_bridge (Flutter 3.22.3 row)
#
# Usage: scripts/bootstrap-mac.sh
set -euo pipefail
# shellcheck source=lib/mac-env.sh
source "$(dirname "$0")/lib/mac-env.sh"

[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || die "this script targets Apple Silicon macOS"
[[ -f "$RUSTDESK_DIR/vcpkg.json" ]] || die "vendor/rustdesk is not checked out; run: git submodule update --init --recursive --depth 1"
mkdir -p "$RS_CACHE" "$TOOLS_DIR/bin" "$VCPKG_DEFAULT_BINARY_CACHE"

# --- Xcode -------------------------------------------------------------------------------------
# GitHub's macos-14 image ships Xcode + CocoaPods; here they have to exist already.
xcodebuild -version >/dev/null 2>&1 || die "full Xcode is required (xcodebuild not usable); install Xcode and run: sudo xcode-select -s /Applications/Xcode.app"
command -v python3 >/dev/null || die "python3 is required (build.py)"

# --- Homebrew packages -------------------------------------------------------------------------
# flutter-build.yml:815-823 installs `llvm create-dmg` and pkg-config.
#  - pkg-config: installed below (as pkgconf, brew's provider of pkg-config).
#  - cocoapods: preinstalled on the runner image, needed by `flutter build macos`.
#  - llvm: NOT installed. brew's llvm is keg-only under /opt/homebrew/opt/llvm, which is not on
#    clang-sys's macOS search path (it probes /usr/local/opt/llvm*, the CommandLineTools and the
#    Xcode toolchain), so on the arm64 runner bindgen already uses Xcode's libclang. LLVM_VERSION
#    15.0.6 (flutter-build.yml:26) is only used by the Windows/Linux jobs.
#  - create-dmg: NOT installed; we ship the .app, signing/dmg is scripts/release-mac.sh's job.
command -v brew >/dev/null || die "Homebrew is required"
need_brew=()
command -v pkg-config >/dev/null || need_brew+=(pkgconf)
command -v pod >/dev/null || need_brew+=(cocoapods)
if ((${#need_brew[@]})); then
  log "brew install ${need_brew[*]}"
  brew install "${need_brew[@]}"
fi

# --- NASM --------------------------------------------------------------------------------------
# flutter-build.yml:825-834 installs NASM 2.16.03 from nasm.us (explicitly not brew's NASM 3.x).
# The overlay port res/vcpkg/aom/portfile.cmake:2 requires nasm even on arm64. The prebuilt
# macosx zip CI uses is i386/x86_64 only, which runs on the runner through Rosetta but not on a Mac
# without Rosetta, so we build the same 2.16.03 release natively from the official source tarball.
NASM_SHA256=1412a1c760bbd05db026b6c0d1657affd6631cd0a63cddb6f73cc6d4aa616148   # nasm-2.16.03.tar.xz
if "$TOOLS_DIR/bin/nasm" -v 2>/dev/null | grep -q "version $NASM_VERSION "; then
  echo "nasm $NASM_VERSION: present"
else
  log "building NASM $NASM_VERSION from source into $TOOLS_DIR"
  src="$RS_CACHE/downloads/nasm-$NASM_VERSION.tar.xz"
  mkdir -p "$RS_CACHE/downloads"
  [[ -f "$src" ]] && echo "$NASM_SHA256  $src" | shasum -a 256 -c --status \
    || curl -fsSL --retry 3 -o "$src" "https://www.nasm.us/pub/nasm/releasebuilds/$NASM_VERSION/nasm-$NASM_VERSION.tar.xz"
  echo "$NASM_SHA256  $src" | shasum -a 256 -c || die "nasm source checksum mismatch"
  bdir="$(mktemp -d "${TMPDIR:-/tmp}/remotesmith-nasm.XXXXXX")"
  tar -xJf "$src" -C "$bdir"
  (cd "$bdir/nasm-$NASM_VERSION" && ./configure --prefix="$TOOLS_DIR" >/dev/null && make -j"$(sysctl -n hw.ncpu)" nasm ndisasm >/dev/null \
    && install -m 755 nasm ndisasm "$TOOLS_DIR/bin/")
  rm -rf "$bdir"
fi
nasm -v

# --- Flutter -----------------------------------------------------------------------------------
# flutter-build.yml:836-840 (subosito/flutter-action, stable, 3.24.5) and bridge.yml:79-84 (3.22.3).
# subosito downloads the same official archive; sha256 values are from releases_macos.json.
install_flutter() { # version dir sha256
  local ver="$1" dir="$2" sha="$3"
  if [[ -x "$dir/bin/flutter" && -f "$dir/.remotesmith-ok" ]]; then
    echo "flutter $ver: present at $dir"
    return
  fi
  log "installing Flutter $ver into $dir"
  local zip="$RS_CACHE/downloads/flutter_macos_arm64_${ver}-stable.zip"
  mkdir -p "$RS_CACHE/downloads"
  if [[ ! -f "$zip" ]] || ! echo "$sha  $zip" | shasum -a 256 -c --status; then
    curl -fsSL --retry 3 -o "$zip.part" \
      "https://storage.googleapis.com/flutter_infra_release/releases/stable/macos/flutter_macos_arm64_${ver}-stable.zip"
    mv "$zip.part" "$zip"
  fi
  echo "$sha  $zip" | shasum -a 256 -c || die "flutter $ver archive checksum mismatch"
  rm -rf "$dir" "$dir.tmp"
  mkdir -p "$dir.tmp"
  ditto -x -k "$zip" "$dir.tmp"
  mv "$dir.tmp/flutter" "$dir"
  rmdir "$dir.tmp"
  # Precache the macOS engine artifacts once; analytics off for this SDK's invocations only.
  "$dir/bin/flutter" --suppress-analytics --version
  "$dir/bin/flutter" --suppress-analytics precache --macos
  touch "$dir/.remotesmith-ok"
  rm -f "$zip"   # 1+ GB; the marker above means it is not needed again
}
install_flutter "$FLUTTER_VERSION" "$FLUTTER_ROOT_DIR" ca9624a6de7a1a8962f2f0a3e2a57b16434ca34f1137b8d9dc68c5ca1f187133
install_flutter "$BRIDGE_FLUTTER_VERSION" "$BRIDGE_FLUTTER_ROOT_DIR" 130f14d063e7cbe3fb7e982f6726a9a4cfa19164850119fa6bdd2875589ba70e

# Native arm64 gen_snapshot for macOS release builds. Flutter 3.24.5's tool uses
# darwin-x64-release/gen_snapshot_arm64, which is an x86_64 binary (it targets arm64 but runs on an
# Intel host), so `flutter build macos --release` needs Rosetta. The macos-14 runner has Rosetta;
# this Mac does not. The same engine build (bin/internal/engine.version) also publishes a native
# arm64-host gen_snapshot at darwin-arm64-release/artifacts.zip, which is what later Flutter
# releases use on Apple Silicon. Same engine commit, so the same Dart VM and AOT compiler, only the
# host architecture differs. No sha256 is published for it: the value below was taken from the
# download (it matches the object's GCS md5 W+hCanQ5jR91ZTC6PPT4cA==).
GEN_SNAPSHOT_ARM64_SHA256=e56194b76c8eab484af4a7dfb2e8fb6cb205a94543f9a87a3430c015978da143
engine="$(cat "$FLUTTER_ROOT_DIR/bin/internal/engine.version")"
gs="$FLUTTER_ROOT_DIR/bin/cache/artifacts/engine/darwin-x64-release/gen_snapshot_arm64"
[[ -f "$gs" ]] || die "missing $gs; precache did not fetch the macOS release engine"
if [[ "$(lipo -archs "$gs")" != arm64 ]]; then
  log "installing native arm64 gen_snapshot from engine $engine darwin-arm64-release"
  [[ "$engine" == a18df97ca57a249df5d8d68cd0820600223ce262 ]] || die "unexpected engine $engine for Flutter $FLUTTER_VERSION; update GEN_SNAPSHOT_ARM64_SHA256"
  gdir="$(mktemp -d "${TMPDIR:-/tmp}/remotesmith-gensnap.XXXXXX")"
  curl -fsSL --retry 3 -o "$gdir/a.zip" "https://storage.googleapis.com/flutter_infra_release/flutter/$engine/darwin-arm64-release/artifacts.zip"
  echo "$GEN_SNAPSHOT_ARM64_SHA256  $gdir/a.zip" | shasum -a 256 -c || die "darwin-arm64-release artifacts checksum mismatch"
  ditto -x -k "$gdir/a.zip" "$gdir/x"
  [[ "$(lipo -archs "$gdir/x/gen_snapshot")" == arm64 ]] || die "downloaded gen_snapshot is not arm64"
  mv "$gs" "$gs.x86_64"
  install -m 755 "$gdir/x/gen_snapshot" "$gs"
  rm -rf "$gdir"
fi
echo "gen_snapshot_arm64: $(lipo -archs "$gs")"

# flutter-build.yml:842-847 "Patch flutter": the dropdown_menu enableFilter fix for 3.24.x.
dropdown_diff="$RUSTDESK_DIR/.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff"
if git -C "$FLUTTER_ROOT_DIR" apply --check "$dropdown_diff" 2>/dev/null; then
  log "patching Flutter $FLUTTER_VERSION: dropdown_menu enableFilter"
  git -C "$FLUTTER_ROOT_DIR" apply "$dropdown_diff"
elif git -C "$FLUTTER_ROOT_DIR" apply --check --reverse "$dropdown_diff" 2>/dev/null; then
  echo "flutter dropdown patch: already applied"
else
  die "flutter dropdown patch neither applies nor is already applied"
fi
# flutter-build.yml:849-855 "Workaround for flutter issue" (flutter/flutter#133533).
binding="$FLUTTER_ROOT_DIR/packages/flutter/lib/src/scheduler/binding.dart"
if grep -q '^[[:space:]]*_setFramesEnabledState(false);' "$binding"; then
  log "patching Flutter $FLUTTER_VERSION: comment out _setFramesEnabledState(false)"
  sed -i '' -e 's/_setFramesEnabledState(false);/\/\/_setFramesEnabledState(false);/g' "$binding"
fi
grep -n '_setFramesEnabledState(false);' "$binding"

# --- Rust --------------------------------------------------------------------------------------
# flutter-build.yml:857-862 Rust 1.81 + target aarch64-apple-darwin + rustfmt.
# bridge.yml:66-71 Rust 1.75 + rustfmt for the codegen tools.
command -v rustup >/dev/null || die "rustup is required (brew install rustup)"
log "rust toolchains $MAC_RUST_VERSION and $BRIDGE_RUST_VERSION (default toolchain untouched)"
rustup toolchain install "$MAC_RUST_VERSION" --profile minimal --component rustfmt --target aarch64-apple-darwin --no-self-update
rustup toolchain install "$BRIDGE_RUST_VERSION" --profile minimal --component rustfmt --no-self-update

# bridge.yml:86-90 cargo-expand 1.0.95 and flutter_rust_bridge_codegen 1.80.1 --features uuid,
# both --locked and built with RUST_VERSION 1.75. Installed into $TOOLS_DIR, not ~/.cargo/bin.
cargo_tool() { # crate version binary [extra args]
  local crate="$1" ver="$2" bin="$3"; shift 3
  if [[ -x "$TOOLS_DIR/bin/$bin" ]] && "$TOOLS_DIR/bin/$bin" --version 2>/dev/null | grep -qF "$ver"; then
    echo "$crate $ver: present"
    return
  fi
  log "cargo install $crate $ver"
  rustup run "$BRIDGE_RUST_VERSION" cargo install "$crate" --version "$ver" --locked --root "$TOOLS_DIR" "$@"
}
cargo_tool cargo-expand "$CARGO_EXPAND_VERSION" cargo-expand
cargo_tool flutter_rust_bridge_codegen "$FLUTTER_RUST_BRIDGE_VERSION" flutter_rust_bridge_codegen --features uuid

# --- vcpkg -------------------------------------------------------------------------------------
# flutter-build.yml:879-883 (lukka/run-vcpkg at VCPKG_COMMIT_ID) and :885-899 (manifest install
# from the repo root into $VCPKG_ROOT/installed; host triplet arm64-osx).
if [[ "$(git -C "$VCPKG_ROOT" rev-parse HEAD 2>/dev/null || true)" != "$VCPKG_COMMIT_ID" ]]; then
  log "vcpkg at $VCPKG_COMMIT_ID"
  if [[ ! -d "$VCPKG_ROOT/.git" ]]; then
    # Blobless clone: vcpkg reads port history from git objects, which are fetched on demand.
    git clone --filter=blob:none https://github.com/microsoft/vcpkg.git "$VCPKG_ROOT"
  fi
  git -C "$VCPKG_ROOT" fetch --filter=blob:none origin
  git -C "$VCPKG_ROOT" -c advice.detachedHead=false checkout --force "$VCPKG_COMMIT_ID"
  rm -f "$VCPKG_ROOT/vcpkg"
fi
if [[ ! -x "$VCPKG_ROOT/vcpkg" ]]; then
  "$VCPKG_ROOT/bootstrap-vcpkg.sh" -disableMetrics
fi
"$(dirname "$0")/lib/vcpkg-install.sh"

log "bootstrap complete: $RS_CACHE ($(du -sh "$RS_CACHE" | cut -f1))"
