//! `appd apply`: render configs into units, install them, report what changed.
//!
//! The work is split from the reporting so the TUI can trigger a sync without touching stdout:
//! [`sync`] writes units and returns a report, [`run`] adds the root check, confirmation and
//! human-readable output.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::commands::{self, confirm};
use crate::table::Table;
use crate::{naming, paths, registry, systemd, unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Created,
    Updated,
    Unchanged,
    Orphaned,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Outcome::Created => "created",
            Outcome::Updated => "updated",
            Outcome::Unchanged => "unchanged",
            Outcome::Orphaned => "orphaned (no config)",
        }
    }
}

pub(crate) struct Report {
    pub(crate) applied: usize,
    pub(crate) changed: Vec<String>,
    pub(crate) provisioned: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) orphans: Vec<(String, PathBuf)>,
    pub(crate) pruned: Vec<String>,
    pub(crate) rows: Vec<(String, Outcome)>,
    pub(crate) dir_exists: bool,
}

/// Write and reload units, returning what happened. No output, no confirmation, no root check:
/// callers decide those. Used by both `appd apply` and the TUI.
pub(crate) fn sync(apps: &[String], prune: bool) -> Result<Report> {
    let managing = paths::manages_system_units();
    let registry = registry::load()?;
    // All-or-nothing: never write half a configuration.
    registry.require_sound()?;

    let selected = registry.select(apps)?;
    let orphans = registry.orphaned_units()?;
    let dir_exists = registry.dir.exists();

    let mut rows: Vec<(String, Outcome)> = Vec::new();
    let mut changed = Vec::new();
    let mut provisioned = Vec::new();
    let mut warnings = Vec::new();

    for app in &selected {
        let user = app.config.service_user(&app.name);
        if managing
            && app.config.provisions_user()
            && systemd::ensure_user(&user, &format!("appd app {}", app.name))?
        {
            provisioned.push(user);
        }

        // Catch a typo'd reload command now rather than when someone tries to use it.
        if let Some(argv) = &app.config.reload {
            if !Path::new(&argv[0]).exists() {
                warnings.push(format!(
                    "{}: reload command {} does not exist; `appd reload` will fail",
                    app.name, argv[0]
                ));
            }
        }

        let rendered = unit::render(&app.name, &app.config);
        let path = paths::unit_dir().join(naming::unit_name(&app.name));
        let outcome = write_unit(&path, &rendered)?;
        if matches!(outcome, Outcome::Created | Outcome::Updated) {
            changed.push(app.name.clone());
        }
        rows.push((app.name.clone(), outcome));
    }

    for (app, _) in &orphans {
        rows.push((app.clone(), Outcome::Orphaned));
    }

    if !changed.is_empty() && managing {
        systemd::daemon_reload()
            .context("units were written but `systemctl daemon-reload` failed")?;
    }

    let pruned = if prune {
        prune_units(&orphans, managing)?
    } else {
        Vec::new()
    };

    Ok(Report {
        applied: selected.len(),
        changed,
        provisioned,
        warnings,
        orphans,
        pruned,
        rows,
        dir_exists,
    })
}

pub fn run(apps: &[String], prune: bool, yes: bool) -> Result<i32> {
    let managing = paths::manages_system_units();
    if managing && !systemd::is_root() {
        bail!(
            "`apply` writes to {} and manages system accounts; it needs root.\n\n  {}",
            paths::unit_dir().display(),
            crate::commands::root_hint("apply")
        );
    }

    // Load once for the confirmation decision; `sync` loads again to do the work.
    let registry = registry::load()?;
    registry.require_sound()?;

    let selected = registry.select(apps)?;
    let all_requested = apps.is_empty() || apps.iter().any(|app| app == "all");
    if all_requested && selected.len() > 1 && !confirm("Apply all configs", yes)? {
        println!("aborted, nothing applied");
        return Ok(0);
    }

    let report = sync(apps, prune)?;

    if report.applied == 0 && report.orphans.is_empty() {
        if report.dir_exists {
            println!("no configs in {}", paths::config_dir().display());
        } else {
            println!(
                "no config directory at {}; run `appd init` first",
                paths::config_dir().display()
            );
        }
        return Ok(0);
    }

    let mut table = Table::new(&["APP", "UNIT", "RESULT"]);
    for (app, outcome) in &report.rows {
        table.push(vec![
            app.clone(),
            naming::unit_name(app),
            outcome.label().to_string(),
        ]);
    }

    println!(
        "Applied {}, {} changed.",
        commands::plural(report.applied, "config", "configs"),
        report.changed.len()
    );
    println!();
    print!("{}", table.render());

    if !report.provisioned.is_empty() {
        println!();
        println!(
            "Created system accounts: {}",
            report.provisioned.join(", ")
        );
    }

    if !report.warnings.is_empty() {
        println!();
        for warning in &report.warnings {
            println!("warning: {warning}");
        }
    }

    println!();
    if report.changed.is_empty() {
        println!("All installed units already match their configs.");
    } else {
        println!(
            "Unit files synchronized{}.",
            if managing { ", systemd reloaded" } else { "" }
        );
        println!(
            "Running services are still using the previous units. Restart or reload them to pick up the changes:"
        );
        println!();
        println!("  appd restart {}", report.changed.join(" "));
    }

    if prune {
        if report.pruned.is_empty() {
            println!();
            println!("Nothing to prune.");
        } else {
            println!();
            for app in &report.pruned {
                println!("Pruned {}.", naming::unit_name(app));
            }
        }
    } else if !report.orphans.is_empty() {
        println!();
        let message = if report.orphans.len() == 1 {
            "1 orphaned unit has no config; run `appd apply --prune` to remove it.".to_string()
        } else {
            format!(
                "{} orphaned units have no config; run `appd apply --prune` to remove them.",
                report.orphans.len()
            )
        };
        println!("{message}");
    }

    Ok(0)
}

fn prune_units(orphans: &[(String, PathBuf)], managing: bool) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for (app, path) in orphans {
        let unit = naming::unit_name(app);
        if managing {
            // Best effort: the unit may already be gone from systemd's view.
            let _ = systemd::systemctl(&["stop", &unit]);
            let _ = systemd::systemctl(&["disable", &unit]);
        }
        fs::remove_file(path)
            .with_context(|| format!("cannot delete {}", path.display()))?;
        removed.push(app.clone());
    }
    if managing && !removed.is_empty() {
        systemd::daemon_reload()?;
    }
    Ok(removed)
}

fn write_unit(path: &Path, rendered: &str) -> Result<Outcome> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }

    if path.exists() {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if !existing.contains(unit::MARKER) {
            bail!(
                "refusing to overwrite {}: it exists but does not carry `{}`, so appd did not create it",
                path.display(),
                unit::MARKER
            );
        }
        if existing == rendered {
            return Ok(Outcome::Unchanged);
        }
        write_atomic(path, rendered)?;
        Ok(Outcome::Updated)
    } else {
        write_atomic(path, rendered)?;
        Ok(Outcome::Created)
    }
}

/// Write via a temporary file in the same directory so a crash cannot leave a half-written
/// unit behind. systemd ignores the `.tmp` name.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unit");
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));

    fs::write(&tmp, contents).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reports_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("appd-demo.service");
        let contents = "[Service]\nX-Appd-Managed=1\n";

        assert_eq!(write_unit(&path, contents).unwrap(), Outcome::Created);
        assert_eq!(write_unit(&path, contents).unwrap(), Outcome::Unchanged);
    }

    #[test]
    fn updates_managed_unit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("appd-demo.service");

        write_unit(&path, "[Service]\nX-Appd-Managed=1\nA=1\n").unwrap();
        assert_eq!(
            write_unit(&path, "[Service]\nX-Appd-Managed=1\nA=2\n").unwrap(),
            Outcome::Updated
        );
        assert!(fs::read_to_string(&path).unwrap().contains("A=2"));
    }

    #[test]
    fn refuses_to_clobber_foreign_unit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("appd-demo.service");
        fs::write(&path, "[Service]\nExecStart=/usr/bin/other\n").unwrap();

        let err = write_unit(&path, "[Service]\nX-Appd-Managed=1\n").unwrap_err();
        assert!(err.to_string().contains("refusing to overwrite"), "{err}");
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("/usr/bin/other"),
            "foreign unit must be left alone"
        );
    }
}
