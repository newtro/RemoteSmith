#!/usr/bin/env bash
# check-private.sh: fail if the repo carries a deployment's personal infrastructure
# (PLAN AC-33, DEC-15). Run by scripts/verify.sh. Scans tracked files plus untracked,
# non-ignored ones (what the next commit could add), excluding vendor/.
#
#   1. Always, CI included: no public IP address literals. IPv4 must be loopback,
#      private, link-local, documentation (RFC 5737) or a well-known public resolver;
#      IPv6 global addresses must be documentation (2001:db8::/32, RFC 3849).
#   2. When deploy/server/site.env exists (gitignored): none of its identifying values
#      (RS_HOST, CF_ZONE, RS_IPV4, RS_IPV6 and its /64, LIGHTSAIL_INSTANCE,
#      CF_CREDENTIALS_FILE, "ssh SSH_TARGET").
#   3. When .private-patterns exists at the repo root (gitignored): none of its lines
#      (fixed strings, whole-word; blank lines and # comments ignored).
#
# Findings print as file:line only, never the matched text, so the output can't leak
# the values it guards.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
scope=(-- . ':(exclude)vendor' ':(exclude)vendor/**')

files() { git ls-files -z --cached --others --exclude-standard "${scope[@]}"; }

# 1. IP literals.
ip_hits=$(files | xargs -0 grep -IHnoE '[0-9]+(\.[0-9]+){3,}|[0-9A-Fa-f]{4}(:[0-9A-Fa-f]{0,4}){3,7}' 2>/dev/null \
  | awk -F: '
    function bad4(a,  p, n) {
      n = split(a, p, ".")
      if (n != 4) return 0
      if (p[1] > 255 || p[2] > 255 || p[3] > 255 || p[4] > 255) return 0
      if (p[1] == 0 || p[1] == 10 || p[1] == 127 || p[1] >= 224) return 0
      if (p[1] == 169 && p[2] == 254) return 0
      if (p[1] == 172 && p[2] >= 16 && p[2] <= 31) return 0
      if (p[1] == 192 && p[2] == 168) return 0
      if (a ~ /^(192\.0\.2|198\.51\.100|203\.0\.113)\./) return 0
      if (a == "1.1.1.1" || a == "1.0.0.1" || a == "8.8.8.8" || a == "8.8.4.4") return 0
      return 1
    }
    {
      file = $1; line = $2
      val = substr($0, length($1) + length($2) + 3)
      if (val ~ /^[0-9.]+$/) { if (bad4(val)) print file ":" line ": public IPv4 literal" }
      else if (tolower(val) ~ /^[23]/ && tolower(val) !~ /^2001:0?db8/) print file ":" line ": global IPv6 literal"
    }' | sort -u)
if [[ -n $ip_hits ]]; then
  printf '%s\n' "$ip_hits"
  echo "check-private: use documentation addresses (192.0.2.x, 2001:db8::) in the repo; real ones go in deploy/server/site.env"
  fail=1
fi

# 2 + 3. Fixed strings from the local, gitignored files.
pat=$(mktemp)
trap 'rm -f "$pat"' EXIT
if [[ -f deploy/server/site.env ]]; then
  # shellcheck disable=SC1091  # sourced relative to the repo root
  . deploy/server/site-env.sh
  site_env_load deploy/server/site.env
  site_env_leak_patterns >> "$pat"
fi
if [[ -f .private-patterns ]]; then
  grep -vE '^[[:space:]]*(#|$)' .private-patterns >> "$pat" || true
fi
if [[ -s $pat ]]; then
  # --untracked: the working tree's tracked files plus untracked, non-ignored ones.
  hits=$(git grep -IwnF --untracked -f "$pat" "${scope[@]}" | cut -d: -f1,2 | sort -u) || true
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits" | sed 's/$/: names private infrastructure (from site.env or .private-patterns)/'
    fail=1
  fi
  echo "check-private: checked $(wc -l < "$pat" | tr -d ' ') private strings"
else
  echo "check-private: no deploy/server/site.env or .private-patterns here; IP literal check only"
fi
[[ $fail == 0 ]] && echo "check-private: OK"
exit "$fail"
