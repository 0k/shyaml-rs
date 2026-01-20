use clap::{Parser, Subcommand};

/// Verifies expressions against environment variables
#[derive(Parser)]
#[command(author, about, long_about=None, disable_version_flag(true))]
pub struct Args {
    /// force color mode (defaults to check tty)
    #[arg(long)]
    pub color: bool,

    /// force no-color mode (defaults to check tty)
    #[arg(long)]
    pub no_color: bool,

    /// display version and quit
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// prepend time to each log line
    #[arg(long)]
    pub log_time: bool,

    /// Turn general verbose logging
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Configure component wise logging
    #[arg(long, short, action = clap::ArgAction::Append)]
    pub log: Option<Vec<String>>,

    /// quiet path errors
    #[arg(short, long)]
    pub quiet: bool,

    /// Output raw YAML
    #[arg(short = 'y', long)]
    pub yaml: bool,

    #[command(subcommand)]
    pub action: Option<Actions>,
}

#[derive(Subcommand)]
pub enum Actions {
    GetValue {
        /// Get node value from given path

        /// The path to get value of
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Default
        #[clap(name = "DEFAULT")]
        default: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,

        /// Line-buffering
        #[arg(short = 'L')]
        line_buffer: bool,
    },
    GetType {
        /// Get node type from given path

        /// The path to get type of
        #[clap(name = "PATH")]
        path: Option<String>,
    },
    GetLength {
        /// Get node length from given path

        /// The path to get length of
        #[clap(name = "PATH")]
        path: Option<String>,
    },
    Keys {
        /// Get keys of mapping from given path

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },

    #[clap(name = "keys-0")]
    Keys0 {
        /// Get keys of mapping from given path, separated by NUL char

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    Values {
        /// Get values of mapping from given path

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    #[clap(name = "values-0")]
    Values0 {
        /// Get values of mapping from given path, separated by NUL char

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    KeyValues {
        /// Get key and values of mapping from given path

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    #[clap(name = "key-values-0")]
    KeyValues0 {
        /// Get key and values of mapping from given path, separated by NUL char

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    GetValues {
        /// Get key and values of mapping from given path

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    #[clap(name = "get-values-0")]
    GetValues0 {
        /// Get key and values of mapping from given path, separated by NUL char

        /// The path to get keys from
        #[clap(name = "PATH")]
        path: Option<String>,

        /// Output raw YAML
        #[arg(short = 'y', long)]
        yaml: bool,
    },
    Apply {
        /// Apply overlay YAML file(s) to base YAML from stdin

        /// Merge policy for specific paths (PATH=POLICY where POLICY is merge|replace|prepend)
        #[arg(short = 'm', long = "merge-policy", value_delimiter = ',', action = clap::ArgAction::Append)]
        merge_policy: Option<Vec<String>>,

        /// Overlay file(s) to apply
        #[clap(name = "OVERLAY", required = true)]
        overlays: Vec<String>,
    },
    SetValue {
        /// Set a value at a given path in YAML from stdin

        /// The path where to set the value
        #[clap(name = "KEY")]
        key: String,

        /// The value to set
        #[clap(name = "VALUE")]
        value: String,

        /// Interpret value as YAML instead of literal string
        #[arg(short = 'y', long)]
        yaml: bool,
    },
}
