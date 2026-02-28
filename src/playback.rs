/// mpv playback controller via JSON IPC over Unix socket.
///
/// Manages an mpv subprocess with `--input-ipc-server` for transport controls:
/// play, pause, seek, volume, position queries.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

/// mpv playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Controller for an mpv subprocess.
pub struct MpvController {
    socket_path: PathBuf,
    child: Option<Child>,
    state: PlaybackState,
    current_file: Option<PathBuf>,
}

impl MpvController {
    /// Create a new controller (mpv not yet spawned).
    #[must_use]
    pub fn new() -> Self {
        let socket_path = std::env::temp_dir().join(format!(
            "mls_mpv_{}.sock",
            std::process::id()
        ));
        Self {
            socket_path,
            child: None,
            state: PlaybackState::Stopped,
            current_file: None,
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

        // Wait briefly for socket to appear
        for _ in 0..20 {
            if self.socket_path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Ok(())
    }

    /// Toggle play/pause.
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
        let cmd = format!(
            r#"{{"command": ["seek", "{seconds}", "relative"]}}"#
        );
        self.send_command(&cmd).await
    }

    /// Get current playback position in seconds.
    ///
    /// # Errors
    /// Returns an error if the IPC query fails.
    #[expect(dead_code, reason = "will be used for playback progress display")]
    pub async fn get_position(&self) -> Result<f64> {
        let resp = self
            .send_command_with_response(
                r#"{"command": ["get_property", "time-pos"]}"#,
            )
            .await?;

        let val: serde_json::Value = serde_json::from_str(&resp)
            .context("failed to parse mpv response")?;
        val.get("data")
            .and_then(serde_json::Value::as_f64)
            .context("no time-pos in response")
    }

    /// Stop playback and kill mpv process.
    pub async fn stop(&mut self) {
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

    async fn send_command(&self, cmd: &str) -> Result<()> {
        self.send_command_with_response(cmd).await?;
        Ok(())
    }

    async fn send_command_with_response(&self, cmd: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("failed to connect to mpv IPC socket")?;

        stream
            .write_all(cmd.as_bytes())
            .await
            .context("failed to write to mpv IPC")?;
        stream.write_all(b"\n").await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .context("failed to read mpv response")?;

        Ok(line)
    }
}

impl Default for MpvController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        // Best-effort cleanup
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
