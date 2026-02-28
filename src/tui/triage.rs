/// Triage mode — single-keypress keep/delete/move workflow.
///
/// In triage mode, files are presented one at a time.
/// y = keep (skip), n = delete (move to trash), m = move to directory.
/// u = undo last action. q = exit triage mode.
use super::App;
use crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

/// A triage action that can be undone.
#[derive(Debug, Clone)]
pub enum TriageAction {
    Keep {
        #[expect(dead_code, reason = "will be used for triage undo navigation")]
        index: usize,
    },
    Delete {
        #[expect(dead_code, reason = "will be used for triage undo navigation")]
        index: usize,
        path: PathBuf,
    },
    Move {
        #[expect(dead_code, reason = "will be used for triage undo navigation")]
        index: usize,
        from: PathBuf,
        to: PathBuf,
    },
}

/// Triage mode state.
#[derive(Debug)]
pub struct TriageState {
    /// Current position in the triage queue.
    pub current: usize,
    /// Total files to triage.
    pub total: usize,
    /// Number of files kept.
    pub kept: usize,
    /// Number of files deleted.
    pub deleted: usize,
    /// Number of files moved.
    pub moved: usize,
    /// Action history for undo.
    pub history: Vec<TriageAction>,
}

impl TriageState {
    /// Create a new triage state.
    #[must_use]
    pub fn new(total: usize) -> Self {
        Self {
            current: 0,
            total,
            kept: 0,
            deleted: 0,
            moved: 0,
            history: Vec::new(),
        }
    }

    fn advance(&mut self) {
        if self.current + 1 < self.total {
            self.current += 1;
        }
    }
}

/// Handle a key event in triage mode.
#[expect(clippy::too_many_lines)]
pub async fn handle_triage_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            let msg = app.triage.as_ref().map(|t| {
                format!(
                    "Triage complete: {} kept, {} deleted, {} moved",
                    t.kept, t.deleted, t.moved
                )
            });
            app.triage = None;
            app.move_input = None;
            if let Some(msg) = msg {
                app.status_message = Some(msg);
            }
        }
        KeyCode::Char('y') => {
            if let Some(ref mut triage) = app.triage {
                let idx = triage.current;
                triage.history.push(TriageAction::Keep { index: idx });
                triage.kept += 1;
                triage.advance();
                app.sync_triage_selection();
            }
        }
        KeyCode::Char('n') => {
            // Get file info before borrowing triage mutably
            let file_info = app.selected_entry().map(|e| e.path.clone());
            if let Some(path) = file_info {
                // Try to use `trash` command (macOS safe delete)
                let result = tokio::process::Command::new("trash")
                    .arg(&path)
                    .output()
                    .await;

                match result {
                    Ok(output) if output.status.success() => {
                        if let Some(ref mut triage) = app.triage {
                            let idx = triage.current;
                            triage
                                .history
                                .push(TriageAction::Delete { index: idx, path });
                            triage.deleted += 1;
                            triage.advance();
                            app.sync_triage_selection();
                        }
                        app.status_message = Some("Moved to trash".to_string());
                    }
                    _ => {
                        app.status_message =
                            Some("Failed to trash file (install: brew install trash)".to_string());
                    }
                }
            }
        }
        KeyCode::Char('m') => {
            if let Some(entry) = app.selected_entry() {
                let parent = entry
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                app.move_input = Some(parent);
                app.status_message = Some("Enter destination directory".to_string());
            }
        }
        KeyCode::Char('u') => {
            let action = app.triage.as_mut().and_then(|t| t.history.pop());
            if let Some(action) = action {
                match action {
                    TriageAction::Keep { .. } => {
                        if let Some(ref mut triage) = app.triage {
                            triage.kept = triage.kept.saturating_sub(1);
                            triage.current = triage.current.saturating_sub(1);
                            app.sync_triage_selection();
                        }
                        app.status_message = Some("Undid keep".to_string());
                    }
                    TriageAction::Delete { path, .. } => {
                        // Delete cannot be undone — file is in macOS system
                        // Trash. Don't adjust counters or cursor; just inform
                        // the user. The action is consumed from history so
                        // subsequent undos operate on prior actions.
                        app.status_message = Some(format!(
                            "Cannot undo delete \u{2014} file is in system Trash: {}",
                            path.display()
                        ));
                    }
                    TriageAction::Move { from, to, .. } => {
                        let result = tokio::fs::rename(&to, &from).await;
                        if result.is_ok() {
                            if let Some(ref mut triage) = app.triage {
                                triage.moved = triage.moved.saturating_sub(1);
                                triage.current = triage.current.saturating_sub(1);
                                app.sync_triage_selection();
                            }
                            app.status_message = Some("Undid move".to_string());
                        } else {
                            app.status_message = Some("Failed to undo move".to_string());
                        }
                    }
                }
            } else {
                app.status_message = Some("Nothing to undo".to_string());
            }
        }
        // Navigation within triage
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(ref mut triage) = app.triage {
                triage.advance();
                app.sync_triage_selection();
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some(ref mut triage) = app.triage {
                triage.current = triage.current.saturating_sub(1);
                app.sync_triage_selection();
            }
        }
        // Playback in triage
        KeyCode::Char('p') => {
            if app.mpv.state() == crate::playback::PlaybackState::Stopped {
                let info = app
                    .selected_entry()
                    .map(|entry| (entry.path.clone(), entry.media.video.is_none()));
                if let Some((path, audio_only)) = info {
                    let _ = app.mpv.play(&path, audio_only).await;
                }
            } else {
                let _ = app.mpv.toggle_pause().await;
            }
        }
        _ => {}
    }
}

/// Execute a move operation: move the currently selected file to the
/// given destination directory. Handles cross-device moves (EXDEV)
/// by falling back to copy + remove.
pub async fn execute_move(app: &mut App, dest_dir: &str) {
    let dest = PathBuf::from(dest_dir);

    if !dest.is_dir() {
        app.status_message = Some(format!("Not a directory: {dest_dir}"));
        return;
    }

    let Some(entry) = app.selected_entry() else {
        app.status_message = Some("No file selected".to_string());
        return;
    };

    let from = entry.path.clone();
    let file_name = entry.file_name.clone();
    let to = dest.join(&file_name);

    if to.exists() {
        app.status_message = Some(format!("File already exists: {}", to.display()));
        return;
    }

    // Try rename first (fast, same-filesystem)
    let result = tokio::fs::rename(&from, &to).await;
    let ok = match result {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            // Cross-device: fall back to copy + remove
            match tokio::fs::copy(&from, &to).await {
                Ok(_) => match tokio::fs::remove_file(&from).await {
                    Ok(()) => true,
                    Err(e) => {
                        // Copy succeeded but remove failed — try to clean up
                        let _ = tokio::fs::remove_file(&to).await;
                        app.status_message = Some(format!("Failed to remove original: {e}"));
                        false
                    }
                },
                Err(e) => {
                    app.status_message = Some(format!("Failed to copy file: {e}"));
                    false
                }
            }
        }
        Err(e) => {
            app.status_message = Some(format!("Failed to move file: {e}"));
            false
        }
    };

    if ok {
        if let Some(ref mut triage) = app.triage {
            let idx = triage.current;
            triage.history.push(TriageAction::Move {
                index: idx,
                from,
                to,
            });
            triage.moved += 1;
            triage.advance();
            app.sync_triage_selection();
        }
        app.status_message = Some(format!("Moved {file_name} to {dest_dir}"));
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::thumbnail::ThumbnailCache;
    use crate::types::{ContainerInfo, FsInfo, MediaInfo, MediaKind, MediaTags, ProbeInfo};

    fn make_entry(path: PathBuf) -> crate::types::MediaEntry {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        crate::types::MediaEntry {
            path,
            file_name,
            extension,
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

    fn make_triage_app(entries: Vec<crate::types::MediaEntry>) -> App {
        let count = entries.len();
        let tmp = tempfile::tempdir().unwrap();
        let thumb_cache = ThumbnailCache::new(10, tmp.path().to_path_buf()).unwrap();
        let picker = ratatui_image::picker::Picker::halfblocks();
        let mut app = App::new(
            entries,
            vec![],
            PathBuf::from("/test"),
            thumb_cache,
            picker,
            4,
            5000,
        );
        app.triage = Some(TriageState::new(count));
        app
    }

    #[test]
    fn triage_state_new_initializes_correctly() {
        let state = TriageState::new(10);
        assert_eq!(state.current, 0);
        assert_eq!(state.total, 10);
        assert_eq!(state.kept, 0);
        assert_eq!(state.deleted, 0);
        assert_eq!(state.moved, 0);
        assert!(state.history.is_empty());
    }

    #[test]
    fn triage_advance_increments_within_bounds() {
        let mut state = TriageState::new(3);
        state.advance();
        assert_eq!(state.current, 1);
        state.advance();
        assert_eq!(state.current, 2);
        // Should not advance past total - 1
        state.advance();
        assert_eq!(state.current, 2);
    }

    #[test]
    fn triage_advance_noop_when_total_zero() {
        let mut state = TriageState::new(0);
        state.advance();
        assert_eq!(state.current, 0);
    }

    #[test]
    fn triage_advance_noop_when_total_one() {
        let mut state = TriageState::new(1);
        state.advance();
        assert_eq!(state.current, 0);
    }

    #[tokio::test]
    async fn execute_move_moves_file_and_updates_state() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let file_path = src_dir.path().join("video.mp4");
        std::fs::write(&file_path, b"fake video data").unwrap();

        let entry = make_entry(file_path.clone());
        let mut app = make_triage_app(vec![entry]);

        execute_move(&mut app, &dest_dir.path().to_string_lossy()).await;

        // File should be at destination
        assert!(dest_dir.path().join("video.mp4").exists());
        // File should be gone from source
        assert!(!file_path.exists());
        // Triage state should be updated
        let triage = app.triage.as_ref().unwrap();
        assert_eq!(triage.moved, 1);
        assert_eq!(triage.history.len(), 1);
        assert!(matches!(triage.history[0], TriageAction::Move { .. }));
    }

    #[tokio::test]
    async fn execute_move_rejects_collision() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let file_path = src_dir.path().join("video.mp4");
        std::fs::write(&file_path, b"source data").unwrap();
        // Pre-existing file at destination
        std::fs::write(dest_dir.path().join("video.mp4"), b"existing").unwrap();

        let entry = make_entry(file_path.clone());
        let mut app = make_triage_app(vec![entry]);

        execute_move(&mut app, &dest_dir.path().to_string_lossy()).await;

        // Source should still exist (move was rejected)
        assert!(file_path.exists());
        // Triage state should NOT have advanced
        let triage = app.triage.as_ref().unwrap();
        assert_eq!(triage.moved, 0);
        assert!(triage.history.is_empty());
        // Status should mention collision
        let msg = app.status_message.as_ref().unwrap();
        assert!(msg.contains("already exists"));
    }

    #[tokio::test]
    async fn execute_move_rejects_non_directory() {
        let src_dir = tempfile::tempdir().unwrap();

        let file_path = src_dir.path().join("video.mp4");
        std::fs::write(&file_path, b"data").unwrap();

        let entry = make_entry(file_path.clone());
        let mut app = make_triage_app(vec![entry]);

        execute_move(&mut app, "/nonexistent/path").await;

        // Source should still exist
        assert!(file_path.exists());
        let triage = app.triage.as_ref().unwrap();
        assert_eq!(triage.moved, 0);
        let msg = app.status_message.as_ref().unwrap();
        assert!(msg.contains("Not a directory"));
    }
}
