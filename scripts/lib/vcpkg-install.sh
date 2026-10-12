#!/usr/bin/env bash
# vcpkg manifest install for vendor/rustdesk, as flutter-build.yml:885-899 does it: run from the
# repo root (vcpkg.json, overlay ports in res/vcpkg) into $VCPKG_ROOT/installed. Idempotent: an
# unchanged manifest is a no-op; built packages are cached in $VCPKG_DEFAULT_BINARY_CACHE.
set -euo pipefail
# shellcheck source=mac-env.sh
source "$(dirname "$0")/mac-env.sh"
[[ -x "$VCPKG_ROOT/vcpkg" ]] || die "vcpkg missing; run scripts/bootstrap-mac.sh"
log "vcpkg install ($VCPKG_TRIPLET) for $RUSTDESK_DIR/vcpkg.json"
cd "$RUSTDESK_DIR"
if ! "$VCPKG_ROOT/vcpkg" install --triplet "$VCPKG_TRIPLET" --x-install-root="$VCPKG_ROOT/installed"; then
  find "$VCPKG_ROOT/buildtrees" -name '*.log' -newer "$VCPKG_ROOT/vcpkg" -print 2>/dev/null | tail -20 >&2
  die "vcpkg install failed; logs listed above"
fi
[[ -d "$VCPKG_ROOT/installed/$VCPKG_TRIPLET/include/vpx" ]] || die "vcpkg finished but libvpx headers are missing"
