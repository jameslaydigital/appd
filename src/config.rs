//! One JSON file per application, normally `/etc/appd/<app>.json`.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::naming;

/// Longest app name that still leaves room for the `appd-` prefix inside Linux's
/// 32-character username limit.
pub const MAX_APP_NAME_LEN: usize = 32 - naming::PREFIX.len();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub description: Option<String>,
    /// Absolute path to the binary to run.
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Defaults to the app's state directory.
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Defaults to a provisioned `appd-<app>` account. `"root"` is allowed but must be
    /// written down.
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub restart: Restart,
    /// Seconds between restarts (`RestartSec=`). systemd's default is 100ms.
    #[serde(default)]
    pub restart_sec: Option<u32>,
    /// Octal umask for the service's processes, e.g. `"0027"`.
    #[serde(default)]
    pub umask: Option<String>,
    /// Command used by `appd reload` / `systemctl reload`.
    #[serde(default)]
    pub reload: Option<Vec<String>>,
    #[serde(default = "default_wanted_by")]
    pub wanted_by: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// File(s) of `KEY=value` lines, rendered as `EnvironmentFile=`. systemd reads them as root,
    /// so secrets can stay out of the config.
    #[serde(default)]
    pub environment_file: EnvironmentFiles,
    #[serde(default)]
    pub permissions: Permissions,
}

/// Accepts either a single path (`"environment_file": "/etc/appd/x.env"`) or an array.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EnvironmentFiles {
    One(String),
    Many(Vec<String>),
}

impl Default for EnvironmentFiles {
    fn default() -> Self {
        EnvironmentFiles::Many(Vec::new())
    }
}

impl EnvironmentFiles {
    pub fn paths(&self) -> Vec<&str> {
        match self {
            EnvironmentFiles::One(path) => vec![path.as_str()],
            EnvironmentFiles::Many(paths) => paths.iter().map(String::as_str).collect(),
        }
    }
}

/// What the unit is allowed to do. Empty means locked down.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    /// Opens `AF_INET`/`AF_INET6`. Unix sockets are always available.
    pub network: bool,
    pub read: Vec<String>,
    pub write: Vec<String>,
    /// Disables `MemoryDenyWriteExecute`, for runtimes that JIT.
    pub executable_memory: bool,
    pub devices: Vec<String>,
    /// Attaches an existing AppArmor profile by name. appd never generates one.
    pub apparmor: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    No,
    Always,
    #[default]
    OnFailure,
    OnSuccess,
    OnAbnormal,
    OnAbort,
    OnWatchdog,
}

impl Restart {
    pub fn as_str(self) -> &'static str {
        match self {
            Restart::No => "no",
            Restart::Always => "always",
            Restart::OnFailure => "on-failure",
            Restart::OnSuccess => "on-success",
            Restart::OnAbnormal => "on-abnormal",
            Restart::OnAbort => "on-abort",
            Restart::OnWatchdog => "on-watchdog",
        }
    }
}

fn default_wanted_by() -> Vec<String> {
    vec!["multi-user.target".to_string()]
}

impl Config {
    pub fn parse(name: &str, json: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(json)
            .with_context(|| format!("invalid config for `{name}`"))?;
        config.validate(name)?;
        Ok(config)
    }

    pub fn validate(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        require_absolute("exec", &self.exec)?;

        if let Some(dir) = &self.working_directory {
            require_absolute("working_directory", dir)?;
        }
        for path in &self.permissions.read {
            require_absolute("permissions.read", path)?;
        }
        for path in &self.permissions.write {
            require_absolute("permissions.write", path)?;
        }
        for path in self.environment_file.paths() {
            require_absolute("environment_file", path)?;
        }

        for key in self.environment.keys() {
            if key.is_empty()
                || key.contains('=')
                || key.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                bail!("environment variable name `{key}` is not valid");
            }
        }

        if let Some(argv) = &self.reload {
            if argv.is_empty() {
                bail!("`reload` must be a non-empty array of arguments");
            }
            require_absolute("reload", &argv[0])?;
        }

        if let Some(umask) = &self.umask {
            if umask.is_empty()
                || umask.len() > 4
                || !umask.chars().all(|c| matches!(c, '0'..='7'))
            {
                bail!("`umask` must be 1-4 octal digits, got `{umask}`");
            }
        }

        if let Some(user) = &self.user {
            validate_account_name("user", user)?;
        }
        if let Some(group) = &self.group {
            validate_account_name("group", group)?;
        }

        Ok(())
    }

    /// The account the service runs as, after applying the default.
    pub fn service_user(&self, app: &str) -> String {
        self.user
            .clone()
            .unwrap_or_else(|| naming::default_user(app))
    }

    /// Whether `apply` should create the account rather than use an existing one.
    pub fn provisions_user(&self) -> bool {
        self.user.is_none()
    }

    /// Environment variables mean the file may hold secrets.
    pub fn has_environment(&self) -> bool {
        !self.environment.is_empty()
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("app name is empty");
    }
    if name.len() > MAX_APP_NAME_LEN {
        bail!(
            "app name `{name}` is {} characters; the limit is {MAX_APP_NAME_LEN} so that `{}{name}` \
             fits in a 32-character Linux username",
            name.len(),
            naming::PREFIX
        );
    }
    if name == "all" {
        bail!("`all` is reserved and cannot be used as an app name");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("app name `{name}` may only contain lowercase letters, digits and dashes");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("app name `{name}` must not start or end with a dash");
    }
    Ok(())
}

fn require_absolute(field: &str, value: &str) -> Result<()> {
    if !value.starts_with('/') {
        bail!("`{field}` must be an absolute path, got `{value}`");
    }
    Ok(())
}

fn validate_account_name(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 32
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        bail!("`{field}` is not a valid account name: `{value}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> &'static str {
        r#"{ "exec": "/srv/app/bin/app" }"#
    }

    #[test]
    fn parses_minimal_config_with_defaults() {
        let cfg = Config::parse("demo", minimal()).unwrap();
        assert_eq!(cfg.exec, "/srv/app/bin/app");
        assert_eq!(cfg.restart, Restart::OnFailure);
        assert_eq!(cfg.wanted_by, vec!["multi-user.target"]);
        assert!(cfg.provisions_user());
        assert!(!cfg.permissions.network);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Config::parse("demo", r#"{ "exec": "/bin/true", "execc": "/bin/false" }"#)
            .unwrap_err();
        // `{:#}` prints the context chain; the bare Display would only say "invalid config".
        let err = format!("{err:#}");
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn rejects_bad_names() {
        for name in ["", "Demo", "demo.app", "-demo", "demo-", "all", &"a".repeat(28)] {
            assert!(validate_name(name).is_err(), "`{name}` should be rejected");
        }
        assert!(validate_name("goal-tracker-2").is_ok());
        assert!(validate_name(&"a".repeat(27)).is_ok());
    }

    #[test]
    fn rejects_relative_paths() {
        let err = Config::parse("demo", r#"{ "exec": "bin/app" }"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("absolute"), "{err}");
    }

    #[test]
    fn rejects_bad_environment_names() {
        for json in [
            r#"{ "exec": "/bin/true", "environment": { "A B": "1" } }"#,
            r#"{ "exec": "/bin/true", "environment": { "A=B": "1" } }"#,
        ] {
            assert!(Config::parse("demo", json).is_err(), "{json}");
        }
    }

    #[test]
    fn rejects_empty_reload() {
        let err = Config::parse("demo", r#"{ "exec": "/bin/true", "reload": [] }"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-empty"), "{err}");
    }

    #[test]
    fn root_is_allowed_but_explicit() {
        let cfg = Config::parse("demo", r#"{ "exec": "/bin/true", "user": "root" }"#).unwrap();
        assert!(!cfg.provisions_user());
        assert_eq!(cfg.service_user("demo"), "root");
    }

    #[test]
    fn validates_restart_sec_and_umask() {
        let cfg = Config::parse(
            "demo",
            r#"{ "exec": "/bin/true", "restart_sec": 3, "umask": "0027" }"#,
        )
        .unwrap();
        assert_eq!(cfg.restart_sec, Some(3));
        assert_eq!(cfg.umask.as_deref(), Some("0027"));

        for bad in ["0899", "999", "", "00271"] {
            let json = format!(r#"{{ "exec": "/bin/true", "umask": "{bad}" }}"#);
            assert!(Config::parse("demo", &json).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn rejects_restart_typos() {
        assert!(Config::parse("demo", r#"{ "exec": "/bin/true", "restart": "onfail" }"#).is_err());
    }

    #[test]
    fn accepts_single_or_multiple_environment_files() {
        let one = Config::parse(
            "demo",
            r#"{ "exec": "/bin/true", "environment_file": "/etc/appd/demo.env" }"#,
        )
        .unwrap();
        assert_eq!(one.environment_file.paths(), vec!["/etc/appd/demo.env"]);

        let many = Config::parse(
            "demo",
            r#"{ "exec": "/bin/true", "environment_file": ["/etc/appd/a.env", "/etc/appd/b.env"] }"#,
        )
        .unwrap();
        assert_eq!(
            many.environment_file.paths(),
            vec!["/etc/appd/a.env", "/etc/appd/b.env"]
        );

        assert!(
            Config::parse(
                "demo",
                r#"{ "exec": "/bin/true", "environment_file": "relative.env" }"#
            )
            .is_err()
        );
    }
}
