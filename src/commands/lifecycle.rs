//! start / stop / restart / reload / enable / disable, with `all` confirmation.

use anyhow::{Result, bail};

use crate::commands::confirm;
use crate::{naming, registry, systemd};

#[derive(Debug, Clone, Copy)]
pub enum Verb {
    Start,
    Stop,
    Restart,
    Reload,
    Enable,
    Disable,
}

impl Verb {
    fn as_str(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Stop => "stop",
            Verb::Restart => "restart",
            Verb::Reload => "reload",
            Verb::Enable => "enable",
            Verb::Disable => "disable",
        }
    }

    fn is_destructive(self) -> bool {
        matches!(self, Verb::Stop | Verb::Restart | Verb::Reload | Verb::Disable)
    }
}

pub fn run(verb: Verb, apps: &[String], yes: bool) -> Result<i32> {
    if !systemd::is_root() {
        bail!(
            "`{}` drives systemd and requires root (it runs `systemctl {} …`)",
            verb.as_str(),
            verb.as_str()
        );
    }

    let registry = registry::load()?;
    registry.require_sound()?;

    let selected = registry.select(apps)?;
    if selected.is_empty() {
        println!("no apps configured in {}", registry.dir.display());
        return Ok(0);
    }

    let all_requested = apps.is_empty() || apps.iter().any(|app| app == "all");
    if verb.is_destructive()
        && all_requested
        && selected.len() > 1
        && !confirm(&format!("{} all apps", verb.as_str()), yes)?
    {
        println!("aborted");
        return Ok(0);
    }

    let mut failed = Vec::new();

    for app in &selected {
        if matches!(verb, Verb::Reload) && app.config.reload.is_none() {
            eprintln!(
                "{}: no `reload` command configured; use `appd restart {}` instead",
                app.name, app.name
            );
            failed.push(app.name.clone());
            continue;
        }

        let unit = naming::unit_name(&app.name);
        if let Err(err) = systemd::systemctl(&[verb.as_str(), &unit]) {
            eprintln!("{}: {err:#}", app.name);
            failed.push(app.name.clone());
        }
    }

    if failed.is_empty() {
        Ok(0)
    } else {
        eprintln!(
            "{} failed: {}",
            verb.as_str(),
            failed.join(", ")
        );
        Ok(1)
    }
}
