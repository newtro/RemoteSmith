# RemoteSmith two-machine acceptance checklist

Each unit appends its own section. Record results per release in `docs/test-runs/<date>-<version>.md`
(pass/fail per step, OS versions, keyboard mode, input source).

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
   "reachable · LAN".
2. Click it: a normal RemoteSmith session window opens to that peer using the saved password
   (no password prompt), exactly as if started from the RemoteSmith window.
3. Click it again while that session is open: the existing session is focused, no second session.
4. Put the PC on another network (phone hotspot): within 20 s it shows "reachable · ID";
   Connect opens a session through the ID server.
5. Repeat 1-4 from the PC towards the Mac.

### B4-2 (AC-21) Unavailable with reason and last seen
1. Shut the PC down. Within 20 s the Mac's tray shows it "unavailable · offline" with a last-seen
   time close to the shutdown.
2. Click Connect: the tray reports "offline" at once (no session window spins).
3. Block the Mac's access to rs.johnnycode.ai (e.g. turn Wi-Fi to a network without internet,
   PC on another network): the PC shows "unavailable · server unreachable".

### B4-3 (AC-60) Host session badge and connection window
1. From the PC, connect to the Mac. On the Mac: RemoteSmith's connection window appears, and the
   Mac's tray shows the badge naming the PC (alias or ID), with no "agent".
2. Disconnect from the PC. Badge and window are gone within 2 s.
3. Open two sessions (remote + file transfer) from the PC, close one: the badge stays; close
   the other: it clears within 2 s.
4. Repeat 1-3 with the Mac controlling the PC (Windows host: badge in the PC's tray tooltip and
   first menu rows; connection window shown).
5. In RemoteSmith settings on the host, try to enable "hide connection management window" if
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
5. Cold boot: with every check passing, restart the host and, while it sits at the login screen
   (no one logged in), connect to it from the other machine within 15 minutes of power-on and
   authenticate. Log in through the session. The host's tray now shows "Unattended ready".
   The ledger is `/Library/Application Support/RemoteSmith/remotesmith-unattended.json`
   (Mac, owned by root) or `%ProgramData%\RemoteSmith\remotesmith-unattended.json` (PC).
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
