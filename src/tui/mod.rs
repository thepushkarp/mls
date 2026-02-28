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
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    self, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

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
    /// Thumbnail cache.
    #[expect(dead_code, reason = "thumbnail preview not yet wired to UI")]
    thumb_cache: ThumbnailCache,
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
}

impl App {
    /// Create a new app with discovered entries.
    pub fn new(
        entries: Vec<MediaEntry>,
        errors: Vec<ProbeError>,
        current_dir: PathBuf,
        thumb_cache: ThumbnailCache,
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
            thumb_cache,
            triage: None,
            current_dir,
            should_quit: false,
            scroll_offset: 0,
            status_message: None,
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
    fn apply_filter(&mut self) {
        if self.filter_text.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let query = self.filter_text.to_lowercase();
            self.filtered_indices = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.file_name.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
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
    let current_dir = paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let current_dir = std::fs::canonicalize(&current_dir)
        .unwrap_or(current_dir);

    let (mut entries, errors) =
        scan::scan_all(paths, max_depth, concurrency, timeout_ms).await?;

    // Sort by name initially
    sort_entries(&mut entries, SortKey::Name, SortDir::Asc);

    let thumb_cache = ThumbnailCache::new(
        100,
        crate::thumbnail::default_cache_dir(),
    )?;

    let mut app = App::new(entries, errors, current_dir, thumb_cache);

    // Setup terminal
    terminal::enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    stdout
        .execute(EnterAlternateScreen)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .context("failed to create terminal")?;

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

    // Handle triage mode
    if app.triage.is_some() {
        triage::handle_triage_key(app, key).await;
        return;
    }

    // Normal mode
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
            app.status_message = Some("Triage mode — y:keep n:delete m:move u:undo q:quit".to_string());
        }
        // Playback
        (KeyCode::Char('p'), _) => {
            if app.mpv.state() == PlaybackState::Stopped {
                let info = app.selected_entry().map(|entry| {
                    (entry.path.clone(), entry.media.video.is_none(), entry.file_name.clone())
                });
                if let Some((path, audio_only, name)) = info {
                    let _ = app.mpv.play(&path, audio_only).await;
                    app.status_message = Some(format!("Playing: {name}"));
                }
            } else {
                let _ = app.mpv.toggle_pause().await;
            }
        }
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
}

fn open_file(path: &std::path::Path) -> Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .context("failed to open file")?;
    Ok(())
}
