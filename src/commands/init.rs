//! `appd init`: create the config directory and say what to do next.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use anyhow::{Result, bail};

use crate::commands::root_hint;
use crate::{paths, systemd};

pub fn run() -> Result<i32> {
    let dir = paths::config_dir();

    if dir.exists() && !dir.is_dir() {
        bail!("{} exists but is not a directory", dir.display());
    }

    if !dir.exists() {
        if paths::manages_system_config() && !systemd::is_root() {
            // No point failing with "run as root" and no command: a cargo-installed appd is not
            // on sudo's PATH, so spell out the working invocation.
            bail!(
                "creating {} needs root.\n\n  {}",
                dir.display(),
                root_hint("init")
            );
        }
        paths::ensure_config_dir()?;
        println!("Created {} (mode 0755).", dir.display());
        println!();
        println!("Next:");
        println!("  1. write one config per app: {}/<app>.json", dir.display());
        println!("  2. sudo appd doctor");
        println!("  3. sudo appd apply <app>");
        println!("  4. sudo appd enable <app> && sudo appd start <app>");
        println!();
        println!("A worked example is in TUTORIAL.md.");
        return Ok(0);
    }

    // Already there: this is a status check, and needs no privileges.
    let mode = fs::metadata(&dir)
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0o755);
    println!("{} already exists (mode {mode:04o}).", dir.display());

    // Listing the directory is what lets non-root `status` show app names.
    if mode & 0o055 != 0o055 {
        println!(
            "note: non-root `appd status` cannot list apps at this mode; chmod 755 {}",
            dir.display()
        );
    }

    Ok(0)
}
