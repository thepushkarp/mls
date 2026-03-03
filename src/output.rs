/// Structured output formatting (JSON envelope, NDJSON streaming).
///
/// Handles both `--json` (single document) and `--ndjson` (streaming) modes.
use crate::types::{ListSummary, MediaEntry, NdjsonRecord, ProbeError};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::io::Write;

const SCHEMA_VERSION: &str = "0.2.0";
const MLS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Borrowing envelope for JSON serialization (avoids cloning entries).
#[derive(Serialize)]
struct ListEnvelopeRef<'a> {
    #[serde(rename = "type")]
    doc_type: &'a str,
    schema_version: &'a str,
    mls_version: &'a str,
    generated_at: DateTime<Utc>,
    summary: ListSummary,
    entries: &'a [MediaEntry],
    #[serde(skip_serializing_if = "slice_is_empty")]
    errors: &'a [ProbeError],
}

fn slice_is_empty(s: &&[ProbeError]) -> bool {
    s.is_empty()
}

/// Write a complete JSON envelope to the given writer.
///
/// # Errors
/// Returns an error if serialization or writing fails.
pub fn write_json<W: Write>(
    writer: &mut W,
    entries: &[MediaEntry],
    errors: &[ProbeError],
) -> Result<()> {
    let summary = ListSummary {
        entries_total: entries.len() + errors.len(),
        entries_emitted: entries.len(),
        probe_ok: entries.len(),
        probe_error: errors.len(),
    };

    let envelope = ListEnvelopeRef {
        doc_type: "mls.list",
        schema_version: SCHEMA_VERSION,
        mls_version: MLS_VERSION,
        generated_at: Utc::now(),
        summary,
        entries,
        errors,
    };

    serde_json::to_writer_pretty(&mut *writer, &envelope).context("failed to write JSON output")?;
    writer.flush().context("failed to flush JSON output")?;
    Ok(())
}

/// Write NDJSON header record.
///
/// # Errors
/// Returns an error if writing fails.
pub fn write_ndjson_header<W: Write>(writer: &mut W) -> Result<()> {
    let record = NdjsonRecord::Header {
        schema_version: SCHEMA_VERSION.to_string(),
        mls_version: MLS_VERSION.to_string(),
        generated_at: Utc::now(),
    };
    serde_json::to_writer(&mut *writer, &record).context("failed to write NDJSON header")?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

/// Borrowing wrapper for NDJSON entry serialization (avoids cloning).
#[derive(Serialize)]
#[serde(tag = "type", rename = "mls.entry")]
struct NdjsonEntryRef<'a> {
    entry: &'a MediaEntry,
}

/// Write a single NDJSON entry record.
///
/// # Errors
/// Returns an error if writing fails.
pub fn write_ndjson_entry<W: Write>(writer: &mut W, entry: &MediaEntry) -> Result<()> {
    let record = NdjsonEntryRef { entry };
    serde_json::to_writer(&mut *writer, &record).context("failed to write NDJSON entry")?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

/// Borrowing wrapper for NDJSON footer serialization (avoids cloning).
#[derive(Serialize)]
#[serde(tag = "type", rename = "mls.footer")]
struct NdjsonFooterRef<'a> {
    summary: &'a ListSummary,
    #[serde(skip_serializing_if = "slice_is_empty")]
    errors: &'a [ProbeError],
}

/// Write NDJSON footer record.
///
/// # Errors
/// Returns an error if writing fails.
pub fn write_ndjson_footer<W: Write>(
    writer: &mut W,
    summary: &ListSummary,
    errors: &[ProbeError],
) -> Result<()> {
    let record = NdjsonFooterRef { summary, errors };
    serde_json::to_writer(&mut *writer, &record).context("failed to write NDJSON footer")?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

/// Write a single `MediaEntry` as pretty-printed JSON (for `mls info`).
///
/// # Errors
/// Returns an error if writing fails.
pub fn write_info_json<W: Write>(writer: &mut W, entries: &[MediaEntry]) -> Result<()> {
    if entries.len() == 1 {
        serde_json::to_writer_pretty(&mut *writer, &entries[0])
            .context("failed to write info JSON")?;
    } else {
        serde_json::to_writer_pretty(&mut *writer, entries).context("failed to write info JSON")?;
    }
    writer.flush().context("failed to flush info JSON")?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::{
        ContainerInfo, FsInfo, MediaEntry, MediaInfo, MediaKind, MediaTags, ProbeError, ProbeInfo,
    };
    use std::borrow::Cow;
    use std::path::PathBuf;

    fn make_entry() -> MediaEntry {
        MediaEntry {
            file_name: "test.mp4".to_string(),
            path: PathBuf::from("/test/test.mp4"),
            extension: "mp4".to_string(),
            fs: FsInfo {
                size_bytes: 1000,
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
                exif: None,
                doc: None,
            },
            probe: ProbeInfo {
                backend: Cow::Borrowed("ffprobe"),
                took_ms: 0,
                error: None,
            },
        }
    }

    fn make_error() -> ProbeError {
        ProbeError {
            path: PathBuf::from("/bad.mp4"),
            error: "timeout".to_string(),
        }
    }

    #[test]
    fn slice_is_empty_true() {
        assert!(slice_is_empty(&&[][..]));
    }

    #[test]
    fn slice_is_empty_false() {
        assert!(!slice_is_empty(&&[make_error()][..]));
    }

    #[test]
    fn write_json_empty_entries_valid() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(val["type"], "mls.list");
        assert_eq!(val["schema_version"], "0.2.0");
        assert_eq!(val["summary"]["entries_total"], 0);
    }

    #[test]
    fn write_json_with_entries_and_errors() {
        let entries = vec![make_entry()];
        let errors = vec![make_error()];
        let mut buf = Vec::new();
        write_json(&mut buf, &entries, &errors).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(val["summary"]["entries_total"], 2);
        assert_eq!(val["entries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn write_json_errors_omitted_when_empty() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let obj = val.as_object().unwrap();
        assert!(obj.get("errors").is_none());
    }

    #[test]
    fn write_json_errors_present_when_nonempty() {
        let errors = vec![make_error()];
        let mut buf = Vec::new();
        write_json(&mut buf, &[], &errors).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let obj = val.as_object().unwrap();
        assert!(obj.get("errors").is_some());
    }

    #[test]
    fn write_json_has_mls_version() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let ver = val["mls_version"].as_str().unwrap();
        assert!(!ver.is_empty());
    }

    #[test]
    fn write_json_has_generated_at() {
        let mut buf = Vec::new();
        write_json(&mut buf, &[], &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let ts = val["generated_at"].as_str().unwrap();
        assert!(!ts.is_empty());
    }

    #[test]
    fn write_ndjson_header_type() {
        let mut buf = Vec::new();
        write_ndjson_header(&mut buf).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(val["type"], "mls.header");
    }

    #[test]
    fn write_ndjson_entry_type() {
        let entry = make_entry();
        let mut buf = Vec::new();
        write_ndjson_entry(&mut buf, &entry).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(val["type"], "mls.entry");
    }

    #[test]
    fn write_ndjson_footer_type() {
        let summary = ListSummary::default();
        let mut buf = Vec::new();
        write_ndjson_footer(&mut buf, &summary, &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(val["type"], "mls.footer");
    }

    #[test]
    fn write_ndjson_footer_errors_omitted_when_empty() {
        let summary = ListSummary::default();
        let mut buf = Vec::new();
        write_ndjson_footer(&mut buf, &summary, &[]).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let obj = val.as_object().unwrap();
        assert!(obj.get("errors").is_none());
    }

    #[test]
    fn write_info_json_single_not_array() {
        let entries = vec![make_entry()];
        let mut buf = Vec::new();
        write_info_json(&mut buf, &entries).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn write_info_json_multiple_as_array() {
        let entries = vec![make_entry(), make_entry()];
        let mut buf = Vec::new();
        write_info_json(&mut buf, &entries).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(val.is_array());
    }
}
