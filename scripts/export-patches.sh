#!/usr/bin/env bash
# Regenerate patches/rustdesk/ and patches/hbb_common/ from the commits on branch `remotesmith`
# above each submodule's pinned commit. See docs/IMPLEMENTATION.md "Thin-fork patch workflow".
#
# Every commit's subject must be `remotesmith(NNNN): topic`. NNNN is the patch number and the file
# is NNNN-<topic-slug>.patch, so numbering is stable: re-exporting never renumbers, and a new commit
# carries the number its author picked from the unit's range. apply-patches.sh applies in number
# order, so before anything is written the new set is test-applied in that order onto the pin
# (apply-patches.sh --check against a scratch copy). Patch files whose number is no longer on the
# branch are removed only if the last apply-patches.sh applied them unchanged (this branch dropped
# them); others (another unit's patches pulled since) are kept and test-applied with yours.
# A file on the branch that changed since the last apply is never overwritten without --force.
#
# Usage: scripts/export-patches.sh [--dry-run] [--prune] [--force]
#   --prune   also remove patch files not on the branch that this checkout never applied
#   --force   overwrite patch files that changed since the last apply
set -euo pipefail
# shellcheck source=lib/patches-common.sh
source "$(dirname "$0")/lib/patches-common.sh"

dry=0 prune=0 force=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry=1 ;;
    --prune) prune=1 ;;
    --force) force=1 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

[[ -e "$RUSTDESK_DIR/.git" && -e "$HBB_DIR/.git" ]] || die "submodules are not checked out"
RD_PIN="$(rustdesk_pin)"
HBB_PIN="$(hbb_pin)"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/remotesmith-export.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

slug() { tr '[:upper:]' '[:lower:]' <<<"$1" | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//' | cut -c1-50 | sed -E "s/-+$//; s/^$/patch/"; }

generate() { # target repo-dir pin  -> $SCRATCH/<target>/*.patch
  local target="$1" dir="$2" pin="$3" out="$SCRATCH/$1" c short subj num topic seen=" " count=0
  mkdir -p "$out"
  git -C "$dir" rev-parse --verify --quiet "refs/heads/$PATCH_BRANCH" >/dev/null \
    || die "$target: no branch $PATCH_BRANCH; run scripts/apply-patches.sh first"
  git -C "$dir" merge-base --is-ancestor "$pin" "refs/heads/$PATCH_BRANCH" \
    || die "$target: $PATCH_BRANCH does not contain the pin ${pin:0:12}; rebase it onto the pin"
  [[ -z "$(git -C "$dir" rev-list --merges "$pin..refs/heads/$PATCH_BRANCH")" ]] \
    || die "$target: merge commits above the pin; linearize the branch"
  [[ "$(git -C "$dir" symbolic-ref --quiet --short HEAD || true)" == "$PATCH_BRANCH" ]] \
    || echo "note: $target HEAD is not on $PATCH_BRANCH; exporting the branch, not HEAD"
  [[ -z "$(git -C "$dir" status --porcelain --untracked-files=no --ignore-submodules=all)" ]] \
    || echo "warning: $target has uncommitted changes; they are NOT exported" >&2

  for c in $(git -C "$dir" rev-list --reverse "$pin..refs/heads/$PATCH_BRANCH"); do
    short="$(git -C "$dir" rev-parse --short "$c")"
    subj="$(git -C "$dir" log -1 --format=%s "$c")"
    [[ "$subj" =~ $SUBJECT_RE ]] || die "$target $short: subject must be 'remotesmith(NNNN): topic', got: $subj"
    num="${BASH_REMATCH[1]}" topic="${BASH_REMATCH[2]}"
    [[ "$seen" != *" $num "* ]] || die "$target $short: patch number $num is used by two commits"
    seen="$seen$num "
    [[ -n "$(git -C "$dir" diff-tree --no-commit-id -r "$c")" ]] || die "$target $short: empty commit"
    if [[ $target == rustdesk ]] && git -C "$dir" diff-tree --no-commit-id --name-only -r "$c" | grep -x libs/hbb_common >/dev/null; then
      die "rustdesk $short changes the libs/hbb_common gitlink; hbb_common changes belong in patches/hbb_common"
    fi
    # -k keeps the subject verbatim; --zero-commit, --no-signature and a fixed abbrev keep the file
    # byte-stable across re-exports and machines.
    git_clean -C "$dir" -c core.abbrev=12 -c diff.noprefix=false -c diff.mnemonicPrefix=false \
      -c format.signOff=false -c format.useAutoBase=false -c diff.suppressBlankEmpty=false \
      format-patch -1 "$c" --stdout -k --zero-commit --no-signature --no-numbered --no-renames --binary \
      -U3 --inter-hunk-context=0 --diff-algorithm=myers --no-relative --no-notes --no-thread --no-attach \
      --no-to --no-cc --no-from >"$out/$num-$(slug "$topic").patch"
    count=$((count + 1))
  done
  echo "== $target: $count commit(s) above ${pin:0:12}"
}

generate rustdesk "$RUSTDESK_DIR" "$RD_PIN"
generate hbb_common "$HBB_DIR" "$HBB_PIN"

# Reconcile with the patch files already in patches/, using the manifest of the last apply:
#  - a file whose number is not on the branch ("orphan"):
#      in the manifest unchanged -> this branch dropped it, so it is removed;
#      otherwise (new or changed since the last apply, i.e. another unit's) -> kept, and included
#      in the test-apply; --prune removes it anyway.
#  - a file whose number IS on the branch but which changed since the last apply (someone else
#    updated it on main): refuse, since writing the branch's copy would revert their change.
#    Re-run apply-patches.sh (after exporting, or move your commits aside) or pass --force.
stale=() kept=() dropped=()
for target in rustdesk hbb_common; do
  if [[ $target == rustdesk ]]; then repo="$RUSTDESK_DIR"; else repo="$HBB_DIR"; fi
  manifest="$(manifest_path "$repo")"
  PATCHES_ROOT="$REPO_ROOT/patches" load_patches "$target"
  for f in ${PATCHES[@]+"${PATCHES[@]}"}; do
    base="$(basename "$f")" num="$(basename "$f" | cut -c1-4)"
    applied_line="$(grep "^$num-" "$manifest" 2>/dev/null || true)"
    same_as_applied=0
    [[ -n "$applied_line" && "${applied_line##* }" == "$(file_patch_id "$f")" ]] && same_as_applied=1
    if compgen -G "$SCRATCH/$target/$num-*.patch" >/dev/null; then
      if ((same_as_applied == 0)) && ! cmp -s "$f" "$SCRATCH/$target/$num-"*.patch; then
        stale+=("patches/$target/$base")
      fi
    elif ((same_as_applied)) || ((prune)); then
      dropped+=("patches/$target/$base")
    else
      kept+=("patches/$target/$base")
      cp "$f" "$SCRATCH/$target/"
    fi
  done
done
if ((${#stale[@]})) && ((force == 0)); then
  printf '  %s\n' "${stale[@]}" >&2
  die "these patch files changed since your last apply-patches.sh (e.g. updated on main); exporting would overwrite them with your branch's older copy. Rebase your work onto them (apply-patches.sh, then re-commit), or pass --force to overwrite"
fi
((${#kept[@]})) && { echo "keeping patches not on your branch (not applied here, or changed since the last apply):"; printf '  %s\n' "${kept[@]}"; }

echo "== test-applying the exported set in number order"
REMOTESMITH_PATCHES_DIR="$SCRATCH" "$(dirname "$0")/apply-patches.sh" --check \
  || die "the exported patches do not apply in number order onto the pins; reorder or renumber the commits"

for target in rustdesk hbb_common; do
  dest="$REPO_ROOT/patches/$target"
  PATCHES_ROOT="$REPO_ROOT/patches" load_patches "$target"
  old=(${PATCHES[@]+"${PATCHES[@]}"})
  changed=0
  for f in "$SCRATCH/$target"/*.patch; do
    [[ -e "$f" ]] || continue
    base="$(basename "$f")"
    if [[ ! -f "$dest/$base" ]]; then echo "add     patches/$target/$base"; changed=1
    elif ! cmp -s "$f" "$dest/$base"; then echo "update  patches/$target/$base"; changed=1; fi
  done
  for f in ${old[@]+"${old[@]}"}; do
    [[ -f "$SCRATCH/$target/$(basename "$f")" ]] || { echo "remove  patches/$target/$(basename "$f")"; changed=1; }
  done
  ((changed)) || echo "patches/$target: unchanged"
  if ((dry == 0)); then
    mkdir -p "$dest"
    for f in ${old[@]+"${old[@]}"}; do rm -f "$f"; done
    for f in "$SCRATCH/$target"/*.patch; do [[ -e "$f" ]] && cp "$f" "$dest/"; done
    # The written files now match the branch: record them (not the kept orphans) as this
    # checkout's baseline, as apply-patches.sh would.
    if [[ $target == rustdesk ]]; then repo="$RUSTDESK_DIR"; else repo="$HBB_DIR"; fi
    for f in "$SCRATCH/$target"/*.patch; do
      [[ -e "$f" ]] || continue
      [[ " ${kept[*]+${kept[*]}} " == *" patches/$target/$(basename "$f") "* ]] && continue
      echo "$(basename "$f") $(file_patch_id "$f")"
    done >"$(manifest_path "$repo")"
  fi
done
((dry)) && echo "(dry run: patches/ not modified)"
echo "export-patches: OK"
