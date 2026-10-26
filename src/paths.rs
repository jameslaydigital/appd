//! Filesystem locations. The environment overrides exist so the tool can be exercised against
//! a temporary directory in tests; they are not part of the documented interface.

use std::path::PathBuf;

pub const DEFAULT_CONFIG_DIR: &str = "/etc/appctl";
pub const DEFAULT_UNIT_DIR: &str = "/etc/systemd/system";

fn overridable(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

pub fn config_dir() -> PathBuf {
    overridable("APPCTL_CONFIG_DIR", DEFAULT_CONFIG_DIR)
}

pub fn unit_dir() -> PathBuf {
    overridable("APPCTL_UNIT_DIR", DEFAULT_UNIT_DIR)
}

/// True when we are driving the real systemd instance. Writing to a redirected unit directory
/// skips the root check and `daemon-reload`, which is what makes the tests possible.
pub fn manages_system_units() -> bool {
    unit_dir() == PathBuf::from(DEFAULT_UNIT_DIR)
}
