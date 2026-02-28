/// Video thumbnail generation via ffmpeg subprocess.
///
/// Extracts a single frame from a video file at a configurable seek position.
/// Uses an LRU cache to avoid re-generating thumbnails.
use anyhow::{Context, Result};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::process::Command;

/// Thumbnail cache: maps file path to decoded image bytes (JPEG).
pub struct ThumbnailCache {
    cache: Mutex<LruCache<PathBuf, Vec<u8>>>,
    cache_dir: PathBuf,
}

impl ThumbnailCache {
    /// Create a new cache with the given capacity.
    ///
    /// Cache dir is created if it doesn't exist.
    ///
    /// # Errors
    /// Returns an error if the cache directory cannot be created.
    pub fn new(capacity: usize, cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)
            .context("failed to create thumbnail cache directory")?;
        let size = NonZeroUsize::new(capacity)
            .context("cache capacity must be > 0")?;
        Ok(Self {
            cache: Mutex::new(LruCache::new(size)),
            cache_dir,
        })
    }

    /// Get or generate a thumbnail for the given video file.
    ///
    /// Returns JPEG bytes. Uses memory cache first, then disk cache,
    /// then generates via ffmpeg.
    ///
    /// # Errors
    /// Returns an error if thumbnail generation fails.
    pub async fn get_or_generate(&self, path: &Path) -> Result<Vec<u8>> {
        let canonical = path.to_path_buf();

        // Check memory cache
        {
            let mut cache = self.cache.lock().map_err(|e| {
                anyhow::anyhow!("cache lock poisoned: {e}")
            })?;
            if let Some(data) = cache.get(&canonical) {
                return Ok(data.clone());
            }
        }

        // Check disk cache
        let disk_path = self.disk_cache_path(path);
        if disk_path.exists() {
            let data = tokio::fs::read(&disk_path).await
                .context("failed to read cached thumbnail")?;
            let mut cache = self.cache.lock().map_err(|e| {
                anyhow::anyhow!("cache lock poisoned: {e}")
            })?;
            cache.put(canonical, data.clone());
            return Ok(data);
        }

        // Generate
        let data = generate_thumbnail(path).await?;

        // Write to disk cache (best effort)
        let _ = tokio::fs::write(&disk_path, &data).await;

        // Store in memory cache
        {
            let mut cache = self.cache.lock().map_err(|e| {
                anyhow::anyhow!("cache lock poisoned: {e}")
            })?;
            cache.put(path.to_path_buf(), data.clone());
        }

        Ok(data)
    }

    fn disk_cache_path(&self, path: &Path) -> PathBuf {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let hash = hasher.finish();
        self.cache_dir.join(format!("{hash:016x}.jpg"))
    }
}

/// Generate a JPEG thumbnail from a video file using ffmpeg.
///
/// Seeks to 5 seconds (or 0 for short clips) and extracts one frame,
/// scaled to fit 320px width.
async fn generate_thumbnail(path: &Path) -> Result<Vec<u8>> {
    let output_buf = tempfile_path();

    let result = Command::new("ffmpeg")
        .args([
            "-ss", "5",
            "-i",
        ])
        .arg(path)
        .args([
            "-frames:v", "1",
            "-vf", "scale=320:-1",
            "-q:v", "5",
            "-y",
        ])
        .arg(&output_buf)
        .output()
        .await
        .context("failed to execute ffmpeg for thumbnail")?;

    if !result.status.success() {
        // Retry at position 0 (file might be shorter than 5s)
        let result = Command::new("ffmpeg")
            .args(["-ss", "0", "-i"])
            .arg(path)
            .args([
                "-frames:v", "1",
                "-vf", "scale=320:-1",
                "-q:v", "5",
                "-y",
            ])
            .arg(&output_buf)
            .output()
            .await
            .context("ffmpeg retry at 0s failed")?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            anyhow::bail!("ffmpeg thumbnail generation failed: {stderr}");
        }
    }

    let data = tokio::fs::read(&output_buf).await
        .context("failed to read generated thumbnail")?;
    let _ = tokio::fs::remove_file(&output_buf).await;
    Ok(data)
}

fn tempfile_path() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("mls_thumb_{ts}.jpg"))
}

/// Default thumbnail cache directory.
#[must_use]
pub fn default_cache_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        dirs_cache_macos()
    } else {
        dirs_cache_xdg()
    }
}

fn dirs_cache_macos() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Caches")
        .join("mls")
        .join("thumbnails")
}

fn dirs_cache_xdg() -> PathBuf {
    let cache = std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.cache")
    });
    PathBuf::from(cache).join("mls").join("thumbnails")
}
