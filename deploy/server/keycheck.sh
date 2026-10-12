#!/bin/sh
# keycheck.sh [--wait SECONDS] [DIR]
#
# Succeeds only when DIR (default: the current directory) holds a valid hbbs keypair:
# both are regular files (not symlinks); id_ed25519 decodes to a 64-byte libsodium
# secret key (seed || public key) and id_ed25519.pub to 32 bytes; the public key equals
# the one openssl derives from the seed with real Ed25519; and a signature made with the
# seed verifies against the public key. Needs openssl (3.x, as on Ubuntu 24.04).
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

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
umask 077

# DER prefixes for an Ed25519 PKCS#8 private key (seed follows) and an SPKI public key.
PKCS8_PREFIX='\060\056\002\001\000\060\005\006\003\053\145\160\004\042\004\040'
SPKI_PREFIX='\060\052\060\005\006\003\053\145\160\003\041\000'

check() {
  [ -f "$dir/id_ed25519" ] && [ ! -L "$dir/id_ed25519" ] || return 1
  [ -f "$dir/id_ed25519.pub" ] && [ ! -L "$dir/id_ed25519.pub" ] || return 1
  sk=$(head -c 200 "$dir/id_ed25519" | tr -d ' \n\r')
  pk=$(head -c 100 "$dir/id_ed25519.pub" | tr -d ' \n\r')
  [ -n "$sk" ] && [ -n "$pk" ] || return 1
  printf %s "$sk" | base64 -d > "$tmp/sk.bin" 2>/dev/null || return 1
  printf %s "$pk" | base64 -d > "$tmp/pk.bin" 2>/dev/null || return 1
  [ "$(wc -c < "$tmp/sk.bin" | tr -d ' ')" = 64 ] || return 1
  [ "$(wc -c < "$tmp/pk.bin" | tr -d ' ')" = 32 ] || return 1
  # libsodium layout: secret key = 32-byte seed || 32-byte public key.
  [ "$(tail -c 32 "$tmp/sk.bin" | base64 | tr -d '\n')" = "$pk" ] || return 1
  # Real Ed25519: derive the public key from the seed and compare ...
  # shellcheck disable=SC2059  # the prefix is an octal-escape format on purpose
  { printf "$PKCS8_PREFIX"; head -c 32 "$tmp/sk.bin"; } > "$tmp/key.der"
  openssl pkey -inform DER -in "$tmp/key.der" -pubout -outform DER 2>/dev/null \
    | tail -c 32 | base64 | tr -d '\n' > "$tmp/derived" || return 1
  [ "$(cat "$tmp/derived")" = "$pk" ] || return 1
  # ... and prove the pair works: sign with the seed, verify with the public key.
  # shellcheck disable=SC2059
  { printf "$SPKI_PREFIX"; cat "$tmp/pk.bin"; } > "$tmp/pub.der"
  printf 'remotesmith keycheck %s' "$$" > "$tmp/msg"
  openssl pkeyutl -sign -rawin -inkey "$tmp/key.der" -keyform DER -in "$tmp/msg" -out "$tmp/sig" 2>/dev/null || return 1
  openssl pkeyutl -verify -rawin -pubin -inkey "$tmp/pub.der" -keyform DER -in "$tmp/msg" -sigfile "$tmp/sig" >/dev/null 2>&1 || return 1
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
