/// mls — Media LS: terminal-native audio/video browser.
///
/// Dual-mode tool: interactive TUI for humans, structured JSON/NDJSON
/// output for scripts and AI agents. Think `fd` meets `ffprobe` meets `lazygit`.
mod cli;
mod deps;
mod filter;
mod output;
mod playback;
mod probe;
mod scan;
mod sort;
mod thumbnail;
mod tui;
mod types;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command, OutputMode};
use std::io::Write;

/// Agent-friendly exit codes per PRD spec.
mod exit_code {
    /// CLI usage error (bad flag, invalid filter/sort).
    pub const USAGE: i32 = 2;
    // exit code 3 (walk/path error) reserved per PRD — add when walk errors are wired
    /// Backend/dependency failure (ffprobe not found).
    pub const DEPENDENCY: i32 = 4;
}

/// Error wrapper that carries a specific process exit code.
#[derive(Debug)]
struct ExitCodeError {
    code: i32,
    msg: String,
}

impl std::fmt::Display for ExitCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ExitCodeError {}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        let _ = writeln!(std::io::stderr(), "mls: {e:#}");
        let code = e.downcast_ref::<ExitCodeError>().map_or(1, |ec| ec.code);
        std::process::exit(code);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing (unless quiet)
    if !cli.quiet {
        tracing_subscriber::fmt()
            .with_env_filter("mls=info")
            .with_writer(std::io::stderr)
            .init();
    }

    // Check external dependencies
    let dep_check = deps::DepCheck::run();
    if !dep_check.hard_deps_ok() {
        if let Some(msg) = dep_check.missing_message() {
            let _ = writeln!(std::io::stderr(), "{msg}");
        }
        return Err(ExitCodeError {
            code: exit_code::DEPENDENCY,
            msg: "required dependencies missing".into(),
        }
        .into());
    }

    // Warn about optional deps (mpv)
    if dep_check.mpv.is_none() && !cli.quiet {
        let _ = writeln!(
            std::io::stderr(),
            "Warning: mpv not found. Playback features disabled. Install: brew install mpv"
        );
    }

    // Route to subcommand
    match &cli.command {
        Some(Command::Info { files }) => run_info(&cli, files).await,
        Some(Command::Play { file }) => run_play(file).await,
        Some(Command::Triage { paths }) => {
            let paths = if paths.is_empty() {
                vec![std::path::PathBuf::from(".")]
            } else {
                paths.clone()
            };
            run_tui(&cli, &paths).await
        }
        Some(Command::List { paths }) => {
            let paths = if paths.is_empty() {
                vec![std::path::PathBuf::from(".")]
            } else {
                paths.clone()
            };
            run_list(&cli, &paths).await
        }
        None => {
            let paths = cli.resolved_paths();
            run_list(&cli, &paths).await
        }
    }
}

async fn run_list(cli: &Cli, paths: &[std::path::PathBuf]) -> Result<()> {
    match cli.output_mode() {
        OutputMode::Tui => run_tui(cli, paths).await,
        OutputMode::Json => run_json(cli, paths).await,
        OutputMode::Ndjson => run_ndjson(cli, paths).await,
    }
}

async fn run_tui(cli: &Cli, paths: &[std::path::PathBuf]) -> Result<()> {
    tui::run(paths, cli.max_depth, cli.threads, cli.timeout_ms).await
}

async fn run_json(cli: &Cli, paths: &[std::path::PathBuf]) -> Result<()> {
    let (mut entries, errors) =
        scan::scan_all(paths, cli.max_depth, cli.threads, cli.timeout_ms).await?;

    // Apply filter
    if let Some(ref filter_expr) = cli.filter {
        let f = filter::Filter::parse(filter_expr).map_err(|e| ExitCodeError {
            code: exit_code::USAGE,
            msg: format!("invalid filter: {e}"),
        })?;
        entries.retain(|entry| f.matches(entry).unwrap_or(false));
    }

    // Apply sort
    if let Some(ref sort_spec) = cli.sort {
        if let Some((key, dir)) = sort::parse_sort_spec(sort_spec) {
            sort::sort_entries(&mut entries, key, dir);
        } else {
            return Err(ExitCodeError {
                code: exit_code::USAGE,
                msg: format!("unknown sort key: {sort_spec}"),
            }
            .into());
        }
    }

    // Apply limit
    if let Some(limit) = cli.limit {
        entries.truncate(limit);
    }

    let mut stdout = std::io::stdout().lock();
    output::write_json(&mut stdout, &entries, &errors)?;
    writeln!(stdout)?;
    Ok(())
}

async fn run_ndjson(cli: &Cli, paths: &[std::path::PathBuf]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    output::write_ndjson_header(&mut stdout)?;

    let files = scan::discover_media_files(paths, cli.max_depth);
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);

    let concurrency = cli.threads;
    let timeout_ms = cli.timeout_ms;
    let filter_expr = cli.filter.clone();

    let filter = filter_expr
        .as_ref()
        .map(|expr| filter::Filter::parse(expr))
        .transpose()
        .map_err(|e| ExitCodeError {
            code: exit_code::USAGE,
            msg: format!("invalid filter: {e}"),
        })?;

    tokio::spawn(async move {
        scan::probe_files(files, concurrency, timeout_ms, tx).await;
    });

    let mut summary = types::ListSummary::default();
    let mut errors = Vec::new();

    while let Some(result) = rx.recv().await {
        match result {
            scan::ScanResult::Entry(entry) => {
                summary.entries_total += 1;
                summary.probe_ok += 1;

                let passes = filter
                    .as_ref()
                    .is_none_or(|f| f.matches(&entry).unwrap_or(false));

                if passes {
                    summary.entries_emitted += 1;
                    output::write_ndjson_entry(&mut stdout, &entry)?;
                }
            }
            scan::ScanResult::Error(e) => {
                summary.entries_total += 1;
                summary.probe_error += 1;
                errors.push(e);
            }
        }
    }

    output::write_ndjson_footer(&mut stdout, &summary, &errors)?;
    Ok(())
}

async fn run_info(cli: &Cli, files: &[std::path::PathBuf]) -> Result<()> {
    let mut entries = Vec::new();
    for file in files {
        match probe::probe_file(file, cli.timeout_ms).await {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                tracing::error!(path = %file.display(), "error probing file: {e}");
            }
        }
    }

    if entries.is_empty() {
        anyhow::bail!("no files could be probed");
    }

    let mut stdout = std::io::stdout().lock();
    output::write_info_json(&mut stdout, &entries)?;
    writeln!(stdout)?;
    Ok(())
}

async fn run_play(file: &std::path::Path) -> Result<()> {
    let mut mpv = playback::MpvController::new();
    let audio_only = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| !types::is_video_extension(ext));

    mpv.play(file, audio_only)
        .await
        .context("failed to start playback")?;

    // Wait for mpv to finish — Ctrl-C is handled by tokio's default signal handler
    // which will drop the MpvController and clean up the subprocess
    loop {
        tokio::select! {
            () = async { let _ = tokio::signal::ctrl_c().await; } => {
                mpv.stop().await;
                break;
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                if !mpv.is_alive() {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_constants_match_prd_spec() {
        assert_eq!(exit_code::USAGE, 2);
        assert_eq!(exit_code::DEPENDENCY, 4);
    }

    #[test]
    fn exit_code_error_displays_message() {
        let err = ExitCodeError {
            code: exit_code::DEPENDENCY,
            msg: "ffprobe not found".into(),
        };
        assert_eq!(err.to_string(), "ffprobe not found");
        assert_eq!(err.code, 4);
    }

    #[test]
    fn exit_code_error_downcast_from_anyhow() {
        let err: anyhow::Error = ExitCodeError {
            code: exit_code::USAGE,
            msg: "bad filter".into(),
        }
        .into();
        let ec = err.downcast_ref::<ExitCodeError>();
        assert!(ec.is_some());
        assert_eq!(ec.map(|e| e.code), Some(2));
    }

    #[test]
    fn generic_anyhow_error_falls_back_to_code_1() {
        let err = anyhow::anyhow!("some generic error");
        let code = err.downcast_ref::<ExitCodeError>().map_or(1, |ec| ec.code);
        assert_eq!(code, 1);
    }
}
