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

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Print error to stderr (not stdout, which may be JSON)
        let _ = writeln!(std::io::stderr(), "mls: {e:#}");
        std::process::exit(1);
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
        anyhow::bail!("required dependencies missing (exit code 4)");
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
    let (mut entries, errors) = scan::scan_all(
        paths,
        cli.max_depth,
        cli.threads,
        cli.timeout_ms,
    )
    .await?;

    // Apply filter
    if let Some(ref filter_expr) = cli.filter {
        let f = filter::Filter::parse(filter_expr).map_err(|e| {
            anyhow::anyhow!("invalid filter: {e}")
        })?;
        entries.retain(|entry| f.matches(entry).unwrap_or(false));
    }

    // Apply sort
    if let Some(ref sort_spec) = cli.sort {
        if let Some((key, dir)) = sort::parse_sort_spec(sort_spec) {
            sort::sort_entries(&mut entries, key, dir);
        } else {
            anyhow::bail!("unknown sort key: {sort_spec}");
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
        .map_err(|e| anyhow::anyhow!("invalid filter: {e}"))?;

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
                let _ = writeln!(
                    std::io::stderr(),
                    "mls: error probing {}: {e}",
                    file.display()
                );
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

    // Wait for mpv to finish
    loop {
        if !mpv.is_alive() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(())
}
