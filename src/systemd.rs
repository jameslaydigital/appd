//! Thin wrappers around the systemd, journal and account tools. Everything here shells out;
//! there is no D-Bus dependency.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

fn run_attach<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run `{program}`; is it installed?"))?;
    if !status.success() {
        bail!("`{program}` exited with {status}");
    }
    Ok(())
}

fn run_capture<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run `{program}`; is it installed?"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`{program}` failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn systemctl(args: &[&str]) -> Result<()> {
    run_attach("systemctl", args)
}

pub fn systemctl_output(args: &[&str]) -> Result<String> {
    run_capture("systemctl", args)
}

pub fn daemon_reload() -> Result<()> {
    systemctl(&["daemon-reload"])
}

/// `systemctl is-enabled` exits 1 for a disabled unit but still prints the state, so it cannot
/// go through `run_capture`'s success check.
pub fn is_enabled(unit: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .args(["is-enabled", unit])
        .output()
        .context("failed to run `systemctl`; is it installed?")?;

    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if state.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`systemctl is-enabled {unit}` failed: {}", stderr.trim());
    }
    Ok(state)
}

pub fn journalctl(args: &[String]) -> Result<()> {
    run_attach("journalctl", args)
}

pub fn journalctl_output(args: &[String]) -> Result<String> {
    run_capture("journalctl", args)
}

/// `systemctl show` properties for one unit, as a key/value map.
pub fn unit_properties(unit: &str) -> Result<BTreeMap<String, String>> {
    let raw = systemctl_output(&[
        "show",
        "--property=LoadState,ActiveState,SubState,Result,MainPID,ExecMainStartTimestamp",
        unit,
    ])?;

    let mut props = BTreeMap::new();
    for line in raw.lines() {
        if let Some((key, value)) = line.split_once('=') {
            props.insert(key.to_string(), value.to_string());
        }
    }
    Ok(props)
}

pub fn user_exists(name: &str) -> bool {
    Command::new("id")
        .args(["-u", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Group lookup via NSS. Returns `true` when `getent` is unavailable, so a missing tool never
/// produces a false alarm.
pub fn group_exists(name: &str) -> bool {
    Command::new("getent")
        .args(["group", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}

/// Create a no-login system account for an app. Returns true if it was created.
pub fn ensure_user(name: &str, comment: &str) -> Result<bool> {    if user_exists(name) {
        return Ok(false);
    }
    run_attach(
        "useradd",
        [
            "--system",
            "--user-group",
            "--no-create-home",
            "--home-dir",
            "/nonexistent",
            "--shell",
            "/usr/sbin/nologin",
            "--comment",
            comment,
            name,
        ],
    )?;
    Ok(true)
}

pub fn remove_user(name: &str) -> Result<()> {
    run_attach("userdel", [name])
}

pub fn account_ids(name: &str) -> Option<(u32, u32)> {
    let uid = run_capture("id", ["-u", name]).ok()?.trim().parse().ok()?;
    let gid = run_capture("id", ["-g", name]).ok()?.trim().parse().ok()?;
    Some((uid, gid))
}
