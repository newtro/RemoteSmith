# RemoteSmith two-machine acceptance checklist

Each unit appends its own section. Record results per release in `docs/test-runs/<date>-<version>.md`
(pass/fail per step, OS versions, keyboard mode, input source).

## Keyboard: DEC-22 key mapping and escape chord (unit B2, patches 0201–0202)

**Supported path:** keyboard mode **Map** (upstream's default). Translate and Legacy keyboard
modes are best effort: Translate mode on a Windows controller sends Ctrl+letter as text, so
shortcuts don't translate there. If a step fails, first confirm toolbar → Keyboard → Map mode.

**Setup.** Both machines on the release build. Start each session with the defaults: keyboard mode
**Map**, Settings → General → Input source **1** (the keyboard grab), toolbar → Keyboard →
**Translate Mac/Windows shortcuts** on. "Captured" means the pointer is over the remote image and
the image has been clicked. A Windows host with Notepad and a `cmd`/PowerShell window running
`ping -t 127.0.0.1`; a Mac host with TextEdit and Terminal running `ping 127.0.0.1`.
After each step that releases modifiers, check the host's held keys: on Windows, the On-Screen
Keyboard (`osk`) shows held modifiers highlighted; on macOS, enable Keyboard Viewer (menu bar
input menu → Show Keyboard Viewer). Neither should show a held modifier after a step finishes.

### B2-1 (AC-11) Mac → Windows copy/paste
1. In Notepad on the host, type `hello`, press Cmd+A, Cmd+C, End, Cmd+V. Expect `hellohello`.
2. Cmd+Z undoes; Cmd+S opens Save. Cancel it.

### B2-2 (AC-11) Windows → Mac copy/paste
1. In TextEdit on the host, type `hello`, press Ctrl+A, Ctrl+C, Right arrow, Ctrl+V. Expect `hellohello`.
2. Ctrl+Z undoes.

### B2-3 (AC-35) Mac → Windows terminal and app switching
1. In the host's `ping -t` window press **Ctrl+C**: ping stops (Ctrl stays Ctrl).
2. Press and hold Cmd, tap Tab twice, release Cmd: the Windows Alt+Tab switcher appears, moves
   two entries, and switches app. The Mac's own app switcher must **not** appear.
3. Cmd+Shift+Tab cycles backwards.
4. After releasing, OSK shows no held Alt or Ctrl; typing `a` in Notepad types `a`.
5. Toolbar → Control actions → **Send Win key**: the Windows Start menu opens. Press it again to close.

### B2-4 (AC-35) Windows → Mac terminal and app switching
1. In the host's Terminal running `ping`, press **Win+C**: ping stops (Win → Ctrl).
2. In TextEdit press Ctrl+C on selected text and paste with Ctrl+V: copy works (Ctrl → Cmd).
3. Hold Alt, tap Tab twice, release Alt: the macOS app switcher appears and switches app. The
   Windows Alt+Tab switcher must **not** appear on the PC.
4. Keyboard Viewer shows no held Cmd or Option afterwards.

### B2-5 (AC-12) Raw mode
1. Mac controller, Windows host: toolbar → Keyboard → turn **Translate Mac/Windows shortcuts** off.
2. Press Cmd+C in Notepad: nothing is copied; Cmd+R opens the Run dialog (Cmd reaches Windows as Win).
3. Turn translation back on; Cmd+C copies again.
4. Windows controller, Mac host, translation off: Ctrl+C in Terminal interrupts `ping`, Win+C copies.

### B2-6 (AC-13) Escape chord, Mac controller
For each of translated mode and raw mode:
1. Capture the session (click the image). Press Ctrl+Option+Cmd+Esc.
2. Expect: the local Mac takes the keyboard back immediately — Cmd+Tab now shows the **Mac** app
   switcher; the session window stays connected and keeps updating.
3. On the host: no Esc was received (in Notepad nothing happens; in a host-side key logger such as
   `https://keyboard-tester.com` no Escape appears), and OSK shows no held modifier.
4. Hold the chord and keep Esc held for 2 s (auto-repeat): still nothing reaches the host.
5. Move the pointer off and back onto the image, or click it: capture returns; Cmd+C copies again.
6. After the chord, press Esc alone while captured: the host receives a normal Esc.

### B2-7 (AC-13) Escape chord, Windows controller
For each of translated mode and raw mode:
1. Capture the session. Press Ctrl+Alt+Win+Esc.
2. Expect: Alt+Tab on the PC now shows the **Windows** switcher, Win opens the local Start menu;
   the session stays connected.
3. On the Mac host no Esc arrives and Keyboard Viewer shows no held modifier.
4. Click the remote image: capture returns.

### B2-8 (AC-59) No stuck modifiers on mode or capture change
Mac controller → Windows host, then repeat Windows controller → Mac host with the matching keys.
1. Hold Cmd (Windows controller: Ctrl). With it held, move the pointer to the toolbar and toggle
   **Translate Mac/Windows shortcuts** off, then on. Release Cmd. OSK shows nothing held.
2. Hold Cmd, tap Tab (switcher open, host Alt held), keep holding Cmd and toggle translation via
   the toolbar. Release. Nothing held; typing `a` types `a`.
3. Hold Shift+Option, move the pointer off the remote image (capture released), release the keys
   outside, move back in and click. Type `a`: lowercase `a`, no Alt-menu activation. Then press
   Cmd+C on selected text: it copies (the first Cmd press after recapture is not dropped).
4. Hold Ctrl+Option+Cmd, press Esc (chord), release everything. Recapture and press Cmd+C: copies.
5. Hold Cmd before clicking into the image (press outside, then click in while holding), release:
   no stray Ctrl/Win on the host.

### B2-9 Input source 2 (Flutter input) smoke test
Settings → General → Input source **2**, reconnect.
1. Mac controller: Cmd+C/Cmd+V copy and paste on Windows; the escape chord releases focus.
   (Known limit: macOS handles Cmd+Tab itself in this input source, so app switching stays local.)
2. Windows controller: Ctrl+C copies on the Mac. (Known limit, upstream: Flutter drops the Win key
   in this input source, so Win → Ctrl needs input source 1; the escape chord still works because
   the keyboard hook tracks the Win key.)

## Fork IPC endpoint, presence, sessions, readiness (unit B4, patches 0400, 0500, 0501)

**Setup.** Both machines on the release build with the service installed (Mac: app in
/Applications, LaunchDaemon + LaunchAgent; Windows: installed service and the `--tray` startup
shortcut). Each has connected to the other at least once, so each appears in the other's
"Recent" list. Run the sidecar on each: `remotesmith-tray`. For a text view of what the fork
serves, run `remotesmith-tray --print-menu --wait 20` in a terminal on that machine.
On Windows the pipe is `\\.\pipe\RemoteSmith-ipc-<user>`, created by the `--tray` process: check
it exists with PowerShell `[System.IO.Directory]::GetFiles("\\.\pipe\") | sls RemoteSmith`.

### B4-1 (AC-20) Reachable with route, one-click connect
1. Both machines on the same LAN. Within 20 s of starting the tray, the other machine shows
   "reachable · LAN" (the fork sends one discovery broadcast per interface every 15 s and listens
   at most 2.5 s; only saved peers count).
2. Click it: a normal RemoteSmith session window opens to that peer using the saved password
   (no password prompt), exactly as if started from the RemoteSmith window.
3. Click it again while that session is open: the existing session is focused, no second session.
4. Put the PC on another network (phone hotspot): within 20 s it shows "reachable · ID";
   Connect opens a session through the ID server.
5. Repeat 1-4 from the PC towards the Mac.

### B4-2 (AC-21) Unavailable with reason and last seen
1. Shut the PC down. Within 20 s the Mac's tray shows it "unavailable · offline" with a last-seen
   time close to the shutdown.
2. Click Connect: the session window opens and reports the peer offline; the tray menu shows
   the reason.
3. Block the Mac's access to rs.example.com (e.g. turn Wi-Fi to a network without internet,
   PC on another network): the PC shows "unavailable · server unreachable".

### B4-3 (AC-60) Host session badge and connection window
1. From the PC, connect to the Mac. On the Mac: RemoteSmith's connection window appears, and the
   Mac's tray shows the badge naming the PC (alias or ID), with no "agent".
2. **Quit the RemoteSmith tray on the Mac first**, then connect from the PC and keep the session
   open for at least 30 s: the connection window stays visible (small, top-right, on top) the
   whole time; it is not minimized after 3 s. Then restart the tray: the badge appears.
3. Disconnect from the PC. Badge and window are gone within 2 s.
4. Open two sessions (remote + file transfer) from the PC, close one: the badge stays; close
   the other: it clears within 2 s.
5. Repeat 1-4 with the Mac controlling the PC (Windows host: badge in the PC's tray tooltip and
   first menu rows; connection window shown).
6. In RemoteSmith settings on the host, try to enable "hide connection management window" if
   offered: the connection window must still appear on the next inbound session.

### B4-4 (AC-55) Readiness and "Unattended ready"
1. On the Mac set Energy so it sleeps on power (`sudo pmset -c sleep 10`): within 35 s the
   tray's readiness names "sleep enabled" and does not show "Unattended ready". Restore
   `sudo pmset -c sleep 0`.
2. Remove RemoteSmith from System Settings → Privacy → Accessibility: within 10 s readiness names
   Accessibility. Restore it. Repeat for Screen & System Audio Recording and Input Monitoring.
3. Turn FileVault on (or check one where it is on): readiness names the pre-boot unlock.
4. On the PC: set "Sleep after" on AC to 30 min: readiness names sleep. Restore "Never".
   With BitLocker using TPM+PIN, readiness names the pre-boot unlock; TPM-only passes.
5. Cold boot: with every check passing, **Restart** the host (on Windows not Shut down: with Fast
   Startup a shutdown resumes from hibernation and does not count as a boot) and, while it sits at the login screen
   (no one logged in), connect to it from the other machine within 15 minutes of power-on and
   authenticate. Log in through the session. The host's tray now shows "Unattended ready".
   The ledger is `/Library/Application Support/RemoteSmith/remotesmith-unattended.json`
   (Mac, owned by root) or registry value `HKLM\SOFTWARE\RemoteSmith\RemoteSmith\Unattended` (PC).
6. A connection made after logging in, or more than 15 minutes after boot, does not create
   the ledger (delete it first as admin to check).

### B4-5 (AC-25) Fork keeps working without the sidecar
1. Quit the sidecar on both machines (or `kill -9` it). Mac → PC and PC → Mac sessions work:
   screen, input, clipboard, file transfer, audio, both monitors.
2. Feed the endpoint garbage while a session runs: Mac
   `head -c 3000000 /dev/urandom | nc -U /tmp/RemoteSmith-$(id -u)/remotesmith-ipc.sock`;
   the session is unaffected and a fresh `remotesmith-tray --print-menu` still works.
3. Start an old/mismatched sidecar build (or `REMOTESMITH_IPC_PATH` pointing at a fake server
   speaking v2): the tray shows unknown, sessions are unaffected.

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
   With two port-forward mappings to the same unenrolled peer opened at once (two prompts),
   confirming one prompt pins only the identity that prompt showed.
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
7. Keyboard mode note: held-key release on disconnect is verified in Map mode (the supported
   mode, as in B2's section). In Legacy mode a character key's key-up on disconnect cleanup is not
   guaranteed (deferred); run steps 1-2 in Map mode.

### B3-7 (AC-39) Changing the host's password ends sessions; the old credential stops working
1. In an open session (repeat with a switch-sides session, toolbar > Switch sides), change the
   host's permanent password (host: Settings > Security > Change
   password). Within ~1 s the controller shows "Session closed by the peer: its password changed"
   and does not retry.
2. Connect again: the saved password is refused (wrong-password prompt); entering the new one
   works.
3. Open a port forward (or RDP) tunnel to the host, change the password: the tunnel closes within
   30 s.
4. Resume cache: drop the controller's network for ~5 s right after changing the password (so
   it reconnects within 30 s): the reconnect is refused, it does not resume the old session.
5. Forget the peer on the controller: its saved credential is removed (peer file deleted).

### B3-8 (AC-61) Host refuses a relay it did not choose
1. Prerequisite: a client with the right server host but no key (a stock RustDesk with only the
   ID server set, or `force-always-relay=Y` and a custom `relay-server=<attacker host>:21117` on a
   RemoteSmith controller).
2. From it, connect to the host's ID with relay forced and the attacker relay set. The host log
   shows "refused a relay request from ... naming relay "<attacker host>:21117"; this host only
   uses rs.example.com:21117", and nothing connects to the attacker host (watch with `nc -lk
   21117` on that host: no connection).
3. AC-9 regression: from a normal controller with `force-always-relay=Y`, connect in both
   directions: the session opens over Relay.

## B1: branding, no default servers, hardened defaults, first-run provisioning

Run on the Mac and the Windows PC, each on a clean profile (no earlier RemoteSmith config):
macOS `~/Library/Preferences/ai.johnnycode.RemoteSmith/`, Windows `%APPDATA%\RemoteSmith\config\` (and the service copy under
`C:\Windows\ServiceProfiles\LocalService\AppData\Roaming\RemoteSmith\config\`).

### B1-1 First run and provisioning (AC-40, FLOW-2)
1. Launch RemoteSmith. The setup dialog appears before anything else; nothing is prefilled.
2. Try Save with an empty host, a host with `https://`, and a 43-character key: each is
   refused with a reason. A password under 12 characters is refused.
3. Choose "Use my ID server", enter the server host and the 44-character key from the
   server's `id_ed25519.pub` (or copy the config from another machine's Settings > Network and
   use "Paste exported config"). Click "Generate a strong password", copy it into the password
   manager, Save.
4. Settings > Network shows the host and key; the home page shows an ID and "Ready".
5. Quit and relaunch: the dialog does not come back.
6. Windows only, headless: on a clean install, as administrator,
   `remotesmith.exe --option custom-rendezvous-server <host>`, `--option key <key>`,
   `--password <12+ chars>`. Launch the UI: no setup dialog. `--option key short` prints an error
   and changes nothing. A server without a key (`--option custom-rendezvous-server` only) still
   shows the setup dialog. `--config "host=<host>,.exe"` (no key) is refused.

### B1-2 Direct LAN only (AC-40, AC-8)
1. On one machine, clean profile, choose "Direct LAN only" and set a password.
2. From the other machine, connect by the first machine's LAN IP: the session opens.
3. With `nettop -m tcp -p RemoteSmith` (Mac) or Resource Monitor > Network (Windows) open,
   confirm no connection to any rendezvous server or `*.rustdesk.com`.

4. Switch a provisioned machine to LAN only: run setup again (or, headless,
   `--option remotesmith-lan-only Y`, which also clears the stored ID/relay/API servers), restart
   RemoteSmith, and confirm with `nettop -P -p <pid>` that it no longer contacts the ID server
   while LAN connections by IP still work. Setting a server again (`--option
   custom-rendezvous-server <host>` or Settings > Network) turns LAN only off.

### B1-2a Controller only (FLOW-2)
1. On a clean profile, choose a route, tick "Controller only" and Save without a password.
   The dialog closes; the home page shows the service as stopped.
2. From the other machine, try to connect to it by ID and by LAN IP: both fail (no
   registration, nothing listening on 21118: `lsof -nP -iTCP:21118` is empty).
3. From this machine, connect to the other one: it works.
4. Settings > General > "Start service": the setup dialog comes back and requires a
   12+ character permanent password; after saving, the other machine can connect.
5. Headless equivalent: `--option stop-service Y` (controller only) / `--option stop-service ""`.

### B1-3 Never contacts RustDesk's servers (AC-6)
1. With both machines provisioned against the RemoteSmith server, run a session each way, a
   file transfer, and leave both idle for 10 minutes.
2. Over the whole run, the firewall/DNS log (or `sudo tcpdump -n port 53` plus `lsof -i -P`
   snapshots on the Mac) shows no lookup of or connection to `rustdesk.com`, `rs-ny.rustdesk.com`,
   `api.rustdesk.com` or `admin.rustdesk.com`.

### B1-4 Password-only access (AC-10, DEC-10)
1. Settings > Security: "Use permanent password" is selected and the three options are
   greyed out; the home page shows no one-time password.
2. From the other machine, connect with a wrong password: refused. Repeat quickly 5+ times:
   RustDesk's backoff ("Too many wrong password attempts") applies.
3. Connect with the permanent password: the session opens without a click on the host
   (approve mode "password").
4. Settings show "Enable direct IP access" on and "Deny LAN discovery" off by default; both can
   be changed and the change survives a restart.

### B1-5 Branding (AC-33, DEC-15)
1. Mac: the app is `/Applications/RemoteSmith.app`, bundle id `ai.johnnycode.remotesmith`;
   after "Install" the launchd jobs are `ai.johnnycode.RemoteSmith_service` and
   `ai.johnnycode.RemoteSmith_server` (`sudo launchctl print system/ai.johnnycode.RemoteSmith_service`).
2. Windows: the installer, Start menu entry, service and Apps & features entry say RemoteSmith;
   Apps & features links go to github.com/newtro/RemoteSmith; the MSI license page shows the
   AGPL notice that credits RustDesk.
3. Settings > About shows the AGPL notice, "Based on RustDesk" and the source link.

## C2: transfer data safety, config import, no-display state (unit C2, patches 0801, 0803, 0998, hbb_common 0802)

Fixtures: a folder `xfer/` with `a.bin` (1 MB), `big.bin` (20 MB, e.g. `mkfile -n 20m` /
`fsutil file createnew big.bin 20971520`) and `z.txt` (a few bytes), in that order. A transfer
"fails" in the UI when its row shows the error state with the file name and reason; it must
never show done. The staging file of `<name>` is `<name>.download` (+ `<name>.digest`) next to
the destination.

### C2-1 (AC-18, AC-48) Download onto a full disk
1. Mac controller: `hdiutil create -size 10m -fs HFS+ -volname small /tmp/small.dmg && hdiutil attach /tmp/small.dmg`.
2. In the file manager, download `xfer/` from the Windows host into `/Volumes/small`.
3. Expect: the job fails with `big.bin: ... No space left on device` (or `incomplete file`);
   `a.bin` is in place and byte-identical (`shasum` both sides); there is no `big.bin`,
   `big.bin.download` or `big.bin.digest`, and no `z.txt`. The host's transfer stops (no more
   traffic in its connection window).
4. Free space (`hdiutil detach /Volumes/small`, recreate at 40m, attach) and download `xfer/`
   again, choosing Skip for the existing `a.bin`: the rest arrives byte-identical and the job
   shows done. (There is no Restart button on a failed job in v1; re-sending with Skip is the
   restart. Deferred.)

### C2-2 (AC-18, AC-48) Upload onto a full disk (host side)
1. On the Windows host, create a small volume: Disk Management > Action > Create VHD, 10 MB,
   initialise and format NTFS as `S:`.
2. From the Mac, upload `xfer/` to `S:\`.
3. Expect the same outcome as C2-1 on `S:\`: failed job naming `big.bin`, `a.bin` intact, no
   `big.bin*` left, and the connection window's transfer log shows the job as failed, not done.

### C2-3 (AC-48) Disconnect mid-transfer, then Resume
1. Download `xfer/` (use a larger `big.bin`, e.g. 500 MB, so it takes several seconds).
2. While `big.bin` is copying, pull the host's network cable (or disable Wi-Fi) for longer than
   the reconnect window, then restore it.
3. Expect: `a.bin` is complete and byte-identical; `big.bin` exists only as
   `big.bin.download`/`.digest`; no `big.bin`.
4. Reconnect and open the file manager: the job is listed paused with Resume. Resume: `big.bin`
   completes byte-identical (`shasum`), the staging files are gone, `z.txt` arrives, job done.

### C2-4 (AC-48) App crash mid-transfer, then a new transfer of the same folder
1. Start downloading `xfer/` with the 500 MB `big.bin`; while it copies, kill the client
   (`pkill -9 RemoteSmith` / Task Manager > End task).
2. Expect: `a.bin` intact; `big.bin.download` and `big.bin.digest` left behind; no `big.bin`.
3. Relaunch and start a **new** download of `xfer/` to the same folder (do not Resume), Skip
   for `a.bin`: once the transfer reaches `big.bin`, its stale staging files are replaced; at
   the end `big.bin` and `z.txt` are byte-identical and no `*.download`/`*.digest` remains.
4. Ambiguous names are preserved: in an empty destination create `notes.txt.download` (any
   content, no `.digest`) and download a folder containing `notes.txt`: the job fails with
   "... already exists and is not transfer staging data; not overwritten", and
   `notes.txt.download` is unchanged.

### C2-5 (AC-52) Last display unplugged while viewing All displays
1. Windows host with one monitor (or a laptop with the lid open and no external display). From
   the Mac, connect and choose All displays in the toolbar's monitor menu.
2. Hold the left mouse button down over the remote image (start a drag), then unplug the
   host's monitor / switch it off so Windows drops it (on a laptop: Win+P > "Second screen only"
   with nothing attached).
3. Expect: the client shows "No display available" (with only a Close button); the drag is
   released on the host (no stuck button once a display is back) and the Mac cursor is free.
4. Close the message and plug the monitor back in: the session shows the display again
   without reconnecting.
5. Repeat with a single display selected instead of All: same message and behaviour.

### C2-6 (finding 10) `--import-config` rejects a malformed options file
1. Mac: copy `RemoteSmith.toml` and `RemoteSmith2.toml` from
   `~/Library/Preferences/ai.johnnycode.RemoteSmith/` to `/tmp/imp/`.
2. Overwrite `/tmp/imp/RemoteSmith2.toml` with `[options` (a syntax error) and touch it newer.
3. Run `/Applications/RemoteSmith.app/Contents/MacOS/RemoteSmith --import-config /tmp/imp/RemoteSmith.toml`:
   it prints `Invalid config file: ...` and both live config files are unchanged (`shasum` before/after);
   the ID server setting still shows the old value.

## B5: signed release, bundled tray, install and uninstall (unit B5, patches 0601-0603)

Build with `scripts/release-mac.sh` (Mac) and take the Windows MSI / portable zip from CI
(`remotesmith-unsigned-windows-x86_64-installers`, `remotesmith-unsigned-windows-x86_64`).
Already verified by the release script on the build machine: Developer ID signature with the
hardened runtime on every executable, `spctl` "Notarized Developer ID", stapled ticket, and the
bundled tray runs (`build/release/<version>/verify.txt`).

### B5-1 (FLOW-1, AC-1) Mac install from the dmg
1. Download `RemoteSmith-<version>-arm64.dmg` to the Mac (any way that sets quarantine, e.g. a
   browser), open it, drag RemoteSmith to Applications, eject.
2. Open RemoteSmith from Applications. Expect Gatekeeper's "downloaded from the Internet" prompt
   naming RemoteSmith (not "cannot be verified" / "damaged"). Click Open.
3. Expect the RemoteSmith menu-bar tray icon within a few seconds, and RemoteSmith Tray listed in
   System Settings > General > Login Items & Extensions > Open at Login (on).
4. Do the first-run provisioning (B1-1). Then RemoteSmith's own prompts: install the service
   (administrator password), then grant Accessibility, Screen Recording, Input Monitoring and
   Screen & System Audio Recording when asked (System Settings > Privacy & Security). Quit and
   reopen RemoteSmith if it asks.
5. In Terminal:
   `"/Applications/RemoteSmith.app/Contents/Library/LoginItems/RemoteSmith Tray.app/Contents/MacOS/remotesmith-tray" --print-menu`.
   Expect "Service running" and the four permissions as ✓.
6. Reboot. Without opening RemoteSmith, expect the tray icon after login and the service running
   (tray Readiness). The other machine shows this Mac as a saved peer once both are set up (AC-1).

### B5-2 (AC-3) Upgrade keeps permissions
1. Build a newer release (bump the build or just rebuild) and install it over the old one from its
   dmg (replace in Applications). Open it.
2. Expect no permission prompt; tray Readiness still shows all four permissions ✓; the tray process
   is the new one (`pgrep -fl remotesmith-tray`, start time after the install).

### B5-3 (AC-39) Mac uninstall removes the tray autostart
1. RemoteSmith > Settings > General: uninstall the service (administrator password).
2. Expect the tray icon to disappear, RemoteSmith Tray gone from Login Items, and
   `~/Library/LaunchAgents/ai.johnnycode.remotesmith.tray.plist` absent. Reopen RemoteSmith:
   the tray does not come back. Install the service again and reopen: it does.
3. To remove the product: uninstall the service as above, then move RemoteSmith.app to the Trash.

### B5-4 (FLOW-1, AC-1, AC-39) Windows MSI install, autostart and uninstall
1. Run `remotesmith-<version>-x86_64.msi` (double-click; not `/qn`). Finish the installer.
2. Expect the RemoteSmith tray icon in the notification area right after the installer closes, and
   `reg query HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run /v RemoteSmithTray` showing
   `"C:\Program Files\RemoteSmith\remotesmith-tray.exe"`.
3. `"C:\Program Files\RemoteSmith\remotesmith-tray.exe" --install-autostart` refuses (installer
   already registered it). `--print-menu` shows real status.
4. Sign out and in (or reboot): the tray starts by itself.
5. Uninstall from Settings > Apps. Expect no reboot prompt, no `remotesmith-tray.exe` process, and
   `reg query ...\Run /v RemoteSmithTray` failing in both HKLM and HKCU.

### B5-5 (AC-39) Windows portable + RemoteSmith's own installer
1. Unzip the portable folder, run `RemoteSmith.exe`, and install from its UI.
2. Expect the HKLM `Run` value `RemoteSmithTray` pointing at the install folder's
   `remotesmith-tray.exe`; the tray starts at the next sign-in.
3. Uninstall (Settings > Apps, or RemoteSmith's uninstall). Expect the tray process gone and both
   `Run` values gone.

## C1: network boundary and identity gaps (patches 0701-0705)

**Setup.** As for B3 (release build on both machines, service installed, `direct-server=Y`, a
permanent password). Mac logs `~/Library/Logs/RemoteSmith/` (the `--server` log for the host
side), Windows `%APPDATA%\RemoteSmith\log\`. A third device on the same LAN with `nc` (any Mac
or Linux box) and a VPN the host can join (WireGuard, Tailscale or the office VPN) help.

### C1-1 (AC-51) Direct port 21118 refuses VPN and outside-subnet sources
1. LAN positive: from the other machine, connect by the host's LAN IP. The session opens; the
   host log shows `direct access from <ip>:<port> on LAN interface en0` (Windows: the adapter
   GUID).
2. Loopback: on the host, `nc -vz 127.0.0.1 21118` connects (TCP accept), but the host log shows
   `direct access from 127.0.0.1:... refused: interface lo0 is not a LAN interface` and no
   handshake follows (`nc 127.0.0.1 21118 | xxd` prints nothing and the connection closes).
3. VPN source (a routed VPN, no NAT onto the LAN: plain WireGuard/Tailscale peer-to-peer, not a
   Tailscale subnet router or a VPN server on the LAN that masquerades; those make remote users
   look like a LAN host, which no layer-3 check can tell apart). Join the host and a second
   device to the VPN. From the second device, `nc -vz <host VPN address> 21118` and `nc -vz <host LAN address> 21118` over the
   VPN. Both are refused in the host log (`interface utun.. is not a LAN interface`, or
   `source is outside the local subnet of en0`); a RemoteSmith controller connecting that way
   gets no session (it reports the peer as gone / timed out, never a password prompt).
3a. VPN with an overlapping subnet (receiving-interface check): give a VPN client an address
   inside the host's LAN subnet (e.g. WireGuard peer `192.168.1.231/32` with the host's
   `AllowedIPs` covering it, so the host routes replies to it over the tunnel), and from that
   client `nc -vz <host LAN IP> 21118` through the tunnel. Mac host: the connection never
   completes (the direct listeners are pinned to en0; `lsof -nP -iTCP:21118 -sTCP:LISTEN` shows
   one listener per en0 address, none on `*`). Windows host: refused (strong host model and
   the LAN/wireless-only firewall rule), and the log shows nothing or a refusal.
   Known limit (deferred, not a failure of this step): a VPN built with asymmetric routing or
   proxy ARP for the LAN prefix can still complete a handshake on macOS (IP_BOUND_IF pins the
   reply path, it does not filter ingress), and on Windows a VPN client that enables weak host
   mode on its adapter (e.g. GlobalProtect) defeats the destination-owner check. Both need an
   attacker on such a VPN; the identity check and the password still stand. Note the VPN
   product if step 3a connects.
3b. Windows with Hyper-V/WSL: on the PC, `Get-NetAdapter | ft Name,InterfaceDescription,MacAddress,HardwareInterface`.
   Connecting from the LAN works when the PC reaches the LAN through a Hyper-V *external*
   switch (vEthernet carrying the NIC's MAC; the NIC itself has no IP and is matched from the
   full interface table); a connection to the PC's `vEthernet (WSL)` or
   Default Switch address from a WSL/VM guest is refused (`interface ... is not a LAN interface`).
4. Routed, non-local subnet: from a device on another VLAN/subnet that can route to the host's
   LAN address (or a phone hotspot NATed through a router port-forward, if available),
   `nc -vz <host LAN IP> 21118`: refused, host log `source is outside the local subnet`.
5. Windows firewall (PC as host): `netsh advfirewall firewall show rule name="RemoteSmith Service" verbose`
   lists the Out rule (any), In UDP (any port), In TCP local ports `1-21117,21119-65535`, and
   In TCP local port `21118` with RemoteIP `LocalSubnet`, InterfaceTypes `LAN` / `Wireless`
   (netsh: two rules, one per type; MSI: one rule with both) and edge traversal off. With the VPN of
   step 3 connected on the PC, the attempt to 21118 from a VPN peer is dropped by the firewall
   (no log line on the host at all) or, if the VPN subnet counts as local, refused by the app
   (log line as in step 3). Repeat for the MSI install. Uninstall: `show rule` finds none.
6. Scan flood: `for i in $(seq 200); do nc -z 127.0.0.1 21118; done` on the host: at most one
   refusal line per second in the log, with "(N more refused in the last second)", and the
   service stays responsive (a LAN connect right after still opens).
7. Moved by DHCP (C1-3 covers the tray side): not applicable here.

### C1-2 (AC-43, FLOW-2) LAN-only host shows its fingerprint before any registration
1. On a host set up with "Direct LAN only" (B1-2) that never had an ID server (fresh profile:
   move its config aside first), open Settings > About: the Fingerprint is shown (not blank),
   and the tray's status shows the same value.
2. From the controller, connect by the host's LAN IP: the "Confirm peer identity" dialog shows
   exactly that fingerprint. Confirm; the session opens with the password.
3. Repeat with the roles swapped (both machines LAN-only, fresh profiles): each machine's
   fingerprint differs from the other's, and each controller dialog matches the host it dialled.

### C1-3 (AC-20, AC-21) A peer enrolled by IP shows as reachable on the LAN
1. With no ID server reachable (LAN-only, or the server stopped), enroll the other machine by its
   LAN IP (B3-1 step 7). In the tray, that peer (listed under its IP) shows
   `reachable (LAN)` within one probe round (about 15 s), and one click opens a session.
2. Quit RemoteSmith on that peer: within about 15 s the tray shows it `unavailable: offline`
   with "not answering on the LAN (saved by address)" and a last-seen time, not "server
   unreachable".
2a. Multihomed host (Ethernet and Wi-Fi on the same LAN), enrolled via its Ethernet IP: the
   tray shows `reachable (LAN)`, whichever interface answers discovery first.
3. Start it again and give the peer a different LAN IP (DHCP reservation change, or a manual
   address): the tray shows `unavailable: offline` with "answers on the LAN at <new IP>, not at
   the saved address" (never `reachable`, since a click would dial the old IP).

### C1-4 (AC-39, AC-43) Forget peer ends its sessions; the next connection enrolls again
1. Enroll the peer (B3-1) with a remembered password; open a session to it.
2. Disconnect the network on the host (unplug or turn Wi-Fi off) so the controller's session
   enters "Reconnecting...". While it retries, open the controller's main window and remove
   the peer (peer card > Remove).
3. The session window shows "Peer forgotten" ("This peer was forgotten, so this session
   ended...") and stops retrying; restore the host's network: the session does **not**
   reconnect, and the host log shows no new login.
4. The peer file is gone and stays gone (Mac: `~/Library/Preferences/ai.johnnycode.RemoteSmith/peers/<id>.toml`),
   even after waiting a minute: no password was re-saved.
5. Connect to the peer again: the first-use dialog appears (B3-1) and the password is asked for.

### C1-5 (AC-45) "Server unreachable" within 10 s
1. Controller off the host's LAN (phone hotspot). Make the ID server unreachable: stop the
   Lightsail instance, or on the controller blackhole its address
   (Mac: `sudo route add -host <server IP> 127.0.0.2 -blackhole`; remove afterwards with
   `sudo route delete -host <server IP>`).
2. Connect to the peer by ID and start a stopwatch: within 10 s the session shows
   "Server unreachable: the ID server did not answer within 10 s ..." (controller log:
   "ID server not reached within 10s; giving up"). No automatic retry starts.
3. Stop only hbbs/hbbr on the VM (the PLAN §14 recipe: the VM answers, the port refuses), or
   point the controller's ID server at an unresolvable name: the session shows
   "Server unreachable: cannot connect to the ID server (<OS error>) ..." at once.
4. With the server back, connect: the session opens normally (relay included; a relayed session
   taking longer than 10 s to set up after the server answered is not cut).
