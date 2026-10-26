pub mod apply;
pub mod diff;
pub mod doctor;
pub mod generate;
pub mod init;
pub mod install;
pub mod lifecycle;
pub mod logs;
pub mod remove;
pub mod status;

use std::io::{IsTerminal, Write};

use anyhow::{bail, Result};

use crate::cli::{Cli, Command};

/// Runs the command and returns a process exit code. `Err` is reserved for failures that stop
/// the command outright.
pub fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Some(Command::Init) => init::run(),
        Some(Command::Install {
            prefix,
            destdir,
            uninstall,
        }) => install::run(&prefix, &destdir.unwrap_or_default(), uninstall),
        Some(Command::Apply { apps, prune }) => apply::run(&apps, prune, cli.yes),
        Some(Command::Status { apps }) => status::run(&apps),
        Some(Command::Diff { apps }) => diff::run(&apps),
        Some(Command::Remove {
            app,
            keep_user,
            keep_state,
        }) => remove::run(&app, keep_user, keep_state, cli.yes),
        Some(Command::Doctor) => doctor::run(),
        Some(Command::Start { apps }) => lifecycle::run(lifecycle::Verb::Start, &apps, cli.yes),
        Some(Command::Stop { apps }) => lifecycle::run(lifecycle::Verb::Stop, &apps, cli.yes),
        Some(Command::Restart { apps }) => lifecycle::run(lifecycle::Verb::Restart, &apps, cli.yes),
        Some(Command::Reload { apps }) => lifecycle::run(lifecycle::Verb::Reload, &apps, cli.yes),
        Some(Command::Enable { apps }) => lifecycle::run(lifecycle::Verb::Enable, &apps, cli.yes),
        Some(Command::Disable { apps }) => lifecycle::run(lifecycle::Verb::Disable, &apps, cli.yes),
        Some(Command::Logs(args)) => logs::run(&args, None),
        Some(Command::ErrorLogs(args)) => logs::run(&args, Some("err")),
        Some(Command::Completions { shell }) => generate::completions(shell),
        Some(Command::Man) => generate::man(),
        None => {
            use std::io::IsTerminal;

            let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            if interactive {
                crate::tui::run()
            } else {
                // Piped or scripted: don't take over the terminal.
                status::run(&[])
            }
        }
    }
}

/// The command to run something as root, including how to make it sudo-accessible when the binary
/// lives outside the system path (`cargo install` puts it in `~/.cargo/bin`, which sudo does not
/// search).
pub fn root_hint(args: &str) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("appd"));
    hint_for(&exe, args)
}

fn hint_for(exe: &std::path::Path, args: &str) -> String {
    let name = exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("appd");

    let sudo = if args.is_empty() {
        format!("sudo {name}")
    } else {
        format!("sudo {name} {args}")
    };

    // The default sudo secure_path on Linux distributions.
    const SYSTEM_DIRS: &[&str] = &[
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    ];
    let reachable = exe
        .parent()
        .is_some_and(|parent| SYSTEM_DIRS.iter().any(|dir| std::path::Path::new(dir) == parent));
    if reachable {
        return sudo;
    }

    let direct = if args.is_empty() {
        format!("sudo {}", exe.display())
    } else {
        format!("sudo {} {args}", exe.display())
    };
    format!(
        "{direct}\n\n  or install it system-wide once, then use `{sudo}`:\n  sudo {} install",
        exe.display()
    )
}

/// Confirm before touching every app. Non-interactive callers must pass `--yes`; they are never
/// assumed to have consented.
pub fn confirm(action: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        bail!("confirmation required: {action}. Pass --yes to run non-interactively");
    }

    eprint!("{action}? [y/N] ");
    std::io::stderr().flush().ok();

    let mut answer = String::new();
    stdin.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

#[cfg(test)]
mod tests {
    use super::hint_for;
    use std::path::Path;

    #[test]
    fn system_paths_use_plain_sudo() {
        assert_eq!(
            hint_for(Path::new("/usr/local/bin/appd"), "apply"),
            "sudo appd apply"
        );
        assert_eq!(hint_for(Path::new("/usr/bin/appd"), ""), "sudo appd");
    }

    #[test]
    fn cargo_installs_get_a_system_install_hint() {
        let hint = hint_for(Path::new("/home/you/.cargo/bin/appd"), "init");
        assert!(
            hint.starts_with("sudo /home/you/.cargo/bin/appd init"),
            "{hint}"
        );
        assert!(
            hint.contains("sudo /home/you/.cargo/bin/appd install"),
            "{hint}"
        );
        assert!(hint.contains("`sudo appd init`"), "{hint}");
    }
}
