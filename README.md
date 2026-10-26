APPD
====

Manage multiple application services and long-running processes with systemd-level security and hardening.

`appd` is a thin, opinionated frontend for systemd: you describe each application in a JSON file,
and `appd` generates the systemd unit, installs it, and gives you short commands for the operations
you actually perform day to day. Today it is a CLI and nothing else — there is no background
process. A daemon is planned, and will be a mode of this same binary.

New here? [TUTORIAL.md](TUTORIAL.md) walks through a full deployment — first config, day-to-day
operation, a second service, and the failures you'll actually hit.

## How It Works

- One JSON file per application lives in `/etc/appd/`, e.g. `/etc/appd/goal-tracker.json`.
- `appd apply` renders that file into `/etc/systemd/system/appd-goal-tracker.service`, then
  runs `systemctl daemon-reload`.
- Every unit is namespaced with an `appd-` prefix, so appd never collides with units from
  packages or hand-written ones. An app you call `apachectl` becomes
  `appd-apachectl.service`; the real `apachectl.service`, if any, is untouched.
- Appd only manages units it generated. Every unit it writes carries an `X-Appd-Managed=1`
  marker, and `apply` refuses to overwrite a unit that lacks it.
- Hardening is on by default. A generated unit is locked down unless the config explicitly opens
  a permission — `"network": true`, a writable path, and so on. There is no "disable hardening"
  switch; you open what you need.
- Every app gets its own unprivileged system user by default, provisioned on `apply`.

## Requirements

- Linux with systemd and journald, system scope (not `--user` units)
- `systemctl`, `journalctl` and `useradd` — present in any normal install. (`useradd` lives in
  `shadow-utils` on most distros and in `passwd` on Debian.)

To build, you also need a stable Rust toolchain, plus `make` and `install` for the Makefile route.
Nothing else is required at runtime: the binary links no systemd libraries, it invokes the tools
above as subprocesses.

## Install

### From crates.io

```bash
cargo install --locked appd
```

This is the Rust-native route: it downloads the published crate, builds it, and installs `appd` to
`~/.cargo/bin`. `--locked` uses the dependency versions recorded in the published `Cargo.lock`, so
the build matches what was released. No daemon, no service to enable — the binary is the whole
program.

appd is a system tool: `init`, `apply`, `remove` and the service verbs need root. A cargo install
lands in `~/.cargo/bin`, which `sudo` does not search, so install it system-wide once:

```bash
sudo "$(command -v appd)" install
```

That copies the binary to `/usr/local/bin` and writes the man page and shell completions; from then
on `sudo appd init` works. `appd install --prefix ~/.local` does a user-local install instead.

### From a source checkout

```bash
make
sudo make install
```

`make` runs `cargo build --release`; `make install` copies the binary along with the man page and
shell completions:

| What | Where (`PREFIX=/usr/local`) |
| --- | --- |
| `appd` | `$(PREFIX)/bin/appd` |
| `appd.1` | `$(PREFIX)/share/man/man1/appd.1` |
| bash completion | `$(PREFIX)/share/bash-completion/completions/appd` |
| zsh completion | `$(PREFIX)/share/zsh/site-functions/_appd` |
| fish completion | `$(PREFIX)/share/fish/vendor_completions.d/appd.fish` |

`PREFIX` and `DESTDIR` are honoured, so `make install DESTDIR=/tmp/stage PREFIX=/usr` produces a
tree you can hand to a packager.

For an unprivileged, user-local install from the checkout:

```bash
cargo install --path .                 # build this checkout into ~/.cargo/bin
# or
make install PREFIX="$HOME/.local"     # ~/.local/bin, ~/.local/share/…
```

One practical note: `~/.cargo/bin` is usually not on `sudo`'s `secure_path`, and `apply`,
`remove` and the service verbs need root. For a machine where you'll actually use appd, prefer
`sudo make install` or `sudo cargo install --root /usr/local`.

### From a prebuilt binary

If you have a release tarball, its `install.sh` does the whole job:

```bash
sudo ./appd-0.2.5-x86_64-unknown-linux-musl/install.sh
```

With a bare binary, let it install itself:

```bash
sudo ./appd install
```

Either way you get the binary in `/usr/local/bin`, `appd.1` in `share/man/man1`, and bash, zsh and
fish completions. `--prefix` and `--destdir` are supported for non-standard layouts and packaging,
and `--uninstall` removes all five files.

`appd init` creates `/etc/appd` (mode `0755`) and is idempotent, so none of these installation
routes need to. `apply` never creates the directory; if it is missing, it tells you to run `init`.

### Verify

```bash
appd --version
systemctl is-system-running      # expect "running" or "degraded"
sudo appd init                 # create /etc/appd; safe to re-run
sudo appd doctor               # "0 apps checked" on a fresh install
```

`doctor` exits non-zero only when it finds a problem, so it's safe to run as a post-install check.
Open a new shell (or `exec $SHELL`) for completions to load.

### Upgrading and uninstalling

appd keeps no state of its own — configs, units and accounts are the state. Upgrading is
replacing the binary.

From a checkout:

```bash
make
sudo make install
sudo appd diff        # check whether the new version renders units differently
sudo appd apply all   # then restart anything it reports as changed
```

With cargo, remember that `cargo install` only updates the copy in `~/.cargo/bin`; the system-wide
copy is separate:

```bash
cargo install --locked appd
sudo "$HOME/.cargo/bin/appd" install   # replace /usr/local/bin/appd, man page, completions
sudo appd diff
sudo appd apply all
```

`appd doctor` warns if the two copies ever disagree, so a missed self-install does not go unnoticed.

To remove the installed files:

```bash
sudo make uninstall            # Makefile route
cargo uninstall appd         # cargo route
```

Neither touches `/etc/appd`, `/etc/systemd/system/appd-*.service`, accounts or state
directories. Per-app teardown is `sudo appd remove <app>`.

### Distributing it

#### An artifact for a server without a toolchain

```bash
make dist
# target/dist/appd-0.2.5-x86_64-unknown-linux-musl.tar.gz  (+ .sha256)
```

`make dist` builds a **static musl binary** for `TARGET` (default
`x86_64-unknown-linux-musl`) and packages it with the man page, completions, README, tutorial and an
`install.sh` that honours `PREFIX`/`DESTDIR` and supports `--uninstall`. Static means there is no
glibc version to match:

```bash
scp target/dist/appd-*-x86_64-unknown-linux-musl.tar.gz server:/tmp/
ssh server 'cd /tmp && tar xzf appd-*.tar.gz && sudo ./appd-*/install.sh'
```

For an ARM server, `TARGET=aarch64-unknown-linux-musl` needs a cross linker, so `cargo-zigbuild`
(`cargo install cargo-zigbuild`, then `cargo zigbuild --target …`) or a CI runner of the same
architecture is the easy path. If you'd rather match the host libc, `TARGET=x86_64-unknown-linux-gnu`
works when the server's glibc is at least as new as the build machine's.

#### Packages and releases

The crate is published on crates.io as `appd`, so the Rust-native route needs no artifact:
`cargo install --locked appd`. For everyone else, packaging is mostly about producing the binary,
since appd links libc but no systemd libraries — it drives `systemctl`, `journalctl` and `useradd`
as subprocesses:

```bash
cargo install cargo-deb && cargo deb           # .deb for Debian/Ubuntu
```

For release automation, `cargo-dist` generates shell installers, archives and a GitHub release once
the project lives in a git repository with tags. Any channel should ship the binary, `appd.1` from
`make man`, and the completion files from `make completions`. Both are generated from the clap
definitions, so they cannot drift from the CLI.

## Usage

```bash
sudo appd init                    # create /etc/appd, then exit
appd apply all                    # render /etc/appd/*.json into systemd units, then daemon-reload
appd apply all --prune            # also stop, disable, and delete managed units whose config is gone
appd apply goal-tracker           # same, for a single app

appd doctor                       # preflight check: permissions, orphans, drift, enablement

appd status all                   # table of every appd app and its current state
appd status goal-tracker          # detail for one app
appd diff                         # show drift between installed units and configs; exits 1 if any

appd start goal-tracker
appd stop goal-tracker
appd restart all                  # for every appd app
appd reload goal-tracker          # runs the app's configured reload command

appd enable goal-tracker          # systemctl enable appd-goal-tracker.service
appd disable goal-tracker         # systemctl disable appd-goal-tracker.service

appd remove goal-tracker          # unit, config, provisioned account and state directory; prompts
appd remove goal-tracker --yes --keep-state   # leave /var/lib/appd-goal-tracker behind

appd logs goal-tracker            # app's journal output
appd error-logs goal-tracker      # same, filtered to errors
appd logs goal-tracker -f         # follow
appd logs goal-tracker -n 200     # last 200 lines (default 50)
appd logs goal-tracker --since 1h

appd                              # no arguments: interactive TUI (status, logs, restarts)
```

App names are always written *without* the prefix. appd adds and strips `appd-` itself.

`all` always means *all apps managed by appd*, never every unit on the machine.

### Root vs. non-root

Write operations (`apply`, `enable`, `disable`, `start`, `stop`, `restart`, `reload`) require
root, since they write to `/etc/systemd/system`, manage users, and drive systemd.

Read operations (`status`, `logs`, `error-logs`, the TUI) work as any user, showing whatever
`systemctl` and the journal will show them. When a specific app's state or logs are not readable
at the current privilege level, the row says so explicitly:

```
APP                 UNIT                          STATE          SINCE             PID     NOTES
goal-tracker        appd-goal-tracker.service   running        2026-10-26 09:23  1842
nginx-proxy         appd-nginx-proxy.service    ?              -                 -       permission denied
backup-worker       appd-backup-worker.service  failed         2026-10-26 09:09  -       result exit-code
```

Configs are `0600` because they can hold secrets, so a non-root `status` takes the app names from
the config directory and the state from systemd, but cannot render the config itself: the detail
view omits `USER` and `STATE DIR`, and `doctor` reports the config as unreadable. The directory is
`0755` so the names are listable; the files are not.

### TUI

`appd` with no arguments opens a live view: the app table, a streaming journal pane, and the
service verbs on single keys. Piped or non-interactive invocations print `status all` instead of
taking over the terminal.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | move between apps |
| `enter` / `l` | toggle the log pane for the selection (follows the journal) |
| `a` | apply configs: render units, install, `daemon-reload` — does not restart |
| `r` | restart (asks first) |
| `s` | stop (asks first) |
| `t` | start |
| `e` | enable at boot |
| `x` | disable at boot (asks first) |
| `/` | filter apps by name |
| `?` | key help |
| `q` | quit |

Not root? The TUI stays read-only: mutating keys print the equivalent `sudo appd …` command
instead of failing, and the header says `read-only (not root)`.

### Apply output

`apply` says exactly what it did, and is explicit that it did not restart anything:

```
$ appd apply all
Applied 4 configs, 2 changed.

  APP              UNIT                            RESULT
  goal-tracker     appd-goal-tracker.service     updated
  backup-worker    appd-backup-worker.service    updated
  nginx-proxy      appd-nginx-proxy.service      unchanged
  old-importer     appd-old-importer.service     orphaned (no config) — see appd doctor

Unit files synchronized, systemd reloaded.
Running services are still using the previous units. Restart or reload them to pick up the changes:

  appd restart goal-tracker backup-worker
```

`--prune` stops, disables, and deletes managed units whose config has gone away. It does not
delete the app's user or state directory — that stays with `appd remove`.

## Migrating an existing unit

appd writes `appd-<app>.service`, so it never overwrites the hand-written unit you're replacing.
Both units can exist at once, which also means both can try to bind the same port. The safe order:

1. Write `/etc/appd/<app>.json` and run `sudo appd doctor` — it reports missing paths, env files
   and executables before anything is installed. If `/etc/appd` doesn't exist yet, run
   `sudo appd init` first.
2. `sudo appd apply <app>` — writes the new unit and reloads systemd; starts nothing.
3. `sudo systemctl disable --now <app>.service` — stop the incumbent.
4. `sudo appd enable <app> && sudo appd start <app>`.
5. Check `sudo appd status <app>`, then delete the old unit file and run
   `sudo systemctl daemon-reload`.

Translating the directives:

| Hand-written unit | Config |
| --- | --- |
| `User=` / `Group=` | `user` / `group`; omit both to provision an app-specific account |
| `WorkingDirectory=` | `working_directory` |
| `ExecStart=` | `exec` + `args` |
| `Environment=` | `environment` |
| `EnvironmentFile=` | `environment_file` |
| `Restart=` / `RestartSec=` | `restart` / `restart_sec` |
| `UMask=` | `umask` |
| `ReadWritePaths=` | `permissions.write` |
| `RestrictAddressFamilies=` with inet | `permissions.network: true` |
| `After=network.target` | nothing; `permissions.network` adds `network-online.target` |
| `NoNewPrivileges=`, `Protect*`, `Private*`, `Restrict*` | defaults — no need to repeat them |

Two differences bite in practice:

- **`ProtectSystem=strict` is the default**, stricter than `ProtectSystem=full`. Anything the service
  writes must be in `permissions.write` (or its state directory), and those paths must already exist,
  or it fails with `226/NAMESPACE` or `EROFS`.
- **`MemoryDenyWriteExecute=yes` is the default** and breaks JIT runtimes — Node/V8 dies with
  `SIGTRAP` at startup. Set `"executable_memory": true` for those services.

There is no global switch to turn hardening off; the only opt-outs are the specific permissions.

## Configuration

One file per application, named after the app: `/etc/appd/goal-tracker.json` becomes
`appd-goal-tracker.service`. `appd init` creates the directory if it doesn't exist.

App names are capped at 27 characters, because the derived account `appd-<app>` has to fit
Linux's 32-character username limit. `apply` rejects longer names before doing anything.

```json
{
  "description": "Goal Tracker HTTP API",
  "exec": "/srv/goal-tracker/bin/api",
  "args": ["--bind", "127.0.0.1:8080"],
  "working_directory": "/srv/goal-tracker",
  "restart": "on-failure",
  "environment": {
    "RUST_LOG": "info",
    "DATABASE_URL": "postgres:///goaltracker"
  },
  "permissions": {
    "network": true,
    "read": ["/srv/goal-tracker"]
  }
}
```

With no `user` field, `apply` provisions a system account `appd-goal-tracker` (no shell, no
password, system UID range) and runs the service as it. There is no writable path listed because
the service gets a private state directory automatically — see *Users and state*.

### Fields

| Field | Required | Default | Meaning |
| --- | --- | --- | --- |
| `description` | no | app name | `Description=` in the unit |
| `exec` | yes | — | Absolute path to the binary |
| `args` | no | `[]` | Arguments passed to `exec` |
| `working_directory` | no | state directory | `WorkingDirectory=` |
| `user` | no | `appd-<app>` | Existing user to run as. `"root"` allowed, but must be written explicitly |
| `group` | no | `user` | `Group=` |
| `restart` | no | `on-failure` | `Restart=` |
| `restart_sec` | no | systemd default (100ms) | `RestartSec=` in seconds |
| `umask` | no | systemd default (`0022`) | `UMask=`, 1–4 octal digits |
| `reload` | no | — | Command used by `appd reload`, as an argv array. Without it, `reload` refuses and points at `restart` |
| `wanted_by` | no | `["multi-user.target"]` | What `enable` links into |
| `environment` | no | `{}` | Environment variables |
| `environment_file` | no | — | Path or array of paths to `KEY=value` files, rendered as `EnvironmentFile=`. Read by systemd as root, so secrets stay out of the config |
| `permissions.network` | no | `false` | Opens `AF_INET` / `AF_INET6` |
| `permissions.read` | no | `[]` | `ReadOnlyPaths=` |
| `permissions.write` | no | `[]` | `ReadWritePaths=`, for paths beyond the state directory |
| `permissions.executable_memory` | no | `false` | Disables `MemoryDenyWriteExecute` for JIT runtimes |
| `permissions.devices` | no | `[]` | `DeviceAllow=` entries |
| `permissions.apparmor` | no | `null` | Named AppArmor profile to attach, if present |

`user` defaults to a provisioned account on purpose: running as root must be an explicit,
visible choice in the config.

### Users and state

`apply` creates `appd-<app>` as a system user if it does not exist, and never modifies or
deletes an existing account. Combined with `StateDirectory=appd-<app>`, systemd then creates
`/var/lib/appd-<app>`, owned by that user, and appd adds it to the writable set automatically
even though `ProtectSystem=strict` is in effect.

So the default for a new app is: own user, own private writable directory, read-only filesystem
everywhere else, no network, no home directory. Everything beyond that is in `permissions`.

AppArmor profiles are referenced by name only. appd attaches an existing profile to the unit;
it does not generate or install one yet.

### What the example generates

Roughly this, minus some hardening lines elided for length:

```ini
[Unit]
Description=Goal Tracker HTTP API
X-Appd-Managed=1

[Service]
Type=simple
User=appd-goal-tracker
Group=appd-goal-tracker
WorkingDirectory=/var/lib/appd-goal-tracker
ExecStart=/srv/goal-tracker/bin/api --bind 127.0.0.1:8080
Restart=on-failure
Environment=RUST_LOG=info
Environment=DATABASE_URL=postgres:///goaltracker
StateDirectory=appd-goal-tracker
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
ReadOnlyPaths=/srv/goal-tracker
```

## Logs

The backend is journald, because it already handles rotation, rate limiting, and structured
metadata. appd just puts a sane interface on it. You never pass `-u` or `-xe`, and the unit
name is always derived from the app name.

| appd | journalctl |
| --- | --- |
| `appd logs app` | `journalctl -u appd-app.service` |
| `appd logs app -f` | `... -f` |
| `appd logs app -n 200` | `... -n 200` |
| `appd logs app --since 1h` | `... --since 1h` |
| `appd error-logs app` | `... -p err` |

Pipe to `grep` as usual. `error-logs` is just `logs` with an error priority filter.

One thing that trips people up: journald records both stdout and stderr at priority 6 for systemd
services, so writing to stderr is not enough to make a line show up in `error-logs`. An app tags
its own lines with a syslog priority prefix — `<3>` for errors — and journald strips the prefix
and stores the priority. That is what `sd_journal_print(LOG_ERR, …)` does under the hood. Apps
that never tag anything will produce nothing in `error-logs`; `logs` is the fallback.

### Why journald only

Separate files per stream (`StandardError=append:…`) would give exact stderr separation with no
app cooperation, but systemd does not rotate `append:` files, so appd would inherit rotation,
truncation and disk-growth concerns per app. v1 stays on the journal: one view, rotation for
free, and priorities when the app provides them. This is a deliberate trade, not an oversight;
file-backed logging is on the roadmap.

## Doctor

`appd doctor` is a read-only preflight over installed configs and units. It doesn't fix
anything; it tells you what's wrong and which command fixes it.

```
$ appd doctor
goal-tracker
  ✗ /srv/goal-tracker/bin/api is not executable by appd-goal-tracker
      fix: chmod o+rx /srv/goal-tracker/bin/api, or set "user" to an account that can run it
  ⚠ /etc/appd/goal-tracker.json is world readable and contains 2 environment variables
      fix: chmod 600 /etc/appd/goal-tracker.json

backup-worker
  ✗ unit is installed but not enabled
      fix: appd enable backup-worker

old-importer
  ! orphaned: appd-old-importer.service has no config file
      fix: appd apply --prune

4 apps checked, 2 problems, 1 warning, 1 orphan
```

Checks include:

- `exec` missing, or not readable/executable by the service account (the classic `status=203/EXEC`)
- configs with environment variables that are group or world readable
- app names too long for the derived account name
- orphaned units: managed units with no config
- drift: a managed unit that differs from what the current config would generate
- enablement drift: installed but not enabled, or enabled but not installed
- provisioned user missing
- a second `appd` on root's PATH at a different version (the cargo copy vs `/usr/local/bin`)
- journald unavailable, or the unit's output not reaching the journal

Exit status is non-zero when any problem is found, so it can be used in scripts.

## Safety

- `apply` validates every config file before writing any unit. One malformed file aborts the
  whole run rather than leaving the system half-applied.
- Config files in `/etc/appd/` can contain secrets, so appd warns when one is group or
  world readable. It does not change the mode for you.
- `all` never touches units appd did not generate.
- `apply`, `restart`, and `reload` with `all` prompt for confirmation. Pass `--yes` for scripts.
- `enable` is deliberately separate from `apply`. Applying a new config does not start it.
- `remove` refuses to delete a unit file that lacks the appd marker, deletes a system account
  only when appd provisioned it, and takes `--keep-user` / `--keep-state` to leave either in
  place. The application binary is never touched: appd did not install it.

## Status

`status` reports the systemd state per app plus whatever else is useful and cheap to obtain. The
primary source is `systemctl`, but it does not have to be the only one — if process or cgroup
information gives a better answer, use it. Keep the output to one line per app plus an optional
detail view. Every row includes the real unit name so it can be pasted into `systemctl` or
`journalctl`. Managed units with no config appear as orphaned rows pointing at `appd doctor`.

## Roadmap

- File-backed logging mode (`"logging": "files"`), stdout and stderr to separate files, with
  rotation appd owns
- Generating AppArmor profiles, not just attaching existing ones
- `after` / `requires` for app-to-app ordering

## Example app

`examples/hello-world/` is a small service for exercising appd on a real machine. It logs a
heartbeat to stdout, writes a simulated problem to stderr every tenth beat, and logs a line on
SIGHUP so `reload` has something to show.

```bash
sudo examples/hello-world/install.sh    # installs, applies, enables and starts it

appd status hello-world
appd logs hello-world -f
appd error-logs hello-world
appd reload hello-world

sudo examples/hello-world/uninstall.sh  # unit, config, account and state directory
```

`install.sh` uses `appd` from `PATH`, falling back to `target/release` or `target/debug`, and
sets the config to `0600` so `doctor` stays quiet about environment variables.

## Development

```bash
cargo build
cargo test
```

`src/unit.rs` is a pure renderer — config in, unit text out — so unit changes are testable without
systemd. Config parsing uses `deny_unknown_fields`, so a typo'd key is an error rather than a
silently ignored setting.

`APPD_CONFIG_DIR` and `APPD_UNIT_DIR` redirect the config and unit directories. When the unit
directory is redirected, `apply` skips the root check and `daemon-reload`. `tests/cli.rs` uses this
to drive the real binary end to end — apply, diff, doctor and the privilege guard — against temp
directories instead of a live system.

