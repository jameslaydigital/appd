//! End-to-end tests through the real binary, with the config and unit directories redirected to
//! temp dirs. This is the suite that proves the CLI behaves, not just the renderer.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new(configs: &[(&str, &str)]) -> Self {
        let fixture = Self::bare();
        fs::create_dir_all(fixture.config_dir()).unwrap();

        for (name, json) in configs {
            fixture.write_config(name, json);
        }
        fixture
    }

    /// A fixture whose config directory does not exist yet.
    fn bare() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("units")).unwrap();
        Self { root }
    }

    fn config_dir(&self) -> PathBuf {
        self.root.path().join("config")
    }

    fn unit_dir(&self) -> PathBuf {
        self.root.path().join("units")
    }

    fn write_config(&self, name: &str, json: &str) {
        fs::write(self.config_dir().join(format!("{name}.json")), json).unwrap();
    }

    fn unit(&self, app: &str) -> PathBuf {
        self.unit_dir().join(format!("appd-{app}.service"))
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_appd"))
            .args(args)
            .env("APPD_CONFIG_DIR", self.config_dir())
            .env("APPD_UNIT_DIR", self.unit_dir())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn apply_is_idempotent_and_writes_hardened_units() {
    let fixture = Fixture::new(&[(
        "demo",
        r#"{ "exec": "/bin/true", "permissions": { "network": true } }"#,
    )]);

    let first = fixture.run(&["apply", "all", "--yes"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("created"), "{}", stdout(&first));

    let path = fixture.unit("demo");
    let unit = fs::read_to_string(&path).unwrap();
    assert!(unit.contains("X-Appd-Managed=1"));
    assert!(unit.contains("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"));
    assert!(unit.contains("NoNewPrivileges=yes"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644,
        "unit files should be world-readable"
    );

    let second = fixture.run(&["apply", "all", "--yes"]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(stdout(&second).contains("unchanged"), "{}", stdout(&second));
    assert!(
        stdout(&second).contains("All installed units already match their configs."),
        "{}",
        stdout(&second)
    );
}

#[test]
fn init_creates_the_config_directory_and_is_idempotent() {
    let fixture = Fixture::bare();
    assert!(!fixture.config_dir().exists());

    let first = fixture.run(&["init"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("Created"), "{}", stdout(&first));
    assert!(fixture.config_dir().is_dir());
    assert_eq!(
        fs::metadata(fixture.config_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "the directory must be listable so non-root status works"
    );

    let second = fixture.run(&["init"]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(
        stdout(&second).contains("already exists"),
        "{}",
        stdout(&second)
    );
}

#[test]
fn init_warns_when_others_cannot_list_the_directory() {
    let fixture = Fixture::bare();
    fs::create_dir_all(fixture.config_dir()).unwrap();
    fs::set_permissions(fixture.config_dir(), fs::Permissions::from_mode(0o750)).unwrap();

    let output = fixture.run(&["init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("chmod 755"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn apply_does_not_create_the_config_directory() {
    let fixture = Fixture::bare();

    let output = fixture.run(&["apply", "all", "--yes"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        !fixture.config_dir().exists(),
        "creating the directory is init's job"
    );
    assert!(stdout(&output).contains("appd init"), "{}", stdout(&output));
}

#[test]
fn doctor_reports_a_missing_config_directory() {
    let fixture = Fixture::bare();
    let output = fixture.run(&["doctor"]);
    let text = stdout(&output);
    assert!(text.contains("config directory"), "{text}");
    assert!(text.contains("does not exist"), "{text}");
    assert!(text.contains("appd init"), "{text}");
}

#[test]
fn install_and_uninstall_into_a_prefix() {
    let fixture = Fixture::bare();
    let prefix = fixture.root.path().join("prefix");
    let prefix_arg = prefix.to_str().unwrap();

    let install = fixture.run(&["install", "--prefix", prefix_arg]);
    assert!(install.status.success(), "{}", stderr(&install));

    let bin = prefix.join("bin/appd");
    let man = prefix.join("share/man/man1/appd.1");
    assert!(bin.is_file(), "binary missing");
    assert!(man.is_file(), "man page missing");
    assert!(
        prefix
            .join("share/bash-completion/completions/appd")
            .is_file(),
        "bash completion missing"
    );
    assert!(fs::read_to_string(&man).unwrap().contains(".TH appd"));

    // The copied binary runs.
    let version = Command::new(&bin).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains("appd "));

    let uninstall = fixture.run(&["install", "--prefix", prefix_arg, "--uninstall"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    assert!(!bin.exists(), "binary should be removed");
    assert!(!man.exists(), "man page should be removed");
}

#[test]
fn one_bad_config_writes_nothing() {
    let fixture = Fixture::new(&[
        ("good", r#"{ "exec": "/bin/true" }"#),
        ("bad", r#"{ "exec": "relative/path" }"#),
    ]);

    let output = fixture.run(&["apply", "all", "--yes"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("nothing applied"),
        "{}",
        stderr(&output)
    );
    assert!(
        !fixture.unit("good").exists(),
        "a failed apply must not write any unit"
    );
}

#[test]
fn diff_reports_drift_and_becomes_clean_after_apply() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());

    let clean = fixture.run(&["diff"]);
    assert!(clean.status.success(), "{}", stderr(&clean));
    assert!(stdout(&clean).contains("in sync"));

    fixture.write_config("demo", r#"{ "exec": "/bin/true", "args": ["--changed"] }"#);

    let drifted = fixture.run(&["diff"]);
    assert_eq!(drifted.status.code(), Some(1), "drift must set a non-zero exit");
    let text = stdout(&drifted);
    assert!(text.contains("-ExecStart=/bin/true\n"), "{text}");
    assert!(text.contains("+ExecStart=/bin/true --changed\n"), "{text}");

    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());
    assert!(fixture.run(&["diff"]).status.success());
}

#[test]
fn doctor_reports_a_missing_exec_and_exits_non_zero() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/nonexistent/demo/bin" }"#)]);

    let output = fixture.run(&["doctor"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("/nonexistent/demo/bin does not exist"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn environment_file_is_written_into_the_unit() {
    let fixture = Fixture::new(&[(
        "demo",
        r#"{ "exec": "/bin/true", "environment_file": "/etc/appd/demo.env" }"#,
    )]);
    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());

    let unit = fs::read_to_string(fixture.unit("demo")).unwrap();
    assert!(unit.contains("EnvironmentFile=/etc/appd/demo.env\n"), "{unit}");
}

#[test]
fn service_verbs_refuse_to_run_unprivileged() {
    let uid = String::from_utf8(
        Command::new("id").arg("-u").output().unwrap().stdout,
    )
    .unwrap();
    if uid.trim() == "0" {
        return; // root: the guard cannot be exercised
    }

    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    let output = fixture.run(&["restart", "demo"]);

    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("needs root"), "{text}");
    assert!(
        text.contains(env!("CARGO_BIN_EXE_appd")),
        "the hint should name the binary's actual path: {text}"
    );
    assert!(
        text.contains(&format!("{} install", env!("CARGO_BIN_EXE_appd"))),
        "a cargo-installed binary should offer self-install: {text}"
    );
}

#[test]
fn remove_deletes_the_unit_and_the_config() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());
    assert!(fixture.unit("demo").exists());

    let output = fixture.run(&["remove", "demo", "--yes"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("demo removed."), "{}", stdout(&output));
    assert!(!fixture.unit("demo").exists(), "unit should be deleted");
    assert!(
        !fixture.config_dir().join("demo.json").exists(),
        "config should be deleted"
    );
}

#[test]
fn remove_requires_confirmation_when_not_interactive() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());

    let output = fixture.run(&["remove", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--yes"), "{}", stderr(&output));
    assert!(fixture.unit("demo").exists(), "nothing should be deleted");
    assert!(fixture.config_dir().join("demo.json").exists());
}

#[test]
fn remove_refuses_to_delete_a_foreign_unit() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    let foreign = fixture.unit("demo");
    fs::write(&foreign, "[Service]\nExecStart=/usr/bin/something-else\n").unwrap();

    let output = fixture.run(&["remove", "demo", "--yes"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("not created by appd"),
        "{}",
        stderr(&output)
    );
    assert!(foreign.exists(), "foreign unit must survive");
    assert!(fixture.config_dir().join("demo.json").exists());
}

#[test]
fn remove_rejects_an_unknown_app() {
    let fixture = Fixture::new(&[("demo", r#"{ "exec": "/bin/true" }"#)]);
    let output = fixture.run(&["remove", "nope", "--yes"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown app"), "{}", stderr(&output));
}

#[test]
fn man_page_and_completions_render() {
    let bin = env!("CARGO_BIN_EXE_appd");

    let man = Command::new(bin).arg("man").output().unwrap();
    assert!(man.status.success());
    let text = String::from_utf8_lossy(&man.stdout);
    assert!(text.contains(".TH appd 1"), "{text}");
    assert!(text.contains("frontend for systemd"), "{text}");

    let bash = Command::new(bin)
        .args(["completions", "bash"])
        .output()
        .unwrap();
    assert!(bash.status.success());
    let text = String::from_utf8_lossy(&bash.stdout);
    assert!(text.contains("_appd()"), "{text}");
    assert!(text.contains("apply"), "{text}");
}

#[test]
fn the_shipped_example_renders_completely() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/hello-world/hello-world.json"
    ))
    .unwrap();
    let fixture = Fixture::new(&[("hello-world", &json)]);
    assert!(fixture.run(&["apply", "all", "--yes"]).status.success());

    let unit = fs::read_to_string(fixture.unit("hello-world")).unwrap();
    for expected in [
        "ExecStart=/srv/hello-world/bin/hello --interval 2",
        "ExecReload=/bin/kill -HUP $MAINPID",
        "Environment=GREETING=\"hello from appd\"",
        "StateDirectory=appd-hello-world",
        "ReadOnlyPaths=/srv/hello-world",
        "WantedBy=multi-user.target",
    ] {
        assert!(unit.contains(expected), "missing `{expected}` in:\n{unit}");
    }
}
