/// Structured output formatting (JSON envelope, NDJSON streaming).
///
/// Handles both `--json` (single document) and `--ndjson` (streaming) modes.
use crate::types::{
    ListSummary, MediaEntry, NdjsonRecord, ProbeError,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::io::Write;

const SCHEMA_VERSION: &str = "0.1.0";
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

    serde_json::to_writer_pretty(writer, &envelope)
        .context("failed to write JSON output")?;
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
    serde_json::to_writer(&mut *writer, &record)
        .context("failed to write NDJSON header")?;
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
    serde_json::to_writer(&mut *writer, &record)
        .context("failed to write NDJSON entry")?;
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
    serde_json::to_writer(&mut *writer, &record)
        .context("failed to write NDJSON footer")?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

/// Write a single `MediaEntry` as pretty-printed JSON (for `mls info`).
///
/// # Errors
/// Returns an error if writing fails.
pub fn write_info_json<W: Write>(
    writer: &mut W,
    entries: &[MediaEntry],
) -> Result<()> {
    if entries.len() == 1 {
        serde_json::to_writer_pretty(writer, &entries[0])
            .context("failed to write info JSON")?;
    } else {
        serde_json::to_writer_pretty(writer, entries)
            .context("failed to write info JSON")?;
    }
    Ok(())
}
