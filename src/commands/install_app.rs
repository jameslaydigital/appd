//! `appd install-app`: copy a project's `.appd.conf` into `/etc/appd/<project>.json`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Config};
use crate::{paths, systemd};

pub fn run(path: &Path, name: Option<&str>, apply: bool) -> Result<i32> {
    if paths::manages_system_config() && !systemd::is_root() {
        bail!(
            "`install-app` writes to /etc/appd; it needs root.\n\n  {}",
            crate::commands::root_hint("install-app .")
        );
    }

    let source = locate(path)?;
    let project_dir = project_dir(&source)?;
    let app_name = name
        .map(str::to_string)
        .or_else(|| {
            project_dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_default();

    if let Err(err) = config::validate_name(&app_name) {
        bail!(
            "{err}\n\nThe project directory name is not a valid app name; pass `--name <name>`."
        );
    }

    let json = fs::read_to_string(&source)
        .with_context(|| format!("cannot read {}", source.display()))?;
    // Validate before touching /etc: paths, unknown fields, and the app name.
    Config::parse(&app_name, &json)?;

    paths::ensure_config_dir()?;
    let destination = paths::config_dir().join(format!("{app_name}.json"));
    let unchanged = fs::read_to_string(&destination)
        .map(|existing| existing == json)
        .unwrap_or(false);

    if unchanged {
        println!("{app_name} is already installed at {}", destination.display());
    } else {
        write_config(&destination, &json)?;
        println!(
            "Installed {} -> {} (mode 0600)",
            source.display(),
            destination.display()
        );
    }

    if apply {
        println!();
        return crate::commands::apply::run(&[app_name], false, true);
    }

    println!();
    println!("Next:");
    println!("  sudo appd doctor");
    println!("  sudo appd apply {app_name}");
    println!("  sudo appd enable {app_name} && sudo appd start {app_name}");
    Ok(0)
}

fn locate(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }

    let candidate = path.join(".appd.conf");
    if candidate.is_file() {
        return Ok(candidate);
    }

    bail!(
        "no .appd.conf in {} (pass the directory that contains it, or the file itself)",
        path.display()
    );
}

fn project_dir(source: &Path) -> Result<PathBuf> {
    match source.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => std::env::current_dir().context("cannot determine the current directory"),
    }
}

fn write_config(destination: &Path, json: &str) -> Result<()> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    let tmp = destination.with_file_name(format!(".{file_name}.tmp"));

    fs::write(&tmp, json).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, destination)
        .with_context(|| format!("cannot replace {}", destination.display()))?;
    Ok(())
}
