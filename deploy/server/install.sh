#!/usr/bin/env bash
# RemoteSmith server: install, upgrade, roll back and inspect rustdesk-server OSS
# (hbbs + hbbr) under systemd on the server. Run ON the server as root:
#
#   sudo ./install.sh                     install, or upgrade to the pinned version
#   sudo ./install.sh --status            health report (changes nothing)
#   sudo ./install.sh --rollback          swap back to the previous release
#   sudo ./install.sh --restore-data F    restore data/ (the keys) from a backup tarball
#        ./install.sh --dry-run [...]     print every action without changing anything
#        ./install.sh --render-units DIR  write the two units, as they would be installed
#
# The server's public name (RS_HOST) comes from site.env next to this script (copy
# site.env.example), or --host NAME, which wins. --status and --rollback fall back to
# the name in the installed hbbs unit. Options: --version V (must be in the pinned
# table below), --asset FILE (use a local copy of the release zip; it is still
# checksum-verified), --arch amd64|arm64v8 (override uname -m; for a dry run on a Mac).
#
# Layout under /opt/remotesmith-server:
#   releases/<ver>/{hbbs,hbbr,rustdesk-utils}   one dir per installed version (root-owned)
#   bin -> releases/<ver>                        the live version (units run bin/hbbs, bin/hbbr)
#   previous -> releases/<ver>                   the version --rollback returns to
#   data/                                        keys + peer DB (remotesmith:remotesmith 0700)
#   backups/data-<UTC>-<ver>.tar.gz              taken before every change (root 0700)
#   libexec/{keycheck,wait-resolve}.sh           helpers the units run before start
#   kit/                                         copy of this deploy kit, so --status and
#                                                --rollback work without the repo checkout
#   kits/<ver>/                                  the kit (units, helpers) last installed with
#                                                each version; rollback reinstalls it
#   id_ed25519.pub                               public copy of the server key (0644)
#
# It touches nothing outside that tree except: the remotesmith system user, the two
# remotesmith-*.service units (backed up as .bak.<UTC> before replacement), and
# systemctl daemon-reload. Firewall and DNS are separate (firewall.sh, dns.md).
set -euo pipefail

DEFAULT_VERSION=1.1.16
PREFIX=/opt/remotesmith-server
UNIT_DIR=/etc/systemd/system
SVC_USER=remotesmith
HBBS=remotesmith-hbbs.service
HBBR=remotesmith-hbbr.service
RELEASE_URL=https://github.com/rustdesk/rustdesk-server/releases/download
RELAY_PORT=21117
# Set by load_site (site.env / --host); RELAY is what hbbs must hand out.
RS_HOST=
RELAY=

# Pinned release assets: <version>/<asset arch> -> SHA-256 of
# rustdesk-server-linux-<asset arch>.zip, as published on the GitHub release (asset
# digest) and re-checked by download. 1.1.15 is kept only as a known-good rollback
# target and for rehearsing upgrades; 1.1.16 is the release v1 runs (UDP punch-hole
# amplification fix, PLAN §9).
asset_sha256() {
  case "$1/$2" in
    1.1.16/amd64)   echo 0565c41affe6c3f409b0bddfaf5a24ccbf3f64f5f8e3fec250b69a0d5f6bdbcf ;;
    1.1.16/arm64v8) echo 6a4ae3c5ca257a4278ded72fd17eb2ca4eeb0356a5425e63a3e7fcb0ec6c155c ;;
    1.1.15/amd64)   echo c553972fd844c0224bc18eb3776f48ee5e018c6d4748729e1cfb14d32a46b394 ;;
    1.1.15/arm64v8) echo 4998dd6d32431f9aaf5841663339793bc154d7152313e128832d6b610580abe4 ;;
    *) return 1 ;;
  esac
}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
MODE=install
DRY_RUN=0
VERSION=$DEFAULT_VERSION
ASSET_FILE=
ARCH=
RESTORE_FILE=
HOST_FLAG=
RENDER_DIR=
STAGE=
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
TMP=

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'WARNING: %s\n' "$*" >&2; }
die()  { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# run CMD...: execute, or in --dry-run print it. Every state change goes through here.
run() {
  if [[ $DRY_RUN == 1 ]]; then
    printf 'DRY-RUN: %s\n' "$*"
  else
    "$@"
  fi
}

usage() { sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --dry-run) DRY_RUN=1 ;;
      --status) MODE=status ;;
      --rollback) MODE=rollback ;;
      --restore-data) MODE=restore; RESTORE_FILE=${2:?--restore-data needs a tarball}; shift ;;
      --version) VERSION=${2:?--version needs a value}; shift ;;
      --asset) ASSET_FILE=${2:?--asset needs a file}; shift ;;
      --arch) ARCH=${2:?--arch needs amd64 or arm64v8}; shift ;;
      --host) HOST_FLAG=${2:?--host needs a name}; shift ;;
      --render-units) MODE=render; RENDER_DIR=${2:?--render-units needs a directory}; shift ;;
      -h|--help) usage 0 ;;
      *) printf 'unknown argument: %s\n' "$1" >&2; usage 2 ;;
    esac
    shift
  done
}

cleanup() {
  if [[ -n $TMP && -d $TMP ]]; then rm -rf "$TMP"; fi
  if [[ -n $STAGE && -d $STAGE ]]; then rm -rf "$STAGE"; fi
}

# shellcheck disable=SC1091  # site-env.sh is beside this script; shellchecked on its own
. "$SCRIPT_DIR/site-env.sh" || { printf 'ERROR: %s/site-env.sh is missing; copy the whole kit\n' "$SCRIPT_DIR" >&2; exit 1; }

# The host the installed hbbs unit hands out as its relay, or empty.
installed_host() {
  [[ -r $UNIT_DIR/$HBBS ]] || return 0
  sed -n "s/^ExecStart=.* --relay-servers \([^ :]*\):$RELAY_PORT .*/\1/p" "$UNIT_DIR/$HBBS" | head -1 || true
}

# Decide RS_HOST: --host, else site.env beside this script, else (only for modes that
# act on an existing install) the name in the installed unit. Install and render need
# one; status, rollback and restore check the relay against the installed unit anyway
# (health), so they only warn.
load_site() {
  local from
  if [[ -f $SCRIPT_DIR/site.env ]]; then
    site_env_load "$SCRIPT_DIR/site.env" || die "fix $SCRIPT_DIR/site.env (see site.env.example)"
  fi
  if [[ -n $HOST_FLAG ]]; then RS_HOST=$HOST_FLAG; from="--host"
  elif [[ -n $RS_HOST ]]; then from="$SCRIPT_DIR/site.env"
  elif [[ $MODE == status || $MODE == rollback || $MODE == restore ]]; then
    RS_HOST=$(installed_host); from="the installed $HBBS"
  fi
  if [[ -z $RS_HOST ]]; then
    local msg="no server name: set RS_HOST in $SCRIPT_DIR/site.env (copy site.env.example) or pass --host NAME"
    [[ $MODE == install || $MODE == render ]] && die "$msg"
    warn "$msg"
    return 0
  fi
  valid_hostname "$RS_HOST" || die "RS_HOST '$RS_HOST' (from $from) is not a valid DNS name"
  RELAY=$RS_HOST:$RELAY_PORT
  log "server name $RS_HOST (from $from)"
}

# render_units DIR: write both units as they are installed. hbbs is rendered from
# remotesmith-hbbs.service.in (RS_HOST was validated as a DNS name, so it is safe
# to substitute); hbbr has no per-site values and is copied.
render_units() {
  local dir=$1 tmpl=$SCRIPT_DIR/$HBBS.in
  [[ -f $tmpl ]] || die "missing $tmpl"
  [[ -f $SCRIPT_DIR/$HBBR ]] || die "missing $SCRIPT_DIR/$HBBR"
  sed "s/@RS_HOST@/$RS_HOST/g" "$tmpl" > "$dir/$HBBS"
  if grep -q '@[A-Z_]*@' "$dir/$HBBS"; then die "$tmpl has a placeholder install.sh does not fill"; fi
  grep -qx "ExecStart=$PREFIX/bin/hbbs --port 21116 --relay-servers $RELAY --key _" "$dir/$HBBS" \
    || die "rendered $HBBS does not run hbbs with --relay-servers $RELAY"
  cp "$SCRIPT_DIR/$HBBR" "$dir/$HBBR"
}

# The kit as it is installed: this directory's files plus the rendered hbbs unit, in a
# temp dir. Units and helpers are installed from it, and kit/ and kits/<ver>/ are
# copies of it, so a rollback to a kits/<ver> snapshot needs no rendering.
stage_kit() {
  local f
  STAGE=$(mktemp -d)
  for f in "${KIT_FILES[@]}"; do
    [[ $f == "$HBBS" ]] && continue
    [[ -f $SCRIPT_DIR/$f ]] && cp -p "$SCRIPT_DIR/$f" "$STAGE/$f"
  done
  # The site.env kept on the box must name the host the unit was rendered with, or the
  # next run from kit/ would silently re-render with another one. So --host is written
  # into the staged copy (the rest of site.env is kept).
  if [[ -n $HOST_FLAG ]]; then
    # grep -v exits 1 when every line is RS_HOST=; that is not an error.
    { if [[ -f $SCRIPT_DIR/site.env ]]; then grep -vE '^[[:space:]]*RS_HOST=' "$SCRIPT_DIR/site.env" || true; fi
      printf 'RS_HOST=%s\n' "$RS_HOST"; } > "$STAGE/site.env"
  fi
  render_units "$STAGE"
}

# host:port the installed hbbs unit hands out, else the configured RELAY.
expected_relay() {
  local h
  h=$(installed_host)
  if [[ -n $h ]]; then printf '%s:%s' "$h" "$RELAY_PORT"; else printf '%s' "$RELAY"; fi
}

# Revert-on-failure. Once a run starts changing live state (units, helpers, the bin
# link), any failure exit (die, set -e, a failed command) reverts to the version and
# units that were live before the run. arm_revert records what to go back to.
REVERT_ARMED=0
REV_CUR=
REV_PREV=
arm_revert() { REV_CUR=$1; REV_PREV=$2; [[ $DRY_RUN == 1 ]] || REVERT_ARMED=1; }
disarm_revert() { REVERT_ARMED=0; }
revert_install() {
  if [[ -z $REV_CUR ]]; then
    warn "first install failed; there is no earlier version to revert to. Fix the cause and re-run."
    return 0
  fi
  warn "reverting to $REV_CUR and the units it last ran with"
  point_link bin "$REV_CUR"
  if [[ -n $REV_PREV && $REV_PREV != "$REV_CUR" ]]; then point_link previous "$REV_PREV"
  else rm -f "$PREFIX/previous"; fi
  restore_kit_for "$REV_CUR"
  restart_services
  if wait_healthy; then
    write_marker
    warn "reverted to $REV_CUR, which is healthy"
  else
    warn "the revert to $REV_CUR is unhealthy too; see journalctl -u $HBBS -u $HBBR"
  fi
}
on_exit() {
  local rc=$?
  if [[ $rc != 0 && $REVERT_ARMED == 1 ]]; then
    REVERT_ARMED=0
    set +e
    revert_install
  fi
  cleanup
  exit "$rc"
}
trap on_exit EXIT

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}

asset_arch() {
  if [[ -n $ARCH ]]; then echo "$ARCH"; return 0; fi
  case "$(uname -m)" in
    x86_64|amd64) echo amd64 ;;
    aarch64|arm64) echo arm64v8 ;;
    *) die "unsupported CPU architecture $(uname -m)" ;;
  esac
}

# Version that bin/ (or previous/) points at, or empty.
link_version() {
  local t
  t=$(readlink "$PREFIX/$1" 2>/dev/null) || return 0
  basename "$t"
}

is_active() { systemctl is-active --quiet "$1" 2>/dev/null; }

need_root() {
  [[ $DRY_RUN == 1 ]] && return 0
  [[ $(id -u) == 0 ]] || die "run as root (sudo $0 ...), or add --dry-run"
}

need_cmds() {
  local c missing=()
  for c in "$@"; do command -v "$c" >/dev/null 2>&1 || missing+=("$c"); done
  if [[ ${#missing[@]} -gt 0 ]]; then
    if [[ $DRY_RUN == 1 ]]; then warn "not found here (fine for a dry run): ${missing[*]}"
    else die "missing commands: ${missing[*]}"; fi
  fi
}

# Refuse to start if another program holds one of our ports. Ports held by our own
# hbbs/hbbr are fine (that is an upgrade).
check_ports() {
  command -v ss >/dev/null 2>&1 || { warn "ss not found; skipping port collision check"; return 0; }
  local p line bad=0
  for p in 21115 21116 21117 21118 21119; do
    while IFS= read -r line; do
      [[ -z $line ]] && continue
      if [[ $line != *'"hbbs"'* && $line != *'"hbbr"'* ]]; then
        warn "port $p is in use by something other than hbbs/hbbr: $line"
        bad=1
      fi
    done < <(ss -Hlntup "( sport = :$p )" 2>/dev/null || true)
  done
  [[ $bad == 0 ]] || die "port collision; resolve it before installing"
}

check_memory() {
  local avail_kb
  avail_kb=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || true)
  [[ -n $avail_kb ]] || return 0
  log "MemAvailable: $((avail_kb / 1024)) MiB (caps: hbbs 64M, hbbr 128M)"
  if [[ $avail_kb -lt 262144 ]]; then
    warn "less than 256 MiB available on this host"
  fi
}

ensure_user() {
  if getent passwd "$SVC_USER" >/dev/null 2>&1; then
    log "system user $SVC_USER exists"
  else
    log "creating system user $SVC_USER"
    run useradd --system --user-group --home-dir "$PREFIX/data" --no-create-home \
      --shell /usr/sbin/nologin --comment "RemoteSmith hbbs/hbbr" "$SVC_USER"
  fi
}

ensure_dirs() {
  run install -d -m 0755 -o root -g root "$PREFIX" "$PREFIX/releases" "$PREFIX/libexec"
  run install -d -m 0700 -o root -g root "$PREFIX/backups"
  run install -d -m 0700 -o "$SVC_USER" -g "$SVC_USER" "$PREFIX/data"
}

# Tar data/ into backups/ when it has anything in it. Prints the tarball path.
backup_data() {
  local reason=$1 cur out
  if [[ ! -d $PREFIX/data ]] || [[ -z $(ls -A "$PREFIX/data" 2>/dev/null) ]]; then
    log "data/ is empty or absent; nothing to back up"
    return 0
  fi
  cur=$(link_version bin)
  out="$PREFIX/backups/data-$STAMP-${cur:-none}-$reason.tar.gz"
  log "backing up data/ to $out"
  run tar -C "$PREFIX" -czf "$out" data
  run chmod 0600 "$out"
  if [[ $DRY_RUN == 0 && -f $PREFIX/data/id_ed25519 ]]; then
    tar -tzf "$out" | grep -qx 'data/id_ed25519' || die "backup $out does not contain the key"
  fi
}

fetch_release() {
  local arch sha url zip got
  arch=$(asset_arch)
  sha=$(asset_sha256 "$VERSION" "$arch") || die "version $VERSION/$arch is not in the pinned table"
  url="$RELEASE_URL/$VERSION/rustdesk-server-linux-$arch.zip"
  TMP=$(mktemp -d)
  zip="$TMP/rustdesk-server.zip"
  if [[ -n $ASSET_FILE ]]; then
    log "using local asset $ASSET_FILE"
    cp "$ASSET_FILE" "$zip"
  elif [[ $DRY_RUN == 1 ]]; then
    printf 'DRY-RUN: curl -fsSL %s -> %s, then require sha256 %s\n' "$url" "$zip" "$sha"
    return 0
  else
    log "downloading $url"
    curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$zip" "$url"
  fi
  got=$(sha256_of "$zip")
  [[ $got == "$sha" ]] || die "checksum mismatch for $VERSION/$arch: expected $sha, got $got"
  log "sha256 OK ($sha)"
  mkdir "$TMP/x"
  unzip -q "$zip" -d "$TMP/x"
  local b
  for b in hbbs hbbr rustdesk-utils; do
    [[ -f $TMP/x/$arch/$b ]] || die "release zip has no $arch/$b"
  done
  "$TMP/x/$arch/hbbr" --version 2>&1 | grep -q "$VERSION" \
    || warn "hbbr --version does not mention $VERSION: $("$TMP/x/$arch/hbbr" --version 2>&1 | head -1)"
}

install_release() {
  local arch dest b
  arch=$(asset_arch)
  dest="$PREFIX/releases/$VERSION"
  if [[ -d $dest ]]; then
    if [[ $DRY_RUN == 1 ]]; then
      log "releases/$VERSION already present (would re-verify its binaries)"
      return 0
    fi
    for b in hbbs hbbr rustdesk-utils; do
      [[ $(sha256_of "$dest/$b") == "$(sha256_of "$TMP/x/$arch/$b")" ]] \
        || die "$dest/$b differs from the verified release; investigate before reinstalling"
    done
    log "releases/$VERSION already installed and matches the verified release"
    return 0
  fi
  log "installing release $VERSION into $dest"
  run install -d -m 0755 -o root -g root "$dest.tmp"
  for b in hbbs hbbr rustdesk-utils; do
    run install -m 0755 -o root -g root "$TMP/x/$arch/$b" "$dest.tmp/$b"
  done
  run mv "$dest.tmp" "$dest"
}

# Atomically point PREFIX/<name> at releases/<ver>.
point_link() {
  local name=$1 ver=$2
  run ln -sfn "releases/$ver" "$PREFIX/.$name.new"
  run mv -T "$PREFIX/.$name.new" "$PREFIX/$name"
}

UNITS_CHANGED=0
# install_support_files [KITDIR]: install the helpers and unit files from KITDIR
# (default: this script's directory).
install_support_files() {
  local kit=${1:-$STAGE} f src dst
  for f in keycheck.sh wait-resolve.sh; do
    if [[ ! -f $kit/$f ]]; then
      # An older kit snapshot may predate a helper; keep the current copy then.
      [[ $kit == "$STAGE" ]] && die "missing $SCRIPT_DIR/$f"
      warn "$kit has no $f; keeping the installed one"
      continue
    fi
    if ! cmp -s "$kit/$f" "$PREFIX/libexec/$f" 2>/dev/null; then
      log "installing libexec/$f"
      run install -m 0755 -o root -g root "$kit/$f" "$PREFIX/libexec/$f"
      UNITS_CHANGED=1
    fi
  done
  for f in "$HBBS" "$HBBR"; do
    src="$kit/$f"
    dst="$UNIT_DIR/$f"
    [[ -f $src ]] || die "missing $src"
    if cmp -s "$src" "$dst" 2>/dev/null; then
      log "$f unchanged"
      continue
    fi
    if [[ -f $dst ]]; then
      log "backing up $dst to $dst.bak.$STAMP"
      run cp -p "$dst" "$dst.bak.$STAMP"
    fi
    log "installing $dst"
    run install -m 0644 -o root -g root "$src" "$dst"
    UNITS_CHANGED=1
  done
  if [[ $UNITS_CHANGED == 1 ]]; then
    run systemctl daemon-reload
  fi
}

# Keep a copy of the kit on the box: sudo /opt/remotesmith-server/kit/install.sh --status
# site.env goes with it, so the copy knows the server's name. remotesmith-hbbs.service
# is the rendered unit (stage_kit), next to the template it came from.
KIT_FILES=(install.sh site-env.sh site.env keycheck.sh wait-resolve.sh firewall.sh probe.py
  remotesmith-hbbs.service.in remotesmith-hbbs.service remotesmith-hbbr.service
  site.env.example README.md firewall.md dns.md)
# copy_kit DEST: copy the staged kit to DEST (skipped when DEST is where we run from:
# that copy already is the kit, and it holds the running script).
copy_kit() {
  local dest=$1 f
  [[ $SCRIPT_DIR == "$dest" ]] && return 0
  run install -d -m 0755 -o root -g root "$dest"
  for f in "${KIT_FILES[@]}"; do
    [[ -f $STAGE/$f ]] || continue
    cmp -s "$STAGE/$f" "$dest/$f" 2>/dev/null && continue
    case "$f" in *.sh|*.py) run install -m 0755 -o root -g root "$STAGE/$f" "$dest/$f" ;;
      *) run install -m 0644 -o root -g root "$STAGE/$f" "$dest/$f" ;; esac
  done
}

# Going back to a version also means going back to the units and helpers it ran with.
restore_kit_for() {
  local ver=$1
  if [[ -f $PREFIX/kits/$ver/$HBBS ]]; then
    log "reinstalling the units and helpers last used with $ver (kits/$ver)"
    install_support_files "$PREFIX/kits/$ver"
  elif [[ -f $UNIT_DIR/$HBBS.bak.$STAMP || -f $UNIT_DIR/$HBBR.bak.$STAMP ]]; then
    warn "no kits/$ver snapshot; restoring the unit files this run replaced (.bak.$STAMP)"
    local f
    for f in "$HBBS" "$HBBR"; do
      if [[ -f $UNIT_DIR/$f.bak.$STAMP ]]; then run cp -p "$UNIT_DIR/$f.bak.$STAMP" "$UNIT_DIR/$f"; fi
    done
    run systemctl daemon-reload
  else
    warn "no kits/$ver snapshot; keeping the current units and helpers"
  fi
}

# A public key as hbbs writes it: 32 bytes, standard base64. Anything else read from
# data/ (which the daemon owns) is never echoed or published.
valid_pubkey() { [[ $1 =~ ^[A-Za-z0-9+/]{43}=$ ]]; }

# Read data/id_ed25519.pub safely: refuse symlinks and non-regular files (data/ is
# writable by hbbs), read at most 64 bytes, and print it only if it is a valid key.
read_pubkey() {
  local f=$PREFIX/data/id_ed25519.pub v size
  [[ -f $f && ! -L $f ]] || return 1
  # Exactly the 44-byte key, optionally one trailing newline; nothing else.
  size=$(wc -c < "$f")
  [[ $size == 44 || ( $size == 45 && $(tail -c 1 "$f" | od -An -tx1 | tr -d ' \n') == 0a ) ]] || return 1
  v=$(head -c 44 "$f")
  valid_pubkey "$v" || return 1
  printf '%s' "$v"
}

# Fingerprint of what should be running: version + installed units + helpers.
deploy_fingerprint() {
  {
    readlink "$PREFIX/bin" 2>/dev/null
    cat "$UNIT_DIR/$HBBS" "$UNIT_DIR/$HBBR" "$PREFIX/libexec/keycheck.sh" "$PREFIX/libexec/wait-resolve.sh" 2>/dev/null
  } | sha256sum | awk '{print $1}'
}
# Recorded after every healthy install/rollback/restore. A run is only a no-op when the
# marker matches and the processes run the target binaries; an interrupted earlier run
# (even one killed before its revert) therefore always gets a restart.
write_marker() {
  [[ $DRY_RUN == 1 ]] && return 0
  deploy_fingerprint > "$PREFIX/.deployed"
  chmod 0644 "$PREFIX/.deployed"
}
# running_exe_ok UNIT BINARY: the unit's main process runs releases/$VERSION/BINARY.
running_exe_ok() {
  local pid exe want
  pid=$(systemctl show -p MainPID --value "$1" 2>/dev/null || true)
  [[ -n $pid && $pid != 0 ]] || return 1
  exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null) || return 1
  want=$(readlink -f "$PREFIX/releases/$VERSION/$2") || return 1
  [[ $exe == "$want" ]]
}
up_to_date() {
  [[ -f $PREFIX/.deployed && $(cat "$PREFIX/.deployed") == "$(deploy_fingerprint)" ]] || return 1
  is_active "$HBBS" && is_active "$HBBR" || return 1
  running_exe_ok "$HBBS" hbbs && running_exe_ok "$HBBR" hbbr
}

# The server key. hbbs could create it on first start, but 1.1.16 fails open while
# doing so (see keycheck.sh), so it is made here, before any daemon runs. An existing
# key is never replaced: a new key means re-keying every client.
KEY_CREATED=0
ensure_key() {
  local d=$PREFIX/data utils=$PREFIX/releases/$VERSION/rustdesk-utils out pk sk tmpd f
  if [[ -e $d/id_ed25519 || -e $d/id_ed25519.pub || -L $d/id_ed25519 || -L $d/id_ed25519.pub ]]; then
    if [[ $DRY_RUN == 1 ]]; then log "data/ has a keypair (would validate it)"; return 0; fi
    for f in id_ed25519 id_ed25519.pub; do
      [[ -f $d/$f && ! -L $d/$f ]] || die "data/$f is missing, a symlink or not a regular file; refusing to continue"
    done
    "$PREFIX/libexec/keycheck.sh" "$d" \
      || die "data/ holds an invalid or mismatched keypair. Not generating a new one (that re-keys every client); restore it with --restore-data (README, Keys)"
    log "existing keypair is valid"
    return 0
  fi
  log "no keypair yet: generating one with rustdesk-utils genkeypair"
  if [[ $DRY_RUN == 1 ]]; then
    printf 'DRY-RUN: %s genkeypair in a root-owned temp dir, then rename into data/\n' "$utils"
    return 0
  fi
  pk=
  for _ in $(seq 1 100); do
    out=$("$utils" genkeypair)
    pk=$(printf '%s\n' "$out" | awk '/^Public Key:/ {print $3}')
    sk=$(printf '%s\n' "$out" | awk '/^Secret Key:/ {print $3}')
    # hbbs only hands out public keys without '/' or ':'; keep to that.
    if [[ -n $pk && -n $sk && $pk != */* && $pk != *:* ]]; then break; fi
    pk=
  done
  [[ -n $pk ]] || die "could not generate a keypair with $utils"
  # Build the files in a fresh root-owned directory next to data/ (same filesystem, so
  # the final mv is a rename(2)). data/ itself is writable by the daemon, so nothing
  # is created or chowned there by name; rename never follows a destination symlink.
  tmpd=$(mktemp -d "$PREFIX/.keygen.XXXXXX")
  (umask 077; printf '%s' "$sk" > "$tmpd/id_ed25519"; printf '%s' "$pk" > "$tmpd/id_ed25519.pub")
  sk=; out=
  "$PREFIX/libexec/keycheck.sh" "$tmpd" || { rm -rf "$tmpd"; die "the generated keypair failed validation"; }
  chown "$SVC_USER:$SVC_USER" "$tmpd/id_ed25519" "$tmpd/id_ed25519.pub"
  for f in id_ed25519.pub id_ed25519; do
    if [[ -e $d/$f || -L $d/$f ]]; then rm -rf "$tmpd"; die "data/$f appeared while generating the key; refusing"; fi
    mv -T "$tmpd/$f" "$d/$f"
  done
  rmdir "$tmpd"
  "$PREFIX/libexec/keycheck.sh" "$d" || die "the installed keypair failed validation"
  log "generated server key $pk"
  KEY_CREATED=1
}

publish_pubkey() {
  [[ $DRY_RUN == 1 ]] && { printf 'DRY-RUN: install -m 0644 data/id_ed25519.pub %s/id_ed25519.pub\n' "$PREFIX"; return 0; }
  local pk tmpf
  pk=$(read_pubkey) || { warn "data/id_ed25519.pub is not a valid key file; not publishing it"; return 0; }
  tmpf=$(mktemp "$PREFIX/.pub.XXXXXX")
  printf '%s' "$pk" > "$tmpf"
  chmod 0644 "$tmpf"
  mv -T "$tmpf" "$PREFIX/id_ed25519.pub"
}

# hbbs first, then hbbr (hbbr is also ordered After= hbbs).
restart_services() {
  log "restarting $HBBS then $HBBR"
  # A deliberate restart shouldn't trip the crash-loop start limit left by earlier ones.
  run systemctl reset-failed "$HBBS" "$HBBR" 2>/dev/null || true
  # A failed start must not abort the script (set -e): the health check that follows
  # decides, and an upgrade then rolls back.
  run systemctl restart "$HBBS" || warn "$HBBS failed to start"
  run systemctl restart "$HBBR" || warn "$HBBR failed to start"
}

# Prints problems and returns non-zero if the install is unhealthy.
health() {
  local ok=0 pk unit p proto line
  for unit in "$HBBS" "$HBBR"; do
    if is_active "$unit"; then log "$unit active"; else warn "$unit is not active"; ok=1; fi
  done
  if [[ ! -L $PREFIX/data/id_ed25519 ]] && pk=$(read_pubkey) && "$PREFIX/libexec/keycheck.sh" "$PREFIX/data" 2>/dev/null; then
    log "keypair valid"
  else
    warn "keypair missing or invalid in $PREFIX/data"; ok=1
  fi
  for line in tcp:21115 tcp:21116 udp:21116 tcp:21117; do
    proto=${line%%:*}; p=${line#*:}
    if ss -Hln "--$proto" "( sport = :$p )" 2>/dev/null | grep -q .; then
      log "listening $proto/$p"
    else
      warn "nothing listening on $proto/$p"; ok=1
    fi
  done
  # Enforcement, checked live on the wire (not from logs, which rotate):
  # wrong key refused and right key accepted, on hbbs and on hbbr. hbbr treats
  # loopback as its admin console, so it is probed on this host's own address.
  if [[ -n ${pk:-} ]]; then
    local probe="$SCRIPT_DIR/probe.py" self_ip r
    r=$(python3 -I "$probe" punch 127.0.0.1 not-the-server-key 2>&1 || true)
    if [[ $r == LICENSE_MISMATCH ]]; then log "hbbs refuses a wrong key"
    else warn "hbbs did not refuse a wrong key: $r"; ok=1; fi
    r=$(python3 -I "$probe" punch 127.0.0.1 "$pk" 2>&1 || true)
    if [[ $r == ID_NOT_EXIST ]]; then log "hbbs accepts the server key"
    else warn "hbbs with the server key: $r"; ok=1; fi
    self_ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1)}' | head -1)
    if [[ -z $self_ip ]]; then
      warn "no non-loopback IPv4 address found; can't probe hbbr"; ok=1
    else
      r=$(python3 -I "$probe" relay "$self_ip" not-the-server-key 2>&1 || true)
      if [[ $r == refused* ]]; then log "hbbr refuses a wrong key"
      else warn "hbbr did not refuse a wrong key: $r"; ok=1; fi
      r=$(python3 -I "$probe" relay "$self_ip" "$pk" 2>&1 || true)
      if [[ $r == accepted ]]; then log "hbbr accepts the server key"
      else warn "hbbr with the server key: $r"; ok=1; fi
    fi
  fi
  # The relay hbbs hands out right now. hbbs resolves --relay-servers once at start
  # and drops the name if DNS fails; clients then get no relay until it restarts.
  # Expected: what the installed unit says (after a rollback, that is the restored one).
  local want
  want=$(expected_relay)
  line=$(python3 -I "$SCRIPT_DIR/probe.py" relays 2>&1 | tr '\n' ' ' | sed 's/ *$//' || true)
  if [[ -z $want ]]; then
    warn "no server name known (site.env, --host or the installed unit); can't check the relay hbbs hands out ('${line}')"
    ok=1
  elif [[ $line == "$want" ]]; then
    log "hbbs hands out relay $want"
  else
    warn "hbbs hands out relay '${line}', expected '$want'. Does ${want%:*} resolve? Fix DNS, then: sudo systemctl restart $HBBS"
    ok=1
  fi
  if [[ -n $RELAY && -n $want && $want != "$RELAY" ]]; then
    warn "the installed $HBBS hands out $want, but this kit's server name gives $RELAY"
  fi
  return $ok
}


# Retry health for a few seconds: the daemons log asynchronously.
wait_healthy() {
  [[ $DRY_RUN == 1 ]] && { printf 'DRY-RUN: health check (units active, keypair valid, ports listening, key enforced on the wire, relay handed out)\n'; return 0; }
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if health >/dev/null 2>&1; then health; return 0; fi
    sleep 1
  done
  health
}

do_install() {
  need_root
  need_cmds curl unzip tar systemctl useradd getent install ss python3 ip openssl sha256sum
  local cur prev
  stage_kit
  cur=$(link_version bin)
  prev=$(link_version previous)
  log "target version $VERSION; installed: ${cur:-none}; previous: ${prev:-none}"
  if [[ -z $cur ]]; then check_ports; fi
  if ! getent ahosts "$RS_HOST" >/dev/null 2>&1; then
    if [[ $DRY_RUN == 1 ]]; then warn "$RS_HOST does not resolve here"
    else die "$RS_HOST does not resolve. Create the DNS records first (dns.md): hbbs resolves its relay name only at start."; fi
  fi
  check_memory
  fetch_release
  ensure_user
  ensure_dirs
  backup_data "pre-install-$VERSION"
  install_release
  arm_revert "$cur" "$prev"
  install_support_files
  ensure_key
  local switched=0
  if [[ $cur != "$VERSION" ]]; then
    if [[ -n $cur ]]; then
      log "previous -> $cur (rollback target)"
      point_link previous "$cur"
    fi
    log "bin -> $VERSION"
    point_link bin "$VERSION"
    switched=1
  fi
  run systemctl enable "$HBBS" "$HBBR"
  if [[ $switched == 0 && $UNITS_CHANGED == 0 && $DRY_RUN == 0 ]] && up_to_date; then
    log "nothing changed and both daemons run releases/$VERSION with the installed units; not restarting"
  else
    restart_services
  fi
  # On failure the EXIT trap reverts to the version and units that were live before.
  wait_healthy || die "install of $VERSION (this kit) is unhealthy; see journalctl -u $HBBS -u $HBBR"
  disarm_revert
  write_marker
  if [[ $KEY_CREATED == 1 ]]; then backup_data "new-key"; fi
  publish_pubkey
  copy_kit "$PREFIX/kit"
  copy_kit "$PREFIX/kits/$VERSION"
  summary
}

do_rollback() {
  need_root
  local cur prev
  cur=$(link_version bin)
  prev=$(link_version previous)
  [[ -n $prev ]] || die "no previous release recorded; nothing to roll back to"
  [[ -d $PREFIX/releases/$prev ]] || die "previous release dir $PREFIX/releases/$prev is missing"
  [[ $prev != "$cur" ]] || die "previous and current are both $cur"
  log "rolling back $cur -> $prev"
  backup_data "pre-rollback-to-$prev"
  arm_revert "$cur" "$prev"
  point_link bin "$prev"
  if [[ -n $cur ]]; then point_link previous "$cur"; fi
  restore_kit_for "$prev"
  restart_services
  # On failure the EXIT trap returns to $cur.
  wait_healthy || die "unhealthy after rollback to $prev"
  disarm_revert
  write_marker
  publish_pubkey
  summary
}

do_restore() {
  need_root
  [[ -f $RESTORE_FILE ]] || die "no such file: $RESTORE_FILE"
  local entry kind
  # Our own backups hold only plain files and directories; refuse anything else
  # (symlinks, devices, hard links) so a bad tarball can't point data/ elsewhere.
  while IFS= read -r kind; do
    case "$kind" in
      -*|d*) ;;
      *) die "refusing $RESTORE_FILE: it contains a non-regular entry: $kind" ;;
    esac
  done < <(tar -tvzf "$RESTORE_FILE")
  # Only the files hbbs/hbbr keep in data/. In particular no .env: both daemons load
  # one from their working directory and would take settings from it.
  while IFS= read -r entry; do
    case "$entry" in
      data|data/|data/id_ed25519|data/id_ed25519.pub|data/db_v2.sqlite3|data/db_v2.sqlite3-wal|data/db_v2.sqlite3-shm|data/blacklist.txt|data/blocklist.txt) ;;
      *) die "refusing $RESTORE_FILE: unexpected entry '$entry' (only the key, peer DB and block lists are restored)" ;;
    esac
  done < <(tar -tzf "$RESTORE_FILE")
  local total
  total=$(tar -tvzf "$RESTORE_FILE" | awk '{s += $3} END {print s + 0}')
  [[ $total -le 268435456 ]] || die "refusing $RESTORE_FILE: it unpacks to $total bytes"
  TMP=$(mktemp -d)
  tar -C "$TMP" -xzf "$RESTORE_FILE" --no-same-owner
  "$SCRIPT_DIR/keycheck.sh" "$TMP/data" || die "$RESTORE_FILE does not hold a valid keypair"
  log "restoring data/ from $RESTORE_FILE (key $(cat "$TMP/data/id_ed25519.pub"))"
  backup_data "pre-restore"
  run systemctl stop "$HBBR" "$HBBS"
  if [[ -d $PREFIX/data ]]; then
    log "moving the current data/ aside to backups/data-replaced-$STAMP"
    run mv "$PREFIX/data" "$PREFIX/backups/data-replaced-$STAMP"
  fi
  run mv "$TMP/data" "$PREFIX/data"
  run chown -R "$SVC_USER:$SVC_USER" "$PREFIX/data"
  run chmod 0700 "$PREFIX/data"
  run systemctl reset-failed "$HBBS" "$HBBR" 2>/dev/null || true
  run systemctl start "$HBBS" || warn "$HBBS failed to start"
  run systemctl start "$HBBR" || warn "$HBBR failed to start"
  wait_healthy || die "unhealthy after restore; the replaced data is in backups/data-replaced-$STAMP"
  write_marker
  publish_pubkey
  summary
}

summary() {
  [[ $DRY_RUN == 1 ]] && return 0
  echo
  echo "RemoteSmith server $(link_version bin) is running (previous: $(link_version previous))."
  local relay
  relay=$(expected_relay)
  echo "  host: ${relay%:*}   key: $(cat "$PREFIX/data/id_ed25519.pub" 2>/dev/null)"
  echo "  clients: ID server ${relay%:*}, relay $relay, key as above"
}

do_status() {
  local unit pk
  echo "version:  $(link_version bin)   previous: $(link_version previous)"
  echo "releases: $(find "$PREFIX/releases" -mindepth 1 -maxdepth 1 -type d -printf '%f ' 2>/dev/null)"
  for unit in "$HBBS" "$HBBR"; do
    systemctl show "$unit" --no-pager \
      -p ActiveState,SubState,MainPID,NRestarts,MemoryCurrent,MemoryPeak,ActiveEnterTimestamp 2>/dev/null \
      | sed "s/^/$unit: /"
  done
  pk=$(cat "$PREFIX/id_ed25519.pub" 2>/dev/null || true)
  echo "public key: ${pk:-unknown}"
  echo "latest backup: $(find "$PREFIX/backups" -maxdepth 1 -name '*.tar.gz' -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -1 | cut -d' ' -f2-)"
  echo "listening:"
  ss -Hlntup 2>/dev/null | grep -E ':2111[5-9]\b' | sed 's/^/  /' || true
  if [[ $(id -u) == 0 ]]; then
    echo "health:"
    if health 2>&1 | sed 's/^/  /'; then
      echo "  HEALTHY"
    else
      echo "  UNHEALTHY"
      return 1
    fi
  else
    echo "(run with sudo for the full health check: key validity, live key-enforcement probes)"
  fi
}

main() {
  parse_args "$@"
  [[ $DRY_RUN == 1 ]] && log "dry run: nothing will be changed"
  load_site
  case "$MODE" in
    render) [[ -d $RENDER_DIR ]] || die "no such directory: $RENDER_DIR"; render_units "$RENDER_DIR"
            log "wrote $RENDER_DIR/$HBBS and $RENDER_DIR/$HBBR" ;;
    install) do_install ;;
    rollback) do_rollback ;;
    restore) do_restore ;;
    status) do_status ;;
  esac
}

main "$@"
