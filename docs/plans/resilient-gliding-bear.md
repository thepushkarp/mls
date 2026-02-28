# mls — Media LS: Product Requirements Document

## Context

**Problem**: Technical users who work with audio and video files daily are forced into constant context-switching: leave the terminal → open Finder/mpv → check codec/duration/bitrate → return to terminal. Existing terminal file managers treat media as an afterthought — metadata is invisible, playback requires leaving the tool, and there's no way for scripts or AI agents to query media file details in a structured way.

**Opportunity**: No terminal tool exists that treats audio/video files as first-class citizens — where metadata (codec, resolution, duration, bitrate, sample rate) is the primary navigation axis, where triage (keep/delete/move) is a single-keypress workflow, where playback is integrated, and where structured JSON output makes the tool composable for agents and scripts.

**Scope for v0.1**: Audio and video files only. No images/photos in this version.

**Target**: macOS-first. Technical users (developers, video editors, sysadmins, AI agents) who work with audio/video files from the terminal.

**Intended outcome**: A fast, dual-mode tool — interactive TUI for humans, structured JSON output for agents — that replaces the `ls` → Finder → mpv dance with a single unified experience. Think `fd` meets `ffprobe` meets `lazygit`.

---

## Community Research Summary

### Validated Demand (from Reddit, HN, GitHub Issues)

1. **"Stop making me leave the terminal"** — the loudest universal complaint. Every developer knows the pain of context-switching to check an image/video.
2. **Image preview that works** — ranger's GitHub has 9 years of broken preview issues. Yazi's rise to 30k+ stars is directly attributed to finally solving this.
3. **Speed is table stakes** — ranger's 300ms Python startup is unacceptable. Users demand sub-100ms feel.
4. **Protocol handling should be invisible** — users should never need to know what "sixel" or "kitty graphics protocol" means.

### Key User Segments (audio/video focus)

| Segment | Core Need |
|---------|-----------|
| Video editors | Duration/resolution/codec at a glance, thumbnail preview, quick playback |
| Music library users | Tags, duration/bitrate, folder-based navigation, inline playback |
| Developers | Quick media asset inspection, batch operations, scriptable queries |
| AI agents/scripts | Structured JSON metadata output, filter/sort by media properties |
| Content creators | Organize video downloads, screen recordings, podcasts |

### Top Feature Requests (ranked by community frequency, scoped to audio/video)

**Tier 1 — Must have:**
1. Video thumbnail preview (static frame, auto-detect terminal protocol)
2. Fast startup and navigation (sub-100ms feel)
3. Media metadata columns (resolution, codec, duration, bitrate, sample rate)
4. Structured JSON output for scripting and agents

**Tier 2 — High value:**
5. Keyboard-driven triage (single-key keep/delete/move for batch culling)
6. Fuzzy search / filtering by metadata
7. Multiple selection + batch operations
8. Sort/filter by codec, resolution, duration, bitrate

**Tier 3 — Differentiators:**
9. Audio playback with transport controls
10. Tagging/labeling for sorting workflows
11. NDJSON streaming for large directory scans
12. Agent-friendly error codes and schema versioning

### Gaps No Current Tool Addresses

1. **Structured output for agents** — no media CLI tool outputs machine-readable metadata with filter/sort (ffprobe is per-file, no directory scanning)
2. **Media triage workflow** — no tool natively implements view → keep/delete/move → next with single-keypress flow
3. **Metadata-first navigation** — sort/filter by codec, resolution, duration as first-class columns
4. **Dual-mode operation** — TUI when interactive, JSON when piped (like `fd`, `rg`)

---

## Tech Stack Decision

### Consensus: Rust + Ratatui + Crossterm + Tokio

All research sources (community analysis, TUI framework comparison, Codex GPT-5.3) converge on the same recommendation.

**Why Rust + Ratatui:**
- `ratatui-image` (v10.x, 264k downloads, actively maintained) handles Kitty/iTerm2/Sixel protocol detection and rendering as a `StatefulWidget` — the hardest problem is already solved
- Immediate-mode rendering with buffer diffing = minimal terminal I/O
- Tokio async runtime = concurrent thumbnail generation, metadata extraction without blocking UI
- Proven at scale: yazi (24.5k stars) uses this exact stack
- ~30ms startup vs ~300ms for Python tools

**Why not Go + Bubbletea:** Bubbletea's image support is still an open issue (#163, since 2021). You'd spend effort on protocol plumbing instead of product features.

**Why not Python + Textual:** 300ms startup, GIL kills true parallel media loading. Wrong for a file-manager-class tool.

### Core Crate Stack

| Purpose | Crate | Notes |
|---------|-------|-------|
| TUI framework | `ratatui` 0.29+ | Immediate-mode, buffer-diffed rendering |
| Terminal backend | `crossterm` | Cross-platform input/output |
| Image in TUI | `ratatui-image` | Auto Kitty/iTerm2/Sixel/halfblock detection (video thumbnails) |
| Async runtime | `tokio` | Work-stealing thread pool for concurrent metadata extraction |
| Fuzzy matching | `nucleo` | Same engine as Helix editor, <16ms for 100k items |
| FS watching | `notify` | kqueue on macOS, inotify on Linux |
| Image decode | `image` | Pure Rust decode for video thumbnail JPEGs |
| Audio tags | `lofty` | Pure Rust ID3/Vorbis/MP4 tags (in-process, no subprocess) |
| CLI parsing | `clap` | Derive-based subcommands and flags |
| JSON output | `serde_json` | Structured output for agents |
| LRU cache | `lru` | Thumbnail and metadata caching |
| Config | `toml` + `serde` | Standard for Rust TUI tools |

### External Tool Integrations (subprocess) — Hard Requirements

| Tool | Purpose | macOS Install |
|------|---------|---------------|
| `ffprobe` | Video/audio metadata extraction | `brew install ffmpeg` |
| `ffmpeg` | Video thumbnail generation | (included with ffmpeg) |
| `mpv` | Audio/video playback via JSON IPC | `brew install mpv` |
**Hard dependencies**: `ffmpeg` and `mpv` are required. mls checks for them at startup and prints a clear install command if missing.

**Architecture principle**: mls owns UX, state, cache, capability detection. External tools provide battle-tested media operations. No re-implementing codecs.

### Image Protocol Strategy (macOS-focused)

Priority detection order (optimized for user's terminals: iTerm2, Ghostty, Kitty):
1. **Kitty Graphics Protocol** — Kitty, Ghostty (pixel-perfect, placement IDs, best quality)
2. **iTerm2 Inline Images** (OSC 1337) — iTerm2, WezTerm (wide macOS coverage)
3. **Sixel** — foot, Windows Terminal (future-proofing)
4. **Unicode halfblocks** (chafa) — universal fallback (Alacritty, etc.)

`ratatui-image` handles all of this automatically via `Picker::from_query_stdio()`.

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                   tokio runtime                       │
│                                                       │
│  ┌───────────┐  ┌────────────┐  ┌──────────────────┐ │
│  │  UI task   │  │ FS watcher │  │  Media pipeline  │ │
│  │ (ratatui)  │  │  (notify/  │  │  - ffprobe       │ │
│  │ event-     │  │   kqueue)  │  │  - ffmpeg thumb  │ │
│  │ driven     │  │            │  │  - image decode  │ │
│  │ rendering  │  │            │  │  - exiftool      │ │
│  └─────┬──────┘  └─────┬──────┘  └────────┬─────────┘ │
│        │               │                  │            │
│  ┌─────▼───────────────▼──────────────────▼──────────┐ │
│  │            mpsc channels / event bus                │ │
│  └────────────────────────────────────────────────────┘ │
│                                                       │
│  ┌─────────────────┐  ┌──────────────────────────────┐ │
│  │  LRU thumbnail  │  │  Metadata index (SQLite)     │ │
│  │  cache (memory)  │  │  - path, mtime, metadata    │ │
│  │  + disk cache    │  │  - persistent across runs    │ │
│  └─────────────────┘  └──────────────────────────────┘ │
│                                                       │
│  ┌──────────────────────────────────────────────────┐  │
│  │  mpv IPC controller (JSON over Unix socket)       │  │
│  │  - playback, pause, seek, volume, position        │  │
│  └──────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

---

## CLI Interface (Dual-Mode: TUI + Structured Output)

Designed with Codex GPT-5.2 (xhigh reasoning). The core insight: `mls` is both a human TUI and an agent-queryable metadata tool.

### Mode Detection

```
if --tui        → TUI (force)
elif --json     → JSON (single document)
elif --ndjson   → NDJSON (streaming, newline-delimited)
elif stdout.is_terminal() → TUI
else            → NDJSON (default when piped)
```

### Subcommands

```
mls [PATH...]              # default = list; TUI if TTY, NDJSON if piped
mls list [PATH...]         # browse/list media files (alias: mls ls)
mls info FILE...           # detailed metadata for specific file(s)
mls play FILE              # play via mpv
mls triage [PATH...]       # interactive triage mode (always TUI)
```

### Common Flags

```
--json                  Single JSON document to stdout
--ndjson                Newline-delimited JSON (streaming)
--tui                   Force interactive TUI even if piped
--filter <EXPR>         Filter by metadata expression
--sort <KEY[:asc|desc]> Sort by key (default: path:asc)
--limit <N>             Pagination limit
--max-depth <N>         Directory walk depth
--threads <N>           Metadata probe concurrency
--timeout-ms <N>        Per-file probe timeout (default: 5000)
--quiet                 Suppress non-JSON logs
```

### Filter Expression Language (v0.1 — minimal, agent-friendly)

```bash
mls --json --filter 'duration_ms > 60000' ~/Videos
mls --json --filter 'media.video.width >= 1920' ~/Videos
mls --json --filter 'media.audio.codec.name == "aac"' ~/Music
mls --json --filter 'media.kind == "av" && duration_ms > 300000' .
```

Operators: `== != > >= < <=` `&& || !` `()`. Field paths dot-separated.

### Exit Codes (agent-friendly)

- `0` — success
- `2` — CLI usage error (bad flag/filter)
- `3` — walk error (path missing/permission)
- `4` — backend failure (ffprobe not found)

---

## Structured Output Schema (JSON)

### Envelope (`mls list --json`)

```json
{
  "type": "mls.list",
  "schema_version": "0.1.0",
  "mls_version": "0.1.0",
  "generated_at": "2026-02-28T20:15:00Z",
  "summary": {
    "entries_total": 1234,
    "entries_emitted": 120,
    "probe_ok": 118,
    "probe_error": 2
  },
  "entries": [],
  "errors": []
}
```

### NDJSON Streaming (`mls list --ndjson`)

Three record types, one per line:
```
{"type":"mls.header", "schema_version":"0.1.0", ...}
{"type":"mls.entry", "entry": { ...MediaEntry... }}
{"type":"mls.entry", "entry": { ...MediaEntry... }}
{"type":"mls.footer", "summary": { ... }}
```

### MediaEntry (one file)

```json
{
  "path": "/Users/me/Videos/movie.mp4",
  "file_name": "movie.mp4",
  "extension": "mp4",
  "fs": {
    "size_bytes": 1048576000,
    "modified_at": "2026-02-10T03:12:45Z",
    "created_at": "2026-02-09T18:00:00Z"
  },
  "media": {
    "kind": "av",
    "container": { "format_name": "mov,mp4", "format_primary": "mp4" },
    "duration_ms": 7265400,
    "overall_bitrate_bps": 1150000,
    "video": {
      "width": 1920, "height": 1080,
      "fps": { "num": 24000, "den": 1001 },
      "bitrate_bps": 900000,
      "codec": { "name": "h264", "profile": "High", "level": "4.1" },
      "pixel_format": "yuv420p"
    },
    "audio": {
      "channels": 2, "channel_layout": "stereo",
      "sample_rate_hz": 48000, "bitrate_bps": 192000,
      "codec": { "name": "aac", "profile": "LC" }
    },
    "streams": [ /* full stream details */ ],
    "tags": { "title": null, "artist": null, "album": null, "date": null, "genre": null }
  },
  "probe": { "backend": "ffprobe", "took_ms": 38, "error": null }
}
```

Design choices:
- `duration_ms` (milliseconds) — no precision loss
- `fps` as rational `{num, den}` — avoids float lies (23.976 = 24000/1001)
- `media.kind`: `"video"` | `"audio"` | `"av"` — quick triage without inspecting streams
- Both summary (`media.video`, `media.audio`) and `streams[]` — agents use summary, tools use streams

---

## UI Layout (TUI Mode)

### Primary View: Miller Columns + Metadata

```
┌─ mls ── ~/Videos/projects ───────────────────────────────────┐
│ ┌──────────┐ ┌────────────────────────┐ ┌──────────────────┐ │
│ │ Parent   │ │ Current Dir            │ │ Preview          │ │
│ │          │ │                        │ │                  │ │
│ │ 2024/    │ │ intro.mp4  1080p 0:32  │ │  ┌────────────┐ │ │
│ │ 2023/    │ │ main.mkv   4K    2:35  │ │  │  [video    │ │ │
│ │ exports/ │ │▶clip.mov   720p  0:45  │ │  │  thumbnail]│ │ │
│ │ audio/   │ │ bgm.flac   16/44 3:22  │ │  │            │ │ │
│ │          │ │ narr.mp3   44.1k 1:15  │ │  └────────────┘ │ │
│ │          │ │ outro.mp4  1080p 0:18  │ │                  │ │
│ └──────────┘ └────────────────────────┘ └──────────────────┘ │
│ ┌────────────────────────────────────────────────────────────┐│
│ │ H.264 High │ 1280×720 │ 23.976fps │ AAC stereo │ 4.2 Mbps││
│ └────────────────────────────────────────────────────────────┘│
│ [j/k] nav  [Enter] open  [Space] mark  [p] play  [?] help   │
└──────────────────────────────────────────────────────────────┘
```

### Triage Mode (activated with `t`)

```
┌─ TRIAGE ── 47/312 files ── 12 kept, 8 deleted ──────────────┐
│                                                               │
│                    ┌──────────────────────┐                   │
│                    │                      │                   │
│                    │   [video thumbnail   │                   │
│                    │    or waveform]      │                   │
│                    │                      │                   │
│                    └──────────────────────┘                   │
│                                                               │
│  clip.mov │ H.264 │ 1280×720 │ 23.976fps │ 45s │ 4.2 Mbps  │
│  AAC stereo │ 48kHz │ 192kbps │ 12.8 MB                     │
│                                                               │
│  [y] keep   [n] delete   [m] move to...   [←→] prev/next    │
│  [p] play   [u] undo     [q] finish       [i] info detail    │
└──────────────────────────────────────────────────────────────┘
```

---

## MVP Feature Set (v0.1) — Audio/Video Only

### Core (must ship)

1. **Audio/video directory listing** — filter to audio/video files by MIME type, show metadata columns (resolution, duration, codec, bitrate, sample rate)
2. **Dual-mode output** — TUI when TTY, NDJSON when piped; `--json`/`--ndjson`/`--tui` overrides
3. **Video thumbnail preview** — extract frame via `ffmpeg -ss 5 -i <file> -frames:v 1`, display via `ratatui-image`
4. **Metadata panel** — `ffprobe` JSON parsed and displayed (video: resolution/codec/fps/duration/bitrate; audio: codec/sample-rate/channels/duration/bitrate)
5. **Vim-style navigation** — h/j/k/l, gg/G, Ctrl-d/u, `/` for fuzzy filter
6. **Miller column layout** — parent / current / preview three-pane
7. **Contextual help bar** — always-visible footer with keybindings (lazygit pattern)
8. **Structured JSON output** — `MediaEntry` schema with schema versioning, per-file metadata, error handling

### High-value additions for v0.1

9. **Sort by metadata** — toggle sort by name/size/date/resolution/duration/codec/bitrate
10. **Filter expressions** — `--filter 'duration_ms > 60000'`, `--filter 'media.video.width >= 1920'`
11. **Audio playback** — `mpv --no-video` via JSON IPC, transport controls in status bar
12. **Video playback** — launch `mpv` via IPC or `open` command
13. **Triage mode** — `t` to enter; y/n/m single-key keep/delete/move with undo
14. **Bulk selection** — Space to mark, bulk move/copy/delete
15. **`mls info`** — detailed metadata for specific file(s), JSON or pretty-printed
16. **macOS Quick Look** — `qlmanage -p` fallback for native preview

### Deferred (post-MVP)

- Image file support (photos, RAW, EXIF)
- Grid/gallery view
- Audio waveform/spectrum visualization
- Plugin system (Lua)
- Persistent metadata index (SQLite)
- Tagging/labeling system
- SSH + tmux transparent preview
- `reveal in Finder`
- Configurable themes (TOML)
- `--raw-ffprobe` flag to include raw ffprobe output in JSON

---

## Keybinding Design

| Key | Action | Context |
|-----|--------|---------|
| `j/k` | Navigate up/down | Always |
| `h/l` | Parent dir / Enter dir or preview | Always |
| `Enter` | Open with default app (`open` on macOS) | Always |
| `Space` | Toggle mark/select | Always |
| `/` | Fuzzy filter current directory | Always |
| `s` | Cycle sort mode (name→size→date→resolution→duration) | Always |
| `S` | Reverse sort | Always |
| `i` | Toggle metadata panel | Always |
| `p` | Play/pause (audio/video via mpv IPC) | Media selected |
| `]` / `[` | Seek forward/back 10s (during playback) | Playing |
| `t` | Enter triage mode | Always |
| `y` | Keep (triage) | Triage mode |
| `n` | Delete/reject (triage) | Triage mode |
| `m` | Move to... (fuzzy dir picker) | Triage mode |
| `u` | Undo last action | Triage mode |
| `q` | Quit / exit mode | Always |
| `?` | Help overlay | Always |
| `Ctrl-c` | Quit | Always |

---

## macOS-Specific Considerations

- **Quick Look integration**: `qlmanage -p <file>` for native preview popup
- **`open` command**: Default file opening via macOS registered apps
- **Trash**: Use `trash` CLI (`brew install trash`) instead of `rm` for safe deletion
- **iTerm2 protocol priority**: Most macOS terminal users use iTerm2, WezTerm, or Warp — all support OSC 1337
- **Homebrew dependencies**: `ffmpeg`, `mpv`, `exiftool` all available via `brew`
- **kqueue**: Native FS watching via `notify` crate on macOS
- **Metal/GPU**: Not relevant for TUI, but mpv uses hardware decode on macOS automatically
- **Font**: Recommend Nerd Font for file type icons (optional, degrade gracefully)

---

## Technical Risks & Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Terminal protocol fragmentation | High | `ratatui-image` handles detection; test across iTerm2/Kitty/Ghostty/WezTerm/Alacritty |
| Async pipeline complexity (cancel, backpressure) | Medium | Use tokio mpsc with bounded channels; cancel-on-navigate pattern |
| ffprobe/exiftool subprocess overhead for large dirs | Medium | Background extraction with priority queue (viewport-first); persistent cache |
| mpv IPC lifecycle management | Medium | Spawn on first play, keep alive, kill on quit; reconnect on failure |
| Large directory performance (100k+ files) | Medium | Virtual scrolling, lazy metadata extraction, viewport-priority loading |
| Missing external dependencies | Low | Hard-require ffmpeg+mpv at startup with clear `brew install` message; exiftool optional |

---

## Success Metrics

- **Startup**: <50ms to first render on a directory with 1000 media files
- **Navigation**: <16ms response to j/k keypresses (60fps feel)
- **Preview**: <200ms from selecting a file to displaying image preview
- **Metadata**: <100ms to display basic ffprobe metadata for selected file
- **Triage**: <50ms per keep/delete/move action in triage mode

---

## Verification Plan

### TUI Mode
1. **Build & run**: `cargo build --release && ./target/release/mls ~/Videos`
2. **Video thumbnail**: Navigate to MP4 → verify thumbnail frame displayed in preview pane
3. **Metadata panel**: Press `i` → verify resolution/codec/fps/duration/bitrate shown
4. **Audio playback**: Navigate to FLAC/MP3, press `p` → verify mpv plays audio, transport shown
5. **Video playback**: Navigate to MP4, press `p` → verify mpv plays video
6. **Triage mode**: Press `t` → verify y/n/m workflow with undo
7. **Fuzzy filter**: Press `/` → type partial filename → verify instant filtering
8. **Sort**: Press `s` → verify cycling through sort modes (name/duration/size/codec)
9. **Terminal compat**: Test in iTerm2, Kitty, Ghostty (all three user terminals)

### Structured Output Mode
10. **JSON output**: `mls --json ~/Videos | jq .summary` → verify valid JSON with entry count
11. **NDJSON streaming**: `mls --ndjson ~/Videos | head -5` → verify header + entries stream
12. **Filter**: `mls --json --filter 'duration_ms > 60000' ~/Videos` → verify only long files returned
13. **Sort**: `mls --json --sort 'duration_ms:desc' ~/Videos` → verify descending order
14. **Info subcommand**: `mls info --json video.mp4` → verify full MediaEntry with streams
15. **Pipe detection**: `mls ~/Videos | cat` → verify NDJSON output (not TUI)
16. **Error handling**: `mls --json /nonexistent` → verify exit code 3 with error in JSON

### Performance
17. **Startup**: Time `mls --json ~/Videos > /dev/null` with 1000 files → verify < 2s
18. **Large dir**: Test with 10k+ media files → verify smooth scrolling in TUI

---

## Non-Goals

- Not a general-purpose file manager (don't compete with yazi on generic file ops)
- Not an image/photo browser in v0.1 (audio/video only)
- Not a media editor or transcoder
- Not cross-platform in v0.1 (macOS first, Linux follows naturally)
- Not a music player replacement (playback is for preview, not library management)
- No plugin system in v0.1 (get core workflows right first)
- No web UI or remote access
- Not a replacement for ffprobe (mls wraps it, not reimplements)

---

## Resolved Decisions

- **Name**: `mls` (media ls) — confirmed
- **Scope**: Audio + video files only in v0.1 (images deferred)
- **Dependencies**: Hard-require `ffmpeg` + `mpv`
- **Primary terminals**: iTerm2, Ghostty, Kitty
- **Platform**: macOS first
- **Structured output**: JSON + NDJSON with schema versioning, designed for AI agent consumption
- **Default when piped**: NDJSON (streaming-friendly for large dirs)

## Open Questions

1. **Config format**: TOML (Rust convention) vs none-needed-for-MVP?
2. **Thumbnail cache location**: `~/Library/Caches/mls/` (macOS convention) or `~/.cache/mls/` (XDG)?
3. **Filter expression parser**: hand-rolled (simpler, smaller binary) vs `pest`/`nom` crate (more robust)?
