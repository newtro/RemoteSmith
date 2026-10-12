#!/usr/bin/env bash
# Regenerate patches/rustdesk/ and patches/hbb_common/ from the commits on branch `remotesmith`
# above each submodule's pinned commit. See docs/IMPLEMENTATION.md "Thin-fork patch workflow".
#
# Every commit's subject must be `remotesmith(NNNN): topic`. NNNN is the patch number and the file
# is NNNN-<topic-slug>.patch, so numbering is stable: re-exporting never renumbers, and a new commit
# carries the number its author picked from the unit's range. apply-patches.sh applies in number
# order, so before anything is written the new set is test-applied in that order onto the pin
# (apply-patches.sh --check against a scratch copy). Patch files whose number is no longer on the
# branch are only removed with --prune: by default export refuses, because such a file usually
# belongs to another unit and simply is not on this checkout's branch yet (run apply-patches.sh).
#
# Usage: scripts/export-patches.sh [--dry-run] [--prune]
set -euo pipefail
# shellcheck source=lib/patches-common.sh
source "$(dirname "$0")/lib/patches-common.sh"

dry=0 prune=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry=1 ;;
    --prune) prune=1 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
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
      -c format.signOff=false -c format.headers= -c format.useAutoBase=false \
      format-patch -1 "$c" --stdout -k --zero-commit --no-signature --no-numbered --no-renames --binary \
      -U3 --inter-hunk-context=0 --diff-algorithm=myers --no-relative --no-notes --no-thread --no-attach \
      --no-to --no-cc --no-from >"$out/$num-$(slug "$topic").patch"
    count=$((count + 1))
  done
  echo "== $target: $count commit(s) above ${pin:0:12}"
}

generate rustdesk "$RUSTDESK_DIR" "$RD_PIN"
generate hbb_common "$HBB_DIR" "$HBB_PIN"

# Existing patch files whose number is not on the branch.
orphans=()
for target in rustdesk hbb_common; do
  PATCHES_ROOT="$REPO_ROOT/patches" load_patches "$target"
  for f in ${PATCHES[@]+"${PATCHES[@]}"}; do
    num="$(basename "$f" | cut -c1-4)"
    compgen -G "$SCRATCH/$target/$num-*.patch" >/dev/null || orphans+=("patches/$target/$(basename "$f")")
  done
done
if ((${#orphans[@]})) && ((prune == 0)); then
  printf '  %s\n' "${orphans[@]}" >&2
  die "these patches are not on your $PATCH_BRANCH branch. Run scripts/apply-patches.sh to pick them up first, or pass --prune to delete them"
fi

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
  fi
done
((dry)) && echo "(dry run: patches/ not modified)"
echo "export-patches: OK"
