//! Discovery and selection of apps from the config directory.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::{naming, paths, unit};

pub struct App {
    pub name: String,
    pub path: PathBuf,
    pub config: Config,
}

/// An app whose config exists but could not be read — normally because the config is `0600`
/// root and we are not root. Read-only commands can still report on it; anything that needs the
/// config must refuse.
pub struct Opaque {
    pub name: String,
    pub path: PathBuf,
    pub reason: String,
}

pub struct Failure {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Default)]
pub struct Registry {
    pub apps: Vec<App>,
    pub opaque: Vec<Opaque>,
    pub failures: Vec<Failure>,
    pub dir: PathBuf,
}

pub fn load() -> Result<Registry> {
    load_from(&paths::config_dir())
}

pub fn load_from(dir: &Path) -> Result<Registry> {
    let mut registry = Registry {
        dir: dir.to_path_buf(),
        ..Registry::default()
    };

    let entries = match fs::read_dir(&registry.dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(registry),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            bail!(
                "cannot read {}: {err}; run as root, or make the directory listable (chmod 755 {})",
                registry.dir.display(),
                registry.dir.display()
            );
        }
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", registry.dir.display()));
        }
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json") && path.is_file())
        .collect();
    files.sort();

    for path in files {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();

        let json = match fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                registry.opaque.push(Opaque {
                    name,
                    path,
                    reason: "permission denied".to_string(),
                });
                continue;
            }
            Err(err) => {
                registry.failures.push(Failure {
                    path,
                    message: format!("cannot read: {err}"),
                });
                continue;
            }
        };

        match Config::parse(&name, &json) {
            Ok(config) => registry.apps.push(App { name, path, config }),
            Err(err) => registry.failures.push(Failure {
                path,
                message: format!("{err:#}"),
            }),
        }
    }

    Ok(registry)
}

impl Registry {
    /// Refuse to proceed while any config is invalid or unreadable, so `apply` is
    /// all-or-nothing.
    pub fn require_sound(&self) -> Result<()> {
        if self.failures.is_empty() && self.opaque.is_empty() {
            return Ok(());
        }

        let mut message = String::from("configuration is not usable, nothing applied:\n");
        for failure in &self.failures {
            message.push_str(&format!(
                "  {}: {}\n",
                failure.path.display(),
                failure.message
            ));
        }
        for opaque in &self.opaque {
            message.push_str(&format!(
                "  {}: cannot read config ({}); run as root\n",
                opaque.path.display(),
                opaque.reason
            ));
        }
        Err(anyhow::anyhow!(message.trim_end().to_string()))
    }

    pub fn get(&self, name: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.name == name)
    }

    pub fn get_opaque(&self, name: &str) -> Option<&Opaque> {
        self.opaque.iter().find(|entry| entry.name == name)
    }

    /// True if the name belongs to a known app, readable or not.
    pub fn knows(&self, name: &str) -> bool {
        self.get(name).is_some() || self.get_opaque(name).is_some()
    }

    /// Apps whose configs we can actually read. Empty or `all` selects everything readable;
    /// naming an unreadable app is an error rather than a silent skip.
    pub fn select(&self, names: &[String]) -> Result<Vec<&App>> {
        if names.is_empty() || names.iter().any(|name| name == "all") {
            return Ok(self.apps.iter().collect());
        }

        let mut selected = Vec::new();
        let mut unknown = Vec::new();
        for name in names {
            if let Some(app) = self.get(name) {
                selected.push(app);
            } else if let Some(opaque) = self.get_opaque(name) {
                bail!(
                    "cannot use `{}`: {} is unreadable ({}); run as root",
                    opaque.name,
                    opaque.path.display(),
                    opaque.reason
                );
            } else {
                unknown.push(name.as_str());
            }
        }

        if !unknown.is_empty() {
            bail!(
                "unknown app(s): {}{}",
                unknown.join(", "),
                self.known_suffix()
            );
        }
        Ok(selected)
    }

    /// Names for read-only commands, which do not need the config contents.
    pub fn select_names(&self, names: &[String]) -> Result<Vec<String>> {
        if names.is_empty() || names.iter().any(|name| name == "all") {
            let mut all: Vec<String> = self
                .apps
                .iter()
                .map(|app| app.name.clone())
                .chain(self.opaque.iter().map(|entry| entry.name.clone()))
                .collect();
            all.sort();
            return Ok(all);
        }

        let mut selected = Vec::new();
        let mut unknown = Vec::new();
        for name in names {
            if self.knows(name) {
                selected.push(name.clone());
            } else {
                unknown.push(name.as_str());
            }
        }
        if !unknown.is_empty() {
            bail!(
                "unknown app(s): {}{}",
                unknown.join(", "),
                self.known_suffix()
            );
        }
        Ok(selected)
    }

    fn known_suffix(&self) -> String {
        let mut known: Vec<&str> = self
            .apps
            .iter()
            .map(|app| app.name.as_str())
            .chain(self.opaque.iter().map(|entry| entry.name.as_str()))
            .collect();
        known.sort();
        if known.is_empty() {
            String::new()
        } else {
            format!(" (known: {})", known.join(", "))
        }
    }

    /// Units on disk that carry the appctl marker but whose app name we do not know. These are
    /// left behind when a config is renamed or deleted.
    pub fn orphaned_units(&self) -> Result<Vec<(String, PathBuf)>> {
        self.orphaned_units_in(&paths::unit_dir())
    }

    pub fn orphaned_units_in(&self, dir: &Path) -> Result<Vec<(String, PathBuf)>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
        };

        let mut orphans = Vec::new();
        for path in entries.filter_map(|entry| entry.ok()).map(|entry| entry.path()) {
            if path.extension().is_none_or(|ext| ext != "service") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Some(app) = naming::app_from_unit_file(stem) else {
                continue;
            };
            if self.knows(app) {
                continue;
            }
            let is_ours = fs::read_to_string(&path)
                .map(|contents| contents.contains(unit::MARKER))
                .unwrap_or(false);
            if is_ours {
                orphans.push((app.to_string(), path));
            }
        }
        orphans.sort();
        Ok(orphans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn loads_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("good.json"),
            r#"{ "exec": "/srv/good/bin/good" }"#,
        )
        .unwrap();
        fs::write(dir.path().join("bad.json"), r#"{ "exec": "relative" }"#).unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

        let registry = load_from(dir.path()).unwrap();

        assert_eq!(registry.apps.len(), 1);
        assert_eq!(registry.failures.len(), 1);
        assert!(registry.require_sound().is_err());
        assert_eq!(registry.select(&["good".to_string()]).unwrap().len(), 1);
        assert!(registry.select(&["nope".to_string()]).is_err());
    }

    #[test]
    fn unreadable_config_is_opaque_but_still_listed() {
        // Root can read anything, so the permission trick does not apply.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secretive.json");
        fs::write(&path, r#"{ "exec": "/bin/true" }"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        fs::write(dir.path().join("open.json"), r#"{ "exec": "/bin/true" }"#).unwrap();

        let registry = load_from(dir.path()).unwrap();

        assert_eq!(registry.apps.len(), 1);
        assert_eq!(registry.opaque.len(), 1);
        // Read-only commands can still name it...
        assert_eq!(
            registry.select_names(&["secretive".to_string()]).unwrap(),
            vec!["secretive"]
        );
        // ...but anything needing the config refuses, and `apply` aborts entirely.
        assert!(registry.select(&["secretive".to_string()]).is_err());
        assert!(registry.require_sound().is_err());
    }

    #[test]
    fn finds_orphaned_units_but_ignores_foreign_ones() {
        let configs = tempfile::tempdir().unwrap();
        let units = tempfile::tempdir().unwrap();
        fs::write(
            configs.path().join("kept.json"),
            r#"{ "exec": "/srv/kept/bin/kept" }"#,
        )
        .unwrap();
        fs::write(
            units.path().join("appctl-gone.service"),
            format!("[Service]\n{}", unit::MARKER),
        )
        .unwrap();
        fs::write(units.path().join("appctl-kept.service"), "[Service]\n").unwrap();
        fs::write(units.path().join("unrelated.service"), "[Service]\n").unwrap();
        fs::write(
            units.path().join("appctl-foreign.service"),
            "[Service]\n# not ours\n",
        )
        .unwrap();

        let registry = load_from(configs.path()).unwrap();
        let orphans = registry.orphaned_units_in(units.path()).unwrap();
        let names: Vec<&str> = orphans.iter().map(|(app, _)| app.as_str()).collect();

        assert_eq!(names, vec!["gone"]);
    }
}
