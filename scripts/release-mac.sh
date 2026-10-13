#!/usr/bin/env bash
# Build, sign with Developer ID, notarize and package RemoteSmith for macOS arm64 (DEC-14).
#
#   scripts/release-mac.sh [--skip-bridge] [--allow-dirty]
#
# Follows the iSmith release pattern (docs/IMPLEMENTATION.md "Signing"). There is no local
# Developer ID certificate: Xcode signs with the cloud-managed one of the Apple ID signed in to
# Xcode (Settings > Accounts, team 232A77467G), and uploads to the notary service with the same
# account, so no API key or app-specific password is needed.
#
#   1. Checks: Xcode account usable, patches/ applied to vendor/rustdesk with nothing uncommitted.
#   2. scripts/build-mac.sh --archive: build.py, then `xcodebuild archive` of the Runner (Apple
#      Development signature) plus the service and the sidecar login-item helper.
#   3. -exportArchive method=developer-id destination=upload: Developer ID signature with the
#      hardened runtime on every nested binary, uploaded for notarization.
#   4. Poll -exportNotarizedApp until Apple accepts it; that export is stapled.
#   5. Check: codesign --verify --deep --strict, Developer ID + runtime on every Mach-O,
#      spctl (Gatekeeper), stapler validate, the bundled sidecar runs (--version).
#   6. RemoteSmith-<version>-arm64.dmg (app + /Applications link) and a .zip of the stapled app.
#
# Output: build/release/<version>/ (logs, archive, notarized app, dmg, zip, verify.txt).
set -euo pipefail
# shellcheck source=lib/mac-env.sh
source "$(dirname "$0")/lib/mac-env.sh"
# shellcheck source=lib/mac-bundle.sh
source "$(dirname "$0")/lib/mac-bundle.sh"

TEAM=232A77467G
build_args=()
allow_dirty=0
for arg in "$@"; do
  case "$arg" in
    --skip-bridge) build_args+=(--skip-bridge) ;;
    --allow-dirty) allow_dirty=1 ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

[[ "$(uname -s)/$(uname -m)" == Darwin/arm64 ]] || die "release-mac.sh runs on Apple Silicon macOS only"

# --- 1. checks -----------------------------------------------------------------------------------
log "checks"
# The submodules must carry exactly patches/ (one remotesmith(NNNN) commit per file, nothing else,
# nothing uncommitted), so the release is reproducible from this superproject commit.
check_patched() { # repo patches_dir pin
  local repo="$1" dir="$2" pin="$3" want have
  want="$(find "$dir" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.patch' -exec basename {} \; | cut -c1-4 | sort)"
  have="$(git -C "$repo" log --format=%s "$pin..HEAD" | sed -nE 's/^remotesmith\(([0-9]{4})\):.*/\1/p' | sort)"
  [[ "$want" == "$have" ]] || die "$repo does not carry exactly $dir (run scripts/apply-patches.sh)
  patches: $(echo "$want" | tr '\n' ' ')
  commits: $(echo "$have" | tr '\n' ' ')"
  [[ "$(git -C "$repo" rev-list --count "$pin..HEAD")" == "$(echo "$want" | grep -c .)" ]] \
    || die "$repo has commits above the pin that are not RemoteSmith patches"
}
rs_pin="$(git -C "$REPO_ROOT" ls-tree HEAD vendor/rustdesk | awk '{print $3}')"
hbb_pin="$(git -C "$RUSTDESK_DIR" ls-tree "$rs_pin" libs/hbb_common | awk '{print $3}')"
check_patched "$RUSTDESK_DIR" "$REPO_ROOT/patches/rustdesk" "$rs_pin"
check_patched "$RUSTDESK_DIR/libs/hbb_common" "$REPO_ROOT/patches/hbb_common" "$hbb_pin"
[[ -z "$(git -C "$RUSTDESK_DIR" status --porcelain --ignore-submodules=all -- . ':!target')" ]] \
  || die "vendor/rustdesk has uncommitted changes"
[[ -z "$(git -C "$RUSTDESK_DIR/libs/hbb_common" status --porcelain)" ]] || die "libs/hbb_common has uncommitted changes"
if ((!allow_dirty)); then
  [[ -z "$(git -C "$REPO_ROOT" status --porcelain --ignore-submodules=all)" ]] \
    || die "the superproject has uncommitted changes (commit them, or pass --allow-dirty for a test release)"
fi
# The Developer ID cert lives in Apple's cloud; Xcode needs a signed-in account for the team.
defaults read com.apple.dt.Xcode DVTDeveloperAccountManagerAppleIDLists >/dev/null 2>&1 \
  || die "no Apple ID is signed in to Xcode. Open Xcode > Settings > Accounts, add the Apple ID of team $TEAM, then rerun."
security find-identity -v -p codesigning | grep -q '"Apple Development: ' \
  || echo "note: no local Apple Development identity; xcodebuild -allowProvisioningUpdates will create one"

version="$(sed -n 's/^version: \([0-9.]*\).*/\1/p' "$RUSTDESK_DIR/flutter/pubspec.yaml" | head -n1)"
[[ -n "$version" ]] || die "could not read the version from vendor/rustdesk/flutter/pubspec.yaml"
OUT="$REPO_ROOT/build/release/$version"
rm -rf "$OUT"
mkdir -p "$OUT"
exec > >(tee "$OUT/release.log") 2>&1
echo "release $version -> $OUT"

# --- 2. build + archive --------------------------------------------------------------------------
log "build + archive (scripts/build-mac.sh --archive)"
ARCHIVE="$OUT/RemoteSmith.xcarchive"
"$REPO_ROOT/scripts/build-mac.sh" "${build_args[@]}" --archive "$ARCHIVE" --team "$TEAM"
app_name="$(basename "$(find "$ARCHIVE/Products/Applications" -maxdepth 1 -name '*.app' | head -n1)" .app)"
[[ -n "$app_name" ]] || die "no app in $ARCHIVE"

# --- 3. export for Developer ID and upload to the notary service ----------------------------------
log "export for Developer ID and upload for notarization"
cat >"$OUT/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>method</key><string>developer-id</string>
<key>destination</key><string>upload</string>
<key>signingStyle</key><string>automatic</string>
<key>teamID</key><string>$TEAM</string>
</dict></plist>
EOF
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportPath "$OUT/upload" \
  -exportOptionsPlist "$OUT/ExportOptions.plist" -allowProvisioningUpdates >"$OUT/upload.log" 2>&1 \
  || { tail -30 "$OUT/upload.log"; die "export/upload failed (log: $OUT/upload.log)"; }
tail -3 "$OUT/upload.log"

# --- 4. wait for notarization ---------------------------------------------------------------------
# Usually minutes; a first submission for a team can take an hour or more. Bounded at 3 hours.
log "waiting for notarization"
notarized=0
for attempt in $(seq 1 180); do
  if xcodebuild -exportNotarizedApp -archivePath "$ARCHIVE" -exportPath "$OUT/notarized" \
       >"$OUT/notarize.log" 2>&1; then
    notarized=1; break
  fi
  if ! grep -qi "processing\|in progress" "$OUT/notarize.log"; then
    tail -30 "$OUT/notarize.log"
    die "notarization failed (log: $OUT/notarize.log). Apple's log: xcrun notarytool log <id> needs credentials; open Xcode > Window > Organizer > the archive > Show Status Log"
  fi
  ((attempt % 5 == 0)) && echo "   still processing ($attempt min)"
  sleep 60
done
((notarized)) || die "notarization did not finish within 3 hours; rerun the export later:
  xcodebuild -exportNotarizedApp -archivePath '$ARCHIVE' -exportPath '$OUT/notarized'"
APP="$OUT/notarized/$app_name.app"
[[ -d "$APP" ]] || die "exportNotarizedApp produced no $app_name.app"

# --- 5. verify -----------------------------------------------------------------------------------
log "verify"
V="$OUT/verify.txt"
{
  echo "# $app_name $version, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "## codesign --verify --deep --strict"; codesign --verify --deep --strict --verbose=2 "$APP" 2>&1
  echo "## codesign -dvvv (app)"; codesign -dvvv "$APP" 2>&1
  echo "## entitlements (app)"; codesign -d --entitlements - --xml "$APP" 2>/dev/null | plutil -p - 2>&1
  echo "## spctl -a -t exec -vvv"; spctl -a -t exec -vvv "$APP" 2>&1
  echo "## stapler validate"; xcrun stapler validate "$APP" 2>&1
} >"$V"
cat "$V"
codesign --verify --deep --strict "$APP" || die "codesign verification failed"
spctl -a -t exec "$APP" || die "Gatekeeper rejected the app"
xcrun stapler validate "$APP" >/dev/null || die "the notarization ticket is not stapled"
# Every Mach-O in the bundle: Developer ID of the team and a secure timestamp; executables also
# the hardened runtime (the notary requires it of executables only; Xcode signs the embedded
# frameworks without it, as for any app).
bad=0
while IFS= read -r -d '' f; do
  kind="$(file -b "$f")"
  [[ "$kind" == *Mach-O* ]] || continue
  info="$(codesign -dvv "$f" 2>&1)"
  if ! grep -q "^Authority=Developer ID Application: .*($TEAM)" <<<"$info" || ! grep -q '^Timestamp=' <<<"$info"; then
    echo "not Developer ID + timestamp: ${f#"$APP/"}"; bad=1
  elif [[ "$kind" == *executable* ]] && ! grep -qE '^CodeDirectory .*flags=0x[0-9a-f]*\(.*runtime' <<<"$info"; then
    echo "executable without the hardened runtime: ${f#"$APP/"}"; bad=1
  else
    echo "ok  ${f#"$APP/"}"
  fi
done < <(find "$APP" -type f -print0) >>"$V"
grep -v '^ok ' "$V" | grep -E '^(not Developer|executable without)' || true
((bad == 0)) || die "some nested code is not signed for distribution (above)"
helper="$APP/Contents/Library/LoginItems/$TRAY_HELPER_NAME"
[[ -x "$APP/Contents/MacOS/service" ]] || die "the notarized app has no Contents/MacOS/service"
[[ -x "$helper/Contents/MacOS/remotesmith-tray" ]] || die "the notarized app has no tray helper"
{ echo "## bundled sidecar"; "$helper/Contents/MacOS/remotesmith-tray" --version; } | tee -a "$V"

# --- 6. package ----------------------------------------------------------------------------------
log "package"
DMG="$OUT/$app_name-$version-arm64.dmg"
ZIP="$OUT/$app_name-$version-arm64.zip"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
stage="$OUT/dmg"
rm -rf "$stage"; mkdir -p "$stage"
ditto "$APP" "$stage/$app_name.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "$app_name $version" -srcfolder "$stage" -fs HFS+ -format UDZO "$DMG"
rm -rf "$stage"
# The dmg itself is not signed: the Developer ID key is only usable through Xcode's export, which
# signs app bundles. Gatekeeper checks the stapled app inside when it is first opened.
mnt="$(mktemp -d)"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$DMG"
{ echo "## dmg: spctl + stapler on the app inside"
  spctl -a -t exec -vvv "$mnt/$app_name.app" 2>&1; xcrun stapler validate "$mnt/$app_name.app" 2>&1; } | tee -a "$V"
spctl -a -t exec "$mnt/$app_name.app" || { hdiutil detach -quiet "$mnt"; die "Gatekeeper rejects the app in the dmg"; }
hdiutil detach -quiet "$mnt"
rmdir "$mnt" 2>/dev/null || true
shasum -a 256 "$DMG" "$ZIP" | tee "$OUT/SHA256SUMS"

log "done"
echo "dmg:    $DMG"
echo "zip:    $ZIP"
echo "verify: $V"
