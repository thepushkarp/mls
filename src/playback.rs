/// mpv playback controller via JSON IPC over Unix socket.
///
/// Manages an mpv subprocess with `--input-ipc-server` for transport controls:
/// play, pause, seek, volume, position queries.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::process::{Child, Command};

/// mpv playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Persistent IPC connection to mpv.
struct IpcConn {
    writer: OwnedWriteHalf,
    reader: BufReader<OwnedReadHalf>,
}

/// Controller for an mpv subprocess.
pub struct MpvController {
    socket_path: PathBuf,
    child: Option<Child>,
    state: PlaybackState,
    current_file: Option<PathBuf>,
    conn: Option<IpcConn>,
}

impl MpvController {
    /// Create a new controller (mpv not yet spawned).
    #[must_use]
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let socket_path =
            std::env::temp_dir().join(format!("mls_mpv_{}_{id}.sock", std::process::id()));
        Self {
            socket_path,
            child: None,
            state: PlaybackState::Stopped,
            current_file: None,
            conn: None,
        }
    }

    /// Current playback state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Currently playing file.
    #[must_use]
    pub fn current_file(&self) -> Option<&Path> {
        self.current_file.as_deref()
    }

    /// Play a file. If mpv is already running, loads the new file.
    ///
    /// `audio_only`: if true, passes `--no-video` (for audio files or
    /// audio-only playback within TUI).
    ///
    /// # Errors
    /// Returns an error if mpv fails to start or connect.
    pub async fn play(&mut self, path: &Path, audio_only: bool) -> Result<()> {
        // Clean up any existing socket
        let _ = tokio::fs::remove_file(&self.socket_path).await;

        // Kill existing process if any
        self.stop().await;

        let mut cmd = Command::new("mpv");
        cmd.arg(format!("--input-ipc-server={}", self.socket_path.display()));
        if audio_only {
            cmd.arg("--no-video");
        }
        cmd.arg("--really-quiet");
        cmd.arg(path);

        let child = cmd.spawn().context("failed to start mpv")?;
        self.child = Some(child);
        self.state = PlaybackState::Playing;
        self.current_file = Some(path.to_path_buf());

        // Wait for socket to appear
        let mut socket_ready = false;
        for _ in 0..20 {
            if self.socket_path.exists() {
                socket_ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        if !socket_ready {
            // mpv started but socket never appeared — check if process died
            if !self.is_alive() {
                self.state = PlaybackState::Stopped;
                anyhow::bail!("mpv process exited before IPC socket was ready");
            }
            tracing::warn!("mpv IPC socket not ready after 1s, continuing anyway");
        }

        Ok(())
    }

    /// Toggle play/pause.
    ///
    /// Note: internal state tracking may desync if mpv is controlled
    /// externally (e.g., via another IPC client). The TUI event loop
    /// detects process exit but not external state changes.
    ///
    /// # Errors
    /// Returns an error if the IPC command fails.
    pub async fn toggle_pause(&mut self) -> Result<()> {
        self.send_command(r#"{"command": ["cycle", "pause"]}"#)
            .await?;
        self.state = match self.state {
            PlaybackState::Playing => PlaybackState::Paused,
            PlaybackState::Paused => PlaybackState::Playing,
            PlaybackState::Stopped => PlaybackState::Stopped,
        };
        Ok(())
    }

    /// Seek relative (seconds, can be negative).
    ///
    /// # Errors
    /// Returns an error if the IPC command fails.
    pub async fn seek(&mut self, seconds: i64) -> Result<()> {
        let cmd = format!(r#"{{"command": ["seek", {seconds}, "relative"]}}"#);
        self.send_command(&cmd).await
    }

    /// Get current playback position in seconds.
    ///
    /// # Errors
    /// Returns an error if the IPC query fails.
    pub async fn get_position(&mut self) -> Result<f64> {
        let resp = self
            .send_command_with_response(r#"{"command": ["get_property", "time-pos"]}"#)
            .await?;

        let val: serde_json::Value =
            serde_json::from_str(&resp).context("failed to parse mpv response")?;
        val.get("data")
            .and_then(serde_json::Value::as_f64)
            .context("no time-pos in response")
    }

    /// Get total duration of the current file in seconds.
    ///
    /// # Errors
    /// Returns an error if the IPC query fails.
    pub async fn get_duration(&mut self) -> Result<f64> {
        let resp = self
            .send_command_with_response(r#"{"command": ["get_property", "duration"]}"#)
            .await?;

        let val: serde_json::Value =
            serde_json::from_str(&resp).context("failed to parse mpv response")?;
        val.get("data")
            .and_then(serde_json::Value::as_f64)
            .context("no duration in response")
    }

    /// Stop playback and kill mpv process.
    pub async fn stop(&mut self) {
        self.conn = None;
        if let Some(ref mut child) = self.child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        self.child = None;
        self.state = PlaybackState::Stopped;
        self.current_file = None;
        let _ = tokio::fs::remove_file(&self.socket_path).await;
    }

    /// Check if mpv process is still alive.
    pub fn is_alive(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            child.try_wait().ok().flatten().is_none()
        } else {
            false
        }
    }

    async fn send_command(&mut self, cmd: &str) -> Result<()> {
        self.send_command_with_response(cmd).await?;
        Ok(())
    }

    async fn send_command_with_response(&mut self, cmd: &str) -> Result<String> {
        let result = self.try_ipc_command(cmd).await;
        if result.is_err() {
            // Drop broken connection so next call reconnects
            self.conn = None;
        }
        result
    }

    async fn try_ipc_command(&mut self, cmd: &str) -> Result<String> {
        let conn = self.ensure_conn().await?;
        conn.writer
            .write_all(cmd.as_bytes())
            .await
            .context("failed to write to mpv IPC")?;
        conn.writer.write_all(b"\n").await?;

        // Read lines, skipping async event objects.
        // mpv interleaves command responses (have "error" key) with
        // async events (have "event" key). Parse JSON structurally to
        // distinguish them — substring matching false-positives on paths.
        for _ in 0..10 {
            let mut line = String::new();
            conn.reader
                .read_line(&mut line)
                .await
                .context("failed to read mpv response")?;

            // Parse as JSON to distinguish command responses from events
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                // Command responses have an "error" field ("success" on success)
                if val.get("error").is_some() {
                    return Ok(line);
                }
                // If it's not an event either, treat it as a response
                if val.get("event").is_none() {
                    return Ok(line);
                }
                // Otherwise it's an async event — skip
            } else {
                // Non-JSON response — return as-is
                return Ok(line);
            }
        }
        anyhow::bail!("too many mpv async events before command response")
    }

    async fn ensure_conn(&mut self) -> Result<&mut IpcConn> {
        if self.conn.is_none() {
            let stream = UnixStream::connect(&self.socket_path)
                .await
                .context("failed to connect to mpv IPC socket")?;
            let (read, write) = stream.into_split();
            self.conn = Some(IpcConn {
                writer: write,
                reader: BufReader::new(read),
            });
        }
        self.conn.as_mut().context("IPC connection not established")
    }
}

impl Default for MpvController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        // Best-effort cleanup — blocking is acceptable here since it's
        // a quick unlink on a socket in /tmp, and this runs at shutdown.
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_stopped() {
        let ctrl = MpvController::new();
        assert_eq!(ctrl.state(), PlaybackState::Stopped);
    }

    #[test]
    fn new_current_file_is_none() {
        let ctrl = MpvController::new();
        assert!(ctrl.current_file().is_none());
    }

    #[test]
    fn new_is_alive_false() {
        let mut ctrl = MpvController::new();
        assert!(!ctrl.is_alive());
    }

    #[test]
    fn socket_path_contains_pid() {
        let ctrl = MpvController::new();
        let pid = std::process::id().to_string();
        assert!(
            ctrl.socket_path.to_string_lossy().contains(&pid),
            "socket_path {:?} should contain pid {pid}",
            ctrl.socket_path
        );
    }

    #[test]
    fn two_controllers_different_paths() {
        let a = MpvController::new();
        let b = MpvController::new();
        assert_ne!(a.socket_path, b.socket_path);
    }

    #[test]
    fn default_equals_new() {
        let from_default = MpvController::default();
        let from_new = MpvController::new();
        assert_eq!(from_default.state(), from_new.state());
    }
}
