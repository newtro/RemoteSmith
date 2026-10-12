# shellcheck shell=bash
# Shared helpers for apply-patches.sh / export-patches.sh. See docs/IMPLEMENTATION.md
# "Thin-fork patch workflow".

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUSTDESK_DIR="$REPO_ROOT/vendor/rustdesk"
HBB_DIR="$RUSTDESK_DIR/libs/hbb_common"
PATCH_BRANCH="remotesmith"
# Where the patch files live. Overridable so a patch set can be checked from a scratch directory.
PATCHES_ROOT="${REMOTESMITH_PATCHES_DIR:-$REPO_ROOT/patches}"
# Commit subject convention: "remotesmith(NNNN): topic". NNNN is the patch number.
SUBJECT_RE='^remotesmith\(([0-9]{4})\): (.+)$'

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Pinned rustdesk commit: the gitlink in the superproject index (what `git submodule update` uses).
rustdesk_pin() {
  local line
  line="$(git -C "$REPO_ROOT" ls-files -s -- vendor/rustdesk)"
  [[ "$line" != *$'\n'* ]] || die "vendor/rustdesk gitlink is conflicted in the superproject index; resolve it first"
  [[ "$line" == 160000\ * ]] || die "vendor/rustdesk is not a gitlink in the superproject index"
  awk '{print $2}' <<<"$line"
}

# Pinned hbb_common commit: the gitlink inside the pinned rustdesk tree.
hbb_pin() {
  local pin line
  pin="$(rustdesk_pin)"
  line="$(git -C "$RUSTDESK_DIR" ls-tree "$pin" -- libs/hbb_common)" \
    || die "rustdesk pin $pin is not in the vendor/rustdesk object store; run: git submodule update --init --recursive"
  [[ "$line" == 160000\ commit\ * ]] || die "libs/hbb_common is not a gitlink at rustdesk $pin"
  awk '{print $3}' <<<"$line"
}

# Numbered patch files for one target, sorted by name, into the global array PATCHES.
# Validates names and that no number is used twice.
load_patches() { # patches-subdir
  local dir="$PATCHES_ROOT/$1" f base seen=" "
  PATCHES=()
  [[ -d "$dir" ]] || return 0
  for f in "$dir"/*; do   # glob expansion is sorted
    [[ -e "$f" ]] || continue
    base="$(basename "$f")"
    case "$base" in .gitkeep|README*) continue ;; esac
    [[ "$base" =~ ^([0-9]{4})-[A-Za-z0-9._-]+\.patch$ ]] || die "patches/$1/$base: name must be NNNN-topic.patch"
    [[ "$seen" != *" ${BASH_REMATCH[1]} "* ]] || die "patches/$1: patch number ${BASH_REMATCH[1]} used twice"
    seen="$seen${BASH_REMATCH[1]} "
    PATCHES+=("$f")
  done
}

# git with the user's global/system config ignored, so patch output and git am behaviour do not
# depend on whoever runs the scripts (diff.context, format.*, apply.whitespace, am.threeWay...).
git_clean() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git "$@"; }

# Stable patch-id of one commit, computed the same way export-patches.sh writes the patch.
commit_patch_id() { # repo-dir commit
  git_clean -C "$1" diff-tree -p --no-renames --binary --full-index -U3 --diff-algorithm=myers "$2" \
    | git patch-id --stable | awk '{print $1}'
}

# Stable patch-ids of the patch files for one target, one per line (after load_patches).
patch_file_ids() {
  local f
  for f in ${PATCHES[@]+"${PATCHES[@]}"}; do
    git patch-id --stable <"$f" | awk '{print $1}'
  done
}

# Manifest of the patch files the last apply-patches.sh run applied, one "<file> <patch-id>" line
# each, kept in the submodule's git dir. export-patches.sh uses it to tell a patch file that changed
# or appeared since then (another unit's work, pulled from main) from one this branch owns.
manifest_path() { echo "$(git -C "$1" rev-parse --absolute-git-dir)/remotesmith-applied"; }
file_patch_id() { git patch-id --stable <"$1" | awk '{print $1}'; }
