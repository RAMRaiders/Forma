//! Resized copies of artwork, for screens that show it small.
//!
//! Scraped box art is often 1–2 MB at full size. Decoding that for every card while scrolling a
//! grid is what makes a Raspberry Pi stutter, so Forma serves a resized copy at the size the
//! theme asks for, made once in the background and cached on disk.
//!
//! - Widths snap up to a few fixed sizes ([`WIDTHS`]), so a theme asking for 437 px and 451 px
//!   shares one file, and no request can fill the cache with variants.
//! - The cache key is the source's path, size and modification time: a re-scraped image gets a
//!   new thumbnail, and the old one ages out.
//! - Opaque art becomes JPEG, art with transparency (logos, marquees) PNG.
//! - A source already small enough, or one that cannot be decoded, is remembered as "serve the
//!   original", so it is not decoded again.
//! - The cache is bounded and safe to delete at any time.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader};

use crate::error::{Error, Result};

/// The widths thumbnails are made at; a request snaps up to the next one.
pub const WIDTHS: &[u32] = &[160, 320, 480, 640, 960, 1280];

/// Default cap on the cache, in bytes.
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

const JPEG_QUALITY: u8 = 82;
/// Eviction scans the folder, so it runs every this many writes rather than on each.
const EVICT_EVERY: u32 = 64;

/// The width a request for `width` is served at.
pub fn bucket(width: u32) -> u32 {
    WIDTHS
        .iter()
        .copied()
        .find(|w| *w >= width)
        .unwrap_or(WIDTHS[WIDTHS.len() - 1])
}

/// What the cache holds for a source at a width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    Thumbnail(PathBuf),
    /// The source is small enough already, or could not be decoded.
    UseOriginal,
}

pub struct Thumbs {
    dir: PathBuf,
    max_bytes: u64,
    writes: AtomicU32,
}

impl Thumbs {
    pub fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            dir,
            max_bytes,
            writes: AtomicU32::new(0),
        }
    }

    fn key(source: &Path, width: u32) -> Option<String> {
        let meta = std::fs::metadata(source).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (source, meta.len(), mtime, width).hash(&mut h);
        Some(format!("{:016x}-{width}", h.finish()))
    }

    /// What is cached for `source` at `width` (already snapped with [`bucket`]), if anything.
    pub fn cached(&self, source: &Path, width: u32) -> Option<Cached> {
        let key = Self::key(source, width)?;
        if self.dir.join(format!("{key}.orig")).exists() {
            return Some(Cached::UseOriginal);
        }
        ["jpg", "png"]
            .into_iter()
            .map(|ext| self.dir.join(format!("{key}.{ext}")))
            .find(|f| f.is_file())
            .map(Cached::Thumbnail)
    }

    /// Makes (or remembers not to make) the thumbnail. Slow: call it off the UI thread.
    pub fn generate(&self, source: &Path, width: u32) -> Result<Cached> {
        if let Some(done) = self.cached(source, width) {
            return Ok(done);
        }
        let key = Self::key(source, width).ok_or_else(|| Error::Parse {
            path: source.to_path_buf(),
            message: "artwork is missing".into(),
        })?;
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::io(&self.dir, e))?;

        let decoded = ImageReader::open(source)
            .map_err(|e| Error::io(source, e))?
            .with_guessed_format()
            .map_err(|e| Error::io(source, e))?
            .decode();
        let image = match decoded {
            Ok(image) if image.width() > width => image,
            // Small enough already, or not an image we can read: the original it is.
            _ => {
                self.write(&format!("{key}.orig"), |_| Ok(()))?;
                return Ok(Cached::UseOriginal);
            }
        };

        // `resize` keeps the aspect ratio within the box; only the width constrains.
        let small = image.resize(width, u32::MAX, FilterType::Triangle);
        let file = if image.color().has_alpha() {
            self.write(&format!("{key}.png"), |w| {
                small
                    .write_to(w, ImageFormat::Png)
                    .map_err(|e| e.to_string())
            })?
        } else {
            self.write(&format!("{key}.jpg"), |w| {
                JpegEncoder::new_with_quality(w, JPEG_QUALITY)
                    .encode_image(&small.to_rgb8())
                    .map_err(|e| e.to_string())
            })?
        };
        if self.writes.fetch_add(1, Ordering::Relaxed) % EVICT_EVERY == EVICT_EVERY - 1 {
            self.evict();
        }
        Ok(Cached::Thumbnail(file))
    }

    /// Writes through a temporary file, so a reader never sees half a thumbnail.
    fn write(
        &self,
        name: &str,
        body: impl FnOnce(&mut BufWriter<std::fs::File>) -> std::result::Result<(), String>,
    ) -> Result<PathBuf> {
        let file = self.dir.join(name);
        let temp = self.dir.join(format!(".{name}.tmp"));
        let mut out =
            BufWriter::new(std::fs::File::create(&temp).map_err(|e| Error::io(&temp, e))?);
        body(&mut out).map_err(|message| Error::Parse {
            path: file.clone(),
            message,
        })?;
        out.into_inner()
            .map_err(|e| Error::io(&temp, e.into_error()))?
            .sync_all()
            .map_err(|e| Error::io(&temp, e))?;
        std::fs::rename(&temp, &file).map_err(|e| Error::io(&file, e))?;
        Ok(file)
    }

    /// Removes the oldest files until the cache is under 90% of its cap.
    pub fn evict(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                meta.is_file()
                    .then(|| Some((meta.modified().ok()?, meta.len(), e.path())))?
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= self.max_bytes {
            return;
        }
        files.sort();
        let target = self.max_bytes / 10 * 9;
        for (_, len, path) in files {
            if total <= target {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

/// [`Thumbs`] with a background worker: asks are answered at once, from the cache or with
/// "not yet", and missing thumbnails are made one at a time.
pub struct Thumbnailer {
    thumbs: Arc<Thumbs>,
    tx: Mutex<Sender<(PathBuf, u32)>>,
    queued: Arc<Mutex<HashSet<(PathBuf, u32)>>>,
}

impl Thumbnailer {
    /// One worker: on a Pi, decoding in parallel with scrolling costs more than it saves. It
    /// stops when the thumbnailer is dropped.
    pub fn spawn(thumbs: Thumbs) -> Self {
        let thumbs = Arc::new(thumbs);
        let queued: Arc<Mutex<HashSet<(PathBuf, u32)>>> = Arc::default();
        let (tx, rx) = mpsc::channel::<(PathBuf, u32)>();
        let (worker_thumbs, worker_queued) = (thumbs.clone(), queued.clone());
        let _ = std::thread::Builder::new()
            .name("forma-thumbs".into())
            .spawn(move || {
                for job in rx {
                    let _ = worker_thumbs.generate(&job.0, job.1);
                    worker_queued
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&job);
                }
            });
        Self {
            thumbs,
            tx: Mutex::new(tx),
            queued,
        }
    }

    /// The file to serve for `source` at `width`: the thumbnail when it is ready, else the
    /// original, with the thumbnail queued for next time.
    pub fn file_for(&self, source: &Path, width: u32) -> PathBuf {
        let width = bucket(width);
        match self.thumbs.cached(source, width) {
            Some(Cached::Thumbnail(file)) => file,
            Some(Cached::UseOriginal) => source.to_path_buf(),
            None => {
                let job = (source.to_path_buf(), width);
                let fresh = self
                    .queued
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(job.clone());
                if fresh {
                    let _ = self
                        .tx
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .send(job);
                }
                source.to_path_buf()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};
    use std::time::{Duration, Instant};

    fn opaque(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        RgbImage::from_pixel(w, h, Rgb([200, 30, 60]))
            .save(&path)
            .unwrap();
        path
    }

    fn dims(path: &Path) -> (u32, u32) {
        ImageReader::open(path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap()
    }

    #[test]
    fn widths_snap_up_to_a_few_sizes() {
        assert_eq!(bucket(1), 160);
        assert_eq!(bucket(320), 320);
        assert_eq!(bucket(437), 480);
        assert_eq!(bucket(5000), 1280);
    }

    #[test]
    fn resizes_opaque_art_to_jpeg_keeping_the_aspect_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let source = opaque(dir.path(), "box.png", 1200, 900);
        let thumbs = Thumbs::new(dir.path().join("cache"), DEFAULT_MAX_BYTES);

        let Cached::Thumbnail(file) = thumbs.generate(&source, 480).unwrap() else {
            panic!("expected a thumbnail")
        };
        assert_eq!(file.extension().unwrap(), "jpg");
        assert_eq!(dims(&file), (480, 360));
        assert_eq!(thumbs.cached(&source, 480), Some(Cached::Thumbnail(file)));
    }

    #[test]
    fn keeps_transparency_as_png() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("logo.png");
        RgbaImage::from_pixel(800, 200, Rgba([255, 255, 255, 0]))
            .save(&source)
            .unwrap();
        let thumbs = Thumbs::new(dir.path().join("cache"), DEFAULT_MAX_BYTES);
        let Cached::Thumbnail(file) = thumbs.generate(&source, 320).unwrap() else {
            panic!()
        };
        assert_eq!(file.extension().unwrap(), "png");
        assert_eq!(dims(&file), (320, 80));
    }

    #[test]
    fn small_or_broken_sources_are_served_as_they_are_and_not_decoded_again() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = Thumbs::new(dir.path().join("cache"), DEFAULT_MAX_BYTES);
        let small = opaque(dir.path(), "small.png", 100, 100);
        assert_eq!(thumbs.generate(&small, 480).unwrap(), Cached::UseOriginal);

        let broken = dir.path().join("broken.png");
        std::fs::write(&broken, b"not an image").unwrap();
        assert_eq!(thumbs.generate(&broken, 480).unwrap(), Cached::UseOriginal);
        assert_eq!(thumbs.cached(&broken, 480), Some(Cached::UseOriginal));
    }

    #[test]
    fn a_changed_source_gets_a_new_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = Thumbs::new(dir.path().join("cache"), DEFAULT_MAX_BYTES);
        let source = opaque(dir.path(), "box.png", 1200, 900);
        thumbs.generate(&source, 480).unwrap();

        // Re-scraped: different size on disk, so a different key.
        std::thread::sleep(Duration::from_millis(20));
        opaque(dir.path(), "box.png", 1200, 600);
        assert_eq!(thumbs.cached(&source, 480), None);
        let Cached::Thumbnail(file) = thumbs.generate(&source, 480).unwrap() else {
            panic!()
        };
        assert_eq!(dims(&file), (480, 240));
    }

    #[test]
    fn evicts_the_oldest_files_beyond_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        for i in 0..10 {
            std::fs::write(cache.join(format!("{i}.jpg")), vec![0u8; 1000]).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        Thumbs::new(cache.clone(), 5000).evict();
        let left: Vec<String> = std::fs::read_dir(&cache)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left.len(), 4, "down to 90% of the cap: {left:?}");
        assert!(left.contains(&"9.jpg".to_owned()), "the newest stay");
    }

    #[test]
    fn the_thumbnailer_serves_the_original_until_the_thumbnail_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let source = opaque(dir.path(), "box.png", 1200, 900);
        let thumbnailer =
            Thumbnailer::spawn(Thumbs::new(dir.path().join("cache"), DEFAULT_MAX_BYTES));

        assert_eq!(thumbnailer.file_for(&source, 400), source, "not made yet");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let file = thumbnailer.file_for(&source, 400);
            if file != source {
                assert_eq!(dims(&file), (480, 360), "400 snapped to 480");
                break;
            }
            assert!(Instant::now() < deadline, "the worker never made it");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
