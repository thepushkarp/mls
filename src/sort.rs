/// Sorting logic for media entries.
///
/// Supports sorting by all metadata fields with configurable direction.
use crate::types::{MediaEntry, SortDir, SortKey};

/// Parse a sort specification string (e.g., "`duration_ms:desc`", "name:asc").
///
/// # Errors
/// Returns `None` if the sort key is not recognized.
#[must_use]
pub fn parse_sort_spec(spec: &str) -> Option<(SortKey, SortDir)> {
    let parts: Vec<&str> = spec.split(':').collect();
    let key = match parts[0] {
        "path" => SortKey::Path,
        "name" => SortKey::Name,
        "size" => SortKey::Size,
        "date" | "modified" => SortKey::Modified,
        "duration" | "duration_ms" => SortKey::Duration,
        "resolution" => SortKey::Resolution,
        "codec" => SortKey::Codec,
        "bitrate" => SortKey::Bitrate,
        _ => return None,
    };
    let dir = match parts.get(1) {
        Some(&"desc") => SortDir::Desc,
        _ => SortDir::Asc,
    };
    Some((key, dir))
}

/// Sort entries in place by the given key and direction.
pub fn sort_entries(entries: &mut [MediaEntry], key: SortKey, dir: SortDir) {
    entries.sort_by(|a, b| {
        let cmp = compare_by_key(a, b, key);
        match dir {
            SortDir::Asc => cmp,
            SortDir::Desc => cmp.reverse(),
        }
    });
}

fn compare_by_key(
    a: &MediaEntry,
    b: &MediaEntry,
    key: SortKey,
) -> std::cmp::Ordering {
    match key {
        SortKey::Path => a.path.cmp(&b.path),
        SortKey::Name => a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()),
        SortKey::Size => a.fs.size_bytes.cmp(&b.fs.size_bytes),
        SortKey::Modified => a.fs.modified_at.cmp(&b.fs.modified_at),
        SortKey::Duration => {
            a.media.duration_ms.cmp(&b.media.duration_ms)
        }
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
        SortKey::Bitrate => {
            a.media.overall_bitrate_bps.cmp(&b.media.overall_bitrate_bps)
        }
    }
}
