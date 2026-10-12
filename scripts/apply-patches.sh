#!/usr/bin/env bash
# Reset vendor/rustdesk and vendor/rustdesk/libs/hbb_common to their pinned commits and apply
# patches/rustdesk/*.patch and patches/hbb_common/*.patch on a branch named `remotesmith`, one
# commit per patch (git am). Pins come from the gitlinks: vendor/rustdesk from the superproject
# index, libs/hbb_common from the pinned rustdesk tree. See docs/IMPLEMENTATION.md.
#
# Usage:
#   scripts/apply-patches.sh           reset + apply (refuses to drop unexported commits or local edits)
#   scripts/apply-patches.sh --force   same, but discard local edits and unexported commits
#   scripts/apply-patches.sh --check   verify every patch applies cleanly in a throwaway worktree;
#                                      the submodules and their branches are left untouched
set -euo pipefail
# shellcheck source=lib/patches-common.sh
source "$(dirname "$0")/lib/patches-common.sh"

mode=apply force=0
for arg in "$@"; do
  case "$arg" in
    --check) mode=check ;;
    --force) force=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

if [[ ! -e "$RUSTDESK_DIR/.git" || ! -e "$HBB_DIR/.git" ]]; then
  [[ $mode == apply ]] || die "submodules are not checked out; run: git submodule update --init --recursive --depth 1"
  git -C "$REPO_ROOT" submodule update --init --recursive --depth 1
fi

RD_PIN="$(rustdesk_pin)"
HBB_PIN="$(hbb_pin)"
git -C "$RUSTDESK_DIR" cat-file -e "$RD_PIN^{commit}" 2>/dev/null || die "rustdesk pin $RD_PIN not present locally; run: git submodule update --init --recursive"
git -C "$HBB_DIR" cat-file -e "$HBB_PIN^{commit}" 2>/dev/null || die "hbb_common pin $HBB_PIN not present locally; run: git -C vendor/rustdesk submodule update --init libs/hbb_common"

# Deterministic commits: same patches on the same pin give the same branch sha. No --3way: a
# patch must apply as written (line offsets are fine, fuzz is not).
git_am() { # repo-dir patch
  git -C "$1" -c user.name="${GIT_COMMITTER_NAME:-RemoteSmith patches}" \
    -c user.email="${GIT_COMMITTER_EMAIL:-patches@remotesmith.invalid}" -c commit.gpgsign=false \
    am --quiet --committer-date-is-author-date --keep-cr "$2"
}

am_in_progress() { [[ -d "$(git -C "$1" rev-parse --path-format=absolute --git-path rebase-apply)" ]]; }

# git am every patch of one target in order. On the first failure: print git's error, abort the
# am session (the branch stays at the last good patch) and return 1.
am_all() { # repo-dir label patches...
  local dir="$1" label="$2" p out; shift 2
  for p in "$@"; do
    if ! out="$(git_am "$dir" "$p" 2>&1 </dev/null)"; then
      printf '%s\n' "$out" >&2
      am_in_progress "$dir" && git -C "$dir" am --abort
      echo "FAIL  $label: $(basename "$p") does not apply" >&2
      return 1
    fi
    echo "ok    $label: $(basename "$p")"
  done
}

# --- --check: throwaway worktrees, nothing in the submodules changes ----------------------------
if [[ $mode == check ]]; then
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/remotesmith-check.XXXXXX")"
  wt_rd="$tmp/rustdesk" wt_hbb="$tmp/hbb_common"
  cleanup() {
    git -C "$RUSTDESK_DIR" worktree remove --force "$wt_rd" 2>/dev/null || true
    git -C "$HBB_DIR" worktree remove --force "$wt_hbb" 2>/dev/null || true
    git -C "$RUSTDESK_DIR" worktree prune; git -C "$HBB_DIR" worktree prune
    rm -rf "$tmp"
  }
  trap cleanup EXIT
  status=0
  for target in rustdesk hbb_common; do
    if [[ $target == rustdesk ]]; then repo="$RUSTDESK_DIR" pin="$RD_PIN" wt="$wt_rd"; else repo="$HBB_DIR" pin="$HBB_PIN" wt="$wt_hbb"; fi
    load_patches "$target"
    echo "== $target: ${#PATCHES[@]} patch(es) onto pin ${pin:0:12}"
    ((${#PATCHES[@]})) || continue
    # --no-checkout + sparse would be faster, but git am needs real files for 3-way context.
    git -C "$repo" worktree add --quiet --detach "$wt" "$pin"
    am_all "$wt" "$target" "${PATCHES[@]}" || status=1
  done
  ((status == 0)) && echo "apply-patches --check: OK" || echo "apply-patches --check: FAILED" >&2
  exit $status
fi

# --- apply -------------------------------------------------------------------------------------
# Refuse to throw away work unless --force: local edits, or commits on `remotesmith` above the pin
# whose content is not in patches/ (compared by stable patch-id, so re-exported patches match).
guard() { # repo-dir pin label
  local dir="$1" pin="$2" label="$3" c id ids missing=()
  local dirty
  dirty="$(git -C "$dir" status --porcelain --untracked-files=no --ignore-submodules=all)"
  [[ -z "$dirty" ]] || { ((force)) || die "$label has uncommitted changes (use --force to discard):
$dirty"; }
  git -C "$dir" rev-parse --verify --quiet "refs/heads/$PATCH_BRANCH" >/dev/null || return 0
  ids="$(patch_file_ids)"
  for c in $(git -C "$dir" rev-list --no-merges "$pin..refs/heads/$PATCH_BRANCH"); do
    id="$(git -C "$dir" show --format= "$c" | git patch-id --stable | awk '{print $1}')"
    [[ -z "$id" ]] && continue                 # empty commit
    grep -qx "$id" <<<"$ids" || missing+=("$(git -C "$dir" log -1 --format='%h %s' "$c")")
  done
  if ((${#missing[@]})) && ((force == 0)); then
    printf '%s\n' "${missing[@]}" >&2
    die "$label branch $PATCH_BRANCH has commits not in patches/ (run scripts/export-patches.sh, or --force to drop them)"
  fi
  echo "$label: previous $PATCH_BRANCH tip $(git -C "$dir" rev-parse --short "refs/heads/$PATCH_BRANCH")"
}

load_patches rustdesk;   RD_PATCHES=(${PATCHES[@]+"${PATCHES[@]}"})
guard "$RUSTDESK_DIR" "$RD_PIN" vendor/rustdesk
load_patches hbb_common; HBB_PATCHES=(${PATCHES[@]+"${PATCHES[@]}"})
guard "$HBB_DIR" "$HBB_PIN" libs/hbb_common

for d in "$RUSTDESK_DIR" "$HBB_DIR"; do
  if am_in_progress "$d"; then git -C "$d" am --abort; fi
done

echo "== vendor/rustdesk: ${#RD_PATCHES[@]} patch(es) onto ${RD_PIN:0:12}"
git -C "$RUSTDESK_DIR" checkout --quiet --force -B "$PATCH_BRANCH" "$RD_PIN"
# Put hbb_common back on its pinned gitlink before patching it.
git -C "$RUSTDESK_DIR" submodule update --init --force --quiet -- libs/hbb_common
am_all "$RUSTDESK_DIR" rustdesk ${RD_PATCHES[@]+"${RD_PATCHES[@]}"} || die "vendor/rustdesk left at the last good patch on branch $PATCH_BRANCH"

echo "== libs/hbb_common: ${#HBB_PATCHES[@]} patch(es) onto ${HBB_PIN:0:12}"
git -C "$HBB_DIR" checkout --quiet --force -B "$PATCH_BRANCH" "$HBB_PIN"
am_all "$HBB_DIR" hbb_common ${HBB_PATCHES[@]+"${HBB_PATCHES[@]}"} || die "libs/hbb_common left at the last good patch on branch $PATCH_BRANCH"

echo "apply-patches: OK (rustdesk $(git -C "$RUSTDESK_DIR" rev-parse --short HEAD), hbb_common $(git -C "$HBB_DIR" rev-parse --short HEAD))"
