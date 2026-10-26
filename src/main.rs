mod cli;
mod commands;
mod config;
mod naming;
mod paths;
mod registry;
mod systemd;
mod table;
mod tui;
mod unit;

use clap::Parser;

fn main() {
    // Rust ignores SIGPIPE by default, which turns `appctl status | head` into a panic on
    // EPIPE. Restore the usual Unix behaviour: die quietly when the reader goes away.
    // SAFETY: setting a signal disposition to the default has no preconditions.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };

    let cli = cli::Cli::parse();
    match commands::run(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("appctl: {err:#}");
            std::process::exit(1);
        }
    }
}
