//! `appd install`: copy this binary, its man page and shell completions into place, so a
//! `cargo install` is a complete installation rather than just a binary in `~/.cargo/bin`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap_complete::Shell;

use crate::commands::generate;
use crate::systemd;

const BIN: &str = "bin/appd";
const MAN: &str = "share/man/man1/appd.1";
const BASH: &str = "share/bash-completion/completions/appd";
const ZSH: &str = "share/zsh/site-functions/_appd";
const FISH: &str = "share/fish/vendor_completions.d/appd.fish";

pub fn run(prefix: &Path, destdir: &Path, uninstall: bool) -> Result<i32> {
    let root = staged(prefix, destdir);
    let bin = root.join(BIN);
    let man = root.join(MAN);
    let bash = root.join(BASH);
    let zsh = root.join(ZSH);
    let fish = root.join(FISH);

    if uninstall {
        return remove_files(&root, &[&bin, &man, &bash, &zsh, &fish]);
    }

    let exe = std::env::current_exe().context("cannot find the running executable")?;

    // Fail before writing anything if the destination obviously needs root.
    if destdir.as_os_str().is_empty() && !systemd::is_root() && prefix.starts_with("/usr") {
        bail!(
            "writing to {} needs root; run:\n  sudo {} install",
            prefix.display(),
            exe.display()
        );
    }

    for path in [&bin, &man, &bash, &zsh, &fish] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }

    // Never truncate the running binary when it is already the destination.
    let already_there = fs::canonicalize(&exe)
        .ok()
        .zip(fs::canonicalize(&bin).ok())
        .is_some_and(|(source, target)| source == target);
    let previous = if bin.is_file() && !already_there {
        crate::commands::binary_version(&bin)
    } else {
        None
    };
    if !already_there {
        let bytes =
            fs::read(&exe).with_context(|| format!("cannot read {}", exe.display()))?;
        write_file(&bin, &bytes, 0o755)?;
    }

    write_file(&man, &generate::man_roff()?, 0o644)?;
    write_file(&bash, &generate::completions_text(Shell::Bash), 0o644)?;
    write_file(&zsh, &generate::completions_text(Shell::Zsh), 0o644)?;
    write_file(&fish, &generate::completions_text(Shell::Fish), 0o644)?;

    match previous {
        Some(version) => println!("Replaced appd {version} at {}", bin.display()),
        None => println!("Installed appd to {}", bin.display()),
    }
    println!("  man page:    {}", man.display());
    println!("  completions: bash, zsh, fish");

    if root == Path::new("/usr/local") {
        println!();
        println!("Run privileged commands as: sudo appd …");
    }

    Ok(0)
}

fn remove_files(root: &Path, paths: &[&Path]) -> Result<i32> {
    let mut removed = 0;
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {
                println!("Removed {}", path.display());
                removed += 1;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("cannot remove {}", path.display()));
            }
        }
    }

    if removed == 0 {
        println!("Nothing installed under {}", root.display());
    } else {
        println!("Removed appd from {}.", root.display());
    }
    Ok(0)
}

/// `destdir` is prepended to the absolute prefix for packaging (`DESTDIR=/tmp/stage make install`).
fn staged(prefix: &Path, destdir: &Path) -> PathBuf {
    if destdir.as_os_str().is_empty() {
        prefix.to_path_buf()
    } else {
        destdir.join(prefix.strip_prefix("/").unwrap_or(prefix))
    }
}

fn write_file(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "appd".to_string());
    let tmp = path.with_file_name(format!(".{name}.tmp"));

    fs::write(&tmp, contents).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destdir_is_prepended_to_an_absolute_prefix() {
        assert_eq!(
            staged(Path::new("/usr/local"), Path::new("/tmp/stage")),
            PathBuf::from("/tmp/stage/usr/local")
        );
        assert_eq!(
            staged(Path::new("/usr/local"), Path::new("")),
            PathBuf::from("/usr/local")
        );
    }
}
