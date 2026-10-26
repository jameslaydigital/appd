//! `appctl init`: create the config directory and say what to do next.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use anyhow::{Result, bail};

use crate::paths;
use crate::systemd;

pub fn run() -> Result<i32> {
    let dir = paths::config_dir();

    if paths::manages_system_config() && !systemd::is_root() {
        bail!("`init` creates /etc/appctl; run it as root");
    }

    if dir.exists() && !dir.is_dir() {
        bail!("{} exists but is not a directory", dir.display());
    }

    if paths::ensure_config_dir()? {
        println!("Created {} (mode 0755).", dir.display());
        println!();
        println!("Next:");
        println!("  1. write one config per app: {}/<app>.json", dir.display());
        println!("  2. sudo appctl doctor");
        println!("  3. sudo appctl apply <app>");
        println!("  4. sudo appctl enable <app> && sudo appctl start <app>");
        println!();
        println!("A worked example is in TUTORIAL.md.");
    } else {
        let mode = fs::metadata(&dir)
            .map(|meta| meta.permissions().mode() & 0o777)
            .unwrap_or(0o755);
        println!("{} already exists (mode {mode:04o}).", dir.display());

        // Listing the directory is what lets non-root `status` show app names.
        if mode & 0o055 != 0o055 {
            println!(
                "note: non-root `appctl status` cannot list apps at this mode; chmod 755 {}",
                dir.display()
            );
        }
    }

    Ok(0)
}
