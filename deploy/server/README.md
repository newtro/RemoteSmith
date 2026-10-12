# RemoteSmith server (hbbs + hbbr under systemd)

RemoteSmith uses the stock **rustdesk-server OSS 1.1.16** as its rendezvous (`hbbs`) and relay (`hbbr`) server. It runs as two native systemd services on a small Ubuntu 24.04 server, here a shared AWS Lightsail VM that also hosts other apps. This covers FLOW-3 and AC-4, AC-5 and AC-44 in [docs/PLAN.md](../../docs/PLAN.md).

## site.env: your deployment's values

The kit carries no deployment's host, addresses or account names (AC-33, DEC-15). Copy [`site.env.example`](site.env.example) to `site.env` in this directory and fill it in; `site.env` is gitignored.

| Key | Used by | What |
|---|---|---|
| `RS_HOST` | `install.sh`, `verify-server.sh`, clients | the server's public DNS name (ID server; relay is `RS_HOST:21117`) |
| `RS_IPV4`, `RS_IPV6` | `verify-server.sh`, dns.md | the server's public addresses: what the A / AAAA records must say. `RS_IPV6` may be empty. |
| `SSH_TARGET` | `verify-server.sh`, the procedures below | how the Mac reaches the server over ssh (an `~/.ssh/config` alias or `user@host`) |
| `LIGHTSAIL_INSTANCE`, `LIGHTSAIL_REGION`, `AWS_PROFILE` | firewall.md | the Lightsail instance firewall |
| `CF_ZONE`, `CF_CREDENTIALS_FILE` | dns.md | the Cloudflare zone holding `RS_HOST`, and the local file with the API token |

- **How it is read.** `site-env.sh` parses it line by line (`KEY=VALUE`, optional quotes, `#` comments) and never runs it as shell, since `install.sh` reads it as root. Unknown keys are an error.
- **It ships with the kit.** The procedures below copy this directory, `site.env` included, to the server. `install.sh` takes `RS_HOST` from the `site.env` beside it (`--host NAME` overrides) and renders `remotesmith-hbbs.service` from `remotesmith-hbbs.service.in`. The copies it keeps in `kit/` and `kits/<ver>/` hold `site.env` and the rendered unit, so `--status` and `--rollback` work there. Without a `site.env`, `--status` and `--rollback` use the name in the installed hbbs unit.
- **Preview the units:** `deploy/server/install.sh --render-units DIR` writes both units as they would be installed.
- **The repo stays clean.** `scripts/verify.sh` runs `scripts/check-private.sh`. It fails on any public IP literal in the repo, and, when `site.env` exists, on any of its identifying values (plus the fixed strings in an optional, gitignored `.private-patterns` at the repo root). It prints only file and line, never the value.
- **Ports are not settings.** The kit uses RustDesk's defaults (TCP 21115–21117, UDP 21116), which is what clients assume when given only a host.

The commands below are run from the repo root on the Mac after loading it: `set -a; . deploy/server/site.env; set +a`.

| File | What it is |
|---|---|
| `install.sh` | Install or upgrade (pinned version + SHA-256), `--rollback`, `--restore-data`, `--status`, `--dry-run`. Runs **on the VM** as root. |
| `remotesmith-hbbs.service.in`, `remotesmith-hbbr.service` | systemd units: hardened, memory-capped, key enforced. The hbbs unit is a template: `install.sh` fills in `@RS_HOST@`. |
| `site.env.example`, `site-env.sh` | Your deployment's values (copy to the gitignored `site.env`) and the parser the scripts share. |
| `keycheck.sh`, `wait-resolve.sh` | Pre-start helpers. `keycheck.sh` validates the keypair so the units fail closed (see "Key enforcement"). `wait-resolve.sh` waits for DNS at boot (see "Known caveats"). |
| `firewall.sh`, `firewall.md` | The ufw rules (a script) and the Lightsail firewall changes (AWS MCP and CLI). |
| `dns.md` | The A and AAAA records (Cloudflare API), DNS-only. |
| `verify-server.sh`, `probe.py` | Outside-in checks from the Mac: DNS, ports, UDP, AC-5 wire probes, and the public key. |

## Decisions

- **systemd, not Docker.**
  - The target VM has no Docker, and RAM is the binding constraint: about 1.6 GiB available, with swap nearly full when surveyed.
  - Native binaries cost about 10 MiB in total, and every other app on the box is a systemd unit.
  - PLAN §4 says Docker; [IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md) records the deviation.
- **A dedicated `remotesmith` system user, not `User=ubuntu`.**
  - The VM's convention for its other apps (`User=ubuntu`) is for loopback-only apps behind a reverse proxy. hbbs and hbbr are different: they listen on public ports directly, on all interfaces, and they hold the server's private key.
  - Running them as `ubuntu` would let a compromised relay read every other app's `.env`, and `ubuntu` has sudo.
  - Another internet-facing daemon on the same box already runs as its own dedicated user.
  - The user has no shell and no login, and owns only `data/`.
- **Layout.** Everything lives under `/opt/remotesmith-server`:

  ```
  releases/<ver>/   hbbs, hbbr, rustdesk-utils for each installed version (root-owned, read-only)
  bin -> releases/1.1.16      live version; the units run bin/hbbs and bin/hbbr
  previous -> releases/<ver>  what --rollback returns to
  data/             id_ed25519, id_ed25519.pub, db_v2.sqlite3 (remotesmith, 0700)
  backups/          data-<UTC>-<ver>-<reason>.tar.gz before every change (root, 0700)
  libexec/          keycheck.sh, wait-resolve.sh
  kit/              copy of this directory, so status and rollback work without the repo
  kits/<ver>/       the kit (units + helpers) last installed with each version
  id_ed25519.pub    public copy of the key (0644), read by verify-server.sh
  ```

- **Version pinning.** `install.sh` carries a table of version, architecture and the SHA-256 of `rustdesk-server-linux-<arch>.zip`. The SHA-256s come from GitHub's asset digests and match a fresh download. A download that doesn't match is fatal.
  - The VM is `x86_64` (`amd64`). `arm64v8` is in the table too, which is how the units were tested in an arm64 container.
  - 1.1.15 is kept in the table only as a known-good rollback and rehearsal target. v1 runs 1.1.16, which fixes the UDP punch-hole amplification issue (PLAN §9).

## First deploy (orchestrator, from the Mac)

0. **site.env:** copy `site.env.example` to `site.env` and fill it in (see above), then `set -a; . deploy/server/site.env; set +a`.
1. **Survey.** These are read-only. Before the first deploy, expect nothing on 21114–21119, no `remotesmith*` units, no `/opt/remotesmith-server`, and enough free memory (the caps total 192 MiB):
   - `ssh "$SSH_TARGET" 'free -h; ss -ltnup | grep -E ":211(1[4-9])\b"; systemctl list-unit-files | grep remotesmith; ls /opt'`
2. **DNS first:** follow [dns.md](dns.md). On a shared zone this needs approval.
   - Wait until `ssh "$SSH_TARGET" getent ahosts "$RS_HOST"` answers.
   - hbbs resolves its relay name only once, at start, so `install.sh` refuses to run until the name resolves.
3. **Copy the kit and install:**

   ```bash
   rsync -a --delete deploy/server/ "$SSH_TARGET":/tmp/remotesmith-server-kit/   # site.env included
   ssh "$SSH_TARGET" 'sudo /tmp/remotesmith-server-kit/install.sh --dry-run'   # review
   ssh "$SSH_TARGET" 'sudo /tmp/remotesmith-server-kit/install.sh'
   ```

   It downloads and verifies 1.1.16 and creates the user and directories.

   It generates the server keypair with `rustdesk-utils genkeypair` and backs it up straight away to `backups/…-new-key.tar.gz`.

   It installs the units, starts hbbs and then hbbr, checks health, and prints the public key.
4. **Host firewall:** `ssh "$SSH_TARGET" 'sudo /opt/remotesmith-server/kit/firewall.sh'`. This needs approval, as a firewall change.
5. **Cloud firewall:** follow [firewall.md](firewall.md) §1 (Lightsail, AWS MCP or CLI). This also needs approval.
6. **Verify:** run `deploy/server/verify-server.sh` from the Mac. It must end `0 failed`.
7. **Off-box key backup:** see "Keys" below. If the key exists only on the VM, losing the VM means re-keying every client.

**What changes on the box:**
- `/opt/remotesmith-server`;
- the `remotesmith` system user;
- `/etc/systemd/system/remotesmith-{hbbs,hbbr}.service`, plus the `multi-user.target.wants` symlinks;
- two ufw rules.

Nothing else is touched: no Caddy, Postgres or Redis, and no restart of any other unit. The only `systemctl` actions are `daemon-reload` and starts or restarts of these two units.

## Upgrade

1. Add the new version's SHA-256 values to `asset_sha256()` in `install.sh`. Take them from the release page's asset digests, then confirm against a download with `shasum -a 256`. Set `DEFAULT_VERSION`.
2. Update the unit comments if the flags changed. Diff the upstream release notes for argument changes.
3. Run `scripts/verify.sh`, rsync the kit (with `site.env`) as in "First deploy", and run `ssh "$SSH_TARGET" 'sudo /tmp/remotesmith-server-kit/install.sh'`. This:
   - backs up `data/`;
   - installs `releases/<new>` and the new units and helpers;
   - points `previous` at the old version and `bin` at the new one;
   - restarts hbbs, then hbbr;
   - runs the health checks.

   **Any failure after the run starts changing live state reverts it automatically.** That includes an unhealthy result, a failed command, or a `die`. An EXIT trap re-points `bin` and `previous`, reinstalls the units and helpers from `kits/<current>`, restarts both units and re-checks health, then exits non-zero. If there is no snapshot, it uses this run's `.bak.<UTC>` unit files. This covers a new version, and also a kit-only change such as an edited unit with the same version.
4. Run `deploy/server/verify-server.sh`.

   **AC-44:** the key in `data/` never changes on an upgrade, so existing clients keep working without re-keying. In the arm64 rehearsal (1.1.15 → 1.1.16 → rollback → 1.1.16), the public key was identical at every step.

Re-running `install.sh` with nothing changed is a no-op, decided strictly. Nothing is restarted only when all of these hold:
- the `.deployed` marker matches a fingerprint of the bin target, the installed units and the helpers. The marker is written after every healthy install, rollback or restore;
- both units are active;
- both main processes run `releases/<target>` (`/proc/<pid>/exe`, resolved).

So an interrupted earlier run always gets a restart. A no-op run still takes a fresh data backup, which is about 2 KB.

## Rollback

```bash
ssh "$SSH_TARGET" 'sudo /opt/remotesmith-server/kit/install.sh --rollback'
```

This swaps `bin` and `previous` and reinstalls the units and helpers recorded in `kits/<previous>`. If there is no snapshot, it warns and keeps the current ones. Then it restarts both units and runs the health check. It never touches the keys. Run it again to roll forward.

## Keys: backup and restore

- **What the key is.** `data/id_ed25519` is the server's secret key, and `id_ed25519.pub` is what clients are configured with.
  - `install.sh` generates the pair once, with the release's own `rustdesk-utils genkeypair`, before any daemon starts.
  - The files are written, validated and chowned inside a fresh root-owned `mktemp -d` under `/opt/remotesmith-server`, never by name inside the daemon-writable `data/`. They are then moved in with `rename(2)`, which never follows a destination symlink. It refuses if anything already sits at the destination.
  - It never replaces an existing pair: an invalid, mismatched or symlinked pair stops the install and points to `--restore-data`.
  - The public key is published, or used by health, only if it reads back from a regular file as a valid 44-character key. A planted symlink in `data/` can't leak another file.
  - Rotating the key means re-keying every client (PLAN §7 data table).
- **On-box backups.** Every install, upgrade, rollback and restore first writes `backups/data-<UTC>-<ver>-<reason>.tar.gz`. A brand-new key is backed up right after it is created.
  - These are hot copies. The key files are static, so they're always consistent. `db_v2.sqlite3` is only a peer cache that clients re-populate by re-registering.
  - Nothing prunes these backups. Each one is about 2 KB.
- **Off-box copy** (do this once after the first deploy, and keep it private):

  ```bash
  mkdir -p ~/.config/remotesmith/server-keys && chmod 700 ~/.config/remotesmith/server-keys
  f=~/.config/remotesmith/server-keys/rs-keys-$(date -u +%Y%m%d).tar.gz
  ssh "$SSH_TARGET" 'sudo tar -C /opt/remotesmith-server -czf - data/id_ed25519 data/id_ed25519.pub' > "$f" \
    && tar -tzf "$f" | grep -qx data/id_ed25519 && chmod 600 "$f" && echo "saved $f"
  ```

  Check whether the VM's own backup jobs cover `/opt/remotesmith-server`; don't assume they do.
- **Restore** (same VM, or a rebuilt one after `install.sh`):

  ```bash
  scp ~/.config/remotesmith/server-keys/rs-keys-YYYYMMDD.tar.gz "$SSH_TARGET":/tmp/
  ssh "$SSH_TARGET" 'sudo /opt/remotesmith-server/kit/install.sh --restore-data /tmp/rs-keys-YYYYMMDD.tar.gz && rm /tmp/rs-keys-YYYYMMDD.tar.gz'
  ```

  `--restore-data` checks the tarball first:
  - it holds only plain files;
  - the only names allowed are `data/` plus `id_ed25519`, `id_ed25519.pub`, `db_v2.sqlite3*`, `blacklist.txt` and `blocklist.txt`. In particular there is no `.env`: both daemons would load settings from one.
  - it unpacks to less than 256 MiB;
  - it contains a valid keypair.

  Then it backs up the current `data/` and stops both units. It moves the current `data/` aside to `backups/data-replaced-<UTC>`, puts the restored one in place and fixes ownership. Finally it starts both units and checks health.

## How clients get the host and key

On first run (FLOW-2), each RemoteSmith install asks for:

- **ID server:** `RS_HOST`. The ports are hbbs's defaults: 21116, and 21115 for the NAT test.
- **Relay server:** `RS_HOST:21117`. hbbs also hands this out through `-r`, so it can be left blank.
- **Key:** the contents of `/opt/remotesmith-server/id_ed25519.pub`, a 44-character base64 string. Print it with `deploy/server/verify-server.sh` (the last lines) or `ssh "$SSH_TARGET" cat /opt/remotesmith-server/id_ed25519.pub`.

`verify-server.sh` prints all three at the end. The host and key are personal configuration. It stays in local client config and never goes in the repo (AC-33, AC-40).

## Key enforcement (AC-5): what the OSS server actually does

Both daemons run with `-k _`. That means: load the keypair from `data/` and refuse any client whose key differs. This was read from the 1.1.16 source and checked on the wire with `probe.py`, then by the adversarial review:

| Request | Wrong key or no key | Right key |
|---|---|---|
| hbbs `PunchHoleRequest` (TCP 21116): "connect me to peer X" | `PunchHoleResponse{failure: LICENSE_MISMATCH}`, logged as `Authentication failed ... invalid key` | processed (an unknown ID gives `ID_NOT_EXIST`) |
| hbbr `RequestRelay` (TCP 21117) | dropped at once, logged as `Relay authentication failed ... invalid key` | held open for 30 s, waiting for the other side |
| hbbs `RegisterPeer` / `RegisterPk` (UDP 21116): "I'm online as ID N" | **accepted**: the OSS registration messages carry no key | accepted |
| hbbs `RequestRelay` (TCP 21116): "tell peer X to meet me at relay R" | **accepted and forwarded to the peer**: there is no key check on this path (`rendezvous_server.rs` ~L493) | forwarded |

What that means:

- **What the server does refuse.** A wrong-key or keyless client can't get a punch-hole introduction from hbbs, and can't use hbbr.
- **Gap 1: registration.** A client with any key can register an ID; registration is not key-gated in OSS.
- **Gap 2: relay requests through hbbs (found by the review, reproduced on the wire).** A keyless client can send hbbs a `RequestRelay` for any online peer, naming **a relay of its own choosing**. hbbs forwards it. The stock 1.5.0 client then connects to that relay without checking it (`rendezvous_mediator.rs` `handle_request_relay` → `create_relay`).
  - After that, only the host's password stands in the way. The server-side key doesn't.
  - The server kit can't close this: it's in upstream hbbs.
  - The fix belongs in the client patches. A host should refuse a `RequestRelay` whose relay isn't its own configured relay, or isn't the rendezvous host on port 21117.

**Scope decision (orchestrator gate review): the server is not patched.**
- **What the server enforces:** OSS hbbs can't reject a keyless `RegisterPeer`/`RegisterPk`, because the protocol has no key there. It also forwards `RequestRelay` without a key check.
- **Where the gap is closed:** client-side, under **AC-61**. A host accepts relay requests only for its own configured relay.
- **What `verify-server.sh` reports:** it tests exactly what the server enforces, then prints this line instead of passing silently:

  ```
  KNOWN LIMITATION: registration and relay-request forwarding are not key-checked by OSS hbbs;
                    mitigated client-side (host accepts only its configured relay) - see AC-61
  ```

`verify-server.sh` automates the enforceable part: wrong key, missing key and right key against hbbs `PunchHoleRequest` and against hbbr.
- **Addresses probed:** the literal A record address, and the AAAA address when this host has IPv6.
- **Not exercised:** gap 2. That would need a registered peer, and `RegisterPk` persists to the peer DB. AC-61's two-machine test covers it: a keyless client's `RequestRelay` naming a foreign relay is refused by the host.

**Fail-closed guard.** In 1.1.16, `-k _` fails **open**: if the key file can't be read, or can't be created, the daemon silently runs with an empty key and accepts everyone. An unreadable `id_ed25519` also makes hbbs overwrite `id_ed25519.pub` with a fresh key that no longer matches.

The kit prevents all of that:

- **The key comes first.** `install.sh` creates the key before any daemon starts.
- **Both units check it before starting.** Both run `ExecStartPre=keycheck.sh`. It requires:
  - two regular files, not symlinks;
  - a 64-byte libsodium secret key and a 32-byte public key;
  - the public key **derived from the seed with real Ed25519** (`openssl pkey`, OpenSSL 3.0.13 on the VM) equals the stored one;
  - a **sign/verify positive control** passes (`openssl pkeyutl -rawin`).

  If the key is broken, the daemon never starts. It never serves without enforcement and never gets to mint its own key.
- **hbbr can't write `data/`.**
- **The health check confirms it on the wire, not from the journal, which rotates.** The check is `install.sh` health, which also runs on `--status`. From the VM it sends hbbs (127.0.0.1:21116) and hbbr (the VM's own non-loopback address, port 21117) a wrong key, which must be refused, and the server key, which must be accepted.

This was tested in the container: unreadable key, unwritable `data/`, and wrong-key probes during the failure, all refused.

## Memory

These figures were measured on 1.1.16 (arm64 container, same code as amd64) with `systemctl show -p MemoryPeak`:

| | idle | peak under load | cap (`MemoryHigh` / `MemoryMax`) |
|---|---|---|---|
| hbbs | ~5 MiB | ~7 MiB (600 concurrent TCP sessions + 3000 UDP registrations) | 48M / 64M |
| hbbr | ~3 MiB | ~25 MiB (300 pending relays, then 4 relayed streams moving 800 MB) | 96M / 128M |

The caps protect the neighbours rather than the daemons. Each unit also has `TasksMax=256`.

## Hardening (both units)

- **Privileges:** `NoNewPrivileges` and an empty capability set. The ports are above 1024, so no capabilities are needed.
- **Filesystem:**
  - `ProtectSystem=strict`; hbbs gets `ReadWritePaths=` for `data/` only, and hbbr gets none.
  - `ProtectHome`, `PrivateTmp`, `PrivateDevices`.
  - `ProtectKernel*`, `ProtectControlGroups`, `ProtectClock`, `ProtectHostname`.
  - `ProtectProc=invisible`, `ProcSubset=pid`, `UMask=0077`.
- **Restrictions:**
  - `RestrictNamespaces`, `RestrictRealtime`, `RestrictSUIDSGID`, `LockPersonality`, `MemoryDenyWriteExecute`, `RemoveIPC`.
  - `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`.
  - `SystemCallFilter=@system-service ~@privileged @resources`, with `SystemCallArchitectures=native`.
- **Result:** `systemd-analyze security` scores both units **1.2 OK**, and `systemd-analyze verify` is clean on systemd 255 (Ubuntu 24.04, the VM's version).
- **Restarts:** `Restart=on-failure` with backoff (`RestartSec=10`, `RestartSteps=8`, `RestartMaxDelaySec=300`) and `StartLimitIntervalSec=0`. After a long DNS or key outage the units keep retrying, every 5 minutes at most, instead of parking in `failed`. `install.sh` runs `reset-failed` before its own restarts.

## Known caveats

- **WebSocket ports 21118 and 21119 are always bound.** hbbs and hbbr have no flag to disable them. Only the two firewall layers keep them private, and `verify-server.sh` checks that from outside.
- **The loopback admin channels.** Both daemons treat a TCP connection from 127.0.0.1 as an unauthenticated admin console. That's upstream design.
  - hbbs on 21115 can change `relay-servers`, the IP blocker and `always-use-relay`.
  - hbbr on 21117 can change the blocklist and bandwidth limits.
  - Any process on the VM can reach them, including an SSRF in one of the other apps. That fits the box's existing trust model, but it is a real lever: a local compromise could redirect the relay hbbs hands out.
  - It also means probes must come from outside the VM. From inside, the "right key" relay probe reads as "refused".
- **The relay name is resolved once.** hbbs resolves `--relay-servers RS_HOST:21117` only at start, and permanently drops it if DNS fails. Clients then get no relay from hbbs.
  - DNS is therefore a start condition. `ExecStartPre=wait-resolve.sh` waits up to 60 s, then **fails the start**, and `Restart=` retries with backoff until the name resolves. hbbs never runs without its relay.
  - `install.sh` refuses to install before the name resolves.
  - The health check (`--status`) asks hbbs's admin port for its live relay list (`probe.py relays`). That list must be exactly `RS_HOST:21117`; if it isn't, the check prints the restart command. This also catches a relay rewritten through the loopback admin channel.
- **Version check.** Once a day, hbbs posts its version to RustDesk's version-check API and logs `new version is available`. This is outbound only, and no client data goes with it. AC-6 is about clients and is unaffected. The calls can't be blocked per unit without also blocking replies to clients.
- **Log noise.** `ERROR ... Failed to generate new id` appears three times at hbbs start. hbb_common's client `Config` tries to read a MAC address over netlink, which the sandbox blocks (`RestrictAddressFamilies`). The server doesn't use that ID. Allowing `AF_NETLINK` would only let it write a client config file into `data/`.
- **x86_64 sandbox.** The units were exercised under load only on arm64. The amd64 binary is static-pie, and its SHA-256 matches the pin. The first real-VM start is the x86_64 check of `SystemCallFilter`, and `install.sh` health-checks it and fails loudly.
- **Is the IPv4 address static?** Without a static IP, a Lightsail instance gets a new public address when stopped and started (see dns.md). That applies to the whole VM, not just RemoteSmith.

## Verification

- **Static checks:** `scripts/verify.sh` runs `shellcheck` and `bash -n` on these scripts, and `python3 -m py_compile` on `probe.py`.
- **Dry run on the Mac:** `deploy/server/install.sh --dry-run --arch amd64` (add `--host rs.example.com` without a `site.env`). `scripts/verify.sh` runs it with `--host rs.example.com`, renders the units, and runs `scripts/check-private.sh`.
- **Full rehearsal:** in an Ubuntu 24.04 systemd container (`--privileged`, arm64), with ufw active and `firewall.sh` applied:
  - fresh install (key generated by `install.sh`);
  - an idempotent re-run (PID unchanged);
  - upgrade 1.1.15 → 1.1.16;
  - rollback and roll-forward (units restored from `kits/`);
  - auto-rollback of an upgrade that fails to start;
  - `--restore-data`, including refusing a tarball with a symlink;
  - the fail-closed guards;
  - the load test;
  - `verify-server.sh` from a second container: 14 passed, 0 failed, with 21114, 21118 and 21119 filtered.
- **Against production:** `deploy/server/verify-server.sh`.
