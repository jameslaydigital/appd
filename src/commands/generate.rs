//! Machine-readable extras: man page and shell completion scripts, generated from the same
//! clap definitions as the CLI so they cannot drift.

use std::io::{self, Write};

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;

pub fn completions_text(shell: Shell) -> Vec<u8> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut command, name, &mut out);
    out
}

pub fn man_roff() -> Result<Vec<u8>> {
    let command = Cli::command();
    let mut out = Vec::new();
    clap_mangen::Man::new(command).render(&mut out)?;
    Ok(out)
}

pub fn completions(shell: Shell) -> Result<i32> {
    let mut out = io::stdout().lock();
    out.write_all(&completions_text(shell))?;
    out.flush()?;
    Ok(0)
}

pub fn man() -> Result<i32> {
    let mut out = io::stdout().lock();
    out.write_all(&man_roff()?)?;
    out.flush()?;
    Ok(0)
}
