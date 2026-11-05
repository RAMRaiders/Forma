//! The `forma://` protocol: theme files and library media, and nothing else.
//!
//! - `forma://localhost/theme/<id>/<file>` serves a file from inside that theme's folder.
//!   `..`, absolute paths and symlinks leading out of the folder are refused.
//! - `forma://localhost/media/<game id>/<kind>` serves a game's image, video, marquee… looked up
//!   by id, so a theme never names (or learns) a path on disk. `?w=480` asks for a resized copy
//!   for images; the original is served until the copy has been made.
//! - `forma://localhost/ra-media/<path>` serves RetroAchievements badges, icons and avatars,
//!   fetched once and cached, so showing achievements never needs the `network` permission.
//! - `forma://localhost/system/forma-logo.svg` is Forma's logo, compiled in; next to it,
//!   `boot-logo.png`, `boot-logo-4x3.png` (Batocera's boot image) and `batocera-logo.svg`
//!   (Batocera's logo) are Batocera's own files, and nothing else. Forma's cover shows them.
//! - `forma://localhost/shell/shell.js` is Forma's shell, which every theme page gets first in its
//!   `<head>`: Forma's menus over the theme (see `shell.rs`).
//!
//! HTML responses carry the Content-Security-Policy that keeps a theme to its own folder and
//! the origins its manifest declares. Media supports byte ranges, which video seeking needs.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use tauri::http::{Request, Response, StatusCode, header};

/// What the protocol needs from the app, so it can be tested without one.
pub trait Resolver {
    /// The folder of an available theme, and the remote origins it may use.
    fn theme(&self, id: &str) -> Option<(PathBuf, Vec<String>)>;
    /// The file for one kind of a game's media, at about `width` px if asked and it is an image.
    fn media(&self, game_id: i64, kind: &str, width: Option<u32>) -> Option<PathBuf>;
    /// A RetroAchievements media file, fetched and cached if need be.
    fn ra_media(&self, path: &str) -> Option<PathBuf>;
    /// One of Batocera's files for Forma's cover: `boot-logo.png`, `boot-logo-4x3.png`,
    /// `batocera-logo.svg`.
    fn splash_file(&self, name: &str) -> Option<PathBuf>;
    /// What Forma's cover shows on the theme page being served.
    fn cover(&self) -> crate::shell::CoverInfo;
    /// A file of Forma's shell (`shell.js`), the menus drawn over every theme.
    fn shell_file(&self, name: &str) -> Option<PathBuf>;
    /// Page `page` (from 0) of a magazine, as a JPEG about `width` wide (see `magazines`).
    fn magazine_page(&self, id: &str, page: usize, width: u32) -> Option<PathBuf> {
        let _ = (id, page, width);
        None
    }
}

/// A magazine's page URL, with `{page}` for the page number (from 0): `forma.magazines.pageUrl`
/// fills it in and asks for a width.
pub fn magazine_url(id: &str) -> String {
    let path: Vec<String> = id.split('/').map(percent_encode).collect();
    format!("{ORIGIN}/magazine/{{page}}/{}", path.join("/"))
}

fn percent_encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Forma's logo, for its cover.
const LOGO: &str = include_str!("../boot/forma-logo.svg");

pub const ORIGIN: &str = "forma://localhost";

pub fn theme_url(id: &str, entry: &str) -> String {
    format!("{ORIGIN}/theme/{id}/{entry}")
}

/// The widest resized copy a theme can ask for; anything wider gets the original.
const MAX_WIDTH: u32 = 4096;

pub fn media_url(game_id: i64, kind: &str) -> String {
    format!("{ORIGIN}/media/{game_id}/{kind}")
}

/// What RetroAchievements media paths are appended to.
pub const RA_MEDIA_PREFIX: &str = "forma://localhost/ra-media/";

pub fn respond(resolver: &impl Resolver, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = percent_decode(request.uri().path());
    let mut parts = path.trim_start_matches('/').splitn(3, '/');
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());

    if let Some(rest) = path.strip_prefix("/ra-media/") {
        return match resolver.ra_media(rest) {
            Some(file) => serve(&file, range, None),
            None => status(StatusCode::NOT_FOUND, "no such media"),
        };
    }
    match (parts.next(), parts.next(), parts.next()) {
        (Some("theme"), Some(id), rest) => {
            let Some((dir, remote)) = resolver.theme(id) else {
                return status(StatusCode::NOT_FOUND, "no such theme");
            };
            let rel = rest.filter(|r| !r.is_empty()).unwrap_or("index.html");
            match inside(&dir, rel) {
                Some(file) if mime_type(&file).starts_with("text/html") => {
                    serve_page(&file, &csp(&remote), &resolver.cover())
                }
                Some(file) => serve(&file, None, None),
                None => status(StatusCode::NOT_FOUND, "not found"),
            }
        }
        (Some("media"), Some(id), Some(kind)) => {
            let width = request
                .uri()
                .query()
                .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("w=")))
                .and_then(|w| w.parse::<u32>().ok())
                .filter(|w| (1..=MAX_WIDTH).contains(w));
            match id
                .parse()
                .ok()
                .and_then(|id| resolver.media(id, kind, width))
            {
                Some(file) => serve(&file, range, None),
                None => status(StatusCode::NOT_FOUND, "no such media"),
            }
        }
        (Some("magazine"), Some(page), Some(id)) => {
            let width = request
                .uri()
                .query()
                .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("w=")))
                .and_then(|w| w.parse::<u32>().ok())
                .filter(|w| (1..=MAX_WIDTH).contains(w))
                .unwrap_or(1280);
            match page
                .parse()
                .ok()
                .and_then(|page| resolver.magazine_page(id, page, width))
            {
                Some(file) => serve(&file, None, None),
                None => status(StatusCode::NOT_FOUND, "no such page"),
            }
        }
        (Some("system"), Some("forma-logo.svg"), None) => Response::builder()
            .header(header::CONTENT_TYPE, "image/svg+xml")
            .header("X-Content-Type-Options", "nosniff")
            .body(LOGO.as_bytes().to_vec())
            .unwrap_or_default(),
        (Some("shell"), Some(name), None) => match resolver.shell_file(name) {
            Some(file) => serve(&file, None, None),
            None => status(StatusCode::NOT_FOUND, "no such shell file"),
        },
        (
            Some("system"),
            Some(name @ ("boot-logo.png" | "boot-logo-4x3.png" | "batocera-logo.svg")),
            None,
        ) => match resolver.splash_file(name) {
            Some(file) => serve(&file, None, None),
            None => status(StatusCode::NOT_FOUND, "no splash"),
        },
        _ => status(StatusCode::NOT_FOUND, "not found"),
    }
}

/// A theme's page, with Forma's shell brought in first, under the theme's policy.
fn serve_page(file: &Path, csp: &str, cover: &crate::shell::CoverInfo) -> Response<Vec<u8>> {
    let Ok(html) = std::fs::read_to_string(file) else {
        return status(StatusCode::INTERNAL_SERVER_ERROR, "unreadable");
    };
    let body = crate::shell::inject(&html, cover).into_bytes();
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONTENT_SECURITY_POLICY, csp)
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CONTENT_LENGTH, body.len())
        .body(body)
        .unwrap_or_default()
}

/// `rel` resolved inside `dir`, or `None` if it would leave it (including through a symlink).
pub fn inside_dir(dir: &Path, rel: &str) -> Option<PathBuf> {
    inside(dir, rel)
}

fn inside(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let file = root.join(rel).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

/// The theme's sandbox: its own folder (`forma:`), Tauri's IPC, and declared origins only.
fn csp(remote: &[String]) -> String {
    let extra = if remote.is_empty() {
        String::new()
    } else {
        format!(" {}", remote.join(" "))
    };
    format!(
        "default-src 'self' forma:{extra}; \
         script-src 'self' forma:{extra}; \
         style-src 'self' 'unsafe-inline' forma:{extra}; \
         img-src 'self' forma: data: blob:{extra}; \
         media-src 'self' forma: blob:{extra}; \
         font-src 'self' forma: data:{extra}; \
         connect-src ipc: http://ipc.localhost forma:{extra}; \
         object-src 'none'; base-uri 'self'"
    )
}

fn serve(file: &Path, range: Option<&str>, csp: Option<&str>) -> Response<Vec<u8>> {
    let Ok(mut f) = File::open(file) else {
        return status(StatusCode::NOT_FOUND, "not found");
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return status(StatusCode::INTERNAL_SERVER_ERROR, "unreadable");
    };
    let mime = mime_type(file);
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        // Revalidate every time: the same media URL serves the original until its resized copy
        // exists, then the copy. Reading a local file again costs nothing worth caching.
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCEPT_RANGES, "bytes")
        .header("X-Content-Type-Options", "nosniff");
    if let Some(csp) = csp.filter(|_| mime.starts_with("text/html")) {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, csp);
    }

    let (start, end) = match range.map(|r| parse_range(r, len)) {
        None => (0, len.saturating_sub(1)),
        Some(Some(r)) => r,
        Some(None) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap_or_default();
        }
    };
    let mut body = Vec::new();
    if len > 0 {
        let wanted = end - start + 1;
        if f.seek(SeekFrom::Start(start)).is_err() || f.take(wanted).read_to_end(&mut body).is_err()
        {
            return status(StatusCode::INTERNAL_SERVER_ERROR, "unreadable");
        }
    }
    if range.is_some() {
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    builder
        .header(header::CONTENT_LENGTH, body.len())
        .body(body)
        .unwrap_or_default()
}

/// A single `bytes=start-end`, `bytes=start-` or `bytes=-suffix` range, clamped to the file.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let last = len.checked_sub(1)?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), last)
        }
        (s, "") => (s.parse().ok()?, last),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(last)),
    };
    (start <= end && start <= last).then_some((start, end))
}

fn status(code: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap_or_default()
}

fn mime_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ogg" | "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// `%20` → space. Invalid sequences are kept as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        theme: PathBuf,
        media: PathBuf,
        asked_width: std::sync::Mutex<Option<u32>>,
    }

    impl Resolver for Fake {
        fn theme(&self, id: &str) -> Option<(PathBuf, Vec<String>)> {
            (id == "neon").then(|| (self.theme.clone(), vec!["https://cdn.example.com".into()]))
        }
        fn media(&self, game_id: i64, kind: &str, width: Option<u32>) -> Option<PathBuf> {
            *self.asked_width.lock().unwrap() = width;
            (game_id == 7 && kind == "video").then(|| self.media.clone())
        }
        fn ra_media(&self, path: &str) -> Option<PathBuf> {
            (path == "Badge/1.png").then(|| self.media.clone())
        }
        fn splash_file(&self, name: &str) -> Option<PathBuf> {
            (name == "boot-logo.png").then(|| self.media.clone())
        }
        fn cover(&self) -> crate::shell::CoverInfo {
            crate::shell::CoverInfo {
                theme: "Neon 1".into(),
                ..Default::default()
            }
        }
        fn shell_file(&self, name: &str) -> Option<PathBuf> {
            (name == "shell.js").then(|| self.media.clone())
        }
    }

    fn setup() -> (tempfile::TempDir, Fake) {
        let dir = tempfile::tempdir().unwrap();
        let theme = dir.path().join("neon");
        std::fs::create_dir_all(theme.join("assets")).unwrap();
        std::fs::write(theme.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(theme.join("assets/app 1.js"), "console.log(1)").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();
        let media = dir.path().join("clip.mp4");
        std::fs::write(&media, b"0123456789").unwrap();
        (
            dir,
            Fake {
                theme,
                media,
                asked_width: std::sync::Mutex::new(None),
            },
        )
    }

    fn get(fake: &Fake, uri: &str, range: Option<&str>) -> Response<Vec<u8>> {
        let mut req = Request::builder().uri(uri);
        if let Some(r) = range {
            req = req.header(header::RANGE, r);
        }
        respond(fake, &req.body(Vec::new()).unwrap())
    }

    #[test]
    fn serves_formas_logo_and_only_batoceras_own_files() {
        let (_dir, fake) = setup();
        let logo = get(&fake, "forma://localhost/system/forma-logo.svg", None);
        assert_eq!(logo.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert!(String::from_utf8_lossy(logo.body()).contains("aria-label=\"Forma\""));
        let boot = get(&fake, "forma://localhost/system/boot-logo.png", None);
        assert_eq!(boot.status(), StatusCode::OK);
        for uri in [
            "forma://localhost/system/splash.html",
            "forma://localhost/system/splash.mp4",
            "forma://localhost/system/batocera-logo.svg",
            "forma://localhost/system/secret.txt",
            "forma://localhost/system/../secret.txt",
            "forma://localhost/system/boot-logo.png/x",
        ] {
            assert_eq!(
                get(&fake, uri, None).status(),
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    #[test]
    fn serves_theme_files_with_type_and_a_csp_on_html() {
        let (_dir, fake) = setup();
        let page = get(&fake, "forma://localhost/theme/neon/", None);
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(
            String::from_utf8_lossy(page.body()),
            format!(
                "{}<!doctype html>",
                crate::shell::shell_tag(&crate::shell::CoverInfo {
                    theme: "Neon 1".into(),
                    ..Default::default()
                })
            ),
            "Forma's shell comes first"
        );
        assert_eq!(
            page.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let csp = page.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(
            csp.contains("script-src 'self' forma: https://cdn.example.com;"),
            "{csp}"
        );
        assert!(
            csp.contains("connect-src ipc: http://ipc.localhost"),
            "{csp}"
        );

        let js = get(
            &fake,
            "forma://localhost/theme/neon/assets/app%201.js",
            None,
        );
        assert_eq!(js.status(), StatusCode::OK);
        assert_eq!(js.body(), b"console.log(1)");
        assert!(js.headers().get(header::CONTENT_SECURITY_POLICY).is_none());
    }

    #[test]
    fn serves_the_shell() {
        let (_dir, fake) = setup();
        let shell = get(&fake, "forma://localhost/shell/shell.js", None);
        assert_eq!(shell.status(), StatusCode::OK);
        for uri in [
            "forma://localhost/shell/other.js",
            "forma://localhost/shell/../theme/neon/index.html",
        ] {
            assert_eq!(
                get(&fake, uri, None).status(),
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    #[test]
    fn refuses_to_leave_the_theme_folder() {
        let (_dir, fake) = setup();
        for uri in [
            "forma://localhost/theme/neon/../secret.txt",
            "forma://localhost/theme/neon/%2e%2e/secret.txt",
            "forma://localhost/theme/neon//etc/passwd",
            "forma://localhost/theme/other/index.html",
            "forma://localhost/elsewhere",
        ] {
            assert_eq!(
                get(&fake, uri, None).status(),
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_out_of_the_theme_folder() {
        let (dir, fake) = setup();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), fake.theme.join("link.txt"))
            .unwrap();
        let res = get(&fake, "forma://localhost/theme/neon/link.txt", None);
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn serves_media_by_game_id_with_ranges() {
        let (_dir, fake) = setup();
        let whole = get(&fake, "forma://localhost/media/7/video", None);
        assert_eq!(whole.status(), StatusCode::OK);
        assert_eq!(whole.headers()[header::CONTENT_TYPE], "video/mp4");
        assert_eq!(whole.body(), b"0123456789");

        let part = get(&fake, "forma://localhost/media/7/video", Some("bytes=2-5"));
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(part.body(), b"2345");

        let tail = get(&fake, "forma://localhost/media/7/video", Some("bytes=-3"));
        assert_eq!(tail.body(), b"789");
        let open = get(&fake, "forma://localhost/media/7/video", Some("bytes=8-"));
        assert_eq!(open.body(), b"89");
        let bad = get(
            &fake,
            "forma://localhost/media/7/video",
            Some("bytes=20-30"),
        );
        assert_eq!(bad.status(), StatusCode::RANGE_NOT_SATISFIABLE);

        for uri in [
            "forma://localhost/media/8/video",
            "forma://localhost/media/x/video",
        ] {
            assert_eq!(get(&fake, uri, None).status(), StatusCode::NOT_FOUND);
        }
        let badge = get(&fake, "forma://localhost/ra-media/Badge/1.png", None);
        assert_eq!(badge.status(), StatusCode::OK);
        let other = get(&fake, "forma://localhost/ra-media/Badge/2.png", None);
        assert_eq!(other.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn passes_a_sane_width_through_and_ignores_the_rest() {
        let (_dir, fake) = setup();
        let width = |uri: &str| {
            get(&fake, uri, None);
            *fake.asked_width.lock().unwrap()
        };
        assert_eq!(width("forma://localhost/media/7/video?w=480"), Some(480));
        assert_eq!(
            width("forma://localhost/media/7/video?x=1&w=320"),
            Some(320)
        );
        assert_eq!(width("forma://localhost/media/7/video?w=0"), None);
        assert_eq!(width("forma://localhost/media/7/video?w=99999"), None);
        assert_eq!(width("forma://localhost/media/7/video?w=big"), None);
        assert_eq!(width("forma://localhost/media/7/video"), None);
        let res = get(&fake, "forma://localhost/media/7/video", None);
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[test]
    fn decodes_percent_escapes_and_keeps_bad_ones() {
        assert_eq!(percent_decode("/a%20b%2Fc"), "/a b/c");
        assert_eq!(percent_decode("/100%"), "/100%");
        assert_eq!(percent_decode("/%zz"), "/%zz");
    }
}
