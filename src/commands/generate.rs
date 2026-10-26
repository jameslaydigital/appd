//! Machine-readable extras: man page and shell completion scripts, generated from the same
//! clap definitions as the CLI so they cannot drift.

use std::io::{self, Write};

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;

pub fn completions(shell: Shell) -> Result<i32> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    let mut out = io::stdout().lock();
    clap_complete::generate(shell, &mut command, name, &mut out);
    out.flush()?;
    Ok(0)
}

pub fn man() -> Result<i32> {
    let command = Cli::command();
    let mut out = io::stdout().lock();
    clap_mangen::Man::new(command).render(&mut out)?;
    out.flush()?;
    Ok(0)
}
