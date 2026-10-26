//! Naming rules. Every unit, account and state directory appctl creates is derived from the
//! app name, and every one of them is namespaced so it cannot collide with a packaged unit.

pub const PREFIX: &str = "appctl-";

/// Name of the systemd unit, e.g. `appctl-goal-tracker.service`.
pub fn unit_name(app: &str) -> String {
    format!("{PREFIX}{app}.service")
}

/// Name of the system account used when the config does not name a user.
pub fn default_user(app: &str) -> String {
    format!("{PREFIX}{app}")
}

/// The `StateDirectory=` value, which systemd resolves under `/var/lib`.
pub fn state_directory_name(app: &str) -> String {
    format!("{PREFIX}{app}")
}

/// Absolute path of the state directory, for display and validation.
pub fn state_directory(app: &str) -> String {
    format!("/var/lib/{PREFIX}{app}")
}

/// Recover the app name from a unit file name, if it is one of ours.
pub fn app_from_unit_file(stem: &str) -> Option<&str> {
    stem.strip_prefix(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_namespaced_names() {
        assert_eq!(unit_name("goal-tracker"), "appctl-goal-tracker.service");
        assert_eq!(default_user("goal-tracker"), "appctl-goal-tracker");
        assert_eq!(state_directory("goal-tracker"), "/var/lib/appctl-goal-tracker");
    }

    #[test]
    fn round_trips_unit_names() {
        assert_eq!(app_from_unit_file("appctl-goal-tracker"), Some("goal-tracker"));
        assert_eq!(app_from_unit_file("nginx"), None);
    }
}
