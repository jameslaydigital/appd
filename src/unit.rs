//! Rendering a [`Config`] into a systemd unit. Pure: no filesystem, no systemd, fully testable.

use std::fmt::Write as _;

use crate::config::Config;
use crate::naming;

/// Present in every unit appd writes. `apply` refuses to overwrite a unit without it.
pub const MARKER: &str = "X-Appd-Managed=1";

pub fn render(app: &str, cfg: &Config) -> String {
    let user = cfg.service_user(app);
    let group = cfg.group.clone().unwrap_or_else(|| user.clone());
    let working_dir = cfg
        .working_directory
        .clone()
        .unwrap_or_else(|| naming::state_directory(app));
    let description = cfg.description.clone().unwrap_or_else(|| app.to_string());

    let mut out = String::new();

    out.push_str("[Unit]\n");
    let _ = writeln!(out, "Description={description}");
    let _ = writeln!(out, "{MARKER}");
    if cfg.permissions.network {
        out.push_str("After=network-online.target\n");
    }

    out.push_str("\n[Service]\n");
    out.push_str("Type=simple\n");
    let _ = writeln!(out, "User={user}");
    let _ = writeln!(out, "Group={group}");
    let _ = writeln!(out, "WorkingDirectory={working_dir}");
    let _ = writeln!(out, "StateDirectory={}", naming::state_directory_name(app));

    let mut exec = String::from("ExecStart=");
    exec.push_str(&escape_arg(&cfg.exec));
    for arg in &cfg.args {
        exec.push(' ');
        exec.push_str(&escape_arg(arg));
    }
    let _ = writeln!(out, "{exec}");

    if let Some(argv) = &cfg.reload {
        let mut line = String::from("ExecReload=");
        for (i, arg) in argv.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(&escape_arg(arg));
        }
        let _ = writeln!(out, "{line}");
    }

    let _ = writeln!(out, "Restart={}", cfg.restart.as_str());
    if let Some(seconds) = cfg.restart_sec {
        let _ = writeln!(out, "RestartSec={seconds}");
    }
    if let Some(umask) = &cfg.umask {
        let _ = writeln!(out, "UMask={umask}");
    }

    for (key, value) in &cfg.environment {
        let _ = writeln!(out, "Environment={key}={}", escape_value(value));
    }
    for path in cfg.environment_file.paths() {
        let _ = writeln!(out, "EnvironmentFile={path}");
    }

    out.push_str("StandardOutput=journal\n");
    out.push_str("StandardError=journal\n");

    out.push_str("\n# hardening defaults\n");
    out.push_str("NoNewPrivileges=yes\n");
    out.push_str("PrivateTmp=yes\n");
    out.push_str("PrivateDevices=yes\n");
    out.push_str("ProtectSystem=strict\n");
    out.push_str("ProtectHome=yes\n");
    out.push_str("ProtectProc=invisible\n");
    out.push_str("ProtectClock=yes\n");
    out.push_str("ProtectHostname=yes\n");
    out.push_str("ProtectKernelTunables=yes\n");
    out.push_str("ProtectKernelModules=yes\n");
    out.push_str("ProtectKernelLogs=yes\n");
    out.push_str("ProtectControlGroups=yes\n");
    out.push_str("RestrictNamespaces=yes\n");
    out.push_str("RestrictRealtime=yes\n");
    out.push_str("RestrictSUIDSGID=yes\n");
    out.push_str("LockPersonality=yes\n");
    if !cfg.permissions.executable_memory {
        out.push_str("MemoryDenyWriteExecute=yes\n");
    }
    out.push_str("SystemCallArchitectures=native\n");

    out.push_str("\n# opened by permissions\n");
    let families = if cfg.permissions.network {
        "AF_UNIX AF_INET AF_INET6"
    } else {
        "AF_UNIX"
    };
    let _ = writeln!(out, "RestrictAddressFamilies={families}");

    if !cfg.permissions.read.is_empty() {
        let _ = writeln!(out, "ReadOnlyPaths={}", cfg.permissions.read.join(" "));
    }
    if !cfg.permissions.write.is_empty() {
        let _ = writeln!(out, "ReadWritePaths={}", cfg.permissions.write.join(" "));
    }
    for device in &cfg.permissions.devices {
        let _ = writeln!(out, "DeviceAllow={device}");
    }
    if let Some(profile) = &cfg.permissions.apparmor {
        let _ = writeln!(out, "AppArmorProfile={profile}");
    }

    out.push_str("\n[Install]\n");
    let _ = writeln!(out, "WantedBy={}", cfg.wanted_by.join(" "));

    out
}

/// Quote an ExecStart/ExecReload argument the way systemd's parser expects. `%` is escaped so
/// values are not treated as specifiers.
fn escape_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quotes = arg
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\'));

    let mut out = String::new();
    for c in arg.chars() {
        match c {
            '%' => out.push_str("%%"),
            '\\' if needs_quotes => out.push_str("\\\\"),
            '"' if needs_quotes => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

/// Quote an Environment= value. Values are passed literally; systemd does not expand
/// variables here.
fn escape_value(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quotes = value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\'));

    let mut out = String::new();
    for c in value.chars() {
        match c {
            '%' => out.push_str("%%"),
            '\\' if needs_quotes => out.push_str("\\\\"),
            '"' if needs_quotes => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn render_json(name: &str, json: &str) -> String {
        let cfg = Config::parse(name, json).unwrap();
        render(name, &cfg)
    }

    #[test]
    fn minimal_unit_is_locked_down() {
        let unit = render_json("demo", r#"{ "exec": "/srv/demo/bin/demo" }"#);

        assert!(unit.contains("User=appd-demo\n"));
        assert!(unit.contains("StateDirectory=appd-demo\n"));
        assert!(unit.contains("WorkingDirectory=/var/lib/appd-demo\n"));
        assert!(unit.contains("ExecStart=/srv/demo/bin/demo\n"));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("ProtectSystem=strict\n"));
        assert!(unit.contains("ProtectHome=yes\n"));
        assert!(unit.contains("NoNewPrivileges=yes\n"));
        assert!(unit.contains("MemoryDenyWriteExecute=yes\n"));
        assert!(unit.contains("RestrictAddressFamilies=AF_UNIX\n"));
        assert!(!unit.contains("AF_INET"));
        assert!(!unit.contains("ReadWritePaths="));
        assert!(unit.contains(MARKER));
        assert!(unit.contains("WantedBy=multi-user.target\n"));
    }

    #[test]
    fn network_opens_inet_and_ordering() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "permissions": { "network": true } }"#,
        );
        assert!(unit.contains("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6\n"));
        assert!(unit.contains("After=network-online.target\n"));
    }

    #[test]
    fn executable_memory_removes_deny_write_execute() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "permissions": { "executable_memory": true } }"#,
        );
        assert!(!unit.contains("MemoryDenyWriteExecute"));
    }

    #[test]
    fn opens_requested_paths_and_device() {
        let unit = render_json(
            "demo",
            r#"{
                "exec": "/bin/true",
                "permissions": {
                    "read": ["/srv/demo"],
                    "write": ["/var/lib/demo", "/srv/demo/uploads"],
                    "devices": ["/dev/dri/renderD128 rw"]
                }
            }"#,
        );
        assert!(unit.contains("ReadOnlyPaths=/srv/demo\n"));
        assert!(unit.contains("ReadWritePaths=/var/lib/demo /srv/demo/uploads\n"));
        assert!(unit.contains("DeviceAllow=/dev/dri/renderD128 rw\n"));
    }

    #[test]
    fn explicit_user_overrides_provisioning() {
        let unit = render_json("demo", r#"{ "exec": "/bin/true", "user": "www-data" }"#);
        assert!(unit.contains("User=www-data\n"));
        assert!(unit.contains("Group=www-data\n"));
    }

    #[test]
    fn environment_is_escaped() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "environment": { "GREETING": "hello world", "PCT": "100%" } }"#,
        );
        assert!(unit.contains("Environment=GREETING=\"hello world\"\n"));
        assert!(unit.contains("Environment=PCT=100%%\n"));
    }

    #[test]
    fn restart_sec_and_umask_are_rendered() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "restart": "always", "restart_sec": 3, "umask": "0027" }"#,
        );
        assert!(unit.contains("Restart=always
"), "{unit}");
        assert!(unit.contains("RestartSec=3
"), "{unit}");
        assert!(unit.contains("UMask=0027
"), "{unit}");
    }

    #[test]
    fn environment_files_are_rendered_one_per_line() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "environment_file": ["/etc/appd/a.env", "/etc/appd/b.env"] }"#,
        );
        assert!(unit.contains("EnvironmentFile=/etc/appd/a.env\n"));
        assert!(unit.contains("EnvironmentFile=/etc/appd/b.env\n"));
    }

    #[test]
    fn args_with_spaces_are_quoted() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/echo", "args": ["hello world", "plain"] }"#,
        );
        assert!(unit.contains("ExecStart=/bin/echo \"hello world\" plain\n"));
    }

    #[test]
    fn reload_command_is_rendered() {        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "reload": ["/bin/kill", "-HUP", "$MAINPID"] }"#,
        );
        assert!(unit.contains("ExecReload=/bin/kill -HUP $MAINPID\n"));
    }

    #[test]
    fn apparmor_profile_is_attached() {
        let unit = render_json(
            "demo",
            r#"{ "exec": "/bin/true", "permissions": { "apparmor": "demo-profile" } }"#,
        );
        assert!(unit.contains("AppArmorProfile=demo-profile\n"));
    }
}
