#![expect(clippy::unwrap_used)]
#![allow(deprecated)] // cargo_bin deprecation may be reverted (assert-rs/assert_cmd#265)
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
    assert_eq!(json["schema_version"], "0.1.0");
    assert!(json["entries"].is_array());
    assert!(json["summary"].is_object());
    assert!(json["summary"]["entries_total"].is_number());

    let entries = json["entries"].as_array().unwrap();
    // Should find 3 media files (mp4, mkv, mp3) — not the .txt
    assert_eq!(entries.len(), 3);
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
    assert_eq!(header["schema_version"], "0.1.0");

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
