# RemoteSmith server (hbbs + hbbr on Lightsail)

RemoteSmith uses the stock **rustdesk-server OSS 1.1.16** as its rendezvous (`hbbs`) and relay (`hbbr`) server. It runs as two native systemd services on the shared Lightsail VM (`ssh vm`, instance `Ubuntu-1`), under the name `rs.johnnycode.ai`. This covers FLOW-3 and AC-4, AC-5 and AC-44 in [docs/PLAN.md](../../docs/PLAN.md).

| File | What it is |
|---|---|
| `install.sh` | Install or upgrade (pinned version + SHA-256), `--rollback`, `--restore-data`, `--status`, `--dry-run`. Runs **on the VM** as root. |
| `remotesmith-hbbs.service`, `remotesmith-hbbr.service` | systemd units: hardened, memory-capped, key enforced. |
| `keycheck.sh` | Validates the keypair. The units use it to fail closed (see "Key enforcement"). |
| `firewall.sh`, `firewall.md` | The ufw rules (a script) and the Lightsail firewall changes (AWS MCP and CLI). |
| `dns.md` | The Cloudflare A and AAAA records, DNS-only. |
| `verify-server.sh`, `probe.py` | Outside-in checks from the Mac: DNS, ports, UDP, AC-5 wire probes, and the public key. |

## Decisions

- **systemd, not Docker.**
  - The VM has no Docker, and RAM is the binding constraint: about 1.6 GiB available, with swap nearly full on 2026-10-12.
  - Native binaries cost about 10 MiB in total, and every other app on the box is a systemd unit.
  - PLAN §4 says Docker; [IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md) records the deviation.
- **A dedicated `remotesmith` system user, not `User=ubuntu`.**
  - The `aws-vm` convention (`User=ubuntu`) is for loopback-only apps behind Caddy. hbbs and hbbr are different: they listen on public ports directly, on all interfaces, and they hold the server's private key.
  - Running them as `ubuntu` would let a compromised relay read every other app's `.env`, and `ubuntu` has sudo.
  - `waypoint-relay` on the same box already set the precedent of a dedicated user for an internet-facing daemon.
  - The user has no shell and no login, and owns only `data/`.
- **Layout.** Everything lives under `/opt/remotesmith-server`:

  ```
  releases/<ver>/   hbbs, hbbr, rustdesk-utils for each installed version (root-owned, read-only)
  bin -> releases/1.1.16      live version; the units run bin/hbbs and bin/hbbr
  previous -> releases/<ver>  what --rollback returns to
  data/             id_ed25519, id_ed25519.pub, db_v2.sqlite3 (remotesmith, 0700)
  backups/          data-<UTC>-<ver>-<reason>.tar.gz before every change (root, 0700)
  libexec/keycheck.sh
  kit/              copy of this directory, so status and rollback work without the repo
  id_ed25519.pub    public copy of the key (0644), read by verify-server.sh
  ```

- **Version pinning.** `install.sh` carries a table of version, architecture and the SHA-256 of `rustdesk-server-linux-<arch>.zip`. The SHA-256s come from GitHub's asset digests and match a fresh download. A download that doesn't match is fatal.
  - The VM is `x86_64` (`amd64`). `arm64v8` is in the table too, which is how the units were tested in an arm64 container.
  - 1.1.15 is kept in the table only as a known-good rollback and rehearsal target. v1 runs 1.1.16, which fixes the UDP punch-hole amplification issue (PLAN §9).

## First deploy (orchestrator, from the Mac)

1. **Survey.** These are read-only, and their results on 2026-10-12 are listed below:
   - `ssh vm 'free -h; ss -ltnup | grep -E ":211(1[4-9])\b"; systemctl list-unit-files | grep remotesmith; ls /opt'`
   - Nothing was on 21114–21119.
   - No `remotesmith*` units or `/opt/remotesmith-server` existed.
   - MemAvailable was about 1.6 GiB.
2. **Copy the kit and install:**

   ```bash
   rsync -a --delete deploy/server/ vm:/tmp/remotesmith-server-kit/
   ssh vm 'sudo /tmp/remotesmith-server-kit/install.sh --dry-run'   # review
   ssh vm 'sudo /tmp/remotesmith-server-kit/install.sh'
   ```

   It downloads and verifies 1.1.16 and creates the user and directories. It installs the units, starts hbbs (which creates the keypair) and then hbbr. Finally it checks health and prints the public key.
3. **Host firewall:** `ssh vm 'sudo /opt/remotesmith-server/kit/firewall.sh'`. This needs approval, as a firewall change.
4. **Lightsail firewall:** follow [firewall.md](firewall.md) §1 with the AWS MCP. This also needs approval.
5. **DNS:** follow [dns.md](dns.md). This needs approval.
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
3. Run `scripts/verify.sh`, rsync the kit, and run `ssh vm 'sudo /tmp/remotesmith-server-kit/install.sh'`. This:
   - backs up `data/`;
   - installs `releases/<new>`;
   - points `previous` at the old version and `bin` at the new one;
   - restarts hbbs, then hbbr;
   - runs the health checks.

   **If the new version is unhealthy, it automatically switches back** and exits non-zero.
4. Run `deploy/server/verify-server.sh`.

   **AC-44:** the key in `data/` never changes on an upgrade, so existing clients keep working without re-keying. In the arm64 rehearsal (1.1.15 → 1.1.16 → rollback → 1.1.16), the public key was identical at every step.

Re-running `install.sh` with nothing changed is a no-op. It doesn't restart anything, but it still takes a fresh data backup, which is about 2 KB.

## Rollback

```bash
ssh vm 'sudo /opt/remotesmith-server/kit/install.sh --rollback'
```

This swaps `bin` and `previous`, restarts both units and runs the health check. It never touches the keys. Run it again to roll forward.

## Keys: backup and restore

- **What the key is.** `data/id_ed25519` is the server's secret key, and `id_ed25519.pub` is what clients are configured with. hbbs creates the pair on first start and never rotates it. Rotating it means re-keying every client (PLAN §7 data table).
- **On-box backups.** Every install, upgrade, rollback and restore first writes `backups/data-<UTC>-<ver>-<reason>.tar.gz`.
  - These are hot copies. The key files are static, so they're always consistent. `db_v2.sqlite3` is only a peer cache that clients re-populate by re-registering.
  - Nothing prunes these backups. Each one is about 2 KB.
- **Off-box copy** (do this once after the first deploy, and keep it private):

  ```bash
  mkdir -p ~/.config/remotesmith/server-keys && chmod 700 ~/.config/remotesmith/server-keys
  ssh vm 'sudo sh -c "ls -1t /opt/remotesmith-server/backups/*.tar.gz | head -1 | xargs cat"' \
    > ~/.config/remotesmith/server-keys/rs-data-$(date -u +%Y%m%d).tar.gz
  chmod 600 ~/.config/remotesmith/server-keys/*.tar.gz
  ```

  `pg-nightly-backup` does **not** cover this directory.
- **Restore** (same VM, or a rebuilt one after `install.sh`):

  ```bash
  scp ~/.config/remotesmith/server-keys/rs-data-YYYYMMDD.tar.gz vm:/tmp/
  ssh vm 'sudo /opt/remotesmith-server/kit/install.sh --restore-data /tmp/rs-data-YYYYMMDD.tar.gz && rm /tmp/rs-data-YYYYMMDD.tar.gz'
  ```

  `--restore-data` first checks the tarball:
  - it holds only plain files and directories under `data/`;
  - it contains a valid keypair.

  Then it backs up the current `data/` and stops both units. It moves the current `data/` aside to `backups/data-replaced-<UTC>`, puts the restored one in place and fixes ownership. Finally it starts both units and checks health.

## How clients get the host and key

On first run (FLOW-2), each RemoteSmith install asks for:

- **ID server:** `rs.johnnycode.ai`. The ports are hbbs's defaults: 21116, and 21115 for the NAT test.
- **Relay server:** `rs.johnnycode.ai:21117`. hbbs also hands this out through `-r`, so it can be left blank.
- **Key:** the contents of `/opt/remotesmith-server/id_ed25519.pub`, a 44-character base64 string. Print it with `deploy/server/verify-server.sh` (the last lines) or `ssh vm cat /opt/remotesmith-server/id_ed25519.pub`.

The key is public, but it is personal configuration. It stays in local client config and never goes in the repo (AC-33, AC-40).

## Key enforcement (AC-5): what the OSS server actually does

Both daemons run with `-k _`. That means: load the keypair from `data/` and refuse any client whose key differs. This was read from the 1.1.16 source and confirmed on the wire by `probe.py`:

| Request | Wrong key or no key | Right key |
|---|---|---|
| hbbs `PunchHoleRequest` (TCP 21116): "connect me to peer X" | `PunchHoleResponse{failure: LICENSE_MISMATCH}`, logged as `Authentication failed ... invalid key` | processed normally (an unknown ID gives `ID_NOT_EXIST`) |
| hbbr `RequestRelay` (TCP 21117) | connection dropped at once, logged as `Relay authentication failed ... invalid key` | held open for 30 s, waiting for the other side |
| hbbs `RegisterPeer` (UDP 21116): "I'm online as ID N" | **accepted**. The OSS registration message carries no key field. | accepted |

So, precisely: a client with the wrong key or no key **can't connect to anyone and can't relay**. The refusal happens at connect time on hbbs and at relay time on hbbr. It isn't refused at registration, because OSS registration has no key in it.

The client side closes the remaining gap. A client that has the right key verifies that the server signed the peer's identity (`IdPk`) with the matching secret key, so a server or peer with a different key shows a key mismatch. A host configured with a wrong key can still put its ID online, but nobody can reach it through this server.

AC-5's wording ("refused when it tries to register or relay") should be read as connect-or-relay. Its two-machine test (wrong key on a real client) is in the AC checklist. `verify-server.sh` automates the server half.

**Fail-closed guard.** In 1.1.16, `-k _` fails **open**: if the key file can't be read, or can't be created on first start, the daemon silently runs with an empty key and accepts everyone. The units prevent that:

- **hbbs:** `ExecStartPost=keycheck.sh --wait 20` fails the unit unless a valid keypair exists within 20 s of start.
- **hbbr:** `ExecStartPre=keycheck.sh` refuses to start without one. hbbr has no write access to `data/`, so it can never mint a stray key of its own.
- **Ordering:** hbbr is ordered `After=` hbbs.
- **Health check:** `install.sh` also requires the line `Key: <public key>` in the current run's journal for both units.

Both failure paths were tested in the container: an unreadable key and an unwritable `data/`.

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
- **Restarts:** `Restart=on-failure` with `RestartSec=5` and a start limit of 10 per 120 s. `install.sh` runs `reset-failed` before its own restarts.

## Known caveats

- **WebSocket ports 21118 and 21119 are always bound.** hbbs and hbbr have no flag to disable them. Only the two firewall layers keep them private, and `verify-server.sh` checks that from outside.
- **The hbbr loopback admin channel.** hbbr treats any TCP connection from 127.0.0.1 to 21117 as an unauthenticated admin console (blocklist, bandwidth limits). That's upstream design. Only processes on the VM can reach it, so it's no worse than the box's existing trust model. But it means probes must come from outside the VM: from inside, the "right key" relay probe reads as "refused".
- **Version check.** Once a day, hbbs posts its version to RustDesk's version-check API and logs `new version is available`. This is outbound only, and no client data goes with it. AC-6 is about clients and is unaffected. The calls can't be blocked per unit without also blocking replies to clients.
- **Log noise.** `ERROR ... Failed to generate new id` appears three times at hbbs start. hbb_common's client `Config` tries to read a MAC address over netlink, which the sandbox blocks (`RestrictAddressFamilies`). The server doesn't use that ID. Allowing `AF_NETLINK` would only let it write a client config file into `data/`.
- **The IPv4 address isn't static.** `98.93.243.79` is not a Lightsail static IP (see dns.md). This applies to the whole VM, not just RemoteSmith.

## Verification

- **Static checks:** `scripts/verify.sh` runs `shellcheck` and `bash -n` on these scripts, and `python3 -m py_compile` on `probe.py`.
- **Dry run on the Mac:** `deploy/server/install.sh --dry-run --arch amd64`.
- **Full rehearsal:** in an Ubuntu 24.04 systemd container (`--privileged`, arm64), with ufw active and `firewall.sh` applied:
  - fresh install;
  - an idempotent re-run (PID unchanged);
  - upgrade 1.1.15 → 1.1.16;
  - rollback and roll-forward;
  - `--restore-data`, including refusing a tarball with a symlink;
  - the fail-closed guards;
  - the load test;
  - `verify-server.sh` from a second container: 14 passed, 0 failed, with 21114, 21118 and 21119 filtered.
- **Against production:** `deploy/server/verify-server.sh`.
