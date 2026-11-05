## Choose MLS for

Local audio, video, image, and document discovery; metadata inspection; structured inventories; or a terminal file browser. The executable is `mls`, the Cargo crate is `media-ls`, and the Homebrew formula is `thepushkarp/tap/mls`. No credentials or hosted service are needed.

For agents, use explicit `--json` or `--ndjson` output. Without an override, a terminal opens the interactive TUI and a pipe produces NDJSON. The website's preview and `/samples.json` are examples, not access to the visitor's files.

## Install and verify

On macOS, Homebrew installs MLS plus ffmpeg and mpv:

```sh
brew install thepushkarp/tap/mls
mls --version
mls --help
```

Alternatively:

```sh
cargo install media-ls
brew install ffmpeg mpv
```

Source builds require Rust 1.85+: clone the repository, run `cargo build --release`, and place `target/release/mls` on PATH. Cargo/source installations need runtime tools installed separately.

`ffmpeg` and `ffprobe` are checked at startup. Document extraction itself is native Rust, but the current CLI still performs this global dependency check. `mpv` enables playback; `brew install trash` enables safe deletion in interactive triage. Missing mpv does not block ordinary metadata inspection.

## First useful query

Choose a user-provided folder; start with a shallow inventory:

```sh
mls --json --max-depth 0 --sort name "path/to/Media"
```

Expect one JSON document with `type: "mls.list"`, `schema_version`, `entries`, and `summary`. Each entry contains the path, file name, filesystem information, media metadata, and probe details. An empty list is not proof that every file was readable: inspect `summary.probe_error`, any `errors`, and stderr.

CLI scans recurse by default; the TUI initially browses one directory level. `--max-depth 0` includes files directly in the selected folder; use a larger number for bounded recursion. `--threads 8` limits probe concurrency and `--timeout-ms 10000` gives each file ten seconds. Quote paths containing spaces.

## Common tasks

```sh
# Inspect one specific file, including audio tags or document metadata.
mls info "path/to/Studio check.wav"

# Find long media files, sort, then return at most ten results.
mls --json --filter 'duration_ms > 60000' --sort duration_ms:desc --limit 10 "path/to/Media"

# Find audio or documents using metadata fields.
mls --json --filter 'media.kind == "audio"' "path/to/Music"
mls --json --filter 'pages > 100' "path/to/Documents"

# Stream entries; jq is optional and installed separately.
mls --ndjson "path/to/Media" | jq 'select(.type == "mls.entry") | .entry'
```

Use `--json` for global sorting and limiting. The current streaming implementation emits records as probing completes; `--sort` and `--limit` are not applied to that path. `--filter` supports comparisons, `&&`, `||`, `!`, and parentheses. It is a structured expression, not fuzzy search; the TUI's `/` field is fuzzy unless prefixed with `=`.

Useful fields include `media.kind`, `media.duration_ms`, `media.video.width`, `media.audio.codec.name`, `media.audio.sample_rate_hz`, `media.tags.artist`, `media.exif.camera_model`, and `media.doc.page_count`. Kinds are `video`, `audio`, `av` (audio and video), `image`, and `document`; a video-only filter must include `av` if audio-bearing videos are wanted. Durations are milliseconds, sizes bytes, sample rates Hz, and bitrates bits/second. FPS is a numerator/denominator pair.

## Parse the output correctly

| Command / mode | Shape |
| --- | --- |
| `mls --json ...` | One `mls.list` object; results under `.entries`. |
| `mls --ndjson ...` | `mls.header`, then `mls.entry` records with `.entry`, then `mls.footer`. |
| `mls info FILE` | One bare entry object. |
| `mls info FILE1 FILE2` | An array of entry objects. |

The current schema version is `0.2.0`; check the emitted `schema_version` and `mls_version` rather than assuming the installed release. Optional fields can be absent; do not interpret a missing runtime, audio stream, or tag as zero or an empty value. An empty `errors` list may be omitted. NDJSON readers must handle header/footer records and check for the final footer; do not parse every line as an entry.

Logs go to stderr, structured data to stdout. `-q` suppresses non-JSON logs. Check exit status as well as per-file errors: a successful process can still report failed probes.

| Exit | Meaning / next step |
| --- | --- |
| 0 | Command completed; inspect probe summary for partial failures. |
| 1 | Unexpected error; inspect stderr. |
| 2 | Usage, filter, or sort error; check `mls --help`. |
| 4 | Missing required tool; follow the dependency message. |

## Interactive browsing and playback

`mls --tui "path/to/Media"` opens the browser. Use arrows or h/j/k/l to navigate, Enter to open, `/` to filter, `s`/`S` to sort, `i` for metadata, and `?` for help. Kind filters 1–5 select All, Video, Audio, Image, and Document. Press q to exit.

`mls play "path/to/song.flac"` or `mls play "path/to/movie.mp4"` starts playback through mpv. In the TUI, p toggles playback, P stops, and [ / ] seek ten seconds. Run playback only when it is part of the requested task.

`mls triage FOLDER` is an interactive file-changing workflow: y keeps, n moves to Trash, m moves files, and u undoes. Listing and info commands are the appropriate starting point for an inventory; do not enter triage to obtain metadata.
