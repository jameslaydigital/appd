//! `appd skills install`: put the editor skill into a project so an agent can write configs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};

const SKILL: &str = include_str!("../../skills/appd/SKILL.md");

pub fn install(dir: &Path, claude: bool, codex: bool, opencode: bool) -> Result<i32> {
    let mut targets = Vec::new();
    if claude {
        targets.push((".claude/skills/appd/SKILL.md", "Claude Code"));
    }
    if codex {
        targets.push((".codex/skills/appd/SKILL.md", "Codex"));
    }
    if opencode {
        targets.push((".opencode/skills/appd/SKILL.md", "OpenCode"));
    }
    if targets.is_empty() {
        targets.push((".agents/skills/appd/SKILL.md", "agent-compatible"));
    }

    for (relative, label) in targets {
        let target = dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }

        let updated = target.exists();
        fs::write(&target, SKILL).with_context(|| format!("cannot write {}", target.display()))?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644))?;

        println!(
            "{} the appd skill for {label} at {}",
            if updated { "Updated" } else { "Installed" },
            target.display()
        );
    }

    println!("An agent can now write .appd.conf; install it with `appd install-app .`.");
    Ok(0)
}
