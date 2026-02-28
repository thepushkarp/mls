/// Core types for mls structured output.
///
/// All types are designed for dual use: TUI display and JSON serialization.
/// Field naming follows the schema from the PRD (e.g., `duration_ms`, `bitrate_bps`).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level media kind classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
    /// Contains both audio and video streams.
    Av,
}

impl std::fmt::Display for MediaKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Video => write!(f, "video"),
            Self::Audio => write!(f, "audio"),
            Self::Av => write!(f, "av"),
        }
    }
}

/// Rational frame rate (avoids float imprecision: 23.976 = 24000/1001).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    #[must_use]
    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            return 0.0;
        }
        f64::from(self.num) / f64::from(self.den)
    }
}

impl std::fmt::Display for Fps {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = self.as_f64();
        // Display common rates cleanly
        if (val - val.round()).abs() < 0.01 {
            write!(f, "{}", val.round() as u32)
        } else {
            write!(f, "{val:.3}")
        }
    }
}

/// Codec identification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodecInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// Container format information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub format_name: String,
    pub format_primary: String,
}

/// Video stream summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<Fps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    pub codec: CodecInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<String>,
}

impl VideoInfo {
    /// Human-friendly resolution label (e.g., "1080p", "4K").
    #[must_use]
    pub fn resolution_label(&self) -> String {
        match self.height {
            481..=720 => "720p".to_string(),
            721..=1080 => "1080p".to_string(),
            1081..=1440 => "1440p".to_string(),
            1441..=2160 => "4K".to_string(),
            h => format!("{h}p"),
        }
    }
}

/// Audio stream summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub channels: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate_hz: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    pub codec: CodecInfo,
}

/// Raw stream info from ffprobe (full detail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: String,
    pub codec_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Media tags (ID3, Vorbis comments, MP4 atoms, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaTags {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
}

/// Aggregated media metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub container: ContainerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_bitrate_bps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub streams: Vec<StreamInfo>,
    pub tags: MediaTags,
}

/// File-system metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsInfo {
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

/// Probe execution metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub backend: String,
    pub took_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One media file with all its metadata — the core unit of mls output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub extension: String,
    pub fs: FsInfo,
    pub media: MediaInfo,
    pub probe: ProbeInfo,
}

/// JSON envelope for `mls list --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListEnvelope {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub schema_version: String,
    pub mls_version: String,
    pub generated_at: DateTime<Utc>,
    pub summary: ListSummary,
    pub entries: Vec<MediaEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ProbeError>,
}

/// Summary statistics for a list operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSummary {
    pub entries_total: usize,
    pub entries_emitted: usize,
    pub probe_ok: usize,
    pub probe_error: usize,
}

/// Per-file probe error (included in envelope and NDJSON footer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeError {
    pub path: PathBuf,
    pub error: String,
}

/// NDJSON record types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NdjsonRecord {
    #[serde(rename = "mls.header")]
    Header {
        schema_version: String,
        mls_version: String,
        generated_at: DateTime<Utc>,
    },
    #[serde(rename = "mls.entry")]
    Entry { entry: Box<MediaEntry> },
    #[serde(rename = "mls.footer")]
    Footer {
        summary: ListSummary,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        errors: Vec<ProbeError>,
    },
}

/// Sort key for media entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Path,
    Name,
    Size,
    Modified,
    Duration,
    Resolution,
    Codec,
    Bitrate,
}

impl SortKey {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Name => "name",
            Self::Size => "size",
            Self::Modified => "date",
            Self::Duration => "duration",
            Self::Resolution => "resolution",
            Self::Codec => "codec",
            Self::Bitrate => "bitrate",
        }
    }

    /// Cycle to next sort key.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Path => Self::Name,
            Self::Name => Self::Size,
            Self::Size => Self::Modified,
            Self::Modified => Self::Duration,
            Self::Duration => Self::Resolution,
            Self::Resolution => Self::Codec,
            Self::Codec => Self::Bitrate,
            Self::Bitrate => Self::Path,
        }
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

/// Helper to format duration in ms to human-readable "H:MM:SS" or "M:SS".
#[must_use]
pub fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

/// Helper to format bytes to human-readable size.
#[must_use]
#[expect(clippy::cast_precision_loss)]
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Helper to format bitrate (bps) to human-readable.
#[must_use]
#[expect(clippy::cast_precision_loss)]
pub fn format_bitrate(bps: u64) -> String {
    const KBPS: u64 = 1000;
    const MBPS: u64 = 1000 * KBPS;

    if bps >= MBPS {
        format!("{:.1} Mbps", bps as f64 / MBPS as f64)
    } else if bps >= KBPS {
        format!("{:.0} kbps", bps as f64 / KBPS as f64)
    } else {
        format!("{bps} bps")
    }
}

/// Recognized media file extensions.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "wmv", "flv", "webm", "m4v", "mpg", "mpeg",
    "ts", "mts", "m2ts", "vob", "ogv", "3gp", "3g2",
];

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "ogg", "opus", "wma", "m4a", "aiff", "aif",
    "alac", "ape", "dsf", "dff", "wv", "mka",
];

/// Check if a file extension is a recognized media type.
#[must_use]
pub fn is_media_extension(ext: &str) -> bool {
    let ext_lower = ext.to_ascii_lowercase();
    VIDEO_EXTENSIONS.contains(&ext_lower.as_str())
        || AUDIO_EXTENSIONS.contains(&ext_lower.as_str())
}

/// Check if a file extension is a recognized video type.
#[must_use]
pub fn is_video_extension(ext: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}
