use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "appd",
    version,
    about = "A thin, hardened frontend for systemd",
    long_about = "Manages systemd units generated from JSON configs in /etc/appd/. \
                  Hardening is on by default; configs open only the permissions they need."
)]
pub struct Cli {
    /// Skip the confirmation prompt for operations that affect every app.
    #[arg(long, global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create /etc/appd without installing the binary. `install` also does this.
    Init,

    /// Install appd system-wide: binary, man page, completions, and /etc/appd.
    Install {
        #[arg(long, default_value = "/usr/local", value_name = "DIR")]
        prefix: std::path::PathBuf,

        /// Staging root prepended to the prefix, for packaging.
        #[arg(long, value_name = "DIR")]
        destdir: Option<std::path::PathBuf>,

        /// Remove the installed files instead.
        #[arg(long)]
        uninstall: bool,
    },

    /// Install the appd skill into a project for editor agents.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },

    /// Install a project's .appd.conf into /etc/appd.
    #[command(name = "install-app")]
    InstallApp {
        /// Project directory, or the .appd.conf itself.
        #[arg(default_value = ".", value_name = "DIR")]
        path: std::path::PathBuf,

        /// Override the app name; defaults to the project directory name.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// Run `appd apply` after copying the config.
        #[arg(long)]
        apply: bool,
    },

    /// Render configs into systemd units, install them, and reload systemd.
    Apply {
        /// App names, or `all`. Defaults to all.
        #[arg(value_name = "APP")]
        apps: Vec<String>,

        /// Stop, disable and delete managed units whose config is gone.
        #[arg(long)]
        prune: bool,
    },

    /// Show the current state of apps.
    Status {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },

    /// Show how installed units differ from their configs. Exits 1 when anything drifted.
    Diff {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },

    /// Run a preflight check over configs, units and accounts.
    Doctor,

    /// Uninstall an app: unit, config, provisioned account and state directory.
    Remove {
        #[arg(value_name = "APP")]
        app: String,

        /// Keep the provisioned system account.
        #[arg(long)]
        keep_user: bool,

        /// Keep the state directory.
        #[arg(long)]
        keep_state: bool,
    },

    /// Start an app.
    Start {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },
    /// Stop an app.
    Stop {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },
    /// Restart an app.
    Restart {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },
    /// Send the app's configured reload signal (ExecReload).
    Reload {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },
    /// Enable an app at boot.
    Enable {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },
    /// Disable an app at boot.
    Disable {
        #[arg(value_name = "APP")]
        apps: Vec<String>,
    },

    /// Print an app's journal output.
    Logs(LogArgs),

    /// Print an app's journal output, errors only.
    #[command(name = "error-logs")]
    ErrorLogs(LogArgs),

    /// Print a shell completion script to stdout.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Print the man page (roff) to stdout.
    Man,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    pub app: String,

    /// Follow the journal.
    #[arg(short, long)]
    pub follow: bool,

    /// Number of lines to show.
    #[arg(short = 'n', long, default_value_t = 50, value_name = "COUNT")]
    pub lines: usize,

    /// Show entries since this time, e.g. `1h`, `2026-10-26 09:00`.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Filter by syslog priority, e.g. `err`, `warning`.
    #[arg(short = 'p', long, value_name = "PRIORITY")]
    pub priority: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SkillsAction {
    /// Write .agents/skills/appd/SKILL.md into a project.
    Install {
        /// Project directory. Defaults to the current directory.
        #[arg(default_value = ".", value_name = "DIR")]
        dir: std::path::PathBuf,
    },
}
