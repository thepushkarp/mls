/// Media metadata extraction via ffprobe subprocess.
///
/// Runs `ffprobe -v quiet -print_format json -show_format -show_streams <file>`
/// and parses the JSON output into our `MediaEntry` types.
use crate::types::{
    AudioInfo, CodecInfo, ContainerInfo, Fps, FsInfo, MediaEntry, MediaInfo, MediaKind,
    MediaTags, ProbeInfo, StreamInfo, VideoInfo,
};
use anyhow::{Context, Result};
use std::path::Path;
use std::time::Instant;
use tokio::process::Command;

/// Raw ffprobe JSON structure (subset we care about).
#[derive(serde::Deserialize)]
struct FfprobeOutput {
    format: Option<FfprobeFormat>,
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(serde::Deserialize)]
struct FfprobeFormat {
    format_name: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    bit_rate: Option<String>,
    #[serde(default)]
    tags: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(serde::Deserialize)]
struct FfprobeStream {
    index: Option<u32>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    level: Option<i64>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    bit_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    pix_fmt: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Probe a single file with ffprobe and return a `MediaEntry`.
///
/// # Errors
/// Returns an error if ffprobe fails to execute or parse.
pub async fn probe_file(path: &Path, timeout_ms: u64) -> Result<MediaEntry> {
    let start = Instant::now();

    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output(),
    )
    .await
    .context("ffprobe timed out")?
    .context("failed to execute ffprobe")?;

    #[expect(clippy::cast_possible_truncation)]
    let took_ms = start.elapsed().as_millis() as u64;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {stderr}");
    }

    let raw: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .context("failed to parse ffprobe JSON")?;

    let fs_meta = std::fs::metadata(path).context("failed to read file metadata")?;
    let fs = build_fs_info(&fs_meta);
    let media = build_media_info(&raw);
    let file_name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let extension = path
        .extension()
        .map_or_else(String::new, |e| e.to_string_lossy().into_owned());

    Ok(MediaEntry {
        path: path.to_path_buf(),
        file_name,
        extension,
        fs,
        media,
        probe: ProbeInfo {
            backend: "ffprobe".to_string(),
            took_ms,
            error: None,
        },
    })
}

#[expect(clippy::cast_possible_wrap)]
fn build_fs_info(meta: &std::fs::Metadata) -> FsInfo {
    use chrono::{DateTime, Utc};
    use std::time::UNIX_EPOCH;

    let modified_at = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos()));

    let created_at = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos()));

    FsInfo {
        size_bytes: meta.len(),
        modified_at: modified_at.map(|dt| dt.with_timezone(&Utc)),
        created_at: created_at.map(|dt| dt.with_timezone(&Utc)),
    }
}

fn build_media_info(raw: &FfprobeOutput) -> MediaInfo {
    let has_video = raw.streams.iter().any(|s| {
        s.codec_type.as_deref() == Some("video")
    });
    let has_audio = raw.streams.iter().any(|s| {
        s.codec_type.as_deref() == Some("audio")
    });

    let kind = match (has_video, has_audio) {
        (true, true) => MediaKind::Av,
        (true, false) => MediaKind::Video,
        _ => MediaKind::Audio,
    };

    let fmt = raw.format.as_ref();
    let container = ContainerInfo {
        format_name: fmt
            .and_then(|f| f.format_name.clone())
            .unwrap_or_default(),
        format_primary: fmt
            .and_then(|f| {
                f.format_name
                    .as_deref()
                    .and_then(|n| n.split(',').next())
                    .map(String::from)
            })
            .unwrap_or_default(),
    };

    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let duration_ms = fmt
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as u64);

    let overall_bitrate_bps = fmt
        .and_then(|f| f.bit_rate.as_deref())
        .and_then(|b| b.parse::<u64>().ok());

    let video = raw
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .map(build_video_info);

    let audio = raw
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .map(build_audio_info);

    let streams = raw.streams.iter().map(build_stream_info).collect();

    let tags = fmt
        .and_then(|f| f.tags.as_ref())
        .map(extract_tags)
        .unwrap_or_default();

    MediaInfo {
        kind,
        container,
        duration_ms,
        overall_bitrate_bps,
        video,
        audio,
        streams,
        tags,
    }
}

fn build_video_info(s: &FfprobeStream) -> VideoInfo {
    let fps = s.r_frame_rate.as_deref().and_then(parse_rational_fps);
    let bitrate_bps = s.bit_rate.as_deref().and_then(|b| b.parse::<u64>().ok());

    VideoInfo {
        width: s.width.unwrap_or(0),
        height: s.height.unwrap_or(0),
        fps,
        bitrate_bps,
        codec: CodecInfo {
            name: s.codec_name.clone().unwrap_or_default(),
            profile: s.profile.clone(),
            level: s.level.map(|l| l.to_string()),
        },
        pixel_format: s.pix_fmt.clone(),
    }
}

fn build_audio_info(s: &FfprobeStream) -> AudioInfo {
    let sample_rate_hz = s
        .sample_rate
        .as_deref()
        .and_then(|r| r.parse::<u32>().ok());
    let bitrate_bps = s.bit_rate.as_deref().and_then(|b| b.parse::<u64>().ok());

    AudioInfo {
        channels: s.channels.unwrap_or(0),
        channel_layout: s.channel_layout.clone(),
        sample_rate_hz,
        bitrate_bps,
        codec: CodecInfo {
            name: s.codec_name.clone().unwrap_or_default(),
            profile: s.profile.clone(),
            level: None,
        },
    }
}

fn build_stream_info(s: &FfprobeStream) -> StreamInfo {
    StreamInfo {
        index: s.index.unwrap_or(0),
        codec_type: s.codec_type.clone().unwrap_or_default(),
        codec_name: s.codec_name.clone().unwrap_or_default(),
        profile: s.profile.clone(),
        extra: s.extra.clone(),
    }
}

fn parse_rational_fps(s: &str) -> Option<Fps> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().ok()?;
        let den = parts[1].parse::<u32>().ok()?;
        if den > 0 {
            return Some(Fps { num, den });
        }
    }
    None
}

fn extract_tags(tags: &serde_json::Map<String, serde_json::Value>) -> MediaTags {
    let get = |key: &str| -> Option<String> {
        tags.get(key)
            .or_else(|| tags.get(&key.to_uppercase()))
            .and_then(|v| v.as_str())
            .map(String::from)
    };

    MediaTags {
        title: get("title"),
        artist: get("artist"),
        album: get("album"),
        date: get("date"),
        genre: get("genre"),
    }
}
