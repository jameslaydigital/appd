---
name: appd
description: Write appd JSON configs for hardened systemd services. Use when a project needs a long-running Linux service, a systemd unit, a service account, a writable state directory, or a consistent way to install, restart, and inspect app services.
---

# appd

`appd` turns one JSON file into a hardened systemd unit, a dedicated system account, and a private
state directory. It is a frontend for systemd — systemd runs the service; appd writes the unit.

Use this skill when the user wants to deploy a long-running process (HTTP API, worker, bot,
daemon) on a Linux host with systemd.

## Workflow

1. Find the entrypoint and how it expects to run: `package.json` scripts, `Cargo.toml`, `go.mod`,
   `Dockerfile` `CMD`, or the README. Prefer an executable path, not a package manager command.
2. Write `.appd.conf` at the project root. It is JSON. No comments.
3. From the project root, install it:
   ```bash
   appd install-app .
   ```
   The app name defaults to the project directory name. Validation happens before anything is
   copied to `/etc/appd/<app>.json`.
4. Check and deploy:
   ```bash
   sudo appd doctor
   sudo appd apply <app>
   sudo appd enable <app>
   sudo appd start <app>
   ```

If `appd` is not installed: `cargo install --locked appd`, then
`sudo "$(command -v appd)" install`.

## Rules

- The source filename is exactly `.appd.conf`.
- Unknown fields are errors. Do not invent fields such as `port`, `healthcheck`, or `logs`.
- App names are lowercase letters, digits, and dashes, up to 27 characters. `all` is reserved.
  Override an invalid directory name with `appd install-app . --name <name>`.
- Never put secrets in `.appd.conf`. Use `environment_file` for a root-readable `0600` file;
  systemd reads it as root.
- Omit `user` to get a provisioned `appd-<app>` account. Only name an existing user deliberately.
- Every path in `permissions.read` and `permissions.write` must already exist. `permissions.write`
  is required for writes outside the automatic state directory.
- `apply` writes units and reloads systemd. It never starts, enables, or restarts anything.
- `reload` needs a `reload` argv array. Without one, use `appd restart`.
- JIT runtimes need `"executable_memory": true`. Node/V8 dies with `SIGTRAP` under the default
  `MemoryDenyWriteExecute=yes`.
- Logs are journald. A line must start with `<3>` to appear in `appd error-logs`; plain stderr is
  priority 6.

## Schema

```json
{
  "description": "string",
  "exec": "/absolute/path",
  "args": ["--flag", "value"],
  "working_directory": "/absolute/path",
  "user": "existing-user",
  "group": "existing-group",
  "restart": "on-failure",
  "restart_sec": 3,
  "umask": "0027",
  "reload": ["/bin/kill", "-HUP", "$MAINPID"],
  "wanted_by": ["multi-user.target"],
  "environment": { "KEY": "value" },
  "environment_file": "/etc/appd/app.env",
  "permissions": {
    "network": true,
    "read": ["/absolute/path"],
    "write": ["/absolute/path"],
    "executable_memory": false,
    "devices": ["/dev/dri/renderD128 rw"],
    "apparmor": "existing-profile"
  }
}
```

Defaults: `args=[]`, `restart=on-failure`, `working_directory=/var/lib/appd-<app>`,
`user/group=appd-<app>`, `wanted_by=["multi-user.target"]`, `permissions.network=false`,
`read/write=[]`, `executable_memory=false`.

## Recipes

### Node HTTP API

```json
{
  "description": "Notes API",
  "exec": "/usr/bin/node",
  "args": ["server.js"],
  "working_directory": "/srv/notes",
  "environment": { "NODE_ENV": "production", "PORT": "8787" },
  "permissions": { "network": true, "executable_memory": true }
}
```

### Compiled binary

```json
{
  "description": "Notes API",
  "exec": "/srv/notes/bin/notes",
  "args": ["--bind", "127.0.0.1:8787"],
  "working_directory": "/srv/notes",
  "permissions": { "network": true }
}
```

### App that writes rendered files

```json
"permissions": {
  "network": true,
  "write": ["/srv/notes/public"]
}
```

The directory must exist and be owned by the service account after the first `apply`:

```bash
sudo chown appd-notes:appd-notes /srv/notes/public
```

### Secrets

```json
"environment_file": "/etc/appd/notes.env"
```

Create it `0600`; systemd reads it as root. It is not visible to the service account as a file.

## Verify before handing back

- `appd doctor` reports no problems (or the user knows which are expected).
- `appd diff <app>` says `in sync` after apply.
- `appd status all` shows `running`.
- If the app crashes immediately, check `appd logs <app>` for `203/EXEC`, `226/NAMESPACE`, or
  `EROFS`; those map to the executable, a missing permission path, and a missing write path.
