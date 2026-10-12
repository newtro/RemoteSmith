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
    # Any rule mentioning a WebSocket/Pro port, or opening more than the two ranges, is wrong.
    if printf '%s\n' "$status" | grep -E 'ALLOW' | grep -E '\b2111[489]\b|21115:2111[89]|2111[0-9]:2112'; then
      echo "FAIL: a rule exposes 21114, 21118 or 21119"; fail=1
    fi
    if [[ $fail == 0 ]]; then echo "firewall OK"; else exit 1; fi
    ;;
esac
