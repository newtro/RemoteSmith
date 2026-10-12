#!/usr/bin/env bash
# verify-server.sh: check the public RemoteSmith server from the Mac (or any outside host).
#
#   deploy/server/verify-server.sh                 full check against rs.johnnycode.ai
#   deploy/server/verify-server.sh --key KEY       use KEY instead of fetching it over ssh
#   deploy/server/verify-server.sh --ipv6          require the IPv6 port checks (by default
#                                                  they run when this host has IPv6 to the VM)
#   deploy/server/verify-server.sh --no-server-status   skip the ssh health report
#   deploy/server/verify-server.sh --host H --no-dns --key K --no-server-status
#                                                  probe any server (e.g. a test container)
#
# Checks: DNS (A and AAAA must be the VM's own addresses, i.e. DNS-only; --ip6 "" skips AAAA),
# TCP 21115-21117 reachable, UDP 21116 answers a registration, TCP 21114/21118/21119
# NOT reachable, and AC-5: hbbs and hbbr refuse a wrong key and a missing key, and
# accept the right one (positive control). Prints the client configuration at the end.
# Needs: python3, dig, ssh (unless --key is given). Exit 0 only when every check passes.
set -euo pipefail

HOST=rs.johnnycode.ai
IP4=98.93.243.79
IP6=2600:1f18:72b8:5500:81e8:9091:9439:f3ef
SSH_HOST=vm
PREFIX=/opt/remotesmith-server
KEY=
CHECK_V6=0
SERVER_STATUS=1
CHECK_DNS=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host) HOST=$2; shift ;;
    --ip4) IP4=$2; shift ;;
    --ip6) IP6=$2; shift ;;
    --ssh) SSH_HOST=$2; shift ;;
    --key) KEY=$2; shift ;;
    --ipv6) CHECK_V6=1 ;;
    --no-server-status) SERVER_STATUS=0 ;;
    --no-dns) CHECK_DNS=0 ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

PROBE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/probe.py"
PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); printf 'PASS  %s\n' "$*"; }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL  %s\n' "$*"; }
probe() { python3 "$PROBE" "$@" 2>&1 || true; }

# The literal addresses every later check probes. With DNS checks on they are what the
# A and AAAA records actually say (what clients will reach), not a re-resolution of the
# name inside each probe. With --no-dns, --host is used as the IPv4 literal.
TARGET4=$HOST
TARGET6=$IP6
if [[ $CHECK_DNS == 1 ]]; then
echo "== DNS ($HOST)"
for resolver in "" "@1.1.1.1"; do
  # shellcheck disable=SC2086  # $resolver is intentionally word-split (empty = system resolver)
  got=$(dig +short A "$HOST" $resolver | tail -1)
  label=${resolver:-system resolver}
  if [[ $got == "$IP4" ]]; then pass "A $HOST -> $got ($label)"
  else fail "A $HOST -> '${got:-nothing}' ($label), expected $IP4 (a Cloudflare IP means the record is proxied; it must be DNS-only)"; fi
  [[ $resolver == "@1.1.1.1" ]] && TARGET4=${got:-$IP4}
done
if [[ -n $IP6 ]]; then
  got=$(dig +short AAAA "$HOST" @1.1.1.1 | tail -1)
  if [[ $got == "$IP6" ]]; then pass "AAAA $HOST -> $got"
  else fail "AAAA $HOST -> '${got:-nothing}', expected $IP6"; fi
  TARGET6=${got:-$IP6}
fi
fi

check_ports() {
  local target=$1 label=$2 p r
  echo "== ports via $label ($target)"
  for p in 21115 21116 21117; do
    r=$(probe tcp "$target" "$p")
    if [[ $r == open ]]; then pass "tcp/$p open"; else fail "tcp/$p $r (must be open)"; fi
  done
  for p in 21114 21118 21119; do
    r=$(probe tcp "$target" "$p")
    if [[ $r != open ]]; then pass "tcp/$p not reachable ($r)"; else fail "tcp/$p is OPEN (must be closed, DEF-14)"; fi
  done
  r=$(probe udp-register "$target" 21116)
  if [[ $r == register_peer_response ]]; then pass "udp/21116 answers RegisterPeer"
  else fail "udp/21116: $r"; fi
}
check_ports "$TARGET4" "IPv4"
V6=0
if [[ -n $TARGET6 ]]; then
  if [[ $CHECK_V6 == 1 || $(probe tcp "$TARGET6" 22) == open ]]; then
    V6=1
    check_ports "$TARGET6" "IPv6"
  else
    echo "SKIP  IPv6 checks: no IPv6 path from this host to $TARGET6 (use --ipv6 to require them)"
  fi
fi

echo "== server key"
if [[ -z $KEY ]]; then
  KEY=$(ssh -o BatchMode=yes -o ConnectTimeout=10 "$SSH_HOST" "cat $PREFIX/id_ed25519.pub" 2>/dev/null || true)
fi
if [[ $KEY =~ ^[A-Za-z0-9+/]{43}=$ ]]; then
  pass "public key: $KEY"
else
  fail "could not get a valid public key (got '${KEY:-nothing}'); pass --key"
fi

# expect DESC GOT GLOB: pass when GOT matches the glob pattern GLOB.
expect() {
  # shellcheck disable=SC2053  # $3 is a glob on purpose
  if [[ $2 == $3 ]]; then pass "$1 ($2)"; else fail "$1: got '$2'"; fi
}
# AC-5, the part OSS hbbs/hbbr can enforce: connection (punch-hole) requests to hbbs
# and relay sessions on hbbr are refused with a wrong or missing key.
check_ac5() {
  local t=$1 label=$2
  echo "== AC-5 key enforcement via $label ($t)"
  expect "hbbs refuses a connect request with a wrong key" "$(probe punch "$t" not-the-server-key)" LICENSE_MISMATCH
  expect "hbbs refuses a connect request with no key" "$(probe punch "$t" "")" LICENSE_MISMATCH
  expect "hbbr refuses a relay with a wrong key" "$(probe relay "$t" not-the-server-key)" "refused*"
  expect "hbbr refuses a relay with no key" "$(probe relay "$t" "")" "refused*"
  if [[ -n $KEY ]]; then
    # Positive controls: without these, a dead or misrouted server would pass the refusals.
    expect "hbbs accepts the right key (unknown probe ID)" "$(probe punch "$t" "$KEY")" ID_NOT_EXIST
    expect "hbbr accepts the right key" "$(probe relay "$t" "$KEY")" accepted
  fi
}
check_ac5 "$TARGET4" "IPv4"
if [[ $V6 == 1 ]]; then check_ac5 "$TARGET6" "IPv6"; fi
echo "KNOWN LIMITATION: registration and relay-request forwarding are not key-checked by OSS hbbs;"
echo "                  mitigated client-side (host accepts only its configured relay) - see AC-61"

if [[ $SERVER_STATUS == 1 ]]; then
  echo "== server-side status (ssh $SSH_HOST)"
  if out=$(ssh -o BatchMode=yes -o ConnectTimeout=10 "$SSH_HOST" "sudo -n $PREFIX/kit/install.sh --status" 2>&1); then
    printf '%s\n' "$out" | sed 's/^/   /'
    pass "server reports HEALTHY"
  else
    printf '%s\n' "$out" | sed 's/^/   /'
    fail "server status check failed"
  fi
fi

echo
echo "Client configuration (RemoteSmith first run, FLOW-2):"
echo "  ID server:    $HOST"
echo "  Relay server: $HOST:21117"
echo "  Key:          ${KEY:-<unknown>}"
echo
echo "verify-server: $PASS passed, $FAIL failed"
[[ $FAIL == 0 ]]
