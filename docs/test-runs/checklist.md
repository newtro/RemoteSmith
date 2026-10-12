# RemoteSmith two-machine AC checklist

Each unit appends its own section. Record results per release in `docs/test-runs/<date>.md`.

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
