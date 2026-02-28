/// TUI application — event loop, state management, rendering.
///
/// Uses ratatui with crossterm backend. Immediate-mode rendering with
/// buffer diffing for minimal terminal I/O.
pub mod input;
pub mod layout;
pub mod preview;
pub mod triage;
pub mod widgets;

use crate::playback::{MpvController, PlaybackState};
use crate::scan;
use crate::sort::sort_entries;
use crate::thumbnail::ThumbnailCache;
use crate::types::{MediaEntry, ProbeError, SortDir, SortKey};
use anyhow::{Context, Result};
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// Thumbnail preview state machine.
pub enum ThumbState {
    /// No thumbnail (audio-only file or nothing selected).
    Empty,
    /// Thumbnail is being generated in background.
    Loading,
    /// Thumbnail decoded and ready for rendering.
    Ready(Box<StatefulProtocol>),
    /// Thumbnail generation failed.
    Failed,
}

/// TUI application state.
#[expect(clippy::struct_excessive_bools)]
pub struct App {
    /// All discovered media entries.
    entries: Vec<MediaEntry>,
    /// Probe errors.
    errors: Vec<ProbeError>,
    /// Currently selected index.
    selected: usize,
    /// Current sort key and direction.
    sort_key: SortKey,
    sort_dir: SortDir,
    /// Fuzzy filter text (when `/` is active).
    filter_text: String,
    /// Filtered indices into `entries`.
    filtered_indices: Vec<usize>,
    /// Whether filter input is active.
    filter_active: bool,
    /// Whether metadata panel is visible.
    show_metadata: bool,
    /// Whether help overlay is visible.
    show_help: bool,
    /// Marked/selected entries (for bulk operations).
    marked: std::collections::HashSet<usize>,
    /// mpv playback controller.
    mpv: MpvController,
    /// Thumbnail cache (shared with background tasks).
    thumb_cache: Arc<ThumbnailCache>,
    /// Image protocol picker (halfblocks fallback).
    picker: Picker,
    /// Current thumbnail state.
    pub thumb_state: ThumbState,
    /// Receiver for in-flight thumbnail generation.
    thumb_receiver: Option<oneshot::Receiver<anyhow::Result<Vec<u8>>>>,
    /// Path currently loaded/loading for thumbnail.
    thumb_path: Option<PathBuf>,
    /// Triage mode state.
    triage: Option<triage::TriageState>,
    /// Current directory being browsed.
    current_dir: PathBuf,
    /// Should quit.
    should_quit: bool,
    /// Scroll offset for the file list.
    scroll_offset: usize,
    /// Status message (transient).
    status_message: Option<String>,
    /// Reusable fuzzy matcher (allocates ~135KB scratch space).
    fuzzy_matcher: Matcher,
    /// Move-to-directory input text (None = not in move input mode).
    move_input: Option<String>,
}

impl App {
    /// Create a new app with discovered entries.
    pub fn new(
        entries: Vec<MediaEntry>,
        errors: Vec<ProbeError>,
        current_dir: PathBuf,
        thumb_cache: ThumbnailCache,
        picker: Picker,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..entries.len()).collect();
        Self {
            entries,
            errors,
            selected: 0,
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            filter_text: String::new(),
            filtered_indices,
            filter_active: false,
            show_metadata: true,
            show_help: false,
            marked: std::collections::HashSet::new(),
            mpv: MpvController::new(),
            thumb_cache: Arc::new(thumb_cache),
            picker,
            thumb_state: ThumbState::Empty,
            thumb_receiver: None,
            thumb_path: None,
            triage: None,
            current_dir,
            should_quit: false,
            scroll_offset: 0,
            status_message: None,
            fuzzy_matcher: Matcher::new(Config::DEFAULT),
            move_input: None,
        }
    }

    /// Get the currently selected entry (if any).
    #[must_use]
    pub fn selected_entry(&self) -> Option<&MediaEntry> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Apply the current filter and rebuild filtered indices.
    ///
    /// Uses fuzzy matching via nucleo-matcher. Results are sorted by
    /// match score descending (best match first).
    fn apply_filter(&mut self) {
        if self.filter_text.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let pattern = Pattern::parse(
                &self.filter_text,
                CaseMatching::Ignore,
                Normalization::Smart,
            );
            let mut scored: Vec<(usize, u32)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    let chars: Vec<char> = e.file_name.chars().collect();
                    let haystack = nucleo_matcher::Utf32Str::Unicode(&chars);
                    pattern
                        .score(haystack, &mut self.fuzzy_matcher)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered_indices = scored.into_iter().map(|(i, _)| i).collect();
        }
        // Keep selected index in bounds
        if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    /// Apply current sort to entries and rebuild indices.
    fn apply_sort(&mut self) {
        sort_entries(&mut self.entries, self.sort_key, self.sort_dir);
        self.apply_filter();
    }

    /// Number of visible (filtered) entries.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Move selection down.
    fn move_down(&mut self) {
        if self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move to first entry.
    fn move_top(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Move to last entry.
    fn move_bottom(&mut self) {
        self.selected = self.filtered_indices.len().saturating_sub(1);
    }

    /// Page down (half screen).
    fn page_down(&mut self, page_size: usize) {
        let max = self.filtered_indices.len().saturating_sub(1);
        self.selected = (self.selected + page_size).min(max);
    }

    /// Page up (half screen).
    fn page_up(&mut self, page_size: usize) {
        self.selected = self.selected.saturating_sub(page_size);
    }

    /// Toggle mark on current entry.
    fn toggle_mark(&mut self) {
        if let Some(&idx) = self.filtered_indices.get(self.selected) {
            if self.marked.contains(&idx) {
                self.marked.remove(&idx);
            } else {
                self.marked.insert(idx);
            }
        }
    }

    /// Cycle sort key.
    fn cycle_sort(&mut self) {
        self.sort_key = self.sort_key.next();
        self.apply_sort();
        self.status_message = Some(format!("Sort: {}", self.sort_key.label()));
    }

    /// Sync `selected` from triage cursor, clamped to `filtered_indices`.
    pub(crate) fn sync_triage_selection(&mut self) {
        if let Some(ref triage) = self.triage {
            let max = self.filtered_indices.len().saturating_sub(1);
            self.selected = triage.current.min(max);
        }
    }

    /// Reverse sort direction.
    fn reverse_sort(&mut self) {
        self.sort_dir = self.sort_dir.toggle();
        self.apply_sort();
        let dir_label = match self.sort_dir {
            SortDir::Asc => "ascending",
            SortDir::Desc => "descending",
        };
        self.status_message = Some(format!("Sort: {} {dir_label}", self.sort_key.label()));
    }

    /// Start background thumbnail fetch for the currently selected entry.
    /// Skips if already loading the same path or if the file has no video.
    fn kick_thumbnail_fetch(&mut self) {
        let Some(entry) = self.selected_entry() else {
            self.thumb_state = ThumbState::Empty;
            self.thumb_path = None;
            return;
        };

        // Skip audio-only files
        if entry.media.video.is_none() {
            self.thumb_state = ThumbState::Empty;
            self.thumb_path = None;
            return;
        }

        let path = entry.path.clone();

        // Already loading or showing this path
        if self.thumb_path.as_ref() == Some(&path) {
            return;
        }

        self.thumb_path = Some(path.clone());
        self.thumb_state = ThumbState::Loading;

        let cache = Arc::clone(&self.thumb_cache);
        let (tx, rx) = oneshot::channel();
        self.thumb_receiver = Some(rx);

        tokio::spawn(async move {
            let result = cache.get_or_generate(&path).await;
            let _ = tx.send(result);
        });
    }

    /// Poll for completed thumbnail generation and decode the result.
    fn poll_thumbnail(&mut self) {
        let Some(ref mut rx) = self.thumb_receiver else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(jpeg_bytes)) => {
                self.thumb_receiver = None;
                let cursor = std::io::Cursor::new(jpeg_bytes);
                match image::ImageReader::with_format(cursor, image::ImageFormat::Jpeg).decode() {
                    Ok(dyn_img) => {
                        let proto = self.picker.new_resize_protocol(dyn_img);
                        self.thumb_state = ThumbState::Ready(Box::new(proto));
                    }
                    Err(_) => {
                        self.thumb_state = ThumbState::Failed;
                    }
                }
            }
            Ok(Err(_)) | Err(oneshot::error::TryRecvError::Closed) => {
                self.thumb_receiver = None;
                self.thumb_state = ThumbState::Failed;
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // Still loading
            }
        }
    }
}

/// Run the TUI application.
///
/// # Errors
/// Returns an error if terminal setup, rendering, or cleanup fails.
pub async fn run(
    paths: &[PathBuf],
    max_depth: Option<usize>,
    concurrency: usize,
    timeout_ms: u64,
) -> Result<()> {
    // Scan media files
    let current_dir = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let current_dir = std::fs::canonicalize(&current_dir).unwrap_or(current_dir);

    let (mut entries, errors) = scan::scan_all(paths, max_depth, concurrency, timeout_ms).await?;

    // Sort by name initially
    sort_entries(&mut entries, SortKey::Name, SortDir::Asc);

    let thumb_cache = ThumbnailCache::new(100, crate::thumbnail::default_cache_dir())?;

    // Setup terminal (must happen before picker query)
    terminal::enable_raw_mode().context("failed to enable raw mode")?;

    // Query terminal for graphics protocol support; fall back to halfblocks
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

    let mut app = App::new(entries, errors, current_dir, thumb_cache, picker);
    let mut stdout = io::stdout();
    stdout
        .execute(EnterAlternateScreen)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Main event loop
    let result = event_loop(&mut terminal, &mut app).await;

    // Cleanup
    app.mpv.stop().await;
    terminal::disable_raw_mode().context("failed to disable raw mode")?;
    io::stdout()
        .execute(LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    // Kick initial thumbnail fetch for the first selected entry
    app.kick_thumbnail_fetch();

    loop {
        // Render
        terminal.draw(|frame| {
            layout::render(frame, app);
        })?;

        if app.should_quit {
            break;
        }

        // Poll for events with 100ms timeout (for playback state updates)
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => handle_key(app, key).await,
                // Resize triggers a re-render on the next loop iteration
                Event::Resize(..) => continue,
                _ => {}
            }
        }

        // Update mpv state
        if app.mpv.state() != PlaybackState::Stopped && !app.mpv.is_alive() {
            app.mpv.stop().await;
        }

        // Poll for completed thumbnail generation
        app.poll_thumbnail();
    }
    Ok(())
}

async fn handle_key(app: &mut App, key: KeyEvent) {
    // Handle filter input mode
    if app.filter_active {
        match key.code {
            KeyCode::Esc => {
                app.filter_active = false;
                app.filter_text.clear();
                app.apply_filter();
            }
            KeyCode::Enter => {
                app.filter_active = false;
            }
            KeyCode::Backspace => {
                app.filter_text.pop();
                app.apply_filter();
            }
            KeyCode::Char(c) => {
                app.filter_text.push(c);
                app.apply_filter();
            }
            _ => {}
        }
        app.kick_thumbnail_fetch();
        return;
    }

    // Handle help overlay
    if app.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?' | 'q') => {
                app.show_help = false;
            }
            _ => {}
        }
        return;
    }

    // Handle move-to-directory input (from triage mode)
    if app.move_input.is_some() {
        handle_move_input(app, key).await;
        return;
    }

    // Handle triage mode
    if app.triage.is_some() {
        triage::handle_triage_key(app, key).await;
        return;
    }

    // Normal mode
    let prev_selected = app.selected;
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        // Navigation
        (KeyCode::Char('j') | KeyCode::Down, _) => app.move_down(),
        (KeyCode::Char('k') | KeyCode::Up, _) => app.move_up(),
        (KeyCode::Char('g'), _) => app.move_top(),
        (KeyCode::Char('G'), _) => app.move_bottom(),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.page_down(10),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.page_up(10),
        // Actions
        (KeyCode::Char('/'), _) => {
            app.filter_active = true;
            app.filter_text.clear();
        }
        (KeyCode::Char('s'), _) => app.cycle_sort(),
        (KeyCode::Char('S'), _) => app.reverse_sort(),
        (KeyCode::Char('i'), _) => app.show_metadata = !app.show_metadata,
        (KeyCode::Char('?'), _) => app.show_help = true,
        (KeyCode::Char(' '), _) => {
            app.toggle_mark();
            app.move_down();
        }
        (KeyCode::Char('t'), _) => {
            app.triage = Some(triage::TriageState::new(app.filtered_indices.len()));
            app.status_message =
                Some("Triage mode — y:keep n:delete m:move u:undo q:quit".to_string());
        }
        // Playback
        (KeyCode::Char('p'), _) => handle_playback(app).await,
        (KeyCode::Char(']'), _) => {
            let _ = app.mpv.seek(10).await;
        }
        (KeyCode::Char('['), _) => {
            let _ = app.mpv.seek(-10).await;
        }
        // Open with default app
        (KeyCode::Enter, _) => {
            if let Some(entry) = app.selected_entry() {
                let path = entry.path.clone();
                let _ = open_file(&path);
            }
        }
        _ => {}
    }

    // Kick thumbnail fetch if selection changed
    if app.selected != prev_selected {
        app.kick_thumbnail_fetch();
    }
}

async fn handle_playback(app: &mut App) {
    if app.mpv.state() == PlaybackState::Stopped {
        let info = app.selected_entry().map(|entry| {
            (
                entry.path.clone(),
                entry.media.video.is_none(),
                entry.file_name.clone(),
            )
        });
        if let Some((path, audio_only, name)) = info {
            let _ = app.mpv.play(&path, audio_only).await;
            app.status_message = Some(format!("Playing: {name}"));
        }
    } else {
        let _ = app.mpv.toggle_pause().await;
    }
}

async fn handle_move_input(app: &mut App, key: KeyEvent) {
    let Some(ref mut text) = app.move_input else {
        return;
    };
    match key.code {
        KeyCode::Esc => {
            app.move_input = None;
            app.status_message = Some("Move cancelled".to_string());
        }
        KeyCode::Enter => {
            let dest = text.clone();
            app.move_input = None;
            triage::execute_move(app, &dest).await;
        }
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(c) => {
            text.push(c);
        }
        _ => {}
    }
}

fn open_file(path: &std::path::Path) -> Result<()> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        anyhow::bail!("unsupported platform for open_file")
    };
    std::process::Command::new(cmd)
        .arg(path)
        .spawn()
        .context("failed to open file")?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::{ContainerInfo, FsInfo, MediaInfo, MediaKind, MediaTags, ProbeInfo};

    fn make_entry(name: &str) -> MediaEntry {
        MediaEntry {
            path: PathBuf::from(format!("/test/{name}")),
            file_name: name.to_string(),
            extension: name.rsplit('.').next().unwrap_or("").to_string(),
            fs: FsInfo {
                size_bytes: 100,
                modified_at: None,
                created_at: None,
            },
            media: MediaInfo {
                kind: MediaKind::Video,
                container: ContainerInfo {
                    format_name: "mp4".to_string(),
                    format_primary: "mp4".to_string(),
                },
                duration_ms: None,
                overall_bitrate_bps: None,
                video: None,
                audio: None,
                streams: vec![],
                tags: MediaTags::default(),
            },
            probe: ProbeInfo {
                backend: "ffprobe".to_string(),
                took_ms: 10,
                error: None,
            },
        }
    }

    fn make_test_app(names: &[&str]) -> App {
        let entries: Vec<MediaEntry> = names.iter().map(|n| make_entry(n)).collect();
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        App::new(entries, vec![], PathBuf::from("/test"), thumb_cache, picker)
    }

    #[test]
    fn fuzzy_empty_shows_all() {
        let mut app = make_test_app(&["a.mp4", "b.mkv", "c.mp3"]);
        app.filter_text = String::new();
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn fuzzy_matches_subsequence() {
        let mut app = make_test_app(&["my_video.mp4", "readme.txt", "movie.mkv"]);
        app.filter_text = "mvp4".to_string();
        app.apply_filter();
        // "mvp4" should fuzzy-match "my_video.mp4" (m..v..p..4)
        assert!(!app.filtered_indices.is_empty());
        let matched_names: Vec<&str> = app
            .filtered_indices
            .iter()
            .map(|&i| app.entries[i].file_name.as_str())
            .collect();
        assert!(matched_names.contains(&"my_video.mp4"));
    }

    #[test]
    fn fuzzy_no_match_shows_empty() {
        let mut app = make_test_app(&["a.mp4", "b.mkv"]);
        app.filter_text = "zzzzzzz".to_string();
        app.apply_filter();
        assert!(app.filtered_indices.is_empty());
    }

    #[test]
    fn fuzzy_results_sorted_by_score() {
        let mut app = make_test_app(&["zzz_mp4_zzz.txt", "mp4_file.mp4", "other.mkv"]);
        app.filter_text = "mp4".to_string();
        app.apply_filter();
        // Exact prefix "mp4_file.mp4" should rank higher than scattered match
        if app.filtered_indices.len() >= 2 {
            let first = &app.entries[app.filtered_indices[0]].file_name;
            assert_eq!(first, "mp4_file.mp4");
        }
    }

    #[test]
    fn fuzzy_selected_clamped_when_filter_narrows() {
        let mut app = make_test_app(&["a.mp4", "b.mkv", "c.mp3"]);
        app.selected = 2;
        app.filter_text = "a".to_string();
        app.apply_filter();
        // Only 1 result, selected should clamp to 0
        assert!(app.selected < app.filtered_indices.len());
    }

    #[test]
    fn thumb_skips_audio_only_files() {
        // make_entry creates entries with video: None (audio-only)
        let mut app = make_test_app(&["song.mp3"]);
        app.kick_thumbnail_fetch();
        assert!(matches!(app.thumb_state, ThumbState::Empty));
        assert!(app.thumb_path.is_none());
    }

    #[test]
    fn thumb_empty_when_no_selection() {
        let mut app = make_test_app(&[]);
        app.kick_thumbnail_fetch();
        assert!(matches!(app.thumb_state, ThumbState::Empty));
    }
}
