# mls — Media LS

![Crates.io](https://img.shields.io/crates/d/media-ls)

Terminal-native audio/video/image/document file browser with metadata columns, TUI preview, and structured JSON output.

Think `fd` meets `ffprobe` meets `lazygit`. Documents (PDF, DOCX, XLSX, etc.) are probed natively — no external tools needed.

**Dual-mode**: interactive TUI when you're at the terminal, streaming JSON when piped — one tool for humans and scripts.

```
mls ~/Videos          # TUI browser
mls ~/Videos | jq .   # streaming NDJSON
```

## Install

### Homebrew (macOS)

```bash
brew install thepushkarp/tap/mls
```

The Homebrew formula installs `ffmpeg` and `mpv`, so probing and playback work
after a fresh Brew install. Install `trash` separately if you want safe delete
in triage mode:

```bash
brew install trash
```

### Cargo

```bash
cargo install media-ls
```

`cargo install` only installs the `mls` binary. Install runtime tools
separately for probe/playback support:

```bash
brew install ffmpeg mpv

# Optional: safe delete in triage mode
brew install trash
```

### Build from source

```bash
git clone https://github.com/thepushkarp/mls.git
cd mls
cargo build --release   # requires Rust 1.85+
cp target/release/mls ~/.local/bin/  # or anywhere on PATH
```

For source builds, install the same runtime tools as the Cargo route:

```bash
brew install ffmpeg mpv

# Optional: safe delete in triage mode
brew install trash
```

| Dependency | Required | Purpose |
|-----------|----------|---------|
| `ffprobe` (via ffmpeg) | Yes | Metadata extraction (audio/video/image) |
| `ffmpeg` | Yes | Thumbnail generation |
| `mpv` | No | Playback (warned if missing) |
| `trash` | No | Safe delete in triage mode |

Document metadata (PDF, DOCX, XLSX, PPTX, ODF, OLE2, CSV, TXT, etc.) is extracted using pure Rust — no external tools required.

## Usage

### Browse (default)

```bash
mls                           # current directory, TUI
mls ~/Videos ~/Music          # multiple paths
mls --max-depth 3 ~/Media     # limit recursion
mls --threads 8 ~/Videos      # limit probe concurrency (default: 16)
mls -q ~/Videos               # suppress non-JSON logs
```

Output mode is auto-detected:
- **TTY** → TUI
- **Piped** → NDJSON

Force a mode with `--tui`, `--json`, or `--ndjson`.

### Structured output

```bash
# Single JSON document
mls --json ~/Videos

# Streaming NDJSON (one record per line, as files are probed)
mls --ndjson ~/Videos

# Pipe to jq
mls ~/Videos | jq '.entry.media.kind'

# Filter + sort + limit
mls --json --filter 'duration_ms > 60000' --sort duration_ms:desc --limit 10 .
```

### Inspect a file

```bash
mls info movie.mp4
mls info *.mp4                # multiple files
```

Outputs detailed JSON metadata for each file.

### Play

```bash
mls play video.mp4            # video playback via mpv
mls play song.flac            # audio-only (auto-detected)
```

### Triage

Interactive keep/delete workflow for batch culling:

```bash
mls triage ~/Downloads
```

Press `y` to keep, `n` to delete (moves to Trash via `trash`), `u` to undo.

## TUI Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `l` / `→` / `Enter` | Enter directory or open file |
| `h` / `←` / `Backspace` | Go to parent directory |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `Ctrl-d` | Page down (half screen) |
| `Ctrl-u` | Page up (half screen) |

### Features

| Key | Action |
|-----|--------|
| `/` | Filter (fuzzy by default, prefix `=` for structured expressions) |
| `s` | Cycle sort key (name → size → date → duration → resolution → codec → bitrate → pages) |
| `S` | Reverse sort direction |
| `i` | Toggle metadata panel |
| `1`–`5` | Kind filter: All / Video / Audio / Image / Document |
| `Space` | Mark/unmark file |
| `?` | Help overlay |

### Playback

| Key | Action |
|-----|--------|
| `p` | Play/pause current file via mpv |
| `P` | Stop playback |
| `]` | Seek forward 10s |
| `[` | Seek backward 10s |

### Triage mode

| Key | Action |
|-----|--------|
| `t` | Enter triage mode |
| `y` | Keep current file |
| `n` | Delete (move to Trash) |
| `m` | Move to directory (text input) |
| `u` | Undo last action |
| `h` / `l` | Navigate without triaging |
| `p` | Play current file |
| `q` / `Esc` | Exit triage, show summary |

### Quit

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Ctrl-c` | Quit |

## Filter expressions

In the TUI, `/` opens fuzzy name search by default. Prefix with `=` for structured field expressions. On the CLI, `--filter` always uses structured expressions.

Filter media files by metadata fields using a simple expression language:

```bash
mls --filter 'duration_ms > 60000'                    # longer than 1 minute
mls --filter 'media.video.width >= 1920'               # 1080p+
mls --filter 'media.audio.codec.name == "aac"'         # AAC audio
mls --filter 'media.kind == "av" && duration_ms > 300000'  # video over 5 min
mls --filter 'extension == "mp4" || extension == "mkv"'    # specific formats
mls --filter 'pages > 100'                             # documents over 100 pages
mls --filter 'media.tags.artist == "Radiohead"'        # by artist tag
```

**Operators**: `==` `!=` `>` `>=` `<` `<=`

**Combinators**: `&&` (and), `||` (or), `!` (not), `()` (grouping)

**Values**: numbers (`60000`, `1920.0`), quoted strings (`"aac"`, `'mp4'`), bare identifiers

**Field paths** (dot-separated, resolved against the `MediaEntry` JSON schema):
- `path`, `file_name`, `extension`
- `media.kind` (`"video"`, `"audio"`, `"av"`, `"image"`, `"document"`)
- `media.duration_ms`, `media.overall_bitrate_bps`
- `media.container.format_name`, `media.container.format_primary`
- `media.video.width`, `media.video.height`, `media.video.codec.name`, `media.video.codec.profile`, `media.video.pixel_format`, `media.video.bitrate_bps`
- `media.audio.codec.name`, `media.audio.codec.profile`, `media.audio.channels`, `media.audio.channel_layout`, `media.audio.sample_rate_hz`, `media.audio.bitrate_bps`
- `media.tags.title`, `media.tags.artist`, `media.tags.album`, `media.tags.date`, `media.tags.genre`
- `media.exif.camera_make`, `media.exif.camera_model`, `media.exif.lens_model`, `media.exif.focal_length_mm`, `media.exif.aperture`, `media.exif.exposure_time`, `media.exif.iso`, `media.exif.date_taken`, `media.exif.gps_latitude`, `media.exif.gps_longitude`, `media.exif.orientation`
- `media.doc.format`, `media.doc.page_count`, `media.doc.word_count`, `media.doc.line_count`, `media.doc.sheet_count`, `media.doc.author`, `media.doc.title`, `media.doc.subject`, `media.doc.creator_app`, `media.doc.creation_date`, `media.doc.modification_date`
- `fs.size_bytes`, `fs.modified_at`, `fs.created_at`

**Shorthand aliases**: `duration_ms`, `size_bytes`, `kind`, `width`, `height`, `bitrate` / `bitrate_bps`, `camera` (→ `media.exif.camera_model`), `iso` (→ `media.exif.iso`), `pages` (→ `media.doc.page_count`), `author` (→ `media.doc.author`)

## Sort keys

```bash
mls --sort name              # ascending by default
mls --sort duration_ms:desc  # explicit direction
mls --sort size:asc
```

| Key | Aliases | Description |
|-----|---------|-------------|
| `name` | — | File name |
| `path` | — | Full path |
| `size` | — | File size |
| `modified` | `date` | Modification time |
| `duration_ms` | `duration` | Duration in ms |
| `resolution` | — | Pixel area (width x height) |
| `codec` | — | Video codec (falls back to audio) |
| `bitrate` | — | Overall bitrate |
| `pages` | `page_count` | Document page count |

## JSON schema

Version: `0.2.0`

### JSON output (`--json`)

```jsonc
{
  "type": "mls.list",
  "schema_version": "0.2.0",
  "mls_version": "0.0.3",
  "generated_at": "2025-12-01T12:00:00Z",
  "entries": [
    {
      "path": "/Users/me/Videos/clip.mp4",
      "file_name": "clip.mp4",
      "extension": "mp4",
      "media": {
        "kind": "av",
        "container": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "format_primary": "mov" },
        "duration_ms": 125400,
        "overall_bitrate_bps": 5328000,
        "video": {
          "width": 1920, "height": 1080,
          "codec": { "name": "h264", "profile": "High", "level": "4.1" },
          "fps": { "num": 24000, "den": 1001 },
          "pixel_format": "yuv420p",
          "bitrate_bps": 5200000
        },
        "audio": {
          "codec": { "name": "aac", "profile": "LC" },
          "channels": 2, "channel_layout": "stereo", "sample_rate_hz": 48000,
          "bitrate_bps": 128000
        },
        "tags": { "title": "My Clip", "artist": null, "album": null, "date": null, "genre": null },
        "exif": null,  // populated for images
        "doc": null     // populated for documents
      },
      "fs": { "size_bytes": 81920000, "modified_at": "2025-12-01T10:30:00Z", "created_at": "2025-11-30T08:00:00Z" },
      "probe": { "backend": "ffprobe", "took_ms": 42 }
    }
  ],
  "summary": {
    "entries_total": 1, "entries_emitted": 1,
    "probe_ok": 1, "probe_error": 0
  },
  "errors": []
}
```

### NDJSON output (`--ndjson` / piped)

One JSON object per line, streamed as files are probed:

```
{"type":"mls.header","schema_version":"0.2.0","mls_version":"0.0.3","generated_at":"2025-12-01T12:00:00Z"}
{"type":"mls.entry","entry":{...}}
{"type":"mls.footer","summary":{...},"errors":[]}
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Generic/unexpected error |
| `2` | CLI usage error (bad flag, invalid filter/sort) |
| `4` | Missing dependency (`ffprobe`/`ffmpeg`, or `mpv` for playback) |

## Supported formats

**Video**: mp4, mkv, mov, avi, wmv, flv, webm, m4v, mpg, mpeg, ts, mts, m2ts, vob, ogv, 3gp, 3g2

**Audio**: mp3, flac, wav, aac, ogg, opus, wma, m4a, aiff, aif, alac, ape, dsf, dff, wv, mka

**Image**: jpg, jpeg, png, webp, gif, bmp, tiff, tif

**Document**: pdf, docx, doc, odt, xlsx, xls, ods, pptx, ppt, odp, csv, tsv, txt, md

## Development

```bash
cargo build                   # debug build
cargo test
cargo clippy --all-targets --all-features -- -D warnings  # zero warnings
cargo fmt --check             # formatting
```

Release build (LTO + strip):

```bash
cargo build --release
```


## License

MIT
