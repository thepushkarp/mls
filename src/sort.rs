/// Sorting logic for media entries and directory items.
///
/// Supports sorting by all metadata fields with configurable direction.
use crate::types::{DirItem, MediaEntry, SortDir, SortKey};

/// Parse a sort specification string (e.g., "`duration_ms:desc`", "name:asc").
///
/// # Errors
/// Returns `None` if the sort key is not recognized.
#[must_use]
pub fn parse_sort_spec(spec: &str) -> Option<(SortKey, SortDir)> {
    let (key_str, dir_str) = spec
        .split_once(':')
        .map_or((spec, None), |(k, d)| (k, Some(d)));
    let key = match key_str {
        "path" => SortKey::Path,
        "name" => SortKey::Name,
        "size" => SortKey::Size,
        "date" | "modified" => SortKey::Modified,
        "duration" | "duration_ms" => SortKey::Duration,
        "resolution" => SortKey::Resolution,
        "codec" => SortKey::Codec,
        "bitrate" => SortKey::Bitrate,
        "pages" | "page_count" => SortKey::Pages,
        _ => return None,
    };
    let dir = match dir_str {
        Some("desc") => SortDir::Desc,
        Some("asc") | None => SortDir::Asc,
        Some(other) => {
            tracing::debug!(
                direction = other,
                "unrecognized sort direction, defaulting to asc"
            );
            SortDir::Asc
        }
    };
    Some((key, dir))
}

/// Sort entries in place by the given key and direction.
///
/// For optional fields (Modified, Duration, Bitrate), `None` sorts before
/// `Some` in ascending order (via `Option`'s derived `Ord`).
pub fn sort_entries(entries: &mut [MediaEntry], key: SortKey, dir: SortDir) {
    if key == SortKey::Name {
        // Cache lowercased keys: one allocation per entry instead of two per comparison
        entries.sort_by_cached_key(|e| e.file_name.to_lowercase());
        if dir == SortDir::Desc {
            entries.reverse();
        }
        return;
    }
    entries.sort_by(|a, b| {
        let cmp = compare_by_key(a, b, key);
        match dir {
            SortDir::Asc => cmp,
            SortDir::Desc => cmp.reverse(),
        }
    });
}

/// Sort directory items in place by the given key and direction.
///
/// Falls back to Name sort for media-only keys (Duration, Resolution, etc.).
pub fn sort_dir_items(dirs: &mut [DirItem], key: SortKey, dir: SortDir) {
    let effective_key = if key.applies_to_dirs() {
        key
    } else {
        SortKey::Name
    };
    dirs.sort_by(|a, b| {
        let cmp = match effective_key {
            SortKey::Size => a.size_bytes.cmp(&b.size_bytes),
            SortKey::Modified => a.modified_at.cmp(&b.modified_at),
            // Name, Path, and any fallback: sort by lowercased name
            _ => a.name_lower.cmp(&b.name_lower),
        };
        match dir {
            SortDir::Asc => cmp,
            SortDir::Desc => cmp.reverse(),
        }
    });
}

fn compare_by_key(a: &MediaEntry, b: &MediaEntry, key: SortKey) -> std::cmp::Ordering {
    match key {
        SortKey::Path => a.path.cmp(&b.path),
        SortKey::Name => a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()), // Unreachable: sort_entries handles Name via sort_by_cached_key before calling compare_by_key
        SortKey::Size => a.fs.size_bytes.cmp(&b.fs.size_bytes),
        SortKey::Modified => a.fs.modified_at.cmp(&b.fs.modified_at),
        SortKey::Duration => a.media.duration_ms.cmp(&b.media.duration_ms),
        SortKey::Resolution => {
            let res_a = a
                .media
                .video
                .as_ref()
                .map_or(0, |v| u64::from(v.width) * u64::from(v.height));
            let res_b = b
                .media
                .video
                .as_ref()
                .map_or(0, |v| u64::from(v.width) * u64::from(v.height));
            res_a.cmp(&res_b)
        }
        SortKey::Codec => {
            let codec_a = a
                .media
                .video
                .as_ref()
                .map(|v| v.codec.name.as_str())
                .or_else(|| a.media.audio.as_ref().map(|au| au.codec.name.as_str()))
                .unwrap_or("");
            let codec_b = b
                .media
                .video
                .as_ref()
                .map(|v| v.codec.name.as_str())
                .or_else(|| b.media.audio.as_ref().map(|au| au.codec.name.as_str()))
                .unwrap_or("");
            codec_a.cmp(codec_b)
        }
        SortKey::Bitrate => a
            .media
            .overall_bitrate_bps
            .cmp(&b.media.overall_bitrate_bps),
        SortKey::Pages => {
            let pages_a = a.media.doc.as_ref().and_then(|d| d.page_count);
            let pages_b = b.media.doc.as_ref().and_then(|d| d.page_count);
            pages_a.cmp(&pages_b)
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::{
        AudioInfo, CodecInfo, ContainerInfo, FsInfo, MediaInfo, MediaKind, MediaTags, ProbeInfo,
        VideoInfo,
    };
    use std::borrow::Cow;
    use std::path::PathBuf;

    fn make_entry_with(name: &str, size: u64, duration_ms: Option<u64>) -> MediaEntry {
        MediaEntry {
            path: PathBuf::from(format!("/test/{name}")),
            file_name: name.to_string(),
            extension: "mp4".to_string(),
            fs: FsInfo {
                size_bytes: size,
                modified_at: None,
                created_at: None,
            },
            media: MediaInfo {
                kind: MediaKind::Video,
                container: ContainerInfo {
                    format_name: "mp4".to_string(),
                    format_primary: "mp4".to_string(),
                },
                duration_ms,
                overall_bitrate_bps: None,
                video: None,
                audio: None,
                streams: vec![],
                tags: MediaTags::default(),
                exif: None,
                doc: None,
            },
            probe: ProbeInfo {
                backend: Cow::Borrowed("ffprobe"),
                took_ms: 10,
                error: None,
            },
        }
    }

    fn make_entry_with_video(name: &str, width: u32, height: u32, codec: &str) -> MediaEntry {
        let mut entry = make_entry_with(name, 100, None);
        entry.media.video = Some(VideoInfo {
            width,
            height,
            fps: None,
            bitrate_bps: None,
            codec: CodecInfo {
                name: codec.to_string(),
                profile: None,
                level: None,
            },
            pixel_format: None,
        });
        entry
    }

    fn make_entry_with_audio_codec(name: &str, codec: &str) -> MediaEntry {
        let mut entry = make_entry_with(name, 100, None);
        entry.media.audio = Some(AudioInfo {
            channels: 2,
            channel_layout: None,
            sample_rate_hz: None,
            bitrate_bps: None,
            codec: CodecInfo {
                name: codec.to_string(),
                profile: None,
                level: None,
            },
        });
        entry
    }

    // --- parse_sort_spec ---

    #[test]
    fn parse_sort_spec_name_default_asc() {
        assert_eq!(parse_sort_spec("name"), Some((SortKey::Name, SortDir::Asc)));
    }

    #[test]
    fn parse_sort_spec_duration_desc() {
        assert_eq!(
            parse_sort_spec("duration:desc"),
            Some((SortKey::Duration, SortDir::Desc))
        );
    }

    #[test]
    fn parse_sort_spec_duration_ms_alias() {
        assert_eq!(
            parse_sort_spec("duration_ms:asc"),
            Some((SortKey::Duration, SortDir::Asc))
        );
    }

    #[test]
    fn parse_sort_spec_date_alias() {
        assert_eq!(
            parse_sort_spec("date"),
            Some((SortKey::Modified, SortDir::Asc))
        );
    }

    #[test]
    fn parse_sort_spec_modified_alias() {
        assert_eq!(
            parse_sort_spec("modified:desc"),
            Some((SortKey::Modified, SortDir::Desc))
        );
    }

    #[test]
    fn parse_sort_spec_all_keys_recognized() {
        let keys = [
            "path",
            "name",
            "size",
            "date",
            "modified",
            "duration",
            "duration_ms",
            "resolution",
            "codec",
            "bitrate",
            "pages",
            "page_count",
        ];
        for key in keys {
            assert!(
                parse_sort_spec(key).is_some(),
                "key '{key}' should be recognized"
            );
        }
    }

    #[test]
    fn parse_sort_spec_unknown_returns_none() {
        assert_eq!(parse_sort_spec("unknown"), None);
        assert_eq!(parse_sort_spec(""), None);
    }

    #[test]
    fn parse_sort_spec_explicit_asc() {
        let (_, dir) = parse_sort_spec("size:asc").unwrap();
        assert_eq!(dir, SortDir::Asc);
    }

    #[test]
    fn parse_sort_spec_invalid_dir_defaults_asc() {
        let (_, dir) = parse_sort_spec("size:garbage").unwrap();
        assert_eq!(dir, SortDir::Asc);
    }

    // --- sort_entries ---

    #[test]
    fn sort_by_name_asc() {
        let mut entries = vec![
            make_entry_with("charlie.mp4", 100, None),
            make_entry_with("alpha.mp4", 200, None),
            make_entry_with("bravo.mp4", 150, None),
        ];
        sort_entries(&mut entries, SortKey::Name, SortDir::Asc);
        let names: Vec<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, vec!["alpha.mp4", "bravo.mp4", "charlie.mp4"]);
    }

    #[test]
    fn sort_by_name_desc() {
        let mut entries = vec![
            make_entry_with("alpha.mp4", 100, None),
            make_entry_with("charlie.mp4", 200, None),
            make_entry_with("bravo.mp4", 150, None),
        ];
        sort_entries(&mut entries, SortKey::Name, SortDir::Desc);
        let names: Vec<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, vec!["charlie.mp4", "bravo.mp4", "alpha.mp4"]);
    }

    #[test]
    fn sort_by_name_case_insensitive() {
        let mut entries = vec![
            make_entry_with("Zebra.mp4", 100, None),
            make_entry_with("alpha.mp4", 100, None),
        ];
        sort_entries(&mut entries, SortKey::Name, SortDir::Asc);
        assert_eq!(entries[0].file_name, "alpha.mp4");
        assert_eq!(entries[1].file_name, "Zebra.mp4");
    }

    #[test]
    fn sort_by_size_asc() {
        let mut entries = vec![
            make_entry_with("big.mp4", 300, None),
            make_entry_with("small.mp4", 100, None),
            make_entry_with("medium.mp4", 200, None),
        ];
        sort_entries(&mut entries, SortKey::Size, SortDir::Asc);
        let sizes: Vec<u64> = entries.iter().map(|e| e.fs.size_bytes).collect();
        assert_eq!(sizes, vec![100, 200, 300]);
    }

    #[test]
    fn sort_by_size_desc() {
        let mut entries = vec![
            make_entry_with("small.mp4", 100, None),
            make_entry_with("big.mp4", 300, None),
        ];
        sort_entries(&mut entries, SortKey::Size, SortDir::Desc);
        assert_eq!(entries[0].fs.size_bytes, 300);
        assert_eq!(entries[1].fs.size_bytes, 100);
    }

    #[test]
    fn sort_by_duration_asc() {
        let mut entries = vec![
            make_entry_with("long.mp4", 100, Some(300_000)),
            make_entry_with("short.mp4", 100, Some(60_000)),
            make_entry_with("medium.mp4", 100, Some(120_000)),
        ];
        sort_entries(&mut entries, SortKey::Duration, SortDir::Asc);
        let durations: Vec<Option<u64>> = entries.iter().map(|e| e.media.duration_ms).collect();
        assert_eq!(durations, vec![Some(60_000), Some(120_000), Some(300_000)]);
    }

    #[test]
    fn sort_by_resolution() {
        let mut entries = vec![
            make_entry_with_video("4k.mp4", 3840, 2160, "h264"),
            make_entry_with_video("720p.mp4", 1280, 720, "h264"),
            make_entry_with_video("1080p.mp4", 1920, 1080, "h264"),
        ];
        sort_entries(&mut entries, SortKey::Resolution, SortDir::Asc);
        assert_eq!(entries[0].file_name, "720p.mp4");
        assert_eq!(entries[1].file_name, "1080p.mp4");
        assert_eq!(entries[2].file_name, "4k.mp4");
    }

    #[test]
    fn sort_by_codec() {
        let mut entries = vec![
            make_entry_with_video("vp9.mp4", 1920, 1080, "vp9"),
            make_entry_with_video("h264.mp4", 1920, 1080, "h264"),
            make_entry_with_video("av1.mp4", 1920, 1080, "av1"),
        ];
        sort_entries(&mut entries, SortKey::Codec, SortDir::Asc);
        assert_eq!(entries[0].file_name, "av1.mp4");
        assert_eq!(entries[1].file_name, "h264.mp4");
        assert_eq!(entries[2].file_name, "vp9.mp4");
    }

    #[test]
    fn sort_by_codec_falls_back_to_audio() {
        let mut entries = vec![
            make_entry_with_audio_codec("opus.mp3", "opus"),
            make_entry_with_audio_codec("aac.mp3", "aac"),
        ];
        sort_entries(&mut entries, SortKey::Codec, SortDir::Asc);
        assert_eq!(entries[0].file_name, "aac.mp3");
        assert_eq!(entries[1].file_name, "opus.mp3");
    }

    #[test]
    fn sort_by_bitrate() {
        let mut entry_a = make_entry_with("high.mp4", 100, None);
        entry_a.media.overall_bitrate_bps = Some(10_000_000);
        let mut entry_b = make_entry_with("low.mp4", 100, None);
        entry_b.media.overall_bitrate_bps = Some(1_000_000);

        let mut entries = vec![entry_a, entry_b];
        sort_entries(&mut entries, SortKey::Bitrate, SortDir::Asc);
        assert_eq!(entries[0].file_name, "low.mp4");
        assert_eq!(entries[1].file_name, "high.mp4");
    }

    #[test]
    fn sort_empty_slice() {
        let mut entries: Vec<MediaEntry> = vec![];
        sort_entries(&mut entries, SortKey::Name, SortDir::Asc);
        assert!(entries.is_empty());
    }

    #[test]
    fn sort_single_element() {
        let mut entries = vec![make_entry_with("solo.mp4", 100, None)];
        sort_entries(&mut entries, SortKey::Name, SortDir::Asc);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name, "solo.mp4");
    }

    #[test]
    fn sort_duration_none_before_some() {
        let mut entries = vec![
            make_entry_with("has_duration.mp4", 100, Some(60_000)),
            make_entry_with("no_duration.mp4", 100, None),
        ];
        sort_entries(&mut entries, SortKey::Duration, SortDir::Asc);
        assert_eq!(entries[0].file_name, "no_duration.mp4");
        assert_eq!(entries[1].file_name, "has_duration.mp4");
    }

    // --- sort_dir_items ---

    fn make_dir_item(name: &str, size: u64, modified: Option<std::time::SystemTime>) -> DirItem {
        DirItem {
            path: PathBuf::from(format!("/test/{name}")),
            name: name.to_string(),
            name_lower: name.to_lowercase(),
            size_bytes: size,
            modified_at: modified,
        }
    }

    #[test]
    fn sort_dir_items_by_name_asc() {
        let mut dirs = vec![
            make_dir_item("Zebra", 0, None),
            make_dir_item("alpha", 0, None),
            make_dir_item("middle", 0, None),
        ];
        sort_dir_items(&mut dirs, SortKey::Name, SortDir::Asc);
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "middle", "Zebra"]);
    }

    #[test]
    fn sort_dir_items_by_name_desc() {
        let mut dirs = vec![
            make_dir_item("alpha", 0, None),
            make_dir_item("Zebra", 0, None),
            make_dir_item("middle", 0, None),
        ];
        sort_dir_items(&mut dirs, SortKey::Name, SortDir::Desc);
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["Zebra", "middle", "alpha"]);
    }

    #[test]
    fn sort_dir_items_by_size_asc() {
        let mut dirs = vec![
            make_dir_item("big", 300, None),
            make_dir_item("small", 100, None),
            make_dir_item("medium", 200, None),
        ];
        sort_dir_items(&mut dirs, SortKey::Size, SortDir::Asc);
        let sizes: Vec<u64> = dirs.iter().map(|d| d.size_bytes).collect();
        assert_eq!(sizes, vec![100, 200, 300]);
    }

    #[test]
    fn sort_dir_items_by_modified() {
        use std::time::{Duration, SystemTime};
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let t2 = SystemTime::UNIX_EPOCH + Duration::from_secs(2000);
        let t3 = SystemTime::UNIX_EPOCH + Duration::from_secs(3000);
        let mut dirs = vec![
            make_dir_item("newest", 0, Some(t3)),
            make_dir_item("oldest", 0, Some(t1)),
            make_dir_item("middle", 0, Some(t2)),
        ];
        sort_dir_items(&mut dirs, SortKey::Modified, SortDir::Asc);
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["oldest", "middle", "newest"]);
    }

    #[test]
    fn sort_dir_items_media_key_falls_back_to_name() {
        let mut dirs = vec![
            make_dir_item("Zebra", 0, None),
            make_dir_item("alpha", 0, None),
        ];
        // Duration is media-only, should fall back to Name sort
        sort_dir_items(&mut dirs, SortKey::Duration, SortDir::Asc);
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Zebra"]);

        // Codec too
        let mut dirs2 = vec![
            make_dir_item("Zebra", 0, None),
            make_dir_item("alpha", 0, None),
        ];
        sort_dir_items(&mut dirs2, SortKey::Codec, SortDir::Asc);
        let names2: Vec<&str> = dirs2.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names2, vec!["alpha", "Zebra"]);
    }
}
