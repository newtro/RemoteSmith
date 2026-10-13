# shellcheck shell=bash
# What RemoteSmith adds to the macOS app bundle beyond upstream's Xcode project, shared by
# scripts/build-mac.sh (local ad-hoc copy and the release archive) and scripts/release-mac.sh.
# Source after lib/mac-env.sh (uses REPO_ROOT, MIN_MACOS_VERSION, log, die).
#
#   Contents/MacOS/service                                upstream's root service binary. build.py
#                                                         copies it in after `flutter build macos`,
#                                                         so an xcodebuild archive lacks it.
#   Contents/Library/LoginItems/RemoteSmith Tray.app      the sidecar (sidecar/, DEC-20) as an
#                                                         LSUIElement login-item helper; patch 0601
#                                                         registers it with SMAppService.

TRAY_HELPER_NAME="RemoteSmith Tray.app"
TRAY_BUNDLE_ID="ai.johnnycode.remotesmith.tray"
TRAY_TARGET="aarch64-apple-darwin"

# build_tray_helper <out_dir> <version> <build_number> <icns>: build the sidecar with its own
# toolchain (sidecar/rust-toolchain.toml) and wrap it as <out_dir>/RemoteSmith Tray.app.
build_tray_helper() {
  local out="$1" version="$2" build="$3" icon="$4"
  local app="$out/$TRAY_HELPER_NAME" exe
  log "sidecar: cargo build --release --target $TRAY_TARGET (toolchain from sidecar/rust-toolchain.toml)"
  # build-mac.sh pins RUSTUP_TOOLCHAIN and CARGO_TARGET_DIR for vendor/rustdesk; neither applies here.
  (cd "$REPO_ROOT/sidecar" && env -u RUSTUP_TOOLCHAIN -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET \
      MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS_VERSION" \
      cargo build --locked --release --target "$TRAY_TARGET" --bin remotesmith-tray)
  exe="$REPO_ROOT/sidecar/target/$TRAY_TARGET/release/remotesmith-tray"
  [[ -x "$exe" ]] || die "sidecar build did not produce $exe"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$exe" "$app/Contents/MacOS/remotesmith-tray"
  sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" -e "s/@MIN_MACOS@/$MIN_MACOS_VERSION/" \
    "$REPO_ROOT/sidecar/macos/Info.plist.in" >"$app/Contents/Info.plist"
  plutil -lint -s "$app/Contents/Info.plist" || die "bad helper Info.plist"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" == "$TRAY_BUNDLE_ID" ]] \
    || die "helper bundle id is not $TRAY_BUNDLE_ID"
  printf 'APPL????' >"$app/Contents/PkgInfo"
  if [[ -f "$icon" ]]; then cp "$icon" "$app/Contents/Resources/AppIcon.icns"; fi
}

# embed_bundle_extras <app> <helper_app> <service_binary>: copy both into the app bundle.
embed_bundle_extras() {
  local app="$1" helper="$2" service="$3"
  [[ -x "$service" ]] || die "service binary missing: $service"
  [[ -d "$helper" ]] || die "tray helper missing: $helper"
  cp -f "$service" "$app/Contents/MacOS/service"
  mkdir -p "$app/Contents/Library/LoginItems"
  rm -rf "$app/Contents/Library/LoginItems/$TRAY_HELPER_NAME"
  ditto "$helper" "$app/Contents/Library/LoginItems/$TRAY_HELPER_NAME"
}

# signing_authority <code>: the leaf certificate name a bundle is signed with ("" if ad hoc).
signing_authority() {
  codesign -dvv "$1" 2>&1 | sed -n 's/^Authority=//p' | head -n1
}

# reseal_with_extras <app>: after embed_bundle_extras, sign the new nested code with the identity
# the app already carries (hardened runtime, as Xcode signed the rest), then re-seal the app,
# keeping its identifier, entitlements, requirements and flags. Inside out, as codesign requires.
reseal_with_extras() {
  local app="$1" id
  id="$(signing_authority "$app")"
  [[ -n "$id" ]] || die "$app is not signed with a certificate; cannot re-seal it"
  local args=(--force --options runtime --timestamp=none --sign "$id")
  codesign "${args[@]}" "$app/Contents/MacOS/service"
  codesign "${args[@]}" "$app/Contents/Library/LoginItems/$TRAY_HELPER_NAME"
  codesign --force --timestamp=none --sign "$id" \
    --preserve-metadata=identifier,entitlements,requirements,flags,runtime "$app"
  codesign --verify --deep --strict "$app" || die "re-sealed $app does not verify"
}
