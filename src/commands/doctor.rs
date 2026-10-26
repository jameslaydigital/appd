//! `appctl doctor`: read-only preflight. Reports problems and the command that fixes each one.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use anyhow::Result;

use crate::commands::plural;
use crate::{naming, paths, registry, systemd, unit};

enum Finding {
    Problem(String, Option<String>),
    Warning(String, Option<String>),
}

pub fn run() -> Result<i32> {
    let registry = registry::load()?;
    let managing = paths::manages_system_units();

    let mut problems = 0usize;
    let mut warnings = 0usize;

    for failure in &registry.failures {
        println!("{}", failure.path.display());
        println!("  ✗ {}", failure.message);
        problems += 1;
    }

    for app in &registry.apps {
        let findings = inspect(app, managing);
        if findings.is_empty() {
            println!("{}\n  ✓ ok", app.name);
            continue;
        }

        println!("{}", app.name);
        for finding in findings {
            let (mark, message, fix) = match finding {
                Finding::Problem(message, fix) => {
                    problems += 1;
                    ('✗', message, fix)
                }
                Finding::Warning(message, fix) => {
                    warnings += 1;
                    ('⚠', message, fix)
                }
            };
            println!("  {mark} {message}");
            if let Some(fix) = fix {
                println!("      fix: {fix}");
            }
        }
    }

    for opaque in &registry.opaque {
        println!("{}", opaque.name);
        println!(
            "  ⚠ config {} is unreadable ({})",
            opaque.path.display(),
            opaque.reason
        );
        println!("      fix: run as root to check this app");
        warnings += 1;
    }

    let orphans = registry.orphaned_units()?;
    for (app, path) in &orphans {
        println!("{app}");
        println!("  ! orphaned: {} has no config", path.display());
        println!("      fix: appctl apply --prune");
    }

    if !Path::new("/run/systemd/journal/socket").exists() {
        println!("journald");
        println!("  ✗ /run/systemd/journal/socket is missing; `appctl logs` will not work");
        problems += 1;
    }

    println!();
    println!(
        "{} checked, {}, {}, {}",
        plural(
            registry.apps.len() + registry.opaque.len(),
            "app",
            "apps"
        ),
        plural(problems, "problem", "problems"),
        plural(warnings, "warning", "warnings"),
        plural(orphans.len(), "orphan", "orphans")
    );

    Ok(if problems > 0 { 1 } else { 0 })
}

fn inspect(app: &registry::App, managing: bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let user = app.config.service_user(&app.name);
    let unit_name = naming::unit_name(&app.name);
    let unit_path = paths::unit_dir().join(&unit_name);

    // Secrets: env vars plus a permissive mode.
    if let Ok(meta) = fs::metadata(&app.path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 && app.config.has_environment() {
            findings.push(Finding::Warning(
                format!(
                    "{} is readable by group/others and holds environment variables",
                    app.path.display()
                ),
                Some(format!("chmod 600 {}", app.path.display())),
            ));
        }
    }

    // Environment files are read by systemd as root, so 0600 is correct for them.
    for path in app.config.environment_file.paths() {
        match fs::metadata(path) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    findings.push(Finding::Warning(
                        format!("{path} is readable by group/others"),
                        Some(format!("chmod 600 {path}")),
                    ));
                }
            }
            Err(_) => findings.push(Finding::Problem(
                format!("environment file {path} does not exist; the unit will fail to start"),
                None,
            )),
        }
    }

    // Executable reachable by the service account.
    let exec = Path::new(&app.config.exec);
    if !exec.exists() {
        findings.push(Finding::Problem(
            format!("exec path {} does not exist", app.config.exec),
            None,
        ));
    } else if !executable_by(exec, &user) {
        findings.push(Finding::Problem(
            format!("{} is not executable by `{user}`", app.config.exec),
            Some(format!(
                "chmod o+rx {} (or point `user` at an account that can run it)",
                app.config.exec
            )),
        ));
    }

    // Paths opened in the config must exist, or systemd refuses to start the unit.
    for path in app
        .config
        .permissions
        .read
        .iter()
        .chain(&app.config.permissions.write)
    {
        if !Path::new(path).exists() {
            findings.push(Finding::Warning(
                format!("permissions path {path} does not exist; the unit will fail to start"),
                None,
            ));
        }
    }

    if let Some(dir) = &app.config.working_directory {
        if !Path::new(dir).exists() {
            findings.push(Finding::Warning(
                format!("working directory {dir} does not exist; the unit will fail to start"),
                None,
            ));
        }
    }

    if let Some(argv) = &app.config.reload {
        if !Path::new(&argv[0]).exists() {
            findings.push(Finding::Problem(
                format!("reload command {} does not exist; `appctl reload` will fail", argv[0]),
                None,
            ));
        }
    }

    // The account the unit runs as: provisioned by us, or named explicitly in the config.
    if app.config.provisions_user() {
        if managing && !systemd::user_exists(&user) {
            findings.push(Finding::Problem(
                format!("service account `{user}` does not exist"),
                Some(format!("appctl apply {}", app.name)),
            ));
        }
    } else if !systemd::user_exists(&user) {
        findings.push(Finding::Problem(
            format!("user `{user}` does not exist; the unit will fail to start"),
            Some(
                "create the account, or remove `user` so appctl provisions one".to_string(),
            ),
        ));
    }

    if let Some(group) = &app.config.group {
        if !systemd::group_exists(group) {
            findings.push(Finding::Problem(
                format!("group `{group}` does not exist; the unit will fail to start"),
                None,
            ));
        }
    }

    if !unit_path.exists() {
        if managing {
            findings.push(Finding::Warning(
                format!("{unit_name} is not installed"),
                Some(format!("appctl apply {}", app.name)),
            ));
        }
    } else {
        match fs::read_to_string(&unit_path) {
            Ok(existing) if existing != unit::render(&app.name, &app.config) => {
                findings.push(Finding::Warning(
                    format!("{unit_name} differs from what the config generates"),
                    Some(format!("appctl apply {}", app.name)),
                ));
            }
            Ok(_) => {}
            Err(err) => findings.push(Finding::Problem(
                format!("cannot read {}: {err}", unit_path.display()),
                None,
            )),
        }
    }

    if managing && unit_path.exists() {
        match systemd::is_enabled(&unit_name) {
            Ok(state) => {
                let state = state.trim();
                if !matches!(state, "enabled" | "enabled-runtime" | "static" | "alias") {
                    findings.push(Finding::Warning(
                        format!("{unit_name} is `{state}`, not enabled"),
                        Some(format!("appctl enable {}", app.name)),
                    ));
                }
            }
            Err(_) => findings.push(Finding::Warning(
                format!("cannot determine whether {unit_name} is enabled"),
                None,
            )),
        }
    }

    findings
}

/// Approximate `access(2)` for another account: checks the owner/group/other bits on the file
/// and on every directory above it. ACLs are not considered, so a pass here is not a promise.
fn executable_by(path: &Path, user: &str) -> bool {
    if user == "root" {
        return true;
    }
    let Some((uid, gid)) = systemd::account_ids(user) else {
        // Unknown account: `apply` will create it, so don't guess.
        return true;
    };

    for ancestor in path.ancestors() {
        let Ok(meta) = fs::metadata(ancestor) else {
            return false;
        };
        let needed = if ancestor == path { 0o5 } else { 0o1 };
        if meta.mode() & 0o777 & mask_for(&meta, uid, gid) & needed != needed {
            return false;
        }
    }
    true
}

fn mask_for(meta: &fs::Metadata, uid: u32, gid: u32) -> u32 {
    if meta.uid() == uid {
        0o700
    } else if meta.gid() == gid {
        0o070
    } else {
        0o007
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_selects_the_right_class() {
        let meta = fs::metadata("/").unwrap();
        let uid = meta.uid();
        let gid = meta.gid();
        assert_eq!(mask_for(&meta, uid, gid), 0o700);
        assert_eq!(mask_for(&meta, uid.wrapping_add(1), gid), 0o070);
        assert_eq!(
            mask_for(&meta, uid.wrapping_add(1), gid.wrapping_add(1)),
            0o007
        );
    }
}
