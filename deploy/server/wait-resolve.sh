#!/bin/sh
# wait-resolve.sh NAME SECONDS: wait until NAME resolves (getent ahosts), up to SECONDS.
# Used by remotesmith-hbbs.service before start: hbbs resolves its --relay-servers name
# once and drops it for good if that fails.
set -u
name=${1:?name}
secs=${2:-60}
i=0
while [ "$i" -lt "$secs" ]; do
  if getent ahosts "$name" >/dev/null 2>&1; then
    exit 0
  fi
  i=$((i + 1))
  sleep 1
done
echo "wait-resolve: $name did not resolve within ${secs}s; hbbs will hand clients no relay until restarted" >&2
exit 1
