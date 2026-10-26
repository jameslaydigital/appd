//! `appd logs` / `error-logs`: journald behind flags you can remember.

use anyhow::{Result, anyhow};

use crate::cli::LogArgs;
use crate::{naming, registry, systemd};

pub fn run(args: &LogArgs, default_priority: Option<&str>) -> Result<i32> {
    let registry = registry::load()?;
    if !registry.knows(&args.app) {
        return Err(anyhow!("unknown app `{}`", args.app));
    }

    let unit = naming::unit_name(&args.app);
    let mut cmd: Vec<String> = vec![
        "--unit".to_string(),
        unit.clone(),
        "--no-pager".to_string(),
        "--quiet".to_string(),
        "--output=short-iso".to_string(),
    ];

    if args.follow {
        cmd.push("--follow".to_string());
    }

    cmd.push("--lines".to_string());
    cmd.push(args.lines.to_string());

    if let Some(since) = &args.since {
        cmd.push("--since".to_string());
        cmd.push(since.clone());
    }

    let priority = args
        .priority
        .clone()
        .or_else(|| default_priority.map(str::to_string));
    if let Some(priority) = &priority {
        cmd.push("--priority".to_string());
        cmd.push(priority.clone());
    }

    // Following streams; a one-shot read is captured so an empty result can be explained.
    if args.follow {
        systemd::journalctl(&cmd)?;
        return Ok(0);
    }

    let output = systemd::journalctl_output(&cmd)?;
    let text = output.trim_end();
    if !text.is_empty() {
        println!("{text}");
        return Ok(0);
    }

    match priority {
        Some(priority) => {
            eprintln!("no journal entries for {unit} at priority `{priority}`");
            eprintln!(
                "journald records plain stdout and stderr at priority 6 (`info`), so an app has to"
            );
            eprintln!(
                "tag a line with `<3>` to have it logged as `err`. Try `appd logs {}`.",
                args.app
            );
        }
        None => eprintln!("no journal entries for {unit} yet"),
    }

    Ok(0)
}
