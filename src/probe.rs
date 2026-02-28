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

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;

    // --- parse_rational_fps ---

    #[test]
    fn parse_fps_standard_24() {
        let fps = parse_rational_fps("24000/1001").unwrap();
        assert_eq!(fps.num, 24000);
        assert_eq!(fps.den, 1001);
    }

    #[test]
    fn parse_fps_integer_30() {
        let fps = parse_rational_fps("30/1").unwrap();
        assert_eq!(fps.num, 30);
        assert_eq!(fps.den, 1);
    }

    #[test]
    fn parse_fps_60() {
        let fps = parse_rational_fps("60/1").unwrap();
        assert_eq!(fps.num, 60);
        assert_eq!(fps.den, 1);
    }

    #[test]
    fn parse_fps_zero_den_returns_none() {
        assert!(parse_rational_fps("30/0").is_none());
    }

    #[test]
    fn parse_fps_no_slash_returns_none() {
        assert!(parse_rational_fps("30").is_none());
    }

    #[test]
    fn parse_fps_non_numeric_returns_none() {
        assert!(parse_rational_fps("abc/def").is_none());
    }

    #[test]
    fn parse_fps_empty_returns_none() {
        assert!(parse_rational_fps("").is_none());
    }

    // --- extract_tags ---

    #[test]
    fn extract_tags_all_fields() {
        let mut map = serde_json::Map::new();
        map.insert(
            "title".to_string(),
            serde_json::Value::String("My Song".to_string()),
        );
        map.insert(
            "artist".to_string(),
            serde_json::Value::String("Artist".to_string()),
        );
        map.insert(
            "album".to_string(),
            serde_json::Value::String("Album".to_string()),
        );
        map.insert(
            "date".to_string(),
            serde_json::Value::String("2024".to_string()),
        );
        map.insert(
            "genre".to_string(),
            serde_json::Value::String("Rock".to_string()),
        );

        let tags = extract_tags(&map);
        assert_eq!(tags.title.as_deref(), Some("My Song"));
        assert_eq!(tags.artist.as_deref(), Some("Artist"));
        assert_eq!(tags.album.as_deref(), Some("Album"));
        assert_eq!(tags.date.as_deref(), Some("2024"));
        assert_eq!(tags.genre.as_deref(), Some("Rock"));
    }

    #[test]
    fn extract_tags_uppercase_fallback() {
        let mut map = serde_json::Map::new();
        map.insert(
            "TITLE".to_string(),
            serde_json::Value::String("Upper".to_string()),
        );
        map.insert(
            "ARTIST".to_string(),
            serde_json::Value::String("ArtistUpper".to_string()),
        );

        let tags = extract_tags(&map);
        assert_eq!(tags.title.as_deref(), Some("Upper"));
        assert_eq!(tags.artist.as_deref(), Some("ArtistUpper"));
    }

    #[test]
    fn extract_tags_empty_map() {
        let map = serde_json::Map::new();
        let tags = extract_tags(&map);
        assert!(tags.title.is_none());
        assert!(tags.artist.is_none());
        assert!(tags.album.is_none());
        assert!(tags.date.is_none());
        assert!(tags.genre.is_none());
    }

    #[test]
    fn extract_tags_ignores_non_string_values() {
        let mut map = serde_json::Map::new();
        map.insert("title".to_string(), serde_json::Value::Number(42.into()));
        let tags = extract_tags(&map);
        assert!(tags.title.is_none());
    }

    // --- build_media_info ---

    fn make_video_stream() -> FfprobeStream {
        FfprobeStream {
            index: Some(0),
            codec_type: Some("video".to_string()),
            codec_name: Some("h264".to_string()),
            profile: Some("High".to_string()),
            level: Some(41),
            width: Some(1920),
            height: Some(1080),
            r_frame_rate: Some("24/1".to_string()),
            bit_rate: Some("4500000".to_string()),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            pix_fmt: Some("yuv420p".to_string()),
            extra: serde_json::Map::new(),
        }
    }

    fn make_audio_stream() -> FfprobeStream {
        FfprobeStream {
            index: Some(1),
            codec_type: Some("audio".to_string()),
            codec_name: Some("aac".to_string()),
            profile: Some("LC".to_string()),
            level: None,
            width: None,
            height: None,
            r_frame_rate: None,
            bit_rate: Some("128000".to_string()),
            sample_rate: Some("48000".to_string()),
            channels: Some(2),
            channel_layout: Some("stereo".to_string()),
            pix_fmt: None,
            extra: serde_json::Map::new(),
        }
    }

    fn make_format(
        duration: Option<&str>,
        bitrate: Option<&str>,
    ) -> FfprobeFormat {
        FfprobeFormat {
            format_name: Some("mov,mp4,m4a,3gp,3g2,mj2".to_string()),
            duration: duration.map(String::from),
            bit_rate: bitrate.map(String::from),
            tags: None,
        }
    }

    #[test]
    fn build_media_info_av_kind() {
        let raw = FfprobeOutput {
            format: Some(make_format(Some("120.5"), Some("5000000"))),
            streams: vec![make_video_stream(), make_audio_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Av);
        assert!(info.video.is_some());
        assert!(info.audio.is_some());
    }

    #[test]
    fn build_media_info_duration_parsed() {
        let raw = FfprobeOutput {
            format: Some(make_format(Some("120.5"), None)),
            streams: vec![make_video_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.duration_ms, Some(120_500));
    }

    #[test]
    fn build_media_info_bitrate_parsed() {
        let raw = FfprobeOutput {
            format: Some(make_format(None, Some("5000000"))),
            streams: vec![make_video_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.overall_bitrate_bps, Some(5_000_000));
    }

    #[test]
    fn build_media_info_video_only() {
        let raw = FfprobeOutput {
            format: Some(make_format(None, None)),
            streams: vec![make_video_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Video);
        assert!(info.video.is_some());
        assert!(info.audio.is_none());
    }

    #[test]
    fn build_media_info_audio_only() {
        let raw = FfprobeOutput {
            format: Some(make_format(None, None)),
            streams: vec![make_audio_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Audio);
        assert!(info.video.is_none());
        assert!(info.audio.is_some());
    }

    #[test]
    fn build_media_info_no_streams() {
        let raw = FfprobeOutput {
            format: Some(make_format(None, None)),
            streams: vec![],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Audio);
        assert!(info.video.is_none());
        assert!(info.audio.is_none());
    }

    #[test]
    fn build_media_info_no_format() {
        let raw = FfprobeOutput {
            format: None,
            streams: vec![],
        };
        let info = build_media_info(&raw);
        assert!(info.duration_ms.is_none());
        assert!(info.overall_bitrate_bps.is_none());
        assert!(info.container.format_name.is_empty());
    }

    #[test]
    fn build_media_info_container_primary() {
        let raw = FfprobeOutput {
            format: Some(make_format(None, None)),
            streams: vec![make_video_stream()],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.container.format_primary, "mov");
        assert!(info.container.format_name.contains("mp4"));
    }

    // --- build_video_info ---

    #[test]
    fn build_video_info_fields() {
        let stream = make_video_stream();
        let vid = build_video_info(&stream);
        assert_eq!(vid.width, 1920);
        assert_eq!(vid.height, 1080);
        assert_eq!(vid.codec.name, "h264");
        assert_eq!(vid.codec.profile.as_deref(), Some("High"));
        assert_eq!(vid.codec.level.as_deref(), Some("41"));
        assert_eq!(vid.pixel_format.as_deref(), Some("yuv420p"));
        assert_eq!(vid.bitrate_bps, Some(4_500_000));

        let fps = vid.fps.unwrap();
        assert_eq!(fps.num, 24);
        assert_eq!(fps.den, 1);
    }

    #[test]
    fn build_video_info_missing_optionals() {
        let stream = FfprobeStream {
            index: Some(0),
            codec_type: Some("video".to_string()),
            codec_name: None,
            profile: None,
            level: None,
            width: None,
            height: None,
            r_frame_rate: None,
            bit_rate: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            pix_fmt: None,
            extra: serde_json::Map::new(),
        };
        let vid = build_video_info(&stream);
        assert_eq!(vid.width, 0);
        assert_eq!(vid.height, 0);
        assert!(vid.fps.is_none());
        assert!(vid.bitrate_bps.is_none());
        assert!(vid.pixel_format.is_none());
    }

    // --- build_audio_info ---

    #[test]
    fn build_audio_info_fields() {
        let stream = make_audio_stream();
        let aud = build_audio_info(&stream);
        assert_eq!(aud.channels, 2);
        assert_eq!(aud.codec.name, "aac");
        assert_eq!(aud.codec.profile.as_deref(), Some("LC"));
        assert_eq!(aud.sample_rate_hz, Some(48000));
        assert_eq!(aud.bitrate_bps, Some(128_000));
        assert_eq!(aud.channel_layout.as_deref(), Some("stereo"));
    }

    #[test]
    fn build_audio_info_missing_optionals() {
        let stream = FfprobeStream {
            index: Some(1),
            codec_type: Some("audio".to_string()),
            codec_name: None,
            profile: None,
            level: None,
            width: None,
            height: None,
            r_frame_rate: None,
            bit_rate: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            pix_fmt: None,
            extra: serde_json::Map::new(),
        };
        let aud = build_audio_info(&stream);
        assert_eq!(aud.channels, 0);
        assert!(aud.sample_rate_hz.is_none());
        assert!(aud.bitrate_bps.is_none());
        assert!(aud.channel_layout.is_none());
    }

    // --- build_stream_info ---

    #[test]
    fn build_stream_info_copies_fields() {
        let stream = make_video_stream();
        let si = build_stream_info(&stream);
        assert_eq!(si.index, 0);
        assert_eq!(si.codec_type, "video");
        assert_eq!(si.codec_name, "h264");
        assert_eq!(si.profile.as_deref(), Some("High"));
    }

    // --- ffprobe JSON deserialization ---

    #[test]
    fn deserialize_ffprobe_json() {
        let json = r#"{
            "format": {
                "format_name": "matroska,webm",
                "duration": "90.5",
                "bit_rate": "2500000"
            },
            "streams": [
                {
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "vp9",
                    "width": 1280,
                    "height": 720,
                    "r_frame_rate": "30/1"
                },
                {
                    "index": 1,
                    "codec_type": "audio",
                    "codec_name": "opus",
                    "channels": 2,
                    "sample_rate": "48000"
                }
            ]
        }"#;

        let raw: FfprobeOutput = serde_json::from_str(json).unwrap();
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Av);
        assert_eq!(info.duration_ms, Some(90_500));
        assert_eq!(info.overall_bitrate_bps, Some(2_500_000));
        assert_eq!(info.container.format_primary, "matroska");

        let vid = info.video.unwrap();
        assert_eq!(vid.width, 1280);
        assert_eq!(vid.height, 720);
        assert_eq!(vid.codec.name, "vp9");

        let aud = info.audio.unwrap();
        assert_eq!(aud.codec.name, "opus");
        assert_eq!(aud.channels, 2);
        assert_eq!(aud.sample_rate_hz, Some(48000));
    }

    #[test]
    fn deserialize_minimal_ffprobe_json() {
        let json = r#"{"streams": []}"#;
        let raw: FfprobeOutput = serde_json::from_str(json).unwrap();
        let info = build_media_info(&raw);
        assert_eq!(info.kind, MediaKind::Audio);
        assert!(info.duration_ms.is_none());
    }

    // --- tags in format ---

    #[test]
    fn build_media_info_with_tags() {
        let mut tags_map = serde_json::Map::new();
        tags_map.insert(
            "title".to_string(),
            serde_json::Value::String("Test Video".to_string()),
        );
        tags_map.insert(
            "artist".to_string(),
            serde_json::Value::String("Test Artist".to_string()),
        );

        let raw = FfprobeOutput {
            format: Some(FfprobeFormat {
                format_name: Some("mp4".to_string()),
                duration: None,
                bit_rate: None,
                tags: Some(tags_map),
            }),
            streams: vec![],
        };
        let info = build_media_info(&raw);
        assert_eq!(info.tags.title.as_deref(), Some("Test Video"));
        assert_eq!(info.tags.artist.as_deref(), Some("Test Artist"));
    }
}
