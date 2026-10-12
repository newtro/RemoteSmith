# RemoteSmith two-machine acceptance checklist

Each unit appends its own section. Record results per release in `docs/test-runs/<date>-<version>.md`
(pass/fail per step, OS versions, keyboard mode, input source).

## Peer identity, LAN-first, reconnect, relay pinning, password change (unit B3, patches 0301-0306)

**Setup.** Both machines on the release build, service installed, `direct-server=Y` (hardened
default), the same server host and key, and a permanent password. Logs: Mac
`~/Library/Logs/RemoteSmith/` (controller UI and `--server`), Windows
`%APPDATA%\RemoteSmith\log\`. "Host" is the machine being controlled. Run every step in both
directions (Mac→PC and PC→Mac) unless it says otherwise. The host's own fingerprint is under
Settings > About > Fingerprint (and, once the sidecar exposes it, in the tray).

### B3-1 (AC-43) First connection shows ID and fingerprint; pin only after confirming
1. On the controller, remove the peer if present (peer card > Remove).
2. Connect to the host's ID. A "Confirm peer identity" dialog shows the host's ID and
   fingerprint and no password has been asked or sent (host log has no login attempt yet).
3. Compare with the host's Settings > About > Fingerprint: they match character for character.
4. Press Cancel: the tab closes; the peer file has no `remotesmith-host-fingerprint` option
   (Mac: `~/Library/Preferences/ai.johnnycode.RemoteSmith/peers/<id>.toml`).
5. Connect again, wait at least 40 s on the dialog (longer than the host's idle timeout), then
   press "Trust and connect": a "Connecting..." overlay appears and a fresh connection logs in
   (controller log: "identity confirmed and pinned; reconnecting to log in"); log in with the
   password and tick remember.
6. The peer file now has `remotesmith-host-fingerprint`, `remotesmith-host-id` and the saved
   password. Disconnect and connect again: no dialog, straight in.
7. Same steps connecting by the host's LAN IP typed into the ID box.

### B3-2 (AC-37, AC-43) Mismatch stops before the password; Forget peer re-enrolls
1. With the host enrolled (B3-1), stop RemoteSmith on the host and move its config aside so a new
   key pair is generated (Mac: `ai.johnnycode.RemoteSmith/RemoteSmith.toml`; Windows:
   `%APPDATA%\RemoteSmith\config\RemoteSmith.toml`; keep the ID with `--set-id` if it changes),
   start it again, set the same permanent password. The host's fingerprint is now different.
   (hbbs keeps the old key for that ID and refuses the re-keyed host's registration, so the ID
   route may fail with "Key mismatch" until the host gets a new ID; the LAN/IP route always works.)
2. Connect from the controller (by LAN IP, and by ID if the host registered). A "Peer identity mismatch" dialog shows
   the saved and the new ID/fingerprint. The host log shows **no** login attempt and no "wrong
   password" failure; the session never opens. No automatic retry follows.
3. Press "Forget peer": the peer disappears from the controller's lists (saved password gone).
4. Connect again: the first-use dialog (B3-1) appears with the new fingerprint; confirm; the
   session opens after entering the password.
5. Restore the host's original config afterwards (and repeat 4 to re-enroll the old key).
6. AC-37 with a different machine on the LAN address: on the controller set the peer's
   `remotesmith-lan-hint` to the IP of a third machine running RemoteSmith (or point a DHCP
   reservation at it). Connect by ID: the controller log shows "expected peer <id> but <other>
   answered" for the LAN attempt, then the ID route connects to the right host. Nothing is sent to the third
   machine beyond the handshake (its log shows no login).

### B3-3 (AC-38, AC-7, AC-8) LAN first within 3 s, then the ID route
LAN-first runs only for enrolled peers (pinned key), and only the pinned key can win it; the
first connection always enrolls over the ID route (hbbs vouches for the key) or a typed IP.
1. Enroll (B3-1) and connect once by ID with both on the LAN; afterwards the controller's peer file has
   `remotesmith-lan-hint = "<host LAN IP>"`, and the connection tooltip says "... (LAN)" from
   the second connection on.
2. AC-8: stop hbbs/hbbr on the VM (`sudo systemctl stop remotesmith-hbbs remotesmith-hbbr`).
   Connect by ID: the session opens over LAN. Start them again.
3. AC-38: set `remotesmith-lan-hint` to an unused LAN IP (e.g. `192.168.x.254`) and make sure LAN
   discovery has no entry for the host (Discovered tab empty, or the host on another subnet). Time
   Connect to session: the controller log shows "LAN route timed out after 3000 ms; trying the ID
   route" no more than 3 s after Connect, then the ID route (P2P or Relay) connects.
4. Cancel during the 3 s LAN wait (Cancel on the Connecting overlay): the tab closes and no
   session opens later (host log: no login; controller log: "connect cancelled before the session
   opened").

### B3-4 (AC-58) One session per connect; Cancel opens nothing later
1. Close every session window. Click Connect on the peer card five times as fast as possible:
   one window/tab, one session (host log: one login).
2. Repeat with a session window already open: still one tab for the peer.
3. With the host slow to answer (e.g. host on a throttled link, or LAN hint stale so the attempt
   takes 3 s), press Cancel on the Connecting overlay; wait 30 s: no session appears and the host
   log shows no login from the controller.

### B3-5 (AC-24, AC-46) Reconnect up to 60 s, Cancel, expiry
1. In a session, pull the host's network for 30 s and plug it back: the controller shows the
   retry dialog (countdown, Reconnect, Cancel, OK), retries without asking for the password, and
   the session continues within a few seconds of the network returning.
2. Pull it for 90 s: retries stop about 60 s after the first failure, and the window shows
   "Disconnected" with Reconnect and Close; the last frame stays.
3. While disconnected, type and click in the session window, then press Reconnect after the
   network returns: the session reconnects and none of the input made while disconnected appears
   on the host.
4. Start a reconnect (pull the network), press Cancel on the retry dialog or the Connecting
   overlay: "Disconnected" appears at once and no further attempt is logged.
5. Close the session tab yourself during a retry countdown: no reconnect happens afterwards
   (controller log: no new connection attempt; host log: nothing).
6. Enter a wrong saved password (edit it on the host, see B3-7) and reconnect: a wrong-password
   prompt, never an automatic retry.

### B3-6 (AC-41) Host releases held keys and buttons when the connection drops
1. Mac→PC: open Notepad on the PC. In the session hold Shift (or a letter key) and, while holding,
   pull the controller's network (or kill the controller app with `kill -9`).
2. On the PC, within about 10 s (host log: "peer silent while input is held; treating the
   connection as lost", then "connection ended holding N button(s) and M key(s); releasing"), type on the PC's physical keyboard: letters come out lowercase / the held key does not repeat.
3. Start a drag of a file icon on the host's desktop, and mid-drag kill the controller: the drag
   ends where the pointer was; a physical click on the host behaves normally (no stuck button).
4. Repeat 1-3 PC→Mac with TextEdit and a Finder icon.
5. Repeat on Windows with an elevated window focused (Task Manager) so the portable service path
   is used.
6. Stall without loss: in a relayed session, hold Shift and drag a window while the link stalls
   for about 5 s (e.g. `sudo pfctl`/Windows firewall rule blocking the relay for 5 s, then lift
   it): the session continues, the drag and Shift are still held, and releasing them on the
   controller releases them on the host.

### B3-7 (AC-39) Changing the host's password ends sessions; the old credential stops working
1. In an open session (repeat with a switch-sides session, toolbar > Switch sides), change the
   host's permanent password (host: Settings > Security > Change
   password). Within ~1 s the controller shows "Session closed by the peer: its password changed"
   and does not retry.
2. Connect again: the saved password is refused (wrong-password prompt); entering the new one
   works.
3. Open a port forward (or RDP) tunnel to the host, change the password: the tunnel closes within
   30 s.
4. Forget the peer on the controller: its saved credential is removed (peer file deleted).

### B3-8 (AC-61) Host refuses a relay it did not choose
1. Prerequisite: a client with the right server host but no key (a stock RustDesk with only the
   ID server set, or `force-always-relay=Y` and a custom `relay-server=<attacker host>:21117` on a
   RemoteSmith controller).
2. From it, connect to the host's ID with relay forced and the attacker relay set. The host log
   shows "refused a relay request from ... naming relay "<attacker host>:21117"; this host only
   uses rs.johnnycode.ai:21117", and nothing connects to the attacker host (watch with `nc -lk
   21117` on that host: no connection).
3. AC-9 regression: from a normal controller with `force-always-relay=Y`, connect in both
   directions: the session opens over Relay.
