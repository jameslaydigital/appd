//! `appd remove`: uninstall an app — unit, config, provisioned account and state directory.
//!
//! The binary the app runs is never touched: appd did not install it, so it does not own it.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::commands::confirm;
use crate::{naming, paths, registry, systemd, unit};

pub fn run(app_name: &str, keep_user: bool, keep_state: bool, yes: bool) -> Result<i32> {
    let managing = paths::manages_system_units();
    if managing && !systemd::is_root() {
        bail!(
            "`remove` deletes units, configs and system accounts; run:\n  {} remove <app>",
            crate::commands::root_hint()
        );
    }

    let registry = registry::load()?;
    let selected = registry.select(&[app_name.to_string()])?;
    let app = selected[0];

    let unit_name = naming::unit_name(&app.name);
    let unit_path = paths::unit_dir().join(&unit_name);
    let user = app.config.service_user(&app.name);
    let state_dir = naming::state_directory(&app.name);
    let provisions_user = app.config.provisions_user();

    // Never delete a unit that appd did not write, even if the config thinks it owns the name.
    if unit_path.exists() {
        let existing = fs::read_to_string(&unit_path)
            .with_context(|| format!("cannot read {}", unit_path.display()))?;
        if !existing.contains(unit::MARKER) {
            bail!(
                "refusing to delete {}: it was not created by appd",
                unit_path.display()
            );
        }
    }

    println!("APP        {}", app.name);
    println!(
        "UNIT       {unit_name}{}",
        if unit_path.exists() {
            ""
        } else {
            " (not installed)"
        }
    );
    println!("CONFIG     {}", app.path.display());
    if provisions_user {
        println!(
            "ACCOUNT    {user}{}",
            if keep_user { " (kept)" } else { "" }
        );
    }
    if keep_state {
        println!("STATE      {state_dir} (kept)");
    } else {
        println!("STATE      {state_dir}");
    }
    println!();

    if !confirm(&format!("Remove `{}` and the items above", app.name), yes)? {
        println!("aborted");
        return Ok(0);
    }

    if managing && unit_path.exists() {
        let _ = systemd::systemctl(&["stop", &unit_name]);
        let _ = systemd::systemctl(&["disable", &unit_name]);
    }

    if unit_path.exists() {
        fs::remove_file(&unit_path)
            .with_context(|| format!("cannot delete {}", unit_path.display()))?;
        println!("Deleted {}", unit_path.display());
        if managing {
            let _ = systemd::systemctl(&["reset-failed", &unit_name]);
            systemd::daemon_reload()?;
        }
    }

    fs::remove_file(&app.path)
        .with_context(|| format!("cannot delete {}", app.path.display()))?;
    println!("Deleted {}", app.path.display());

    if provisions_user {
        if keep_user {
            println!("Kept account {user}");
        } else if managing && systemd::user_exists(&user) {
            systemd::remove_user(&user)?;
            println!("Deleted account {user}");
        }
    }

    if keep_state {
        println!("Kept state directory {state_dir}");
    } else if managing {
        remove_state_dir(Path::new(&state_dir))?;
    }

    println!();
    println!("{} removed.", app.name);
    Ok(0)
}

fn remove_state_dir(path: &Path) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| format!("cannot inspect {}", path.display()));
        }
    };

    if meta.file_type().is_symlink() {
        println!("Skipped {} (symlink, not a directory)", path.display());
        return Ok(());
    }
    if !meta.is_dir() {
        println!("Skipped {} (not a directory)", path.display());
        return Ok(());
    }

    fs::remove_dir_all(path).with_context(|| format!("cannot delete {}", path.display()))?;
    println!("Deleted {}", path.display());
    Ok(())
}
