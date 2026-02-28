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
    #[expect(dead_code, reason = "move-to-directory triage action not yet implemented")]
    Move {
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
                        app.status_message = Some(
                            "Failed to trash file (install: brew install trash)".to_string(),
                        );
                    }
                }
            }
        }
        KeyCode::Char('m') => {
            app.status_message = Some(
                "Move: directory picker not yet implemented".to_string(),
            );
        }
        KeyCode::Char('u') => {
            let action = app
                .triage
                .as_mut()
                .and_then(|t| t.history.pop());
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
                let info = app.selected_entry().map(|entry| {
                    (entry.path.clone(), entry.media.video.is_none())
                });
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

#[cfg(test)]
mod tests {
    use super::*;

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

}
