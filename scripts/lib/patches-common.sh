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

# A pin must be an upstream commit. If someone `git add`ed the submodule after apply-patches, the
# superproject would now point at a local patch commit that exists nowhere else.
require_upstream_pins() { # rustdesk-pin hbb-pin
  local pair label dir pin
  for pair in "vendor/rustdesk:$RUSTDESK_DIR:$1" "libs/hbb_common:$HBB_DIR:$2"; do
    IFS=: read -r label dir pin <<<"$pair"
    if git -C "$dir" log -1 --format=%s "$pin" 2>/dev/null | grep -E "$SUBJECT_RE" >/dev/null; then
      die "$label is pinned to RemoteSmith patch commit ${pin:0:12}, not an upstream commit; restore the gitlink (git checkout HEAD -- vendor/rustdesk, or fix the rustdesk tree)"
    fi
  done
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

# The exact patch text export-patches.sh writes for one commit. Fixed options and an empty
# global/system config make it byte-stable across machines and re-exports.
format_commit() { # repo-dir commit
  git_clean -C "$1" -c core.abbrev=12 -c diff.noprefix=false -c diff.mnemonicPrefix=false \
    -c format.signOff=false -c format.useAutoBase=false -c diff.suppressBlankEmpty=false \
    format-patch -1 "$2" --stdout -k --zero-commit --no-signature --no-numbered --no-renames --binary \
    -U3 --inter-hunk-context=0 --diff-algorithm=myers --no-relative --no-notes --no-thread --no-attach \
    --no-to --no-cc --no-from
}

# Exact identity of a patch: sha256 of its bytes (message, metadata and diff, whitespace
# included). Deliberately not `git patch-id`, which ignores whitespace and the message.
# sha256 of stdin. macOS ships shasum; Git Bash on Windows CI ships sha256sum but not shasum.
sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi | cut -d' ' -f1
}
file_hash() { sha256_stdin <"$1"; }
commit_hash() { format_commit "$1" "$2" | sha256_stdin; }

# Hashes of the patch files for one target, one per line (after load_patches).
patch_file_hashes() {
  local f
  for f in ${PATCHES[@]+"${PATCHES[@]}"}; do file_hash "$f"; done
}

# Manifest of the patch files the last apply-patches.sh (or export-patches.sh) run left the branch
# matching, one "<file> <sha256>" line each, kept in the submodule's git dir. export-patches.sh
# uses it to tell a patch file that changed or appeared since then (another unit's work, pulled
# from main) from one this branch owns.
manifest_path() { echo "$(git -C "$1" rev-parse --absolute-git-dir)/remotesmith-applied"; }

# Check that a submodule checkout is exactly its applied patch stack: branch $PATCH_BRANCH checked
# out, HEAD = pin + one commit per patch file in file-name order, each commit's export
# byte-identical to its patch file (the same identity apply-patches.sh's guard uses). Prints what
# is wrong to stderr and returns 1; returns 0 when the stack is exact. Uncommitted edits are not
# checked here (see dirty_summary).
check_patch_stack() { # repo-dir pin label patches-subdir
  local dir="$1" pin="$2" label="$3" sub="$4" branch merges c i=0 n bad=0 commits=""
  load_patches "$sub"
  n=${#PATCHES[@]}
  branch="$(git -C "$dir" symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
  if [[ "$branch" != "$PATCH_BRANCH" ]]; then
    echo "$label: HEAD is ${branch:+branch $branch}${branch:-detached at $(git -C "$dir" rev-parse --short=12 HEAD)}, not branch $PATCH_BRANCH" >&2
    bad=1
  fi
  if ! git -C "$dir" merge-base --is-ancestor "$pin" HEAD 2>/dev/null; then
    echo "$label: HEAD does not contain the pinned upstream commit ${pin:0:12}" >&2
    return 1
  fi
  merges="$(git -C "$dir" rev-list --merges HEAD "^$pin")"
  if [[ -n "$merges" ]]; then
    echo "$label: merge commits above the pin" >&2
    return 1
  fi
  commits="$(git -C "$dir" rev-list --reverse HEAD "^$pin")"
  if [[ -z "$commits" ]] && ((n)); then
    echo "$label: none of the $n patches/$sub patches is applied (HEAD is the upstream pin)" >&2
    return 1
  fi
  for c in $commits; do
    if ((i >= n)); then
      echo "$label: commit $(git -C "$dir" log -1 --format='%h %s' "$c") is not in patches/$sub (run scripts/export-patches.sh)" >&2
      bad=1
    elif [[ "$(commit_hash "$dir" "$c")" != "$(file_hash "${PATCHES[$i]}")" ]]; then
      echo "$label: commit $(git -C "$dir" log -1 --format='%h %s' "$c") does not match patches/$sub/$(basename "${PATCHES[$i]}")" >&2
      bad=1
    fi
    i=$((i + 1))
  done
  while ((i < n)); do
    echo "$label: patches/$sub/$(basename "${PATCHES[$i]}") is not applied" >&2
    bad=1
    i=$((i + 1))
  done
  return "$bad"
}

# Tracked files with uncommitted changes in a submodule checkout (empty when clean).
dirty_summary() { # repo-dir
  git -C "$1" status --porcelain --untracked-files=no --ignore-submodules=all
}
