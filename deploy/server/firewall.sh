#!/usr/bin/env bash
# Host firewall (ufw) rules for the RemoteSmith server. Run ON the VM as root.
#
#   sudo ./firewall.sh            add the rules (idempotent; ufw skips existing rules)
#   sudo ./firewall.sh --check    show the RemoteSmith rules and fail if anything is off
#   sudo ./firewall.sh --remove   delete exactly the rules this script adds
#        ./firewall.sh --dry-run  print the commands only
#
# Opens TCP 21115-21117 and UDP 21116, IPv4 and IPv6 (DEF-14). Nothing else:
# TCP 21114 (Pro console) and 21118/21119 (hbbs/hbbr WebSocket listeners, which the
# daemons always bind) stay closed by ufw's default-deny incoming policy.
# The Lightsail firewall in front of the VM needs the same ports; see firewall.md.
set -euo pipefail

COMMENT="remotesmith hbbs/hbbr"
RULES=("21115:21117/tcp" "21116/udp")
MODE=add
DRY_RUN=0

for arg in "$@"; do
  case "$arg" in
    --check) MODE=check ;;
    --remove) MODE=remove ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

run() {
  if [[ $DRY_RUN == 1 ]]; then printf 'DRY-RUN: %s\n' "$*"; else "$@"; fi
}

if [[ $DRY_RUN == 0 ]]; then
  [[ $(id -u) == 0 ]] || { echo "run as root (sudo), or add --dry-run" >&2; exit 1; }
  command -v ufw >/dev/null || { echo "ufw not found; this VM is expected to use ufw" >&2; exit 1; }
fi

case "$MODE" in
  add)
    if [[ $DRY_RUN == 0 ]] && ! ufw status | grep -q '^Status: active'; then
      echo "ufw is not active on this host; refusing to guess the firewall. Investigate first." >&2
      exit 1
    fi
    for r in "${RULES[@]}"; do
      run ufw allow "$r" comment "$COMMENT"
    done
    [[ $DRY_RUN == 1 ]] || exec bash "$0" --check
    ;;
  remove)
    for r in "${RULES[@]}"; do
      run ufw delete allow "$r"
    done
    ;;
  check)
    status=$(ufw status verbose)
    printf '%s\n' "$status" | grep -E '^(Status|Default):'
    printf '%s\n' "$status" | grep -E '2111[4-9]' || true
    fail=0
    printf '%s\n' "$status" | grep -q '^Status: active' || { echo "FAIL: ufw inactive"; fail=1; }
    printf '%s\n' "$status" | grep -q 'Default: deny (incoming)' || { echo "FAIL: default incoming is not deny"; fail=1; }
    for r in "${RULES[@]}"; do
      printf '%s\n' "$status" | grep -qE "^${r}[[:space:]]+ALLOW IN" \
        || { echo "FAIL: missing IPv4 allow $r"; fail=1; }
      printf '%s\n' "$status" | grep -qE "^${r} \(v6\)[[:space:]]+ALLOW IN" \
        || { echo "FAIL: missing IPv6 allow $r"; fail=1; }
    done
    # No ALLOW rule may cover 21114 (Pro), 21118 or 21119 (WebSocket), whatever its
    # shape: single ports, lists, ranges, or port-less rules that allow everything.
    while IFS= read -r line; do
      to=${line%%ALLOW*}
      to=${to//(v6)/}
      to=$(printf '%s' "$to" | sed 's/[[:space:]]*$//')
      if [[ $to == Anywhere* ]]; then
        echo "FAIL: '$line' allows every port (including 21114/21118/21119)"; fail=1; continue
      fi
      spec=${to%%/*}
      spec=${spec%% *}
      if [[ ! $spec =~ ^[0-9,:]+$ ]]; then
        echo "WARN: can't evaluate '$line' (application profile?); check it by hand"; continue
      fi
      IFS=, read -r -a items <<< "$spec"
      for item in "${items[@]}"; do
        lo=${item%%:*}; hi=${item##*:}
        for bad in 21114 21118 21119; do
          if (( lo <= bad && bad <= hi )); then
            echo "FAIL: '$line' exposes $bad"; fail=1
          fi
        done
      done
    done < <(printf '%s\n' "$status" | grep -E '[[:space:]]ALLOW[[:space:]]' || true)
    if [[ $fail == 0 ]]; then echo "firewall OK"; else exit 1; fi
    ;;
esac
