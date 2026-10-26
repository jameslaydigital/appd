//! Filesystem locations. The environment overrides exist so the tool can be exercised against
//! a temporary directory in tests; they are not part of the documented interface.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

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

/// Create the config directory if it is missing. Only write commands call this; `status`,
/// `logs` and `doctor` never touch the filesystem.
///
/// Returns true when the directory was created. Existing directories are left exactly as they
/// are, including their permissions.
pub fn ensure_config_dir() -> Result<bool> {
    let dir = config_dir();
    if dir.exists() {
        return Ok(false);
    }

    fs::create_dir_all(&dir)
        .with_context(|| format!("cannot create config directory {}", dir.display()))?;
    // 0755 so non-root `status` can list app names; the files themselves stay 0600.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot set permissions on {}", dir.display()))?;
    Ok(true)
}

pub fn unit_dir() -> PathBuf {
    overridable("APPCTL_UNIT_DIR", DEFAULT_UNIT_DIR)
}

/// True when we are driving the real systemd instance. Writing to a redirected unit directory
/// skips the root check and `daemon-reload`, which is what makes the tests possible.
pub fn manages_system_units() -> bool {
    unit_dir() == PathBuf::from(DEFAULT_UNIT_DIR)
}
