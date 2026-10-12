# RemoteSmith two-machine acceptance checklist

Each unit appends its own section. Record results per release in `docs/test-runs/<date>-<version>.md`
(pass/fail per step, OS versions, keyboard mode, input source).

## Keyboard: DEC-22 key mapping and escape chord (unit B2, patches 0201–0202)

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
