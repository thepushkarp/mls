/// External dependency checking.
///
/// mls hard-requires ffmpeg (ffprobe + ffmpeg) and soft-requires mpv.
/// This module checks for their presence at startup and provides
/// clear install instructions if missing.
use std::fmt::Write;
use std::process::Command as StdCommand;

/// Result of checking external dependencies.
#[derive(Debug)]
pub struct DepCheck {
    pub ffprobe: Option<String>,
    pub ffmpeg: Option<String>,
    pub mpv: Option<String>,
}

impl DepCheck {
    /// Check all external dependencies. Returns version strings if found.
    pub fn run() -> Self {
        Self {
            ffprobe: probe_version("ffprobe", &["-version"]),
            ffmpeg: probe_version("ffmpeg", &["-version"]),
            mpv: probe_version("mpv", &["--version"]),
        }
    }

    /// Returns true if all hard dependencies are present.
    #[must_use]
    pub fn hard_deps_ok(&self) -> bool {
        self.ffprobe.is_some() && self.ffmpeg.is_some()
    }

    /// Build a user-friendly error message for missing dependencies.
    #[must_use]
    pub fn missing_message(&self) -> Option<String> {
        let mut missing = Vec::new();

        if self.ffprobe.is_none() || self.ffmpeg.is_none() {
            missing.push("ffmpeg (includes ffprobe): brew install ffmpeg");
        }
        if self.mpv.is_none() {
            missing.push("mpv (for playback): brew install mpv");
        }

        if missing.is_empty() {
            return None;
        }

        let severity = if self.hard_deps_ok() {
            "Warning: optional dependencies missing"
        } else {
            "Error: required dependencies missing"
        };

        let mut msg = format!("{severity}:\n");
        for m in &missing {
            let _ = writeln!(msg, "  - {m}");
        }
        Some(msg)
    }
}

/// Run a command and extract the first line of output as version info.
fn probe_version(cmd: &str, args: &[&str]) -> Option<String> {
    StdCommand::new(cmd)
        .args(args)
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(String::from))
            } else {
                None
            }
        })
}
