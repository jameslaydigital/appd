# AGENTS.md

`appd` — a hardened frontend for systemd. This file is the project memory for future agents.

- Repo: <https://github.com/jameslaydigital/appd>
- Crate: <https://crates.io/crates/appd> (`cargo install --locked appd`)
- Site: <https://jameslaydigital.github.io/appd/>
- Current release: **0.2.7**
- Previous name: `appctl`; GitHub redirects the old repo URL. Do not reintroduce the old prefix.

## What it is

appd generates and manages systemd units for long-running application services from one place.
The user writes one JSON config per app; appd renders a hardened unit, provisions an unprivileged
account, creates a private state directory, and gives short commands for apply/status/logs/restart.

The philosophy, in the project owner's words: simplify every aspect of managing a long-running
process. `systemctl` is exact but repetitive — no group restart, unit names to remember, hardening
flags copied from somewhere. `supervisorctl` gives group control but not systemd's out-of-the-box
security. appd is the wrapper around systemd that closes both gaps: generation and management from
one place, security-first provisioning, a convenient TUI, familiar command structure, and group
operations that work. "Swiss army knife of software service management." If you use cloud services
and containers, great; if you run services on a Linux box, this is for you.

It is a **CLI today**. A daemon is planned as a mode of the same binary, not a second product.
Systemd is the daemon; unit files are the state.

## Repo map

```
src/main.rs            entrypoint, SIGPIPE handling
src/cli.rs             clap command surface
src/config.rs          serde config + validation (deny_unknown_fields)
src/unit.rs            pure config -> systemd unit renderer
src/naming.rs          appd- prefixes, app name limits
src/paths.rs           /etc/appd, /etc/systemd/system, env overrides
src/registry.rs        config discovery, selection, opaque/unreadable configs, orphans
src/systemd.rs         systemctl/journalctl/useradd wrappers, root check
src/commands/
  apply.rs             sync() core + CLI output; apply never starts/restarts
  diff.rs              LCS line diff; exits 1 on drift
  doctor.rs            read-only preflight; exits 1 on problems
  init.rs              create /etc/appd (install also does this)
  install.rs           self-install binary/man/completions, and create /etc/appd
  install_app.rs       ./.appd.conf -> /etc/appd/<project>.json
  skills.rs            install .agents/skills/appd/SKILL.md into a project
  lifecycle.rs         start/stop/restart/reload/enable/disable
  logs.rs              journald facade
  remove.rs            unit + config + account + state
  status.rs            status table/detail
  generate.rs          man page + completions from clap definitions
  mod.rs               dispatch, confirmation, root hint
src/tui.rs             ratatui TUI (table, journal pane, actions, read-only mode)
skills/appd/SKILL.md   editor skill embedded into the binary with include_str!
site/                  GitHub Pages site (static HTML/CSS/JS)
tests/cli.rs           end-to-end CLI tests via the real binary
```

## Command surface

- `appd install [--prefix DIR] [--destdir DIR] [--uninstall]` — copy the running binary, man page,
  completions; create `/etc/appd` when root and not staging. Root hints point at this command.
- `appd init` — create `/etc/appd` without installing; optional since `install` does it.
- `appd install-app [DIR] [--name NAME] [--apply]` — find `DIR/.appd.conf`, validate, copy to
  `/etc/appd/<project>.json` at 0600. Name defaults to the project directory name.
- `appd skills install [DIR] [--claude|--codex|--opencode]` — write the skill into a project.
  No flag: `.agents/skills`; flags: `.claude/skills`, `.codex/skills`, `.opencode/skills` (combinable).
- `appd apply [APP…|all] [--prune] [--yes]` — render units, install, `daemon-reload`. No restart.
- `appd diff [APP…|all]` — installed unit vs generated; exit 1 on drift.
- `appd doctor` — read-only preflight; exit 1 on problems.
- `appd status [APP…|all]` — table or detail.
- `appd start|stop|restart|reload|enable|disable`.
- `appd logs APP [-f] [-n N] [--since] [-p LEVEL]` and `appd error-logs APP`.
- `appd remove APP [--keep-user] [--keep-state] [--yes]`.
- `appd man`, `appd completions SHELL`.
- `appd` with no args opens the TUI; piped/non-TTY falls back to `status all`.

## Configuration

One JSON file per app: `/etc/appd/<app>.json`, mode 0600, strict schema. Source configs in projects
are conventionally `.appd.conf`. Fields: `description`, `exec`, `args`, `working_directory`, `user`,
`group`, `restart`, `restart_sec`, `umask`, `reload`, `wanted_by`, `environment`,
`environment_file`, `permissions.{network,read,write,executable_memory,devices,apparmor}`.

Defaults: `args=[]`, `restart=on-failure`, `working_directory=/var/lib/appd-<app>`,
`user/group=appd-<app>`, `wanted_by=["multi-user.target"]`, no network, no read/write paths.
`user` omitted means "provision an account"; `root` must be written explicitly.

## Invariants — do not break

1. Unit prefix is `appd-`; every generated unit carries `X-Appd-Managed=1`. Never overwrite a unit
   without the marker. `remove` refuses foreign units.
2. App name is the config filename stem: lowercase letters, digits, dashes, ≤27 chars (`appd-` is 5
   of the 32-char Linux username limit). `all` is reserved.
3. Derived names are fixed: unit `appd-<app>.service`, account `appd-<app>`, state
   `/var/lib/appd-<app>`, config `/etc/appd/<app>.json`.
4. `apply` never starts, stops, restarts, or enables. Those are separate commands. Output must say
   what still needs a restart.
5. JSON is strict (`deny_unknown_fields`); one invalid config aborts the whole apply before any
   write.
6. Read commands work as a user; write commands need root. Configs are 0600, dirs 0755.
7. Logs are journald only. `error-logs` is `-p err`; journald stores plain stdout/stderr at
   priority 6, so apps must tag errors with `<3>` (or use syslog) to appear there.
8. No user units, no `after`/`requires` yet, no AppArmor generation yet.
9. The app binary is never owned by appd. `remove` deletes unit/config/account/state only.

## Gotchas

- **Cargo fingerprints after moving the checkout**: `cargo build` can say "Finished" without
  rebuilding and run a stale binary. Run `cargo clean -p appd` after moving/renaming the directory
  or when a source change seems ignored.
- **Node/V8 and `MemoryDenyWriteExecute=yes`**: Node crashes with `SIGTRAP`. Set
  `"executable_memory": true`. Verified on Node 26; older/newer may differ.
- **Write paths**: `permissions.write` emits `ReadWritePaths=`. The path must exist at start or the
  unit fails with `226/NAMESPACE`. `StateDirectory=/var/lib/appd-<app>` is writable automatically.
- **Ownership**: opening a path in the sandbox does not chown it. The `appd-<app>` account must own
  or be able to write the path.
- **203/EXEC**: the service account cannot execute the binary or traverse a parent directory.
  `doctor` simulates the owner/group/other bits (not ACLs).
- **Reload**: `appd reload` requires a `reload` argv in the config; otherwise it refuses and points
  at restart. Never send SIGHUP blindly — many programs exit on it.
- **Two copies of appd**: cargo installs `~/.cargo/bin/appd`; `appd install` copies to
  `/usr/local/bin`. `doctor` warns on version drift. Update the system copy with
  `sudo "$HOME/.cargo/bin/appd" install` after a cargo upgrade.
- **sudo PATH**: `~/.cargo/bin` is not on sudo's secure_path. Root hints print absolute paths for
  that reason.

## Development

```bash
cargo test                 # 69 tests: unit + CLI end-to-end
cargo build --release
make dist                  # static musl tarball + checksum in target/dist/
```

`APPD_CONFIG_DIR` and `APPD_UNIT_DIR` redirect paths for tests. When the unit dir is redirected,
`apply` skips the root check and `daemon-reload`. Do not "fix" tests by writing to `/etc`.

`src/unit.rs` is a pure renderer; add renderer tests there. Config parsing tests live in
`src/config.rs`. CLI tests in `tests/cli.rs` invoke the real binary.

## Release checklist

1. Bump `version` in `Cargo.toml`; update version strings in `README.md` and `site/*.html`.
2. `cargo clean -p appd && cargo test && cargo build --release`.
3. `make dist` (musl static tarball) and verify the tarball installs into a temp prefix.
4. Commit and push; tag `vX.Y.Z` and push the tag.
5. `cargo publish` (token configured in `~/.cargo/credentials.toml`).
6. `gh release create vX.Y.Z target/dist/appd-X.Y.Z-x86_64-unknown-linux-musl.tar.gz …sha256`.
7. Push site changes; the Pages workflow deploys `site/`.
8. Verify: crates.io `max_version`, GitHub release asset, `https://jameslaydigital.github.io/appd/`.

## Site

Built with Hallmark v1.1.0 (Long Document macrostructure; custom xterm-256 theme; IBM Plex Mono +
Newsreader). `.hallmark/log.json` records the run. Design tokens live in `site/tokens.css`; all
colors/fonts must go through those tokens. The palette canvas and copy buttons are the only JS.
`site/404.html` uses `<base href="/appd/">` so assets resolve at arbitrary 404 paths. Deploy is
`.github/workflows/pages.yml` with current action majors; Pages is configured for workflow builds.

## Roadmap / open work

- File-backed logging mode (stdout/stderr to files, rotation appd owns) — deferred in v1.
- `after` / `requires` for app-to-app ordering.
- AppArmor profile generation (currently attaches an existing profile by name).
- Daemon mode (planned as a mode of the same binary).
- Maybe `install-app --apply` exists; `skills install` supports `.agents`, `.claude`, `.codex`, `.opencode` layouts.
- Crate page `homepage` can point at the site in a future release (repository is set).
