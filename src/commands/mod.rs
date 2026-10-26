pub mod apply;
pub mod diff;
pub mod doctor;
pub mod generate;
pub mod init;
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

/// The command a user should run to get root, with the absolute path to this binary:
/// `cargo install` puts it in `~/.cargo/bin`, which sudo's secure_path does not search.
pub fn root_hint() -> String {
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "appd".to_string());
    format!("sudo {exe}")
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
