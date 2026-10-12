#!/bin/sh
# keycheck.sh [--wait SECONDS] [DIR]
#
# Succeeds only when DIR (default: the current directory) holds a valid hbbs keypair:
# id_ed25519 decodes to a 64-byte ed25519 secret key, id_ed25519.pub decodes to
# 32 bytes, and the public key is the second half of the secret key.
#
# Why this exists: hbbs and hbbr 1.1.16 FAIL OPEN on key problems. When `-k _` cannot
# read id_ed25519 and cannot write a new one, they log nothing alarming, use an empty
# key and accept every client. Both units run this as ExecStartPre, so a key problem
# fails the unit instead of silently turning enforcement off (PLAN §9, AC-5).
# install.sh also uses it to validate generated and restored keys.
set -eu

wait_s=0
if [ "${1:-}" = "--wait" ]; then
  wait_s=${2:?--wait needs a number of seconds}
  shift 2
fi
dir=${1:-.}

check() {
  [ -r "$dir/id_ed25519" ] && [ -r "$dir/id_ed25519.pub" ] || return 1
  sk=$(tr -d ' \n\r' < "$dir/id_ed25519")
  pk=$(tr -d ' \n\r' < "$dir/id_ed25519.pub")
  [ -n "$sk" ] && [ -n "$pk" ] || return 1
  [ "$(printf %s "$sk" | base64 -d 2>/dev/null | wc -c | tr -d ' ')" = 64 ] || return 1
  [ "$(printf %s "$pk" | base64 -d 2>/dev/null | wc -c | tr -d ' ')" = 32 ] || return 1
  [ "$(printf %s "$sk" | base64 -d | tail -c 32 | base64 | tr -d '\n')" = "$pk" ] || return 1
}

# Poll in 0.2 s steps (5 per second) until the key is valid or the wait runs out.
tries=$((wait_s * 5))
while :; do
  if check; then
    exit 0
  fi
  [ "$tries" -le 0 ] && break
  tries=$((tries - 1))
  sleep 0.2
done
echo "keycheck: no valid hbbs keypair (id_ed25519 / id_ed25519.pub) in $dir" >&2
exit 1
