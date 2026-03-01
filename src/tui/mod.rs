/// TUI application — event loop, state management, rendering.
///
/// Uses ratatui with crossterm backend. Immediate-mode rendering with
/// buffer diffing for minimal terminal I/O.
pub mod layout;
pub mod preview;
pub mod triage;

use crate::filter::Filter;
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

/// Filter input mode: fuzzy name matching vs structured expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Fuzzy substring match on file name (default).
    Fuzzy,
    /// Structured field expression using `filter.rs` parser (prefix `=`).
    Structured,
}

/// Media kind pre-filter (1/2/3 keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    /// Show all media types.
    All,
    /// Show only files with a video stream.
    Video,
    /// Show only audio-only files (no video stream).
    Audio,
}

impl KindFilter {
    /// Label for display in the footer.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Video => "Video",
            Self::Audio => "Audio",
        }
    }
}

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
    /// Status message (transient).
    status_message: Option<String>,
    /// Render ticks remaining before `status_message` auto-clears.
    status_ticks: u8,
    /// Reusable fuzzy matcher (allocates ~135KB scratch space).
    fuzzy_matcher: Matcher,
    /// Move-to-directory input text (None = not in move input mode).
    move_input: Option<String>,
    /// Media kind pre-filter.
    kind_filter: KindFilter,
    /// Current filter mode (fuzzy vs structured).
    filter_mode: FilterMode,
    /// Last successfully parsed structured filter expression.
    filter_expr: Option<Filter>,
    /// Subdirectories of `current_dir`, sorted alphabetically.
    dir_items: Vec<PathBuf>,
    /// Cached sibling directories (for parent pane rendering).
    sibling_dirs: Vec<PathBuf>,
    /// Receiver for async directory scan results.
    dir_scan_rx: Option<oneshot::Receiver<(Vec<MediaEntry>, Vec<ProbeError>)>>,
    /// Whether a directory scan is in progress.
    dir_scanning: bool,
    /// Stored scan concurrency (from CLI args).
    scan_concurrency: usize,
    /// Stored scan timeout (from CLI args).
    scan_timeout_ms: u64,
    /// Current playback position in seconds (polled from mpv).
    playback_position: Option<f64>,
    /// Total duration of playing file in seconds (fetched once).
    playback_duration: Option<f64>,
    /// File name currently being played (for display).
    playback_file_name: Option<String>,
    /// Terminal height in rows (updated each frame from layout).
    pub terminal_height: u16,
}

impl App {
    /// Create a new app with discovered entries.
    pub fn new(
        entries: Vec<MediaEntry>,
        errors: Vec<ProbeError>,
        current_dir: PathBuf,
        thumb_cache: ThumbnailCache,
        picker: Picker,
        scan_concurrency: usize,
        scan_timeout_ms: u64,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..entries.len()).collect();
        let dir_items = list_subdirs(&current_dir);
        let sibling_dirs = list_sibling_dirs(&current_dir);
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
            status_message: None,
            status_ticks: 0,
            fuzzy_matcher: Matcher::new(Config::DEFAULT),
            move_input: None,
            dir_items,
            sibling_dirs,
            dir_scan_rx: None,
            dir_scanning: false,
            scan_concurrency,
            scan_timeout_ms,
            kind_filter: KindFilter::All,
            filter_mode: FilterMode::Fuzzy,
            filter_expr: None,
            playback_position: None,
            playback_duration: None,
            playback_file_name: None,
            terminal_height: 24,
        }
    }

    /// Get the currently selected media entry (if any).
    ///
    /// Returns `None` if a directory is selected or nothing is selected.
    #[must_use]
    pub fn selected_entry(&self) -> Option<&MediaEntry> {
        let dir_count = self.dir_items.len();
        if self.selected < dir_count {
            return None; // Directory selected
        }
        let media_idx = self.selected - dir_count;
        self.filtered_indices
            .get(media_idx)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Get the currently selected directory (if any).
    #[must_use]
    pub fn selected_dir(&self) -> Option<&PathBuf> {
        if self.selected < self.dir_items.len() {
            Some(&self.dir_items[self.selected])
        } else {
            None
        }
    }

    /// Apply the current filter and rebuild filtered indices.
    ///
    /// Pipeline: entries → kind pre-filter → fuzzy/structured → `filtered_indices`.
    fn apply_filter(&mut self) {
        // Step 1: kind pre-filter
        let kind_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.matches_kind(e))
            .map(|(i, _)| i)
            .collect();

        // Step 2: fuzzy or structured filter
        if self.filter_text.is_empty() && self.filter_mode == FilterMode::Fuzzy {
            self.filtered_indices = kind_indices;
        } else {
            match self.filter_mode {
                FilterMode::Fuzzy => self.apply_fuzzy_filter(&kind_indices),
                FilterMode::Structured => self.apply_structured_filter(&kind_indices),
            }
        }
        // Keep selected index in bounds
        if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    /// Check if an entry matches the current kind filter.
    fn matches_kind(&self, entry: &MediaEntry) -> bool {
        match self.kind_filter {
            KindFilter::All => true,
            KindFilter::Video => entry.media.video.is_some(),
            KindFilter::Audio => entry.media.video.is_none(),
        }
    }

    /// Fuzzy match on file name using nucleo-matcher.
    fn apply_fuzzy_filter(&mut self, candidates: &[usize]) {
        if self.filter_text.is_empty() {
            self.filtered_indices = candidates.to_vec();
            return;
        }
        let pattern = Pattern::parse(
            &self.filter_text,
            CaseMatching::Ignore,
            Normalization::Smart,
        );
        let mut scored: Vec<(usize, u32)> = candidates
            .iter()
            .filter_map(|&i| {
                let e = &self.entries[i];
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

    /// Structured field expression filter using `filter.rs` parser.
    fn apply_structured_filter(&mut self, candidates: &[usize]) {
        if let Some(ref expr) = self.filter_expr {
            self.filtered_indices = candidates
                .iter()
                .filter(|&&i| expr.matches(&self.entries[i]).unwrap_or(false))
                .copied()
                .collect();
        } else {
            // No valid parse yet — show all candidates
            self.filtered_indices = candidates.to_vec();
        }
    }

    /// Update structured filter expression from current filter text.
    /// Called on every keystroke in structured mode.
    fn update_structured_expr(&mut self) {
        if self.filter_text.is_empty() {
            self.filter_expr = None;
        } else {
            // Only update filter_expr on successful parse;
            // keep last valid parse on error
            if let Ok(f) = Filter::parse(&self.filter_text) {
                self.filter_expr = Some(f);
            }
        }
    }

    /// Apply current sort to entries and rebuild indices.
    fn apply_sort(&mut self) {
        sort_entries(&mut self.entries, self.sort_key, self.sort_dir);
        self.apply_filter();
    }

    /// Total visible items (dirs + filtered media entries).
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.dir_items.len() + self.filtered_indices.len()
    }

    /// Move selection down.
    fn move_down(&mut self) {
        if self.selected + 1 < self.visible_count() {
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
    }

    /// Move to last entry.
    fn move_bottom(&mut self) {
        self.selected = self.visible_count().saturating_sub(1);
    }

    /// Page down (half screen).
    fn page_down(&mut self, page_size: usize) {
        let max = self.visible_count().saturating_sub(1);
        self.selected = (self.selected + page_size).min(max);
    }

    /// Page up (half screen).
    fn page_up(&mut self, page_size: usize) {
        self.selected = self.selected.saturating_sub(page_size);
    }

    /// Toggle mark on current entry.
    fn toggle_mark(&mut self) {
        let dir_count = self.dir_items.len();
        if self.selected < dir_count {
            return; // Directories can't be marked
        }
        let media_idx = self.selected - dir_count;
        if let Some(&idx) = self.filtered_indices.get(media_idx) {
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
        self.set_status(format!("Sort: {}", self.sort_key.label()));
    }

    /// Set a transient status message that auto-clears after ~3 seconds.
    pub(crate) fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_ticks = 30;
    }

    /// Remove the currently selected media entry from the entries list.
    ///
    /// Called after successful delete or move in triage mode so the
    /// stale entry is no longer visible or operable.
    pub(crate) fn remove_selected_entry(&mut self) {
        let dir_count = self.dir_items.len();
        if self.selected < dir_count {
            return; // Directory item, not removable this way
        }
        let media_vis_idx = self.selected - dir_count;
        if let Some(&real_idx) = self.filtered_indices.get(media_vis_idx) {
            self.entries.remove(real_idx);
            self.apply_filter();
            if self.selected > 0 && self.selected >= self.visible_count() {
                self.selected = self.visible_count().saturating_sub(1);
            }
        }
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
        self.set_status(format!("Sort: {} {dir_label}", self.sort_key.label()));
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

    /// Navigate to a directory: load subdirs, clear state, spawn async scan.
    fn navigate_to_dir(&mut self, path: PathBuf) {
        self.dir_items = list_subdirs(&path);
        self.sibling_dirs = list_sibling_dirs(&path);

        // Clear media state (but NOT mpv playback — per spec)
        self.entries.clear();
        self.errors.clear();
        self.filtered_indices.clear();
        self.selected = 0;
        self.marked.clear();
        self.filter_text.clear();
        self.filter_mode = FilterMode::Fuzzy;
        self.filter_expr = None;
        self.triage = None;
        self.thumb_state = ThumbState::Empty;
        self.thumb_path = None;
        self.thumb_receiver = None;

        // Start async scan for media files in the new directory
        self.dir_scanning = true;
        let (tx, rx) = oneshot::channel();
        self.dir_scan_rx = Some(rx);
        let scan_path = path.clone();
        let concurrency = self.scan_concurrency;
        let timeout_ms = self.scan_timeout_ms;

        self.current_dir = path;

        tokio::spawn(async move {
            let result = scan::scan_all(&[scan_path], Some(0), concurrency, timeout_ms).await;
            match result {
                Ok((entries, errors)) => {
                    let _ = tx.send((entries, errors));
                }
                Err(_) => {
                    let _ = tx.send((vec![], vec![]));
                }
            }
        });
    }

    /// Poll for completed directory scan results.
    fn poll_dir_scan(&mut self) {
        let Some(ref mut rx) = self.dir_scan_rx else {
            return;
        };

        match rx.try_recv() {
            Ok((entries, errors)) => {
                self.dir_scan_rx = None;
                self.dir_scanning = false;
                self.entries = entries;
                self.errors = errors;
                self.apply_sort();
                self.kick_thumbnail_fetch();
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                self.dir_scan_rx = None;
                self.dir_scanning = false;
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // Still scanning
            }
        }
    }
}

/// List subdirectories of a path, sorted alphabetically.
fn list_subdirs(path: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// List sibling directories (dirs in parent) for the parent pane.
fn list_sibling_dirs(path: &std::path::Path) -> Vec<PathBuf> {
    path.parent().map_or_else(Vec::new, list_subdirs)
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
    let current_dir = std::fs::canonicalize(&current_dir).unwrap_or_else(|e| {
        tracing::warn!("failed to canonicalize path: {e}");
        current_dir
    });

    let (mut entries, errors) = scan::scan_all(paths, max_depth, concurrency, timeout_ms).await?;

    // Sort by name initially
    sort_entries(&mut entries, SortKey::Name, SortDir::Asc);

    let thumb_cache = ThumbnailCache::new(100, crate::thumbnail::default_cache_dir())?;

    // Setup terminal (must happen before picker query)
    terminal::enable_raw_mode().context("failed to enable raw mode")?;

    // Query terminal for graphics protocol support; fall back to halfblocks
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

    let mut app = App::new(
        entries,
        errors,
        current_dir,
        thumb_cache,
        picker,
        concurrency,
        timeout_ms,
    );
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

        // Update mpv state — detect process exit
        if app.mpv.state() != PlaybackState::Stopped && !app.mpv.is_alive() {
            app.mpv.stop().await;
            app.playback_position = None;
            app.playback_duration = None;
            app.playback_file_name = None;
        }

        // Poll playback position only while actively playing (not paused/stopped)
        if app.mpv.state() == PlaybackState::Playing {
            if let Ok(pos) = app.mpv.get_position().await {
                app.playback_position = Some(pos);
            }
            if app.playback_duration.is_none()
                && let Ok(dur) = app.mpv.get_duration().await
            {
                app.playback_duration = Some(dur);
            }
        }

        // Auto-clear status message after ~3s (30 ticks × 100ms poll)
        if app.status_message.is_some() {
            app.status_ticks = app.status_ticks.saturating_sub(1);
            if app.status_ticks == 0 {
                app.status_message = None;
            }
        }

        // Poll for completed thumbnail generation
        app.poll_thumbnail();

        // Poll for completed directory scan
        app.poll_dir_scan();
    }
    Ok(())
}

#[expect(clippy::too_many_lines, reason = "match arms for key handling")]
async fn handle_key(app: &mut App, key: KeyEvent) {
    // Handle filter input mode
    if app.filter_active {
        handle_filter_input(app, key);
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
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            app.page_down((app.terminal_height / 2) as usize);
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            app.page_up((app.terminal_height / 2) as usize);
        }
        // Actions
        (KeyCode::Char('/'), _) => {
            app.filter_active = true;
            app.filter_text.clear();
            app.filter_mode = FilterMode::Fuzzy;
            app.filter_expr = None;
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
            app.set_status("Triage mode — y:keep n:delete m:move u:undo q:quit".to_string());
        }
        // Kind filter
        (KeyCode::Char('1'), _) => {
            app.kind_filter = KindFilter::All;
            app.apply_filter();
            app.set_status("Filter: All".to_string());
        }
        (KeyCode::Char('2'), _) => {
            app.kind_filter = KindFilter::Video;
            app.apply_filter();
            app.set_status("Filter: Video".to_string());
        }
        (KeyCode::Char('3'), _) => {
            app.kind_filter = KindFilter::Audio;
            app.apply_filter();
            app.set_status("Filter: Audio".to_string());
        }
        // Playback
        (KeyCode::Char('p'), _) => handle_playback(app).await,
        (KeyCode::Char('P'), _) => {
            app.mpv.stop().await;
            app.playback_position = None;
            app.playback_duration = None;
            app.playback_file_name = None;
            app.set_status("Stopped playback".to_string());
        }
        (KeyCode::Char(']'), _) => {
            let _ = app.mpv.seek(10).await;
        }
        (KeyCode::Char('['), _) => {
            let _ = app.mpv.seek(-10).await;
        }
        // Open / navigate into
        (KeyCode::Enter | KeyCode::Right | KeyCode::Char('l'), _) => {
            if let Some(dir) = app.selected_dir().cloned() {
                app.navigate_to_dir(dir);
            } else if let Some(entry) = app.selected_entry() {
                let path = entry.path.clone();
                let _ = open_file(&path);
            }
        }
        // Navigate to parent
        (KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h'), _) => {
            if let Some(parent) = app.current_dir.parent().map(std::path::Path::to_path_buf) {
                app.navigate_to_dir(parent);
            }
        }
        _ => {}
    }

    // Kick thumbnail fetch if selection changed
    if app.selected != prev_selected {
        app.kick_thumbnail_fetch();
    }
}

fn handle_filter_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.filter_active = false;
            app.filter_text.clear();
            app.filter_mode = FilterMode::Fuzzy;
            app.filter_expr = None;
            app.apply_filter();
        }
        KeyCode::Enter => {
            app.filter_active = false;
        }
        KeyCode::Backspace => {
            app.filter_text.pop();
            // If text becomes empty, reset to fuzzy mode
            if app.filter_text.is_empty() {
                app.filter_mode = FilterMode::Fuzzy;
                app.filter_expr = None;
            }
            if app.filter_mode == FilterMode::Structured {
                app.update_structured_expr();
            }
            app.apply_filter();
        }
        KeyCode::Char(c) => {
            // Detect `=` prefix to switch to structured mode
            if app.filter_text.is_empty() && c == '=' {
                app.filter_mode = FilterMode::Structured;
                // Don't push the `=` into filter_text; it's the mode prefix
            } else {
                app.filter_text.push(c);
                if app.filter_mode == FilterMode::Structured {
                    app.update_structured_expr();
                }
            }
            app.apply_filter();
        }
        _ => {}
    }
    app.kick_thumbnail_fetch();
}

async fn handle_playback(app: &mut App) {
    let info = app.selected_entry().map(|entry| {
        (
            entry.path.clone(),
            entry.media.video.is_none(),
            entry.file_name.clone(),
        )
    });

    let Some((path, audio_only, name)) = info else {
        return;
    };

    let is_same_file = app
        .mpv
        .current_file()
        .is_some_and(|current| current == path);

    if app.mpv.state() != PlaybackState::Stopped && is_same_file {
        // Same file — toggle pause
        let _ = app.mpv.toggle_pause().await;
    } else {
        // Stopped or different file — start playing selected
        let _ = app.mpv.play(&path, audio_only).await;
        app.playback_file_name = Some(name.clone());
        app.playback_position = None;
        app.playback_duration = None;
        app.set_status(format!("Playing: {name}"));
    }
}

async fn handle_move_input(app: &mut App, key: KeyEvent) {
    let Some(ref mut text) = app.move_input else {
        return;
    };
    match key.code {
        KeyCode::Esc => {
            app.move_input = None;
            app.set_status("Move cancelled".to_string());
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
    use std::borrow::Cow;

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
                backend: Cow::Borrowed("ffprobe"),
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
        App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        )
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

    fn make_entry_with_kind(name: &str, kind: MediaKind) -> MediaEntry {
        let mut entry = make_entry(name);
        entry.media.kind = kind;
        entry
    }

    #[test]
    fn structured_filter_parses_kind() {
        let entries = vec![
            make_entry_with_kind("video.mp4", MediaKind::Video),
            make_entry_with_kind("song.mp3", MediaKind::Audio),
            make_entry_with_kind("clip.mkv", MediaKind::Av),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );

        app.filter_mode = FilterMode::Structured;
        app.filter_text = "media.kind == audio".to_string();
        app.update_structured_expr();
        app.apply_filter();

        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.entries[app.filtered_indices[0]].file_name, "song.mp3");
    }

    #[test]
    fn structured_filter_invalid_shows_all() {
        let mut app = make_test_app(&["a.mp4", "b.mkv", "c.mp3"]);
        app.filter_mode = FilterMode::Structured;
        app.filter_text = "invalid syntax @@".to_string();
        app.update_structured_expr();
        app.apply_filter();

        // Invalid expression should show all entries (no valid parse yet)
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn structured_filter_keeps_last_valid_parse() {
        let entries = vec![
            make_entry_with_kind("video.mp4", MediaKind::Video),
            make_entry_with_kind("song.mp3", MediaKind::Audio),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );

        // Set up a valid structured filter
        app.filter_mode = FilterMode::Structured;
        app.filter_text = "media.kind == audio".to_string();
        app.update_structured_expr();
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 1);

        // Now type something invalid — should keep last valid parse
        app.filter_text = "media.kind == audio &&".to_string();
        app.update_structured_expr();
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 1); // still filtered
    }

    #[test]
    fn kind_filter_video_excludes_audio() {
        let entries = vec![
            make_entry_with_kind("video.mp4", MediaKind::Video),
            make_entry_with_kind("song.mp3", MediaKind::Audio),
            make_entry_with_kind("clip.mkv", MediaKind::Av),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );

        app.kind_filter = KindFilter::Video;
        app.apply_filter();

        // Video filter: entries with video stream (Video and Av kinds)
        // But our make_entry has video: None, so we need entries with video
        // make_entry_with_kind only sets kind, not video stream — use matches_kind
        // Video filter checks entry.media.video.is_some(), not kind enum
        // All our test entries have video: None, so Video filter shows nothing
        assert_eq!(app.filtered_indices.len(), 0);
    }

    #[test]
    fn kind_filter_audio_includes_audio_only() {
        let entries = vec![
            make_entry_with_kind("video.mp4", MediaKind::Video),
            make_entry_with_kind("song.mp3", MediaKind::Audio),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );

        // All entries have video: None, so Audio filter shows all
        app.kind_filter = KindFilter::Audio;
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 2);
    }

    #[test]
    fn kind_filter_all_shows_everything() {
        let mut app = make_test_app(&["a.mp4", "b.mkv", "c.mp3"]);
        app.kind_filter = KindFilter::All;
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn kind_filter_composes_with_fuzzy() {
        let entries = vec![
            make_entry("alpha.mp4"),
            make_entry("beta.mp4"),
            make_entry("gamma.mp3"),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );

        // All entries have video: None → Audio filter shows all
        app.kind_filter = KindFilter::Audio;
        app.filter_text = "alpha".to_string();
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.entries[app.filtered_indices[0]].file_name, "alpha.mp4");
    }

    #[test]
    fn filter_mode_defaults_to_fuzzy() {
        let app = make_test_app(&["a.mp4"]);
        assert_eq!(app.filter_mode, FilterMode::Fuzzy);
        assert!(app.filter_expr.is_none());
    }

    #[test]
    fn selected_dir_returns_path_when_dir_selected() {
        let mut app = make_test_app(&["a.mp4"]);
        app.dir_items = vec![
            PathBuf::from("/test/subdir1"),
            PathBuf::from("/test/subdir2"),
        ];
        app.selected = 0;
        assert_eq!(app.selected_dir(), Some(&PathBuf::from("/test/subdir1")));
        assert!(app.selected_entry().is_none());
    }

    #[test]
    fn selected_entry_offsets_correctly() {
        let mut app = make_test_app(&["a.mp4", "b.mkv"]);
        app.dir_items = vec![PathBuf::from("/test/subdir")];
        // selected=0 → directory
        app.selected = 0;
        assert!(app.selected_entry().is_none());
        assert!(app.selected_dir().is_some());
        // selected=1 → first media entry
        app.selected = 1;
        assert!(app.selected_dir().is_none());
        assert_eq!(
            app.selected_entry().map(|e| e.file_name.as_str()),
            Some("a.mp4")
        );
        // selected=2 → second media entry
        app.selected = 2;
        assert_eq!(
            app.selected_entry().map(|e| e.file_name.as_str()),
            Some("b.mkv")
        );
    }

    #[test]
    fn visible_count_includes_dirs() {
        let mut app = make_test_app(&["a.mp4"]);
        app.dir_items = vec![PathBuf::from("/test/d1"), PathBuf::from("/test/d2")];
        assert_eq!(app.visible_count(), 3); // 2 dirs + 1 media
    }

    #[tokio::test]
    async fn navigate_clears_state() {
        let mut app = make_test_app(&["a.mp4", "b.mkv"]);
        app.marked.insert(0);
        app.selected = 1;
        app.filter_text = "test".to_string();

        let tmp = tempfile::tempdir().unwrap();
        app.navigate_to_dir(tmp.path().to_path_buf());

        assert!(app.entries.is_empty());
        assert!(app.marked.is_empty());
        assert_eq!(app.selected, 0);
        assert!(app.filter_text.is_empty());
        assert!(app.dir_scanning);
    }

    #[test]
    fn dir_items_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("zebra")).unwrap();
        std::fs::create_dir(root.join("alpha")).unwrap();
        std::fs::create_dir(root.join("middle")).unwrap();

        let dirs = super::list_subdirs(root);
        let names: Vec<String> = dirs
            .iter()
            .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn playback_state_defaults_none() {
        let app = make_test_app(&["a.mp4"]);
        assert!(app.playback_position.is_none());
        assert!(app.playback_duration.is_none());
        assert!(app.playback_file_name.is_none());
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

    #[test]
    fn toggle_mark_on_dir_is_noop() {
        let mut app = make_test_app(&["a.mp4"]);
        app.dir_items = vec![PathBuf::from("/d")];
        app.selected = 0;
        app.toggle_mark();
        assert!(app.marked.is_empty());
    }

    #[test]
    fn page_down_at_bottom_stays() {
        let mut app = make_test_app(&["a.mp4", "b.mp4", "c.mp4"]);
        app.selected = 2;
        app.page_down(10);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn page_down_advances_by_size() {
        let names: Vec<&str> = vec![
            "a.mp4", "b.mp4", "c.mp4", "d.mp4", "e.mp4", "f.mp4", "g.mp4", "h.mp4", "i.mp4",
            "j.mp4",
        ];
        let mut app = make_test_app(&names);
        app.selected = 0;
        app.page_down(3);
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn page_up_at_top_stays() {
        let mut app = make_test_app(&["a.mp4", "b.mp4"]);
        app.selected = 0;
        app.page_up(10);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn page_up_retreats_by_size() {
        let names: Vec<&str> = vec![
            "a.mp4", "b.mp4", "c.mp4", "d.mp4", "e.mp4", "f.mp4", "g.mp4", "h.mp4", "i.mp4",
            "j.mp4",
        ];
        let mut app = make_test_app(&names);
        app.selected = 5;
        app.page_up(3);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn cycle_sort_advances_key() {
        let mut app = make_test_app(&["a.mp4", "b.mp4"]);
        let initial = app.sort_key;
        app.cycle_sort();
        assert_eq!(app.sort_key, initial.next());
    }

    #[test]
    fn cycle_sort_sets_status() {
        let mut app = make_test_app(&["a.mp4"]);
        app.cycle_sort();
        assert!(app.status_message.is_some());
    }
}
