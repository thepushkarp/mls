# TUI Ecosystem Research — Modern Tools, Frameworks, and Media Preview

Research conducted: 2026-02-28

---

## 1. Top Modern TUI Tools and Their Tech Stacks

### File Managers

| Tool | Lang | Framework | Stars | Key Traits |
|------|------|-----------|-------|------------|
| **yazi** | Rust | ratatui + crossterm + tokio | 32k | Async I/O, multi-protocol image preview, plugin system (Lua), preloading |
| **broot** | Rust | crossterm (custom renderer) | ~11k | Tree-first nav, fuzzy filter, verb system, fast FS traversal |
| **joshuto** | Rust | ncurses bindings | ~3k | ranger-like columns, image preview via ueberzugpp |
| **superfile** | Go | bubbletea + lipgloss | ~9k | Modern panels, image preview via chafa/ueberzugpp |
| **xplr** | Rust | custom (msgpack IPC) | ~4k | Scriptable, Lua plugin, acts as orchestrator not a full UI |
| **ranger** | Python | urwid | ~15k | Mature, slow startup, rifle file opener, ueberzug previews |
| **nnn** | C | raw ANSI / ncurses | ~19k | Minimal, fastest startup, plugin scripts in shell |

### Git / Dev Tools

| Tool | Lang | Framework | Stars | Key Traits |
|------|------|-----------|-------|------------|
| **lazygit** | Go | gocui (custom fork) | ~55k | Panels, complex async git ops, keybinding layers |
| **gitui** | Rust | ratatui + crossterm | ~19k | Event-driven, async git2-rs, fast diff rendering |
| **serie** | Rust | ratatui | ~2k | Git log graph visualization, branchline ASCII art |
| **lazydocker** | Go | gocui | ~38k | Same architecture as lazygit, container/image/volume panels |

### System Monitors

| Tool | Lang | Framework | Stars | Key Traits |
|------|------|-----------|-------|------------|
| **btop** | C++ | custom renderer (no lib) | ~29k | GPU support, themes, mouse, 60fps, minimal deps |
| **bottom (btm)** | Rust | ratatui + crossterm | ~12k | Configurable layout, cross-platform, charts |
| **htop** | C | custom ANSI | mature | The original, proc-based, ncurses |

### Shell / History

| Tool | Lang | Framework | Stars |
|------|------|-----------|-------|
| **atuin** | Rust | ratatui + crossterm | ~22k |

### Music / Media

| Tool | Lang | Framework | Stars | Key Traits |
|------|------|-----------|-------|------------|
| **termusic** | Rust | ratatui + tui-realm | ~1.3k | Album art via ueberzugpp/kitty/sixel |
| **rmpc** | Rust | ratatui + crossterm | ~2k | MPD client, album art via kitty/sixel/ueberzugpp |
| **spotify-tui** | Rust | tui-rs (old) | archived | deprecated, rspotify |

---

## 2. TUI Frameworks — Detailed Comparison

### Rust Ecosystem

#### ratatui (immediate-mode)
- **Origin**: Community fork of the abandoned `tui-rs` (2023)
- **Architecture**: Immediate-mode rendering — you rebuild the entire frame each tick into a `Buffer`, then diff it against previous buffer and only flush changes to the terminal. This means zero retained state in the library itself; all state is yours.
- **Backends**: crossterm (default, cross-platform), termion (Unix only), termwiz (wezterm's terminal lib)
- **Current version**: 0.29.0 (as of late 2025)
- **Async story**: ratatui itself is sync; the ecosystem wraps it with tokio. Common pattern: tokio mpsc channel for events → update state → render frame. Libraries like `ratatui-elm` and `async-ratatui` formalize this.
- **Strengths**: Maximum flexibility, zero retained state, fine control over every pixel, excellent diffing performance, very active community
- **Weaknesses**: No built-in animation, no retained widget tree, you manually manage every state transition
- **Used by**: gitui, bottom, atuin, termusic, rmpc, serie, many others

#### crossterm
- Cross-platform terminal manipulation (input, output, cursor, colors, mouse)
- The de facto backend for ratatui on all platforms
- Handles raw mode, alternate screen, mouse events, key events (including kitty keyboard protocol)

#### termion
- Unix-only alternative to crossterm
- Slightly lower level, faster on Linux/Mac but no Windows

#### termwiz
- WezTerm's own terminal library, used as optional ratatui backend
- Supports more advanced features but heavier dependency

### Go Ecosystem

#### Bubbletea (charm.sh) — v2 released Feb 2026
- **Architecture**: Pure Elm Architecture (TEA) — Model/Update/View. The program is a state machine. All mutations go through `Update(msg)` → returns new Model + optional Cmd. View is a pure string render function.
- **Stars**: 39k+
- **v2 highlights** (Feb 23, 2026):
  - Highly optimized rendering (advanced compositing)
  - Higher-fidelity input handling
  - More declarative API
  - Powers Charm's own AI coding tool "Crush" in production
  - Ecosystem powers 25,000+ open-source apps
- **Companion libs**:
  - **Lipgloss v2**: CSS-like styling/layout for terminal strings. Flex-box-like model, borders, padding, margin, alignment
  - **Bubbles v2**: Ready-made components — text input, viewport, spinner, progress bar, table, list, paginator, file picker
- **Strengths**: Strong mental model (predictable, testable), massive ecosystem, excellent DX, very fast to prototype
- **Weaknesses**: Elm architecture can feel awkward for highly nested UIs; message passing overhead in very complex apps; harder to handle out-of-band async (requires Cmd wrapping)

#### gocui
- Lower-level, used by lazygit/lazydocker
- Retained widget model with view callbacks
- More imperative; less ergonomic than bubbletea but gives direct control
- Being replaced by bubbletea in new Go TUI projects

#### tcell / tview
- **tcell**: Low-level terminal cell manipulation (like crossterm for Go)
- **tview**: High-level retained widget library on top of tcell (forms, tables, tree views, flex layouts)
- Used by k9s (Kubernetes TUI), lazysql

### Python Ecosystem

#### Textual (Textualize)
- **Architecture**: CSS-like declarative layouts, DOM tree of widgets, event system (async def on_xxx handlers), reactive attributes that auto-trigger re-render
- **Styling**: Actual `.tcss` files (Textual CSS) — subset of CSS with terminal-appropriate properties
- **Async**: Built on asyncio natively; workers run background tasks, post messages back to the main thread
- **Performance**: Textual published an "algorithms for high performance" post (Dec 2024) detailing their compositor, dirty-rect rendering, and layout caching
- **Strengths**: Fastest time-to-working-UI, best for Python devs, can run on web (via Textual Web), CSS theming is intuitive
- **Weaknesses**: Python startup overhead (~100-300ms), not suitable for sub-millisecond latency requirements, GIL limits true parallelism

#### Rich
- Output formatting library (not a full TUI framework)
- Powers Textual's rendering layer
- Used standalone for pretty CLI output (tables, progress, syntax, markdown)

#### urwid
- The old guard — used by ranger, pudb
- Retained widget model, more boilerplate
- Still maintained but not the modern choice

### Other Languages

#### Zig
- No dominant TUI framework yet. `libvaxis` is emerging (written in Zig, also has Go/Rust ports) — focuses on the Kitty keyboard protocol and modern terminal features

#### C / ncurses
- Used by nnn, htop, mutt
- Minimal deps, fastest startup
- Not ergonomic for complex UIs

#### Java
- **TamboUI** (announced Feb 2026) — new framework from Micronaut/Quarkus teams
- Based on ratatui's model, adapted for JVM

#### OCaml
- **Mosaic** — announced 2026, early preview

#### TypeScript / Node
- **OpenTUI** — emerging
- **Ink** — React-like components in the terminal, used by some npm CLI tools (well-established)

---

## 3. Image/Media Preview in Terminal

### Protocol Landscape

| Protocol | Origin | Quality | Support | How it works |
|----------|--------|---------|---------|--------------|
| **Kitty Graphics Protocol** | Kitty terminal | Best | Kitty, Ghostty, Konsole, WezTerm (partial) | APC escape sequences (`ESC_G...ESC\`), sends base64-encoded pixel data. Supports placement IDs, deletion, animations, unicode placeholders for cell-precise positioning |
| **iTerm2 Inline Images** | iTerm2 macOS | High | iTerm2, WezTerm, Warp, Tabby | OSC 1337 escape, base64 image data. Simpler than Kitty but widely supported on macOS |
| **Sixel** | DEC VT300 1980s | Medium | foot, Windows Terminal (v1.22+), st-sixel, xterm | Bitmap encoded as character sequences. Lowest common denominator with widest legacy support. Quality limited by dithering. |
| **Unicode block chars** | Universal | Low | Every terminal | Uses `█`, `▄`, `▀`, Braille patterns. No protocol needed but very low resolution. |

### Terminal Compatibility (as of 2026)

| Terminal | Kitty | iTerm2/WezTerm | Sixel |
|----------|-------|----------------|-------|
| Kitty | Native | - | - |
| iTerm2 | - | Native | - |
| WezTerm | Partial | Native | - |
| Ghostty | Native (unicode placeholders) | - | - |
| foot | - | - | Native |
| Windows Terminal | - | - | v1.22+ |
| Alacritty | None | None | None |
| tmux | Passthrough (complex) | Passthrough | Passthrough |

### ueberzugpp (C++ image overlay)
- Drop-in replacement for the original Python `ueberzug` (now defunct)
- **How it works**: Creates a child X11/Wayland window overlaid on top of the terminal region, or uses sixel/kitty/iTerm2 protocols directly
- **Outputs**: X11 child window, Wayland (sway), Sixel, Kitty, iTerm2
- **Dependencies**: libvips (image processing), opencv (GIF/animated WebP), libsixel, libxcb, wayland
- **Used by**: ranger, joshuto, superfile, termusic, ytfzf
- **Limitation**: Requires a running display server for X11/Wayland modes; pure protocol modes (sixel/kitty) work in any terminal

### How yazi handles preview (the gold standard)
yazi's approach is the most sophisticated currently:
1. **Protocol autodetection**: Queries terminal capabilities at startup
2. **Priority order**: Kitty unicode placeholders → iTerm2 → WezTerm → Sixel → ueberzugpp fallback → chafa (Unicode block chars)
3. **Async preloading**: Images are decoded and pre-scaled in background tokio tasks before the user even navigates to them
4. **Caching**: Resized/decoded images are cached to avoid re-processing
5. **tmux/Zellij**: Complex passthrough mechanisms for multiplexers; Kitty unicode placeholders are the only protocol that works reliably inside tmux
6. **Video thumbnails**: ffmpegthumbnailer spawned as subprocess to extract first frame

### chafa (Unicode/Braille image renderer)
- C library + CLI tool
- Converts images to Unicode block chars, Braille patterns, or sixel
- Used as fallback in many tools when no native protocol available
- Supports animated GIFs in Unicode block mode
- Used by: superfile, some ranger configs

### Video preview in terminal
- Essentially impossible to do in-terminal natively (no protocol supports video)
- Approaches:
  1. **Thumbnail extraction**: ffmpegthumbnailer/ffmpeg extracts a frame → display as image
  2. **mpv integration**: Launch mpv in a separate process (not in-terminal)
  3. **chafa animation**: Very low-res, Unicode block char "animation" using chafa's GIF support
  4. **ueberzugpp X11**: Can overlay a mini mpv window in X11 environments (rare)

---

## 4. Performance Considerations

### What makes TUI tools fast

#### Rendering pipeline
- **Immediate mode + buffer diffing** (ratatui model): Rebuild entire UI each frame, but only flush changed cells to the terminal. Terminal I/O is the bottleneck (not CPU), so minimizing writes is key.
- **Dirty rect tracking** (Textual model): Only re-render widgets that changed. More complex bookkeeping but fewer widget renders.
- **Target frame rates**: Most TUIs target 30-60fps for animations; file managers typically render only on events (event-driven, not polled).

#### Async I/O
- **yazi's model** is the benchmark: tokio async runtime with task priority scheduling. File metadata, image decoding, syntax highlighting all happen concurrently on a thread pool. The UI thread never blocks.
- **ratatui pattern**: `tokio::spawn` background tasks → send results via `mpsc::channel` → render loop picks up results. Common template: `crossterm::event::EventStream` for async terminal events.
- **bubbletea pattern**: `Cmd` (Go functions returning `Msg`) run concurrently via goroutines → returned as messages to Update. Goroutines are cheap in Go, so this is very natural.

#### File system traversal
- **Parallel walk**: `rayon` (Rust) or goroutines for parallel directory traversal
- **Lazy loading**: Only stat files visible in the viewport; defer metadata for off-screen items
- **Inotify/kqueue/FSEvents**: Watch for file changes instead of polling

#### Memory
- **Virtual scrolling**: Only keep items in memory that could be visible (viewport + small buffer). For 100k file directories this is essential.
- **String interning**: Many TUI tools intern path strings / labels to reduce allocation churn
- **Rust's zero-copy**: Reading file metadata without allocating strings where possible

#### Startup time
- C/ncurses tools (nnn): < 5ms
- Rust tools (yazi, gitui): 20-80ms cold, < 20ms warm
- Go tools (lazygit): 50-150ms (GC warmup)
- Python tools (ranger, textual): 200-500ms (interpreter startup)

---

## 5. UX Patterns in Successful Modern TUI Tools

### Keybinding Philosophy
- **Vim-modal**: Normal/Insert/Visual modes (gitui, yazi, broot). Users already know them; reduces accidental keystrokes.
- **Single-modifier**: `ctrl-x`, `alt-x` for everything (btop, atuin). Lower learning curve.
- **Contextual help bars**: Always-visible footer showing current keybindings (lazygit's killer feature). Removes need to memorize or look up docs.
- **Kitty Keyboard Protocol**: Enables distinguishing `Shift+Enter` vs `Enter`, `Ctrl+Shift+K` vs `Ctrl+K`, etc. Essential for vim-like modality in modern terminals.

### Navigation
- **Miller columns** (ranger/yazi style): Three panes — parent dir, current dir, preview. User navigates left/right between panes. Spatial memory is intuitive.
- **Fuzzy filtering**: In-place fuzzy filter on any list (fzf-style). Should be instantaneous (<16ms) for up to ~100k items using `skim`/`nucleo` (Rust) or `fzf` (Go).
- **Jump navigation**: `zoxide` integration (yazi) to jump to frequently-visited dirs. `g` then type pattern → instant jump.

### Visual Design
- **256-color / TrueColor themes**: CSS-like theme files (yazi's `theme.toml`, btop's themes). Dark/light mode auto-detection via `COLORFGBG` or OSC 11 query.
- **Unicode box drawing**: Rounded corners (`╭╮╰╯`), thick borders for focus, thin for inactive. Creates visual hierarchy without color.
- **Responsive layout**: Panes resize on terminal resize events (`SIGWINCH`). Minimum viable layout at small sizes. btop and yazi both do this well.
- **Status bars**: Persistent top/bottom bars for context — current path, selected count, mode indicator, keybind hints.
- **Animations/transitions**: Subtle — spinner for async ops, scroll momentum (bubbletea viewport), progress bars. Heavy animations feel wrong in a terminal.

### Mouse Support
- Click to focus, scroll to scroll. Mostly supplementary to keyboard; should never be required.
- Implemented via `MouseEvent` from crossterm/tcell. Most frameworks support it with minimal extra code.
- Mouse selection in text views requires careful coordination with terminal's own selection mechanism.

### Fuzzy Finding Integration
- **fzf** (Go): The gold standard external fuzzy finder. Many tools spawn it as a subprocess.
- **skim** (Rust): fzf clone in Rust, slightly faster, can be used as a library.
- **nucleo** (Rust): Newer, higher-quality fuzzy matching algorithm used by Helix editor. Available as a crate.
- **telescope.nvim model**: Preview pane alongside results, real-time filter. This pattern is now expected in any modern TUI.

### Configuration
- **TOML** is the dominant config format for Rust tools (yazi, gitui, bottom)
- **Lua** for scripting/plugins (yazi's plugin system, xplr)
- **YAML** common in Go tools (lazygit, k9s)
- Sane defaults that work out of the box; config is optional enhancement

---

## 6. Recommendation for a Media-Focused TUI Tool

### Use Case: Fast, interactive, image/video preview

### Stack Recommendation: Rust + ratatui + crossterm + tokio

**Rationale**:

1. **Performance ceiling is highest**: For a media tool, you need async image decoding, thumbnail generation, possibly video frame extraction. Rust's tokio gives you a real async runtime with work-stealing thread pools. Go's goroutines are good but GC pauses are real at scale. Python is out for anything media-heavy.

2. **ratatui gives you full control**: Media tools need custom rendering — rendering an image protocol escape sequence at a specific cell position, overlaying a preview pane, managing image IDs in the Kitty protocol. ratatui's immediate-mode buffer lets you do this; a framework with a retained widget tree would fight you.

3. **Image protocol support is already solved**: Look at `ratatui-image` crate (actively maintained) which handles Kitty/iTerm2/Sixel protocol detection and rendering within a ratatui widget. Also `viuer` (Rust) for simple image display.

4. **tokio for media pipeline**:
   - Spawn tasks for thumbnail generation (ffmpegthumbnailer or image crate)
   - Cache decoded images in an LRU (lru crate)
   - Watch directory with `notify` crate (inotify/kqueue/FSEvents)
   - Async metadata reads with `tokio::fs`

5. **rsmpeg / ffmpeg-sys-next for video**: Rust FFI bindings to FFmpeg for frame extraction, metadata reading, duration, codec info

### Architecture sketch

```
┌─────────────────────────────────────────────────┐
│                  tokio runtime                   │
│                                                  │
│  ┌──────────┐   ┌──────────┐   ┌─────────────┐  │
│  │ UI task  │   │FS watcher│   │ Media loader│  │
│  │(ratatui) │   │ (notify) │   │  (ffmpeg,   │  │
│  │          │   │          │   │  image rs)  │  │
│  └────┬─────┘   └────┬─────┘   └──────┬──────┘  │
│       │              │                │          │
│  ┌────▼──────────────▼────────────────▼──────┐   │
│  │           mpsc channels / event bus        │   │
│  └────────────────────────────────────────────┘   │
│                                                  │
│  ┌────────────────────────────────────────────┐   │
│  │         LRU thumbnail cache                │   │
│  │    (path → decoded pixels, resized)        │   │
│  └────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

### Key crates

| Purpose | Crate |
|---------|-------|
| TUI framework | `ratatui` 0.29 |
| Terminal backend | `ratatui-crossterm` |
| Image in TUI | `ratatui-image` |
| Async runtime | `tokio` |
| FS watching | `notify` |
| Image decode | `image` (pure Rust) |
| Video/audio | `rsmpeg` (FFmpeg bindings) or spawn `ffmpegthumbnailer` |
| Fuzzy matching | `nucleo` |
| Config | `toml` + `serde` |
| LRU cache | `lru` |
| Error handling | `anyhow` (app-level) |
| Logging | `tracing` + `tracing-subscriber` |

### Image protocol implementation strategy

```rust
// Detect at startup, store in app state
enum ImageProtocol {
    Kitty,       // best: supports placement IDs, deletion
    Iterm2,      // good: OSC 1337, wide macOS support
    Sixel,       // fallback: widest compatibility
    Chafa,       // Unicode blocks: universal fallback
}

// ratatui-image crate handles this detection and rendering
// within a StatefulWidget — just give it an image and cell bounds
```

### If you want Go instead

Use: **Bubbletea v2 + Lipgloss v2 + Bubbles v2**

- Much faster to prototype
- `rasterm` Go library handles Kitty/iTerm2 image protocols
- Goroutine-based async is natural for media loading
- Tradeoff: GC pauses, slightly higher memory baseline, no true zero-copy

### If media preview is secondary and Python team

Use: **Textual** — defer image preview to external tools (spawn `viu` or `kitty icat` as subprocess), focus on the interactive UI layer.

---

## Summary Table

| Criterion | Rust/ratatui | Go/Bubbletea | Python/Textual |
|-----------|-------------|-------------|----------------|
| Raw performance | Best | Good | Adequate |
| Startup time | ~30ms | ~80ms | ~300ms |
| Async media pipeline | Excellent (tokio) | Good (goroutines) | Limited (GIL) |
| Image protocol support | ratatui-image crate | rasterm lib | subprocess only |
| Dev velocity | Slower | Medium | Fastest |
| Ecosystem maturity | High | Very High | High |
| Cross-platform | Excellent | Excellent | Good |
| Community momentum | Very strong | Very strong | Strong |

**Bottom line**: For a media-focused TUI tool where image/video preview is a core feature (not an afterthought), **Rust + ratatui + tokio** is the right choice. The performance headroom, zero-copy media handling, and direct image protocol control are decisive advantages. The `ratatui-image` crate takes most of the hard work out of protocol negotiation. yazi proves this stack scales to a production-quality tool with 32k stars.
