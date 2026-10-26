# A tour of appd

This walks through deploying a small real-world service, operating it day to day, adding a second
one, and then the failures you'll actually hit. The app is hypothetical; its requirements are not.

Everything below assumes a Linux machine with systemd and sudo. Commands that write to
`/etc/systemd/system` or manage accounts are prefixed with `sudo`; the read-only ones are not,
because that distinction is part of the design.

---

## 0. Install

```bash
cargo build --release
sudo install -Dm755 target/release/appd /usr/local/bin/appd

appd --version
systemctl is-system-running        # expect "running", or "degraded" with a reason
```

`degraded` is fine — it means some *other* unit failed. appd only ever touches its own.

### Where things live

| What | Path |
| --- | --- |
| Configs | `/etc/appd/<app>.json`, one per app |
| Environment files | wherever you point `environment_file` (conventionally `/etc/appd/<app>.env`) |
| Generated units | `/etc/systemd/system/appd-<app>.service` |
| Service accounts | `appd-<app>`, provisioned by `apply` |
| App state | `/var/lib/appd-<app>`, created by systemd |
| Logs | journald, read through `appd logs` |

The app name is the config file's stem. `sundial-api.json` becomes `appd-sundial-api.service`,
so nothing you install this way can collide with a package's unit.

---

## 1. The app we're deploying

Sundial is a notes API. It listens on loopback, renders pages from templates, and stores
uploads. It has a queue worker that runs alongside it.

```
/srv/sundial/
├── bin/sundial-api        an HTTP server
├── worker.js              a queue worker (Node)
├── templates/             read-only
└── public/                rendered pages, served by nginx
```

What it needs:

| Requirement | Declared as |
| --- | --- |
| Accept connections on `127.0.0.1:8787` | `permissions.network` |
| Read `/srv/sundial` | nothing — reading is allowed by default |
| Write `/srv/sundial/public` | `permissions.write` |
| Scratch space for uploads | the automatic state directory |
| Database URL and session secret | `environment_file` |
| Re-read templates on SIGHUP | `reload` |

Copy the binaries and templates into place:

```bash
sudo install -d -m 0755 /srv/sundial/bin /srv/sundial/templates /srv/sundial/public
sudo install -m 0755 sundial-api /srv/sundial/bin/sundial-api
sudo install -m 0644 sundial-worker.js /srv/sundial/worker.js
```

Read-only is the default, so none of this needs to be world-writable. `/srv/sundial/public` gets
its ownership fixed after `apply` creates the account.

---

## 2. The first config

```bash
sudo appd init
sudo tee /etc/appd/sundial-api.json >/dev/null <<'EOF'
{
  "description": "Sundial notes API",
  "exec": "/srv/sundial/bin/sundial-api",
  "args": ["--bind", "127.0.0.1:8787"],
  "working_directory": "/srv/sundial",
  "restart": "on-failure",
  "reload": ["/bin/kill", "-HUP", "$MAINPID"],
  "environment_file": "/etc/appd/sundial.env",
  "permissions": {
    "network": true,
    "write": ["/srv/sundial/public"]
  }
}
EOF
sudo chmod 600 /etc/appd/sundial-api.json
```

Notice what is *not* there: no `user`, and no `read` paths.

- **No `user`** — `apply` provisions `appd-sundial-api`, a system account with no shell and no
  home, and runs the service as it. Writing `"user": "root"` is possible, but it has to be written
  down; it is never the default.
- **No `read` paths** — everything is readable under the hardening defaults. `permissions.read`
  emits `ReadOnlyPaths=`, which is useful for pinning a subdirectory *inside* something you made
  writable, not for granting read access.

### Check before applying

```bash
sudo appd doctor
```

```
sundial-api
  ✗ environment file /etc/appd/sundial.env does not exist; the unit will fail to start
  ✗ service account `appd-sundial-api` does not exist
      fix: appd apply sundial-api
  ⚠ appd-sundial-api.service is not installed
      fix: appd apply sundial-api

1 app checked, 2 problems, 1 warning, 0 orphans
```

Three useful things just happened: it found the missing secrets file before systemd did, it
predicted the account doesn't exist yet (expected — `apply` creates it), and it exited non-zero,
so this works as a CI gate.

Create the environment file:

```bash
sudo tee /etc/appd/sundial.env >/dev/null <<'EOF'
DATABASE_URL=postgres://sundial@127.0.0.1/sundial
SESSION_SECRET=replace-with-something-long
EOF
sudo chmod 600 /etc/appd/sundial.env
```

This file is read by systemd (as root) and passed to the service. The unit refers to the path, not
the values, so `systemctl cat` never leaks secrets and the file itself needs no read permission for
the app's account.

---

## 3. Apply

```bash
sudo appd apply sundial-api
```

```
Applied 1 config, 1 changed.

APP          UNIT                         RESULT
sundial-api  appd-sundial-api.service   created

Created system accounts: appd-sundial-api

Unit files synchronized, systemd reloaded.
Running services are still using the previous units. Restart or reload them to pick up the changes:

  appd restart sundial-api
```

That last paragraph is the part worth internalizing: **`apply` synchronizes unit files, it does not
restart anything, and it does not enable or start anything.** Three separate decisions.

Look at what it wrote:

```bash
sudo systemctl cat appd-sundial-api.service
```

```ini
[Unit]
Description=Sundial notes API
X-Appd-Managed=1
After=network-online.target

[Service]
Type=simple
User=appd-sundial-api
Group=appd-sundial-api
WorkingDirectory=/srv/sundial
StateDirectory=appd-sundial-api
ExecStart=/srv/sundial/bin/sundial-api --bind 127.0.0.1:8787
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
EnvironmentFile=/etc/appd/sundial.env
StandardOutput=journal
StandardError=journal

# hardening defaults
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ProtectProc=invisible
ProtectClock=yes
ProtectHostname=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
SystemCallArchitectures=native

# opened by permissions
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
ReadWritePaths=/srv/sundial/public

[Install]
WantedBy=multi-user.target
```

`ProtectSystem=strict` makes the entire filesystem read-only, except `/srv/sundial/public` (which
you opened) and `/var/lib/appd-sundial-api` (which `StateDirectory=` creates and owns). The
service cannot write anywhere else, cannot see other processes, has no home directory, and cannot
gain privileges.

The `X-Appd-Managed=1` marker is how `apply` and `remove` know the file is theirs. It matters in
step 11.

Now give the app ownership of the directory it writes:

```bash
sudo chown appd-sundial-api:appd-sundial-api /srv/sundial/public
```

`permissions.write` opened the path in the unit's mount namespace; it did not change ownership.
Both have to be true for a write to succeed.

---

## 4. Enable, start, status

```bash
sudo appd enable sundial-api
sudo appd start sundial-api
```

```
$ appd status all
APP          UNIT                         STATE    SINCE             PID    NOTES
sundial-api  appd-sundial-api.service   running  2026-10-26 14:02  41231
```

`enable` means "start at boot", which is deliberately separate from `apply` and from `start`. A
config can be applied and running for weeks before you decide it should survive a reboot.

The detail view adds the things you'd otherwise look up:

```bash
appd status sundial-api
```

```
APP        sundial-api
UNIT       appd-sundial-api.service
CONFIG     /etc/appd/sundial-api.json
USER       appd-sundial-api
STATE DIR  /var/lib/appd-sundial-api

● appd-sundial-api.service - Sundial notes API
     Loaded: loaded (/etc/systemd/system/appd-sundial-api.service; enabled; preset: enabled)
     Active: active (running) since Mon 2026-10-26 14:02:11 UTC; 3min ago
   Main PID: 41231 (sundial-api)
```

States you'll see: `running`, `stopped`, `failed`, `starting`, `stopping`, `exited`, `?` (couldn't
read it), and `not-installed` (config exists, unit doesn't — run `apply`).

---

## 5. Logs

```bash
appd logs sundial-api -n 3
```

```
2026-10-26T14:02:11+00:00 host sundial-api[41231]: listening on 127.0.0.1:8787
2026-10-26T14:02:11+00:00 host sundial-api[41231]: connected to database
2026-10-26T14:02:12+00:00 host sundial-api[41231]: rendered index page in 12ms
```

| Command | What it does |
| --- | --- |
| `appd logs sundial-api` | last 50 lines |
| `appd logs sundial-api -f` | follow |
| `appd logs sundial-api -n 500` | more history |
| `appd logs sundial-api --since 1h` | time window |
| `appd logs sundial-api -p warning` | filter by priority |
| `appd error-logs sundial-api` | `logs -p err` |

The journal is the only backend; appd just hides `journalctl`'s flag vocabulary. Pipe to `grep`
like any other command.

### Why `error-logs` can be empty

journald records both stdout and stderr at priority 6 (`info`) for services. Writing to stderr does
not make a line an error as far as the journal is concerned. If `error-logs` comes back empty,
appd says so:

```
no journal entries for appd-sundial-api.service at priority `err`
journald records plain stdout and stderr at priority 6 (`info`), so an app has to
tag a line with `<3>` to have it logged as `err`. Try `appd logs sundial-api`.
```

To get real error filtering, the app tags its own lines with a syslog priority. `<3>` is `err`,
`<4>` is `warning`; journald strips the prefix and stores the priority.

```js
// Node
process.stderr.write("<3>database connection lost\n");
```

```rust
// Rust, via the syslog wire format
eprintln!("<3>database connection lost");
```

Anything that logs through `syslog(3)` or `sd_journal_print(LOG_ERR, …)` does this automatically,
which is why compiled services and Python's `SysLogHandler` tend to "just work". This is a journald
convention, not an appd one — see the README for why v1 sticks with it rather than separate log
files.

### The TUI

`appd` with no arguments opens the same information live. The table refreshes every couple of
seconds; `enter` (or `l`) opens a pane that follows the selected app's journal, and moving the
selection while it's open switches streams.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | move between apps |
| `g` / `G` | first / last |
| `enter` / `l` | toggle the log pane |
| `r` | restart, asks first |
| `s` | stop, asks first |
| `t` | start |
| `e` | enable at boot |
| `x` | disable at boot, asks first |
| `/` | filter apps by name |
| `?` | key help |
| `q` | quit |

Read-only sessions are explicitly marked in the header. Pressing an action key as a normal user
prints the command you'd need instead of firing a D-Bus call that would fail:

```
read-only: run `sudo appd restart sundial-api` from a shell
```

---

## 6. Changing a config

Sundial needs more workers. Edit `args`:

```json
"args": ["--bind", "127.0.0.1:8787", "--workers", "4"]
```

Ask what that would change:

```bash
sudo appd diff sundial-api
```

```
appd-sundial-api.service
--- installed: /etc/systemd/system/appd-sundial-api.service
+++ generated: /etc/appd/sundial-api.json
@@ -9,7 +9,7 @@
 Group=appd-sundial-api
 WorkingDirectory=/srv/sundial
 StateDirectory=appd-sundial-api
-ExecStart=/srv/sundial/bin/sundial-api --bind 127.0.0.1:8787
+ExecStart=/srv/sundial/bin/sundial-api --bind 127.0.0.1:8787 --workers 4
 ExecReload=/bin/kill -HUP $MAINPID
 Restart=on-failure
 EnvironmentFile=/etc/appd/sundial.env
```

`diff` exits 1 when anything drifted and 0 when everything is in sync, so it drops straight into a
deploy check. Then:

```bash
sudo appd apply sundial-api       # writes the unit, reloads systemd
sudo appd restart sundial-api     # actually applies it to the running process
appd status sundial-api           # PID should have changed
```

If you only changed something the process reads at runtime — a template, not an argument — and the
config defines `reload`, you can avoid a restart:

```bash
sudo appd reload sundial-api      # runs ExecReload, i.e. kill -HUP
```

Without a `reload` entry in the config, `appd reload` refuses and tells you to restart, rather
than guessing at a signal that might kill the process.

---

## 7. A second app

```bash
sudo tee /etc/appd/sundial-worker.json >/dev/null <<'EOF'
{
  "description": "Sundial queue worker",
  "exec": "/usr/bin/node",
  "args": ["/srv/sundial/worker.js"],
  "working_directory": "/srv/sundial",
  "restart": "always",
  "environment_file": "/etc/appd/sundial.env",
  "permissions": {
    "network": true
  }
}
EOF
sudo chmod 600 /etc/appd/sundial-worker.json
```

Now operations can address both:

```bash
sudo appd apply all
```

```
Apply all configs? [y/N] y
```

`apply` prompts whenever the target is every app and there's more than one. Pass `--yes` in
scripts; in a non-interactive context without it, appd refuses rather than assuming consent.
The same gate applies to `restart all`, `stop all`, and `disable all`.

```
$ appd status all
APP             UNIT                            STATE    SINCE             PID    NOTES
sundial-api     appd-sundial-api.service      running  2026-10-26 14:02  41231
sundial-worker  appd-sundial-worker.service   running  2026-10-26 14:09  41977
```

`all` means *all apps appd manages*. It never touches a unit from a package, even if its name
looks similar.

---

## 8. Secrets

There are two ways to pass environment variables, and they have different threat models:

```json
"environment": { "RUST_LOG": "info" }
```

Inline values live in the config. If a config holds secrets, `doctor` warns when its mode is
group- or world-readable:

```
  ⚠ /etc/appd/sundial-api.json is readable by group/others and holds environment variables
      fix: chmod 600 /etc/appd/sundial-api.json
```

```json
"environment_file": "/etc/appd/sundial.env"
```

The file is read by systemd as root, so it can stay `0600`. `systemctl cat` shows the path;
`systemctl show -p EnvironmentFiles` shows the path; the values never enter the unit or the journal.
`doctor` checks that the file exists and warns if it is group-readable.

Either way, remember that root can read `/proc/<pid>/environ`. Environment variables are not a
secret store; they're a convenience. Anything that must survive a root compromise belongs in a
separate credential mechanism.

---

## 9. Removing an app

```bash
sudo appd remove sundial-worker
```

```
APP        sundial-worker
UNIT       appd-sundial-worker.service
CONFIG     /etc/appd/sundial-worker.json
ACCOUNT    appd-sundial-worker
STATE      /var/lib/appd-sundial-worker

Remove `sundial-worker` and the items above? [y/N]
```

On `y`: stop, disable, delete the unit, `daemon-reload`, delete the config, delete the provisioned
account, delete the state directory.

Things it deliberately will not do:

- `--keep-user` and `--keep-state` leave those behind, for when you're reinstalling or
  investigating.
- It never deletes the application itself. `/srv/sundial` survives, because appd did not install
  it. That's why the demo's `uninstall.sh` removes `/srv` itself after calling `remove`.
- It refuses to delete a unit file that lacks the marker, and aborts before touching anything else.
- It only deletes an account when appd provisioned it. A config with `"user": "www-data"` never
  causes `www-data` to be deleted.

---

## 10. Troubleshooting tour

### A typo in the config

```
$ sudo appd apply sundial-api
appd: configuration is not usable, nothing applied:
  /etc/appd/sundial-api.json: invalid config for `sundial-api`: unknown field `netwrok`,
  expected one of `description`, `exec`, `args`, `working_directory`, `user`, `group`, `restart`,
  `reload`, `wanted_by`, `environment`, `environment_file`, `permissions` at line 9 column 3
```

Configs are parsed with unknown fields rejected, so a typo is an error rather than a silently
ignored setting. `apply` validates every file before writing any unit; one bad file means nothing
is applied, rather than half a deployment.

### An invalid app name

The file name is the app name, and it must work as both a unit name and an account name.

```
app name `Sundial-API` may only contain lowercase letters, digits and dashes
app name `sundial-api-with-a-very-long-name` is 33 characters; the limit is 27 so that
`appd-sundial-api-with-a-very-long-name` fits in a 32-character Linux username
`all` is reserved and cannot be used as an app name
```

Rename the file; `doctor` shows the failure with the offending path.

### `status=203/EXEC`

The unit is `failed` and the journal says:

```
Failed at step EXEC spawning /srv/sundial/bin/sundial-api: Permission denied
```

The account can't execute the binary. `doctor` catches this before you deploy:

```
  ✗ /srv/sundial/bin/sundial-api is not executable by `appd-sundial-api`
      fix: chmod o+rx /srv/sundial/bin/sundial-api (or point `user` at an account that can run it)
```

`doctor` checks every directory above the binary too, because a missing `x` bit on `/srv/sundial`
produces exactly the same error.

### `status=226/NAMESPACE`

```
Failed to set up mount namespacing: /srv/sundial/public: No such file or directory
```

A path in `permissions.read` or `permissions.write` doesn't exist. systemd refuses to start rather
than silently running with a path it couldn't mount. `doctor` reports it as:

```
  ⚠ permissions path /srv/sundial/public does not exist; the unit will fail to start
```

Create the directory, or remove it from the config.

### Writes fail even though the path is in `permissions.write`

The mount namespace permits the write; filesystem ownership still decides it. Check:

```bash
ls -ld /srv/sundial/public
sudo -u appd-sundial-api test -w /srv/sundial/public && echo writable
```

`permissions.write` is a security boundary, not a chown.

### `apply` succeeded but the old code is still running

`diff` says `in sync`, `status` shows a PID from before your change. That's by design: `apply`
synchronizes unit files, and the running process keeps the arguments it started with. Restart (or
reload, if the change is reloadable).

### `error-logs` is empty

See step 5. Either the app doesn't tag priorities, or it has nothing at `err`. The empty-result
message names the unit and priority, and `appd logs` shows the merged stream.

### `appd reload` refuses

```
sundial-api: no `reload` command configured; use `appd restart sundial-api` instead
```

Add a `reload` argv array to the config. Note that many programs exit on SIGHUP if they don't
handle it, which is exactly why appd doesn't send one blindly.

### Renaming a config leaves a running unit behind

You rename `sundial-api.json` to `sundial.json` and apply. Now:

```
$ sudo appd doctor
sundial
  ✓ ok
sundial-api
  ! orphaned: /etc/systemd/system/appd-sundial-api.service has no config
      fix: appd apply --prune

2 apps checked, 0 problems, 0 warnings, 1 orphan
```

The old unit is still installed and still running. `apply --prune` stops it, disables it, and
deletes the unit file. It does not delete the old account or state directory — use `appd remove`
before renaming if you want everything gone:

```bash
sudo appd remove sundial-api --yes
# then rename and apply
```

### Someone hand-edited the generated unit

`diff` shows the edit:

```bash
sudo appd diff sundial-api
```

But `apply` will overwrite it, because the marker is still present. Treat generated units as build
output: change the config, not the unit. If a file exists that appd never wrote, `apply` refuses:

```
appd: refusing to overwrite /etc/systemd/system/appd-sundial-api.service: it exists but
does not carry `X-Appd-Managed=1`, so appd did not create it
```

That's the collision guard, and it's why every generated unit is prefixed `appd-`.

### Working as a non-root user

`status`, `status <app>`, and `logs` work as anyone. Configs are `0600`, so appd can list app
names but not read their contents; state comes from systemd:

```
$ appd status all
APP          UNIT                         STATE    SINCE             PID    NOTES
sundial-api  appd-sundial-api.service   running  2026-10-26 14:02  41231  config unreadable, run as root
```

`apply`, `diff`, `doctor`, `remove`, and the service verbs need root on a real system. The service
verbs say so instead of surfacing a D-Bus authentication error:

```
appd: `restart` drives systemd and requires root (it runs `systemctl restart …`)
```

If the config *directory* itself isn't listable, appd says that explicitly instead of pretending
there are no apps.

### A crash loop that looks healthy

`restart: always` will restart a broken process forever. `status` can show `running` while the PID
keeps changing:

```bash
systemctl show appd-sundial-api -p NRestarts --value
sudo journalctl -u appd-sundial-api -n 50 --no-pager
```

During debugging, switch to `"restart": "on-failure"` and use `appd logs -f` so the failure
actually stops.

### A JIT runtime dies at startup

A service using a JIT (some JVM and JavaScript configurations) may fail immediately with `mmap`
`EPERM`, `SIGSYS`, or "failed to allocate executable memory". That's
`MemoryDenyWriteExecute=yes`, one of the hardening defaults. Opt out for that app only:

```json
"permissions": {
  "executable_memory": true
}
```

Then `apply` and `restart`. This weakens one layer (write-xor-execute) for that service; it's a
deliberate, visible trade, not a global switch.

### A unit that exists only as a file

`appd remove` needs a config, because it needs to know what it's allowed to delete. If a unit was
left behind by a deleted config, it shows up as an orphan and is cleaned with
`apply --prune`; `remove` won't invent permissions from nothing.

---

## 11. Command cheat sheet

| Command | Does |
| --- | --- |
| `appd init` | create `/etc/appd` and print next steps; idempotent |
| `appd apply [APP…\|all] [--prune] [--yes]` | render units, install, `daemon-reload` |
| `appd diff [APP…\|all]` | show drift; exit 1 if any |
| `appd doctor` | preflight / diagnose; exit 1 on problems |
| `appd status [APP…\|all]` | table, or detail for one app |
| `appd logs APP [-f] [-n N] [--since T] [-p LEVEL]` | journal output |
| `appd error-logs APP` | `logs -p err` |
| `appd start\|stop\|restart\|reload APP…` | `systemctl` verbs |
| `appd enable\|disable APP…` | boot behaviour |
| `appd remove APP [--keep-user] [--keep-state] [--yes]` | full uninstall |
| `appd` | interactive TUI: live table, journal pane, restart/stop/enable |

Exit codes: `0` success, `1` failure or drift/problems, so `diff` and `doctor` are usable in CI.

---

## 12. What appd deliberately does not do

- **No daemon today.** It's a frontend for systemd; unit files are the state. A daemon is planned
  as a mode of the same binary.
- **`apply` does not start, stop, restart, or enable anything.** Those are separate commands, and
  the output always tells you which one is still pending.
- **No log files.** journald handles storage, rotation and rate limiting. Apps that want
  `error-logs` must tag priorities; see step 5.
- **System scope only.** No `--user` units.
- **No ordering between apps.** `after`/`requires` are on the roadmap; until then, dependencies
  between your own services are not expressed.
- **AppArmor profiles are attached, not generated.** `permissions.apparmor` names an existing
  profile.
- **It doesn't own your application.** No binaries, no `/srv` directories, no upgrades.

---

## 13. Cleanup

```bash
sudo appd remove sundial-worker
sudo appd remove sundial-api
sudo rm -rf /srv/sundial
```

Both `remove` calls delete their unit, config, account and state directory. `/srv/sundial` is
removed by hand because appd never installed it.
