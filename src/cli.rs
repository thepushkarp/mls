/// CLI argument parsing via clap derive.
///
/// Supports dual-mode operation: TUI when interactive, JSON/NDJSON when piped.
/// Subcommands: list (default), info, play, triage.
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "mls",
    about = "Media LS — terminal-native audio/video browser",
    version,
    after_help = "Examples:\n  \
        mls ~/Videos                    # TUI browser\n  \
        mls --json ~/Videos             # JSON output\n  \
        mls --json --filter 'duration_ms > 60000' .\n  \
        mls info movie.mp4              # Detailed metadata\n  \
        mls triage ~/Downloads          # Triage mode"
)]
#[expect(clippy::struct_excessive_bools)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Paths to scan (defaults to current directory).
    pub paths: Vec<PathBuf>,

    /// Force TUI mode even when piped.
    #[arg(long, global = true)]
    pub tui: bool,

    /// Output as a single JSON document.
    #[arg(long, global = true, conflicts_with = "ndjson")]
    pub json: bool,

    /// Output as newline-delimited JSON (streaming).
    #[arg(long, global = true, conflicts_with = "json")]
    pub ndjson: bool,

    /// Filter by metadata expression.
    #[arg(long, global = true)]
    pub filter: Option<String>,

    /// Sort by key (e.g., "`duration_ms:desc`", "name:asc").
    #[arg(long, global = true)]
    pub sort: Option<String>,

    /// Limit number of results.
    #[arg(long, global = true)]
    pub limit: Option<usize>,

    /// Maximum directory walk depth.
    #[arg(long, global = true)]
    pub max_depth: Option<usize>,

    /// Metadata probe concurrency.
    #[arg(long, global = true, default_value = "16")]
    pub threads: usize,

    /// Per-file probe timeout in milliseconds.
    #[arg(long, global = true, default_value = "5000")]
    pub timeout_ms: u64,

    /// Suppress non-JSON logs.
    #[arg(long, short, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Browse/list media files (default when no subcommand).
    #[command(alias = "ls")]
    List {
        /// Paths to scan.
        #[arg()]
        paths: Vec<PathBuf>,
    },
    /// Show detailed metadata for specific file(s).
    Info {
        /// Files to inspect.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Play a media file via mpv.
    Play {
        /// File to play.
        #[arg(required = true)]
        file: PathBuf,
    },
    /// Enter interactive triage mode.
    Triage {
        /// Paths to scan for triage.
        #[arg()]
        paths: Vec<PathBuf>,
    },
}

/// Resolved output mode after considering flags, TTY, and subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
    Tui,
    Json,
    Ndjson,
}

impl Cli {
    /// Resolve the output mode based on flags and TTY detection.
    ///
    /// Priority: --tui > --json > --ndjson > (tty → TUI, pipe → NDJSON)
    #[must_use]
    pub fn output_mode(&self) -> OutputMode {
        if self.tui {
            return OutputMode::Tui;
        }
        if self.json {
            return OutputMode::Json;
        }
        if self.ndjson {
            return OutputMode::Ndjson;
        }
        if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            OutputMode::Tui
        } else {
            OutputMode::Ndjson
        }
    }

    /// Resolve paths from either subcommand args or top-level args.
    #[must_use]
    pub fn resolved_paths(&self) -> Vec<PathBuf> {
        let paths = match &self.command {
            Some(Command::List { paths } | Command::Triage { paths }) => paths.clone(),
            Some(Command::Info { files }) => files.clone(),
            Some(Command::Play { file }) => vec![file.clone()],
            None => self.paths.clone(),
        };
        if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths
        }
    }
}
