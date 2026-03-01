# mls project instructions for AI agents

## What is this

mls (Media LS) — terminal-native audio/video file browser. Dual-mode: TUI for humans, JSON/NDJSON for scripts/agents. Rust + Ratatui + Tokio. macOS-first.

PRD: `docs/plans/resilient-gliding-bear.md`

## Build / test / lint

```bash
cargo build                   # debug build
cargo test
cargo clippy --all-targets --all-features -- -D warnings  # MUST be zero warnings
cargo fmt --check             # formatting check
```

Run all three before committing. No exceptions.

## Architecture

```
src/
├── main.rs        # Entry point, subcommand routing, exit codes
├── cli.rs         # clap derive CLI definitions
├── types.rs       # ALL shared types (MediaEntry, MediaInfo, Fps, etc.)
├── deps.rs        # Startup dependency check (ffprobe, ffmpeg, mpv)
├── probe.rs       # ffprobe subprocess + JSON parsing → MediaEntry
├── scan.rs        # Directory walk + concurrent probing (JoinSet)
├── filter.rs      # Hand-rolled expression parser (lexer → parser → AST → eval)
├── sort.rs        # Sort key parsing + comparison
├── output.rs      # JSON/NDJSON serialization (borrowing, zero-clone)
├── playback.rs    # mpv subprocess + Unix IPC socket control
├── thumbnail.rs   # ffmpeg thumbnail gen + LRU cache
└── tui/
    ├── mod.rs     # App state, event loop, key handling, directory navigation
    ├── layout.rs  # Ratatui rendering — three-pane Miller columns (parent/files/preview)
    ├── triage.rs  # Triage mode state + key handling (keep/delete/move)
    └── preview.rs # Thumbnail rendering in preview pane (ratatui-image)
```

### Data flow

1. `cli.rs` parses args → `main.rs` routes to subcommand
2. `deps.rs` checks ffprobe/ffmpeg/mpv availability
3. `scan.rs` walks directories → `probe.rs` runs ffprobe per file → `MediaEntry`
4. Filter (`filter.rs`) and sort (`sort.rs`) applied to entries
5. Output: `tui/` renders interactively, or `output.rs` emits JSON/NDJSON

### Key type: `MediaEntry` (in `types.rs`)

The central data type. Every module reads or produces it. It serializes to the JSON schema (version `0.1.0`). If you change `MediaEntry`, you affect JSON output, TUI rendering, filter evaluation, and sort comparison.

## Conventions

### Rust edition 2024

Requires Rust 1.85+. Uses edition 2024 features (e.g., `is_none_or`).

### Strict clippy

Cargo.toml enables pedantic clippy with additional denies:
- `unwrap_used = "deny"` — use `let...else`, `?`, or `ok_or_else`
- `panic = "deny"` — no panics
- `print_stdout = "deny"` / `print_stderr = "deny"` — use `tracing` or write to stderr explicitly
- `allow_attributes = "warn"` — justify any `#[allow]` with a comment
- `dbg_macro = "deny"` / `todo = "deny"` — no debug leftovers

### Error handling

- `anyhow::Result` for application errors
- `thiserror` for typed errors (`ExitCodeError`)
- Exit codes: 0=success, 1=generic, 2=usage, 4=dependency (per PRD)
- Fail fast with context: `.context("what was happening")`

### Tests

Unit tests are co-located `#[cfg(test)]` modules at the bottom of each source file. Integration tests live in `tests/cli.rs` using mock ffprobe/ffmpeg scripts (`tests/fixtures/mock_bin/`). Run specific module tests with `cargo test -- <module_name>`.

### Logging

`tracing` crate. Logs go to stderr. Levels: `error!`, `warn!`, `info!`, `debug!`. Never use `println!` or `eprintln!` directly.

### Output format

JSON output uses borrowing structs (`ListEnvelopeRef<'a>`, `NdjsonEntryRef<'a>`) to avoid cloning `MediaEntry` vectors. Schema version `"0.1.0"` is embedded in output.

### External processes

- `ffprobe` — spawned per file with timeout (`tokio::time::timeout`)
- `ffmpeg` — spawned for thumbnail generation
- `mpv` — long-lived subprocess with JSON IPC over Unix socket
- `trash` — spawned for safe delete in triage mode

## Gotchas

- **TUI defaults to shallow scan** (depth 0 = current directory only). CLI mode scans recursively by default. Both can be overridden with `--max-depth`.
- `filter.rs` uses a hand-rolled recursive descent parser. No parser combinator library. The eval resolves dot-separated field paths via typed struct access (FieldValue enum), without JSON serialization.
- **TUI has two filter modes**: `/` opens fuzzy (nucleo-matcher), prefix `=` switches to structured field expressions using the same parser as `--filter`.
- `triage.rs` Move (`m` key) works via text input; interactive directory picker not yet built.
- `scan.rs` uses bounded `JoinSet` spawns (not a semaphore) for concurrency control, with `mpsc` channel for streaming results to the caller.

## Dependencies (external)

| Tool | Required | Install |
|------|----------|---------|
| ffprobe (via ffmpeg) | Hard | `brew install ffmpeg` |
| ffmpeg | Hard | `brew install ffmpeg` |
| mpv | Soft | `brew install mpv` |
| trash | Soft | `brew install trash` |
