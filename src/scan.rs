/// Directory scanning — walks paths and discovers media files.
///
/// Filters by recognized media file extensions. Uses tokio for concurrent
/// metadata probing with configurable concurrency.
use crate::probe;
use crate::types::{MediaEntry, ProbeError, is_media_extension};
use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

/// Scan result: either a successfully probed entry or a probe error.
#[derive(Debug)]
pub enum ScanResult {
    Entry(Box<MediaEntry>),
    Error(ProbeError),
}

/// Walk directories and collect media file paths (no probing yet).
pub fn discover_media_files(paths: &[PathBuf], max_depth: Option<usize>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut visited = HashSet::new();
    for path in paths {
        if path.is_file() {
            if has_media_extension(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            walk_dir(
                path,
                max_depth.unwrap_or(usize::MAX),
                0,
                &mut files,
                &mut visited,
            );
        }
    }
    files
}

fn walk_dir(
    dir: &Path,
    max_depth: usize,
    current_depth: usize,
    out: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) {
    if current_depth > max_depth {
        return;
    }

    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        tracing::debug!("skipping already-visited directory: {}", dir.display());
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, max_depth, current_depth + 1, out, visited);
        } else if has_media_extension(&path) {
            out.push(path);
        }
    }
}

fn has_media_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_media_extension)
}

/// Probe all files concurrently, sending results through a channel.
///
/// Spawns at most `concurrency` tasks at a time using `JoinSet`,
/// avoiding unbounded task allocation for large file lists.
pub async fn probe_files(
    files: Vec<PathBuf>,
    concurrency: usize,
    timeout_ms: u64,
    tx: mpsc::Sender<ScanResult>,
) {
    let mut tasks = tokio::task::JoinSet::new();

    for file in files {
        // Wait for a slot if at concurrency limit
        while tasks.len() >= concurrency {
            let _ = tasks.join_next().await;
        }

        let tx = tx.clone();
        tasks.spawn(async move {
            match probe::probe_file(&file, timeout_ms).await {
                Ok(entry) => {
                    let _ = tx.send(ScanResult::Entry(Box::new(entry))).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ScanResult::Error(ProbeError {
                            path: file,
                            error: e.to_string(),
                        }))
                        .await;
                }
            }
        });
    }

    // Drain remaining tasks
    while tasks.join_next().await.is_some() {}
}

/// Convenience: discover + probe all, collecting into vectors.
pub async fn scan_all(
    paths: &[PathBuf],
    max_depth: Option<usize>,
    concurrency: usize,
    timeout_ms: u64,
) -> Result<(Vec<MediaEntry>, Vec<ProbeError>)> {
    let files = discover_media_files(paths, max_depth);
    let (tx, mut rx) = mpsc::channel(256);

    tokio::spawn(async move {
        probe_files(files, concurrency, timeout_ms, tx).await;
    });

    let mut entries = Vec::new();
    let mut errors = Vec::new();

    while let Some(result) = rx.recv().await {
        match result {
            ScanResult::Entry(e) => entries.push(*e),
            ScanResult::Error(e) => errors.push(e),
        }
    }

    Ok((entries, errors))
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_media_files_in_flat_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("a.mp4"), b"fake").unwrap();
        fs::write(root.join("b.mp3"), b"fake").unwrap();
        fs::write(root.join("c.txt"), b"not media").unwrap();

        let files = discover_media_files(&[root.to_path_buf()], None);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn respects_max_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("top.mp4"), b"fake").unwrap();
        fs::write(nested.join("deep.mp4"), b"fake").unwrap();

        let files = discover_media_files(&[root.to_path_buf()], Some(1));
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("top.mp4"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_self_referencing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let a = root.join("a");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("song.mp3"), b"fake").unwrap();

        std::os::unix::fs::symlink(&a, a.join("loop")).unwrap();

        let files = discover_media_files(&[root.to_path_buf()], None);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("song.mp3"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_mutual() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();

        std::os::unix::fs::symlink(&b, a.join("to_b")).unwrap();
        std::os::unix::fs::symlink(&a, b.join("to_a")).unwrap();

        fs::write(a.join("song.mp3"), b"fake").unwrap();
        fs::write(b.join("video.mp4"), b"fake").unwrap();

        let files = discover_media_files(&[root.to_path_buf()], None);

        assert_eq!(files.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_parent_does_not_recurse() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let child = root.join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("track.flac"), b"fake").unwrap();

        std::os::unix::fs::symlink(root, child.join("parent_link")).unwrap();

        let files = discover_media_files(&[root.to_path_buf()], None);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("track.flac"));
    }
}
