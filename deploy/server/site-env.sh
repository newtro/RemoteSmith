# shellcheck shell=bash
# site-env.sh: read the deployment's site.env (see site.env.example). Sourced by
# install.sh, verify-server.sh and scripts/verify.sh.
#
# site.env is data, not a script: it is parsed line by line and never evaluated, so a
# value can't run anything even when install.sh reads it as root. Each line is blank,
# a "# comment", or KEY=VALUE with KEY from SITE_ENV_KEYS. The value may be wrapped in
# matching single or double quotes; nothing inside it is expanded.

SITE_ENV_KEYS=(RS_HOST RS_IPV4 RS_IPV6 SSH_TARGET LIGHTSAIL_INSTANCE LIGHTSAIL_REGION
  AWS_PROFILE CF_ZONE CF_CREDENTIALS_FILE)

# site_env_load FILE: set each key FILE names (only those). Returns non-zero, with a
# message on stderr, on a malformed line or an unknown key.
site_env_load() {
  local file=$1 line key val n=0 k ok
  [[ -f $file ]] || { printf 'site-env: no such file: %s\n' "$file" >&2; return 1; }
  while IFS= read -r line || [[ -n $line ]]; do
    n=$((n + 1))
    line=${line%$'\r'}
    [[ $line =~ ^[[:space:]]*(#.*)?$ ]] && continue
    if [[ ! $line =~ ^[[:space:]]*([A-Z][A-Z0-9_]*)=(.*)$ ]]; then
      printf 'site-env: %s:%d: not KEY=VALUE\n' "$file" "$n" >&2
      return 1
    fi
    key=${BASH_REMATCH[1]}
    val=${BASH_REMATCH[2]}
    val=${val%"${val##*[![:space:]]}"}
    if [[ $val =~ ^\"(.*)\"$ || $val =~ ^\'(.*)\'$ ]]; then
      val=${BASH_REMATCH[1]}
    elif [[ $val == *[\"\'\ ]* ]]; then
      printf 'site-env: %s:%d: %s: quote the whole value (no inline comments)\n' "$file" "$n" "$key" >&2
      return 1
    fi
    ok=0
    for k in "${SITE_ENV_KEYS[@]}"; do [[ $k == "$key" ]] && ok=1; done
    if [[ $ok == 0 ]]; then
      printf 'site-env: %s:%d: unknown key %s (known: %s)\n' "$file" "$n" "$key" "${SITE_ENV_KEYS[*]}" >&2
      return 1
    fi
    printf -v "$key" '%s' "$val"
  done < "$file"
}

# valid_hostname NAME: a DNS name with at least one dot (letters, digits, hyphens). This
# is also what keeps RS_HOST safe to substitute into the systemd unit.
valid_hostname() {
  local label='[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?'
  [[ ${#1} -le 253 && $1 =~ ^$label(\.$label)+$ ]]
}

# site_env_leak_patterns: print, one per line, the fixed strings that identify this
# deployment and must never appear in the repo. Uses whatever site_env_load set.
site_env_leak_patterns() {
  local v
  for v in "${RS_HOST:-}" "${CF_ZONE:-}" "${RS_IPV4:-}" "${RS_IPV6:-}" \
    "${LIGHTSAIL_INSTANCE:-}" "${CF_CREDENTIALS_FILE:-}"; do
    [[ -n $v ]] && printf '%s\n' "$v"
  done
  # The IPv4 address as cloud reverse-DNS names spell it (ec2-a-b-c-d...).
  [[ -n ${RS_IPV4:-} ]] && printf '%s\n' "${RS_IPV4//./-}"
  # The IPv6 /64 and /48 too: the interface ID can change while the prefix stays.
  if [[ ${RS_IPV6:-} =~ ^(([0-9A-Fa-f]{1,4}:[0-9A-Fa-f]{1,4}:[0-9A-Fa-f]{1,4}):[0-9A-Fa-f]{1,4}): ]]; then
    printf '%s\n%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
  fi
  # A ~ path in its spelled-out forms as well.
  if [[ ${CF_CREDENTIALS_FILE:-} == \~/* ]]; then
    printf '%s\n' "\$HOME/${CF_CREDENTIALS_FILE#\~/}"
    [[ -n ${HOME:-} ]] && printf '%s\n' "$HOME/${CF_CREDENTIALS_FILE#\~/}"
  fi
  [[ -n ${SSH_TARGET:-} ]] && printf 'ssh %s\n' "$SSH_TARGET"
  return 0
}
