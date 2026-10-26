//! `appd apply`: render configs into units, install them, report what changed.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::commands::{self, confirm};
use crate::table::Table;
use crate::{naming, paths, registry, systemd, unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
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

pub fn run(apps: &[String], prune: bool, yes: bool) -> Result<i32> {
    let managing = paths::manages_system_units();
    if managing && !systemd::is_root() {
        bail!(
            "`apply` writes to {} and manages system accounts; it needs root.\n\n  {}",
            paths::unit_dir().display(),
            crate::commands::root_hint("apply")
        );
    }

    // `apply` never creates the config directory: `init` owns that decision.
    let registry = registry::load()?;
    // All-or-nothing: never write half a configuration.
    registry.require_sound()?;

    let selected = registry.select(apps)?;
    let all_requested = apps.is_empty() || apps.iter().any(|app| app == "all");
    if all_requested && selected.len() > 1 && !confirm("Apply all configs", yes)? {
        println!("aborted, nothing applied");
        return Ok(0);
    }

    let orphans = registry.orphaned_units()?;
    if selected.is_empty() && orphans.is_empty() {
        if registry.dir.exists() {
            println!("no configs in {}", registry.dir.display());
        } else {
            println!(
                "no config directory at {}; run `appd init` first",
                registry.dir.display()
            );
        }
        return Ok(0);
    }

    let mut rows: Vec<(String, Outcome)> = Vec::new();
    let mut changed = Vec::new();
    let mut provisioned = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

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

    let mut table = Table::new(&["APP", "UNIT", "RESULT"]);
    for (app, outcome) in &rows {
        table.push(vec![
            app.clone(),
            naming::unit_name(app),
            outcome.label().to_string(),
        ]);
    }

    println!(
        "Applied {}, {} changed.",
        commands::plural(selected.len(), "config", "configs"),
        changed.len()
    );
    println!();
    print!("{}", table.render());

    if !provisioned.is_empty() {
        println!();
        println!("Created system accounts: {}", provisioned.join(", "));
    }

    if !warnings.is_empty() {
        println!();
        for warning in &warnings {
            println!("warning: {warning}");
        }
    }

    println!();
    if changed.is_empty() {
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
        println!("  appd restart {}", changed.join(" "));
    }

    if prune {
        let removed = prune_units(&orphans, managing)?;
        if removed.is_empty() {
            println!();
            println!("Nothing to prune.");
        } else {
            println!();
            for app in &removed {
                println!("Pruned {}.", naming::unit_name(app));
            }
        }
    } else if !orphans.is_empty() {
        println!();
        let message = if orphans.len() == 1 {
            "1 orphaned unit has no config; run `appd apply --prune` to remove it.".to_string()
        } else {
            format!(
                "{} orphaned units have no config; run `appd apply --prune` to remove them.",
                orphans.len()
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
