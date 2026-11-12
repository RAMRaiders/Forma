//! Magazines: reading scanned retro magazines from `/userdata/magazines`.
//!
//! A magazine is one file (CBZ/ZIP, CBR/RAR, CB7/7Z, CBT/TAR or PDF) or a folder of page
//! images; sub-folders group them (`Edge/1995/…`). Pages are served as JPEGs at the width the
//! screen asks for, made once and cached: a 600 dpi scan is far too heavy for a Raspberry Pi to
//! draw as it is. ZIPs are read in-process; the other archives go through Batocera's `bsdtar`
//! (libarchive) and PDFs through `pdftoppm` (poppler).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::SystemTime;

use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::paths::Paths;

/// Folders below the magazines folder that are looked into.
const MAX_DEPTH: usize = 6;
/// Page widths are rounded up to one of these, so the cache holds a few sizes, not hundreds.
const WIDTHS: [u32; 5] = [320, 640, 1280, 1920, 2560];
const JPEG_QUALITY: u8 = 85;
/// The cache of rendered pages is trimmed back below this.
pub const CACHE_BYTES: u64 = 768 * 1024 * 1024;
/// One page, unpacked, may not be larger than this.
const MAX_PAGE_BYTES: u64 = 128 * 1024 * 1024;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Zip,
    Rar,
    SevenZip,
    Tar,
    Pdf,
    Folder,
    /// One picture: a page on its own (a manual scraped as an image).
    Image,
}

impl Format {
    fn of(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "cbz" | "zip" => Self::Zip,
            "cbr" | "rar" => Self::Rar,
            "cb7" | "7z" => Self::SevenZip,
            "cbt" | "tar" => Self::Tar,
            "pdf" => Self::Pdf,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Magazine {
    /// Its path below the magazines folder, with `/`: stable while the file stays put.
    pub id: String,
    /// From the file name: `Edge_012_1994-09` becomes `Edge 012 1994-09`.
    pub title: String,
    /// The folders it is in, `/`-separated; empty at the top.
    pub folder: String,
    pub format: Format,
}

pub fn magazines_dir(paths: &Paths) -> PathBuf {
    paths.userdata.join("magazines")
}

/// Every magazine, by folder then title, in natural order (issue 9 before issue 10).
pub fn scan(paths: &Paths) -> Vec<Magazine> {
    let root = magazines_dir(paths);
    let mut out = Vec::new();
    walk(&root, &root, 0, &mut out);
    out.sort_by(|a, b| natural(&a.folder, &b.folder).then(natural(&a.title, &b.title)));
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Magazine>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut images = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &path, depth + 1, out);
            }
        } else if let Some(format) = Format::of(&path) {
            out.push(magazine(root, &path, format));
        } else if is_image(&name.to_string_lossy()) {
            images = true;
        }
    }
    // A folder of page images is one magazine; the top folder's loose pictures are not.
    if images && depth > 0 {
        out.push(magazine(root, dir, Format::Folder));
    }
}

fn magazine(root: &Path, path: &Path, format: Format) -> Magazine {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let id = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let folder = rel
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let stem = if format == Format::Folder {
        rel.file_name()
    } else {
        rel.file_stem()
    };
    let title = stem
        .map(|s| s.to_string_lossy().replace('_', " "))
        .unwrap_or_else(|| id.clone());
    Magazine {
        id,
        title: title.split_whitespace().collect::<Vec<_>>().join(" "),
        folder,
        format,
    }
}

fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Compares with runs of digits as numbers, ignoring case: `Issue 9` < `Issue 10`.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(&pb) {
        let ord = match (x, y) {
            ((true, n), (true, m)) => {
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                n.len().cmp(&m.len()).then_with(|| n.cmp(m))
            }
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if ord.is_ne() {
            return ord;
        }
    }
    pa.len().cmp(&pb.len()).then_with(|| a.cmp(b))
}

/// Runs a host program and returns what it wrote. Faked in tests.
pub trait Tools: Send + Sync {
    fn output(&self, program: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>>;
}

/// Batocera's own `bsdtar` and `pdftoppm`, with the host's environment (see [`crate::host`]).
pub struct HostTools;

impl Tools for HostTools {
    fn output(&self, program: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>> {
        let out = crate::host::command(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Error::Unsupported(format!("could not run `{program}`: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr.lines().next().unwrap_or("no message").to_owned();
            return Err(Error::Invalid(format!("`{program}` failed: {why}")));
        }
        Ok(out.stdout)
    }
}

/// Opens magazines and makes their pages.
pub struct Reader {
    root: PathBuf,
    cache: PathBuf,
    tools: Box<dyn Tools>,
    /// Page lists already read, by file and its modification time.
    pages: Mutex<HashMap<PathBuf, (SystemTime, Vec<String>)>>,
    writes: Mutex<u32>,
}

impl Reader {
    pub fn new(paths: &Paths, tools: Box<dyn Tools>) -> Self {
        Self {
            root: magazines_dir(paths),
            cache: paths.cache_dir().join("magazines"),
            tools,
            pages: Mutex::new(HashMap::new()),
            writes: Mutex::new(0),
        }
    }

    /// The file or folder for an id; `None` if it leaves the magazines folder or is not one.
    fn locate(&self, id: &str) -> Result<(PathBuf, Format)> {
        let missing = || Error::Invalid(format!("no magazine `{id}`"));
        let rel = Path::new(id);
        if id.is_empty()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(missing());
        }
        let path = self.root.join(rel);
        let format = if path.is_dir() {
            Format::Folder
        } else if path.is_file() {
            Format::of(&path).ok_or_else(missing)?
        } else {
            return Err(missing());
        };
        Ok((path, format))
    }

    /// How many pages the magazine has. Read once per version of the file.
    pub fn page_count(&self, id: &str) -> Result<usize> {
        Ok(self.page_names(id)?.1.len())
    }

    fn page_names(&self, id: &str) -> Result<(PathBuf, Vec<String>, Format)> {
        let (path, format) = self.locate(id)?;
        self.names_at(path, format, id)
    }

    /// Any document's file, not only the magazines folder's: a game's manual (PDF, an archive
    /// of pages, or one picture). How many pages it has.
    pub fn file_page_count(&self, path: &Path) -> Result<usize> {
        let format = file_format(path)?;
        Ok(self
            .names_at(path.to_path_buf(), format, &path.display().to_string())?
            .1
            .len())
    }

    /// Page `index` (from 0) of any document's file, as [`Reader::page`] makes it.
    pub fn file_page(&self, path: &Path, index: usize, width: u32) -> Result<PathBuf> {
        let format = file_format(path)?;
        let label = path.display().to_string();
        let (path, names, format) = self.names_at(path.to_path_buf(), format, &label)?;
        self.render(&path, &names, format, &label, index, width)
    }

    fn names_at(
        &self,
        path: PathBuf,
        format: Format,
        id: &str,
    ) -> Result<(PathBuf, Vec<String>, Format)> {
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map_err(|e| Error::io(&path, e))?;
        if let Some((at, names)) = self.lock_pages().get(&path)
            && *at == modified
        {
            return Ok((path, names.clone(), format));
        }
        let mut names = match format {
            Format::Pdf => {
                let count = pdf_pages(&self.tools.output("pdfinfo", &[path.as_os_str()])?)
                    .ok_or_else(|| {
                        Error::Invalid(format!("`{id}` has no pages pdfinfo can read"))
                    })?;
                return self.remember(
                    path,
                    modified,
                    (1..=count).map(|n| n.to_string()).collect(),
                    format,
                );
            }
            Format::Image => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return self.remember(path, modified, vec![name], format);
            }
            Format::Zip => zip_names(&path)?,
            Format::Folder => std::fs::read_dir(&path)
                .map_err(|e| Error::io(&path, e))?
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Format::Rar | Format::SevenZip | Format::Tar => {
                let listing = self
                    .tools
                    .output("bsdtar", &["-tf".as_ref(), path.as_os_str()])?;
                String::from_utf8_lossy(&listing)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }
        };
        names.retain(|n| !n.ends_with('/') && is_image(n) && !hidden(n));
        names.sort_by(|a, b| natural(a, b));
        if names.is_empty() {
            return Err(Error::Invalid(format!("`{id}` has no pages")));
        }
        self.remember(path, modified, names, format)
    }

    fn remember(
        &self,
        path: PathBuf,
        modified: SystemTime,
        names: Vec<String>,
        format: Format,
    ) -> Result<(PathBuf, Vec<String>, Format)> {
        self.lock_pages()
            .insert(path.clone(), (modified, names.clone()));
        Ok((path, names, format))
    }

    fn lock_pages(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (SystemTime, Vec<String>)>> {
        self.pages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Page `index` (from 0) as a JPEG about `width` pixels wide, made if not cached yet. Slow
    /// the first time: call it off the UI thread.
    pub fn page(&self, id: &str, index: usize, width: u32) -> Result<PathBuf> {
        let width = WIDTHS
            .iter()
            .copied()
            .find(|w| *w >= width)
            .unwrap_or(WIDTHS[WIDTHS.len() - 1]);
        let (path, names, format) = self.page_names(id)?;
        self.render(&path, &names, format, id, index, width)
    }

    fn render(
        &self,
        path: &Path,
        names: &[String],
        format: Format,
        id: &str,
        index: usize,
        width: u32,
    ) -> Result<PathBuf> {
        let width = WIDTHS
            .iter()
            .copied()
            .find(|w| *w >= width)
            .unwrap_or(WIDTHS[WIDTHS.len() - 1]);
        let name = names
            .get(index)
            .ok_or_else(|| Error::Invalid(format!("`{id}` has {} pages", names.len())))?;
        let file = self
            .cache
            .join(format!("{}-{index}-{width}.jpg", key(path)));
        if file.is_file() {
            return Ok(file);
        }

        let bytes = match format {
            Format::Pdf => {
                // Rendered at the width asked for: no scan to shrink.
                let page = (index + 1).to_string();
                let w = width.to_string();
                return self.store(
                    &file,
                    &self.tools.output(
                        "pdftoppm",
                        &[
                            "-f".as_ref(),
                            page.as_ref(),
                            "-l".as_ref(),
                            page.as_ref(),
                            "-scale-to-x".as_ref(),
                            w.as_ref(),
                            "-scale-to-y".as_ref(),
                            "-1".as_ref(),
                            "-jpeg".as_ref(),
                            "-jpegopt".as_ref(),
                            "quality=85".as_ref(),
                            "-singlefile".as_ref(),
                            path.as_os_str(),
                        ],
                    )?,
                );
            }
            Format::Image => std::fs::read(path).map_err(|e| Error::io(path, e))?,
            Format::Zip => zip_entry(path, name)?,
            Format::Folder => {
                let page = path.join(name);
                std::fs::read(&page).map_err(|e| Error::io(&page, e))?
            }
            Format::Rar | Format::SevenZip | Format::Tar => self.tools.output(
                "bsdtar",
                &[
                    "-xOf".as_ref(),
                    path.as_os_str(),
                    "--".as_ref(),
                    name.as_ref(),
                ],
            )?,
        };
        let image = ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| Error::io(path, e))?
            .decode()
            .map_err(|e| Error::Invalid(format!("page {} of `{id}`: {e}", index + 1)))?;
        let image = if image.width() > width {
            image.resize(width, u32::MAX, FilterType::Triangle)
        } else {
            image
        };
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
            .encode_image(&image.to_rgb8())
            .map_err(|e| Error::Invalid(format!("page {} of `{id}`: {e}", index + 1)))?;
        self.store(&file, &jpeg)
    }

    /// Writes a page through a temporary file, and now and then trims the cache.
    fn store(&self, file: &Path, bytes: &[u8]) -> Result<PathBuf> {
        if bytes.is_empty() {
            return Err(Error::Invalid("the page came out empty".into()));
        }
        std::fs::create_dir_all(&self.cache).map_err(|e| Error::io(&self.cache, e))?;
        let temp = file.with_extension("tmp");
        std::fs::write(&temp, bytes).map_err(|e| Error::io(&temp, e))?;
        std::fs::rename(&temp, file).map_err(|e| Error::io(file, e))?;
        let mut writes = self
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *writes += 1;
        if (*writes).is_multiple_of(32) {
            crate::thumbs::Thumbs::new(self.cache.clone(), CACHE_BYTES).evict();
        }
        Ok(file.to_path_buf())
    }
}

/// A key for a magazine's version: its path, size and modification time.
fn key(path: &Path) -> String {
    let meta = std::fs::metadata(path).ok();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    meta.as_ref().map(std::fs::Metadata::len).hash(&mut h);
    meta.and_then(|m| m.modified().ok()).hash(&mut h);
    format!("{:016x}", h.finish())
}

fn hidden(name: &str) -> bool {
    name.split('/')
        .any(|p| p.starts_with('.') || p == "__MACOSX")
}

fn pdf_pages(info: &[u8]) -> Option<usize> {
    String::from_utf8_lossy(info)
        .lines()
        .find_map(|l| l.strip_prefix("Pages:"))
        .and_then(|n| n.trim().parse().ok())
        .filter(|n| *n > 0)
}

fn zip_names(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let bad = |e: zip::result::ZipError| Error::Invalid(format!("{}: {e}", path.display()));
    let mut archive = zip::ZipArchive::new(file).map_err(bad)?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(bad)?;
        if entry.is_file() {
            names.push(entry.name().map_err(bad)?.into_owned());
        }
    }
    Ok(names)
}

fn zip_entry(path: &Path, name: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let bad = |e: zip::result::ZipError| Error::Invalid(format!("{}: {e}", path.display()));
    let mut archive = zip::ZipArchive::new(file).map_err(bad)?;
    let entry = archive.by_name(name).map_err(bad)?;
    let mut bytes = Vec::new();
    entry
        .take(MAX_PAGE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io(path, e))?;
    Ok(bytes)
}

/// The format of a document's file by its extension: an archive, a PDF or a picture.
fn file_format(path: &Path) -> Result<Format> {
    if !path.is_file() {
        return Err(Error::Invalid(format!("no file `{}`", path.display())));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    Format::of(path)
        .or_else(|| is_image(&name).then_some(Format::Image))
        .ok_or_else(|| Error::Invalid(format!("`{}` is no document", path.display())))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use super::*;
    use crate::test_support::fixture_copy;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn cbz(path: &Path, pages: &[&str]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for name in pages {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(&png(1000, 1400)).unwrap();
        }
        z.start_file("ComicInfo.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.finish().unwrap();
    }

    /// Answers `bsdtar` and `pdf*` like Batocera's would, and records the calls.
    #[derive(Default)]
    struct FakeTools(Arc<Mutex<Vec<String>>>);
    impl Tools for FakeTools {
        fn output(&self, program: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>> {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            self.0
                .lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            Ok(match (program, args.first().map(String::as_str)) {
                ("bsdtar", Some("-tf")) => {
                    b"p10.jpg\np2.jpg\n__MACOSX/._p2.jpg\nnotes.txt\n".to_vec()
                }
                ("bsdtar", Some("-xOf")) => png(800, 1000),
                ("pdfinfo", _) => b"Title: x\nPages:          12\n".to_vec(),
                ("pdftoppm", _) => vec![0xFF, 0xD8, 0xFF, 0xD9],
                _ => return Err(Error::Invalid(format!("unexpected {program}"))),
            })
        }
    }

    #[test]
    fn finds_files_and_image_folders_grouped_by_folder_in_natural_order() {
        let (_dir, paths) = fixture_copy();
        let root = magazines_dir(&paths);
        cbz(&root.join("Edge/Edge_10.cbz"), &["01.png"]);
        cbz(&root.join("Edge/Edge_9.cbz"), &["01.png"]);
        std::fs::write(root.join("Edge/notes.txt"), "").unwrap();
        std::fs::write(root.join("Retro Gamer 1.pdf"), "").unwrap();
        std::fs::write(root.join("cover.jpg"), "").unwrap();
        std::fs::create_dir_all(root.join("Scans/Mean Machines 1")).unwrap();
        std::fs::write(root.join("Scans/Mean Machines 1/001.jpg"), "").unwrap();
        std::fs::write(root.join("Edge/.hidden.cbr"), "").unwrap();

        let found: Vec<(String, String, Format)> = scan(&paths)
            .into_iter()
            .map(|m| (m.folder, m.title, m.format))
            .collect();
        assert_eq!(
            found,
            [
                (String::new(), "Retro Gamer 1".into(), Format::Pdf),
                ("Edge".into(), "Edge 9".into(), Format::Zip),
                ("Edge".into(), "Edge 10".into(), Format::Zip),
                ("Scans".into(), "Mean Machines 1".into(), Format::Folder),
            ]
        );
        assert_eq!(scan(&paths)[3].id, "Scans/Mean Machines 1");
    }

    #[test]
    fn a_cbz_gives_its_images_in_order_resized_and_cached() {
        let (_dir, paths) = fixture_copy();
        let root = magazines_dir(&paths);
        cbz(
            &root.join("a.cbz"),
            &["p10.png", "p2.png", "p1.png", "__MACOSX/._p1.png"],
        );
        let reader = Reader::new(&paths, Box::new(FakeTools::default()));
        assert_eq!(reader.page_count("a.cbz").unwrap(), 3);
        let first = reader.page("a.cbz", 0, 600).unwrap();
        let img = image::open(&first).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (640, 896),
            "to the 640 bucket, same shape"
        );
        // Cached: the same file again.
        assert_eq!(reader.page("a.cbz", 0, 640).unwrap(), first);
        assert!(reader.page("a.cbz", 3, 600).is_err());
        assert!(reader.page("../a.cbz", 0, 600).is_err());
        assert!(reader.page_count("missing.cbz").is_err());
    }

    #[test]
    fn other_archives_go_through_bsdtar_and_pdfs_through_poppler() {
        let (_dir, paths) = fixture_copy();
        let root = magazines_dir(&paths);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("b.cbr"), "rar").unwrap();
        std::fs::write(root.join("c.pdf"), "pdf").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let reader = Reader::new(&paths, Box::new(FakeTools(calls.clone())));

        assert_eq!(reader.page_count("b.cbr").unwrap(), 2);
        reader.page("b.cbr", 1, 2000).unwrap();
        let log = calls.lock().unwrap().clone();
        assert!(
            log[1].starts_with("bsdtar -xOf") && log[1].ends_with("-- p10.jpg"),
            "{log:?}"
        );

        assert_eq!(reader.page_count("c.pdf").unwrap(), 12);
        reader.page("c.pdf", 4, 1000).unwrap();
        let log = calls.lock().unwrap().clone();
        assert!(
            log.last()
                .unwrap()
                .starts_with("pdftoppm -f 5 -l 5 -scale-to-x 1280"),
            "{log:?}"
        );
        // Listed once while the file is unchanged.
        reader.page_count("c.pdf").unwrap();
        assert_eq!(log.iter().filter(|c| c.starts_with("pdfinfo")).count(), 1);
    }

    #[test]
    fn a_manual_anywhere_is_read_as_a_magazine_is_and_a_picture_is_one_page() {
        let (dir, paths) = fixture_copy();
        let manuals = dir.path().join("roms/nes/manuals");
        std::fs::create_dir_all(&manuals).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let reader = Reader::new(&paths, Box::new(FakeTools(calls.clone())));

        let pdf = manuals.join("Mario.pdf");
        std::fs::write(&pdf, "pdf").unwrap();
        assert_eq!(reader.file_page_count(&pdf).unwrap(), 12);
        reader.file_page(&pdf, 0, 640).unwrap();
        assert!(
            calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .starts_with("pdftoppm -f 1 -l 1")
        );

        let picture = manuals.join("Zelda.png");
        std::fs::write(&picture, png(1000, 1400)).unwrap();
        assert_eq!(reader.file_page_count(&picture).unwrap(), 1);
        let page = image::open(reader.file_page(&picture, 0, 600).unwrap()).unwrap();
        assert_eq!(page.width(), 640);
        assert!(reader.file_page(&picture, 1, 600).is_err());

        let text = manuals.join("notes.txt");
        std::fs::write(&text, "no").unwrap();
        assert!(reader.file_page_count(&text).is_err());
        assert!(reader.file_page_count(&manuals.join("gone.pdf")).is_err());
    }

    #[test]
    fn natural_order_counts_numbers() {
        let mut v = vec!["Issue 10", "issue 9", "Issue 009b", "Issue 1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["Issue 1", "issue 9", "Issue 009b", "Issue 10"]);
    }
}
