//! `appd status`: one line per app, or a detail view for one app.

use anyhow::Result;

use crate::table::Table;
use crate::{naming, registry, systemd};

pub fn run(apps: &[String]) -> Result<i32> {    let registry = registry::load()?;
    let names = registry.select_names(apps)?;

    let single = names.len() == 1
        && !apps.is_empty()
        && !apps.iter().any(|app| app == "all");
    if single {
        let name = &names[0];
        if let Some(app) = registry.get(name) {
            return detail(app);
        }
        return detail_opaque(name, registry.get_opaque(name));
    }

    let mut table = Table::new(&["APP", "UNIT", "STATE", "SINCE", "PID", "NOTES"]);

    for name in &names {
        let unit = naming::unit_name(name);
        let (state, since, pid, notes) = probe(&unit);

        // A config we cannot read does not stop us reporting the unit's state.
        let notes = if registry.get_opaque(name).is_some() {
            if notes.is_empty() {
                "config unreadable, run as root".to_string()
            } else {
                format!("{notes}; config unreadable")
            }
        } else {
            notes
        };

        table.push(vec![name.clone(), unit, state, since, pid, notes]);
    }

    // Configs that failed to parse still deserve a row.
    for failure in &registry.failures {
        let name = failure
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("?")
            .to_string();
        table.push(vec![
            name.clone(),
            naming::unit_name(&name),
            "?".to_string(),
            "-".to_string(),
            "-".to_string(),
            "invalid config, see appd doctor".to_string(),
        ]);
    }

    if table.is_empty() {
        println!("no apps configured in {}", registry.dir.display());
        return Ok(0);
    }

    print!("{}", table.render());
    Ok(0)
}

/// Returns `(state, since, pid, notes)`. Unreadable units are reported as such rather than
/// being silently blank.
pub(crate) fn probe(unit: &str) -> (String, String, String, String) {
    let props = match systemd::unit_properties(unit) {
        Ok(props) => props,
        Err(err) => {
            let text = format!("{err:#}");
            let notes = if text.to_ascii_lowercase().contains("permission denied") {
                "permission denied".to_string()
            } else {
                first_line(&text)
            };
            return ("?".to_string(), "-".to_string(), "-".to_string(), notes);
        }
    };

    let load_state = props.get("LoadState").map(String::as_str).unwrap_or("");
    let active_state = props.get("ActiveState").map(String::as_str).unwrap_or("unknown");
    let sub_state = props.get("SubState").map(String::as_str).unwrap_or("");
    let result = props.get("Result").map(String::as_str).unwrap_or("success");

    if load_state == "not-found" {
        let notes = if active_state == "inactive" {
            "not installed, run appd apply"
        } else {
            "not found"
        };
        return (
            "not-installed".to_string(),
            "-".to_string(),
            "-".to_string(),
            notes.to_string(),
        );
    }

    let state = display_state(active_state, sub_state);

    let pid = props
        .get("MainPID")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid != 0)
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "-".to_string());

    let since = short_time(props.get("ExecMainStartTimestamp").map(String::as_str).unwrap_or(""));

    let notes = if state == "failed" && result != "success" {
        format!("result {result}")
    } else {
        String::new()
    };

    (state, since, pid, notes)
}

/// systemd's own vocabulary is accurate but noisy (`active` for a running service). Map it to
/// words people use, while keeping unusual states visible as-is.
fn display_state(active: &str, sub: &str) -> String {
    match (active, sub) {
        ("active", "running") => "running".to_string(),
        ("active", "exited") => "exited".to_string(),
        ("active", "waiting") => "waiting".to_string(),
        ("activating", _) => "starting".to_string(),
        ("deactivating", _) => "stopping".to_string(),
        ("inactive", _) => "stopped".to_string(),
        ("failed", _) | (_, "failed") => "failed".to_string(),
        ("active", _) => "active".to_string(),
        (other, _) => other.to_string(),
    }
}

/// `systemctl` reports e.g. `Mon 2026-10-26 11:23:45 UTC`; keep the date and minute.
fn short_time(raw: &str) -> String {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() >= 3 && parts[1].contains('-') {
        let time = parts[2].split(':').take(2).collect::<Vec<_>>().join(":");
        return format!("{} {}", parts[1], time);
    }
    "-".to_string()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

fn detail(app: &registry::App) -> Result<i32> {
    let unit = naming::unit_name(&app.name);
    println!("APP        {}", app.name);
    println!("UNIT       {unit}");
    println!("CONFIG     {}", app.path.display());
    println!("USER       {}", app.config.service_user(&app.name));
    println!("STATE DIR  {}", naming::state_directory(&app.name));
    println!();

    // `systemctl status` exits non-zero for failed units; the output is the point, not the code.
    let _ = systemd::systemctl(&["status", &unit, "--no-pager", "--full"]);
    Ok(0)
}

/// The config is unreadable, so show what the unit itself can tell us.
fn detail_opaque(name: &str, opaque: Option<&registry::Opaque>) -> Result<i32> {
    let unit = naming::unit_name(name);
    println!("APP        {name}");
    println!("UNIT       {unit}");
    if let Some(opaque) = opaque {
        println!(
            "CONFIG     {} (unreadable: {})",
            opaque.path.display(),
            opaque.reason
        );
    }
    println!();
    println!("Config contents are root-only, so USER and STATE DIR need a privileged run.");
    println!();

    let _ = systemd::systemctl(&["status", &unit, "--no-pager", "--full"]);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_systemctl_timestamps() {
        assert_eq!(
            short_time("Mon 2026-10-26 11:23:45 UTC"),
            "2026-10-26 11:23"
        );
        assert_eq!(short_time(""), "-");
        assert_eq!(short_time("n/a"), "-");
    }

    #[test]
    fn maps_systemd_states_to_plain_words() {
        assert_eq!(display_state("active", "running"), "running");
        assert_eq!(display_state("active", "exited"), "exited");
        assert_eq!(display_state("inactive", "dead"), "stopped");
        assert_eq!(display_state("activating", "start"), "starting");
        assert_eq!(display_state("failed", "failed"), "failed");
        assert_eq!(display_state("reloading", "reload"), "reloading");
    }
}
