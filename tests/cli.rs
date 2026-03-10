#![expect(clippy::unwrap_used)]
#![expect(
    deprecated,
    reason = "cargo_bin deprecation may be reverted (assert-rs/assert_cmd#265)"
)]
//! Integration tests for the mls CLI.
//!
//! Uses mock ffprobe/ffmpeg scripts prepended to PATH so tests
//! don't require real media files or external tools.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;

/// Path to the `mock_bin` directory containing fake ffprobe/ffmpeg.
fn mock_bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mock_bin")
}

/// Build a PATH string with `mock_bin` prepended.
fn mock_path() -> String {
    let mock = mock_bin_dir();
    let system_path = std::env::var("PATH").unwrap_or_default();
    format!("{}:{system_path}", mock.display())
}

/// Create a temp directory with fake media files.
fn setup_media_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("video_a.mp4"), b"fake mp4 data").unwrap();
    fs::write(tmp.path().join("video_b.mkv"), b"fake mkv data").unwrap();
    fs::write(tmp.path().join("song.mp3"), b"fake mp3 data").unwrap();
    fs::write(tmp.path().join("photo.jpg"), b"fake jpg data").unwrap();
    fs::write(tmp.path().join("screenshot.png"), b"fake png data").unwrap();
    fs::write(tmp.path().join("readme.txt"), b"not media").unwrap();
    tmp
}

fn mls_cmd() -> Command {
    let mut cmd = Command::new(cargo_bin("mls"));
    cmd.env("PATH", mock_path());
    cmd.arg("--quiet");
    cmd
}

// --- Basic CLI tests ---

#[test]
fn help_exits_zero() {
    Command::new(cargo_bin("mls"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Media LS"));
}

#[test]
fn version_exits_zero() {
    Command::new(cargo_bin("mls"))
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mls"));
}

// --- Dependency checking ---

#[test]
fn missing_ffprobe_exits_4() {
    Command::new(cargo_bin("mls"))
        .env("PATH", "/nonexistent")
        .arg("--json")
        .arg("--quiet")
        .arg(".")
        .assert()
        .code(4);
}

#[test]
fn play_without_mpv_exits_4_with_install_hint() {
    let tmp = setup_media_dir();
    Command::new(cargo_bin("mls"))
        .env("PATH", mock_bin_dir())
        .arg("--quiet")
        .arg("play")
        .arg(tmp.path().join("song.mp3"))
        .assert()
        .code(4)
        .stderr(predicate::str::contains("Playback requires mpv"))
        .stderr(predicate::str::contains("brew install mpv"));
}

// --- Validation errors ---

#[test]
fn invalid_filter_exits_2() {
    let tmp = setup_media_dir();
    mls_cmd()
        .arg("--json")
        .arg("--filter")
        .arg("<<<invalid>>>")
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid filter"));
}

#[test]
fn unknown_sort_key_exits_2() {
    let tmp = setup_media_dir();
    mls_cmd()
        .arg("--json")
        .arg("--sort")
        .arg("nonexistent_key")
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown sort key"));
}

// --- JSON output ---

#[test]
fn json_output_valid_schema() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    assert!(output.status.success(), "mls --json failed");

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["type"], "mls.list");
    assert_eq!(json["schema_version"], "0.2.0");
    assert!(json["entries"].is_array());
    assert!(json["summary"].is_object());
    assert!(json["summary"]["entries_total"].is_number());

    let entries = json["entries"].as_array().unwrap();
    // 5 AV/image files (mp4, mkv, mp3, jpg, png) + 1 document (txt)
    assert_eq!(entries.len(), 6);
}

#[test]
fn json_entries_have_required_fields() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    for entry in entries {
        assert!(entry["path"].is_string(), "missing path");
        assert!(entry["file_name"].is_string(), "missing file_name");
        assert!(entry["extension"].is_string(), "missing extension");
        assert!(entry["fs"]["size_bytes"].is_number(), "missing size_bytes");
        assert!(entry["media"]["kind"].is_string(), "missing kind");
        assert!(entry["probe"]["backend"].is_string(), "missing backend");
    }
}

// --- NDJSON output ---

#[test]
fn ndjson_has_header_and_footer() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--ndjson").arg(tmp.path()).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();

    assert!(lines.len() >= 2, "need at least header + footer");

    let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(header["type"], "mls.header");
    assert_eq!(header["schema_version"], "0.2.0");

    let footer: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(footer["type"], "mls.footer");
    assert!(footer["summary"].is_object());
}

#[test]
fn ndjson_entries_are_valid_json() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--ndjson").arg(tmp.path()).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    for line in stdout.trim().lines() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "invalid JSON line: {line}");
    }
}

// --- Filter and limit ---

#[test]
fn json_filter_reduces_results() {
    let tmp = setup_media_dir();
    let output = mls_cmd()
        .arg("--json")
        .arg("--filter")
        .arg("extension == \"mp4\"")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    // Only the .mp4 file should pass
    assert_eq!(entries.len(), 1);
    let name = entries[0]["file_name"].as_str().unwrap();
    assert_eq!(
        std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str()),
        Some("mp4")
    );
}

#[test]
fn json_limit_truncates() {
    let tmp = setup_media_dir();
    let output = mls_cmd()
        .arg("--json")
        .arg("--limit")
        .arg("1")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
}

// --- Image support ---

#[test]
fn json_images_have_kind_image() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    let images: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["media"]["kind"] == "image")
        .collect();

    assert_eq!(images.len(), 2, "expected 2 image entries (jpg + png)");

    for img in &images {
        let ext = img["extension"].as_str().unwrap();
        assert!(
            ext == "jpg" || ext == "png",
            "unexpected image extension: {ext}"
        );
    }
}

#[test]
fn json_images_have_dimensions_but_no_duration() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    let img = entries
        .iter()
        .find(|e| e["file_name"] == "photo.jpg")
        .unwrap();

    assert_eq!(img["media"]["kind"], "image");
    // Images have dimensions via video stream
    assert!(img["media"]["video"]["width"].is_number());
    assert!(img["media"]["video"]["height"].is_number());
    // Images should not have duration or bitrate
    assert!(img["media"]["duration_ms"].is_null());
    assert!(img["media"]["overall_bitrate_bps"].is_null());
}

#[test]
fn json_filter_kind_image_returns_only_images() {
    let tmp = setup_media_dir();
    let output = mls_cmd()
        .arg("--json")
        .arg("--filter")
        .arg("kind == image")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    assert_eq!(entries.len(), 2, "expected only 2 image entries");
    for entry in entries {
        assert_eq!(entry["media"]["kind"], "image");
    }
}

#[test]
fn json_filter_kind_excludes_other_kinds() {
    let tmp = setup_media_dir();

    // kind == audio should return only the mp3
    let output = mls_cmd()
        .arg("--json")
        .arg("--filter")
        .arg("kind == audio")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    assert_eq!(entries.len(), 1, "expected only 1 audio entry");
    assert_eq!(entries[0]["media"]["kind"], "audio");
    assert_eq!(entries[0]["extension"], "mp3");
}

// --- Document support ---

#[test]
fn json_documents_have_kind_document() {
    let tmp = setup_media_dir();
    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    let docs: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["media"]["kind"] == "document")
        .collect();

    assert_eq!(docs.len(), 1, "expected 1 document entry (txt)");
    assert_eq!(docs[0]["extension"], "txt");
    assert_eq!(docs[0]["probe"]["backend"], "native");
}

#[test]
fn json_filter_kind_document_returns_only_documents() {
    let tmp = setup_media_dir();
    let output = mls_cmd()
        .arg("--json")
        .arg("--filter")
        .arg("kind == document")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    assert_eq!(entries.len(), 1, "expected only 1 document entry");
    assert_eq!(entries[0]["media"]["kind"], "document");
}

#[test]
fn json_document_has_line_count() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("notes.txt"),
        b"line one\nline two\nline three\n",
    )
    .unwrap();

    let output = mls_cmd().arg("--json").arg(tmp.path()).output().unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();

    assert_eq!(entries.len(), 1);
    let doc = &entries[0];
    assert_eq!(doc["media"]["kind"], "document");
    assert_eq!(doc["media"]["doc"]["format"], "txt");
    assert_eq!(doc["media"]["doc"]["line_count"], 3);
}

#[test]
fn json_sort_by_pages() {
    let tmp = setup_media_dir();
    let output = mls_cmd()
        .arg("--json")
        .arg("--sort")
        .arg("pages")
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
}
