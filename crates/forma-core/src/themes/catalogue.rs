//! The theme catalogue: a JSON index of themes Forma can install and update from its menu.
//!
//! Each entry names a release zip and its SHA-256. Installing downloads it, checks the hash,
//! unpacks it beside the themes folder, and only then checks it is the theme the catalogue
//! promised and that it can load, before moving it into `/userdata/themes-forma/<id>/` in one
//! rename. A bad package never replaces a working theme.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Theme, ThemeOrigin, load, newer, satisfies, user_themes_dir};
use crate::SDK_API_VERSION;
use crate::error::{Error, Result};
use crate::net::{Cache, Fetched, Http, fetch};
use crate::paths::Paths;

/// Where Forma looks for themes unless the setting [`CATALOGUE_URL_SETTING`] says otherwise.
pub const DEFAULT_CATALOGUE_URL: &str =
    "https://raw.githubusercontent.com/RAMRaiders/forma-themes/main/catalogue.json";

/// Forma's setting for another catalogue (an `https://` URL), e.g. one's own for testing.
pub const CATALOGUE_URL_SETTING: &str = "themes.catalogueUrl";

/// How long a fetched catalogue is used before asking again.
const MAX_AGE: Duration = Duration::from_secs(6 * 3600);

/// The largest package Forma downloads.
const MAX_PACKAGE: u64 = 128 * 1024 * 1024;

/// Unpacked, a package may not grow past this (a zip bomb stops here).
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueTheme {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Caret range of the SDK the theme needs, as in its `theme.json`.
    pub sdk: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// `https://` pictures of the theme.
    #[serde(default)]
    pub previews: Vec<String>,
    /// The package: an `https://` zip holding the built theme, `theme.json` at its root or in
    /// one folder.
    pub download: String,
    /// The package's SHA-256, in hex.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub themes: Vec<CatalogueTheme>,
    /// Entries left out, and why: a catalogue with one broken entry still lists the rest.
    pub skipped: Vec<String>,
}

impl Catalogue {
    pub fn find(&self, id: &str) -> Option<&CatalogueTheme> {
        self.themes.iter().find(|t| t.id == id)
    }

    /// The catalogue's version of `theme`, if it is newer than the one installed and this
    /// Forma can run it.
    pub fn update_for(&self, theme: &Theme) -> Option<&CatalogueTheme> {
        let installed = theme.manifest.as_ref()?;
        self.find(&theme.id)
            .filter(|c| c.runs_here() && newer(&c.version, &installed.version))
    }
}

impl CatalogueTheme {
    /// This Forma's SDK satisfies what the theme needs.
    pub fn runs_here(&self) -> bool {
        satisfies(&self.sdk, SDK_API_VERSION)
    }
}

#[derive(Deserialize)]
struct Raw {
    themes: Vec<serde_json::Value>,
}

/// Reads a catalogue. Entries that are not well formed (an id that is not kebab-case, a
/// download that is not `https://`, a hash that is not SHA-256) are skipped with the reason.
pub fn parse(body: &[u8]) -> std::result::Result<Catalogue, String> {
    let raw: Raw = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let mut out = Catalogue::default();
    for value in raw.themes {
        let label = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(no id)")
            .to_owned();
        let entry = match serde_json::from_value::<CatalogueTheme>(value) {
            Ok(entry) => entry,
            Err(e) => {
                out.skipped.push(format!("{label}: {e}"));
                continue;
            }
        };
        match check_entry(&entry) {
            Err(why) => out.skipped.push(format!("{label}: {why}")),
            Ok(()) if out.find(&entry.id).is_some() => {
                out.skipped.push(format!("{label}: listed twice"));
            }
            Ok(()) => out.themes.push(entry),
        }
    }
    Ok(out)
}

fn check_entry(e: &CatalogueTheme) -> std::result::Result<(), String> {
    let kebab = !e.id.is_empty()
        && e.id.split('-').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !kebab {
        return Err("the id must be kebab-case".into());
    }
    if !e.download.starts_with("https://") {
        return Err("the download must be an https:// URL".into());
    }
    if e.sha256.len() != 64 || !e.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("sha256 must be 64 hex digits".into());
    }
    if let Some(p) = e.previews.iter().find(|p| !p.starts_with("https://")) {
        return Err(format!("preview `{p}` is not an https:// URL"));
    }
    Ok(())
}

/// The catalogue, from the cache while it is young, else from `url`; offline, the last one
/// fetched, marked stale.
pub fn fetch_catalogue(http: &dyn Http, cache: &Cache, url: &str) -> Result<Fetched<Catalogue>> {
    if !url.starts_with("https://") {
        return Err(Error::Invalid(format!(
            "the theme catalogue must be an https:// URL, not `{url}`"
        )));
    }
    fetch(http, cache, &cache_key(url), url, MAX_AGE, parse)
}

/// One cache entry per catalogue URL, so changing the setting never shows the old one's themes.
fn cache_key(url: &str) -> String {
    format!(
        "themes-catalogue-{}",
        &hex(&Sha256::digest(url.as_bytes()))[..16]
    )
}

/// The catalogue fetched last, without going to the network: for marking updates in a list
/// that must show at once. `None` before the first fetch.
pub fn cached_catalogue(cache: &Cache, url: &str) -> Option<Catalogue> {
    // No network, and no age too old: the cached copy, whatever its age, if there is one.
    struct Offline;
    impl Http for Offline {
        fn get(&self, _: &str) -> std::result::Result<Vec<u8>, crate::net::HttpError> {
            Err(crate::net::HttpError {
                status: None,
                message: "offline".into(),
            })
        }
    }
    fetch(&Offline, cache, &cache_key(url), url, Duration::MAX, parse)
        .ok()
        .map(|f| f.data)
}

/// Downloads, checks and installs `entry`, replacing an installed copy. Returns the theme as
/// installed. A newer copy of a bundled theme stands in for it, as one copied in by hand does.
pub fn install(http: &dyn Http, paths: &Paths, entry: &CatalogueTheme) -> Result<Theme> {
    if !entry.runs_here() {
        return Err(Error::Invalid(format!(
            "{} {} needs Forma SDK {}; this Forma has {SDK_API_VERSION}",
            entry.name, entry.version, entry.sdk
        )));
    }
    let package = http
        .download(&entry.download, MAX_PACKAGE)
        .map_err(|e| Error::Network(format!("could not download {}: {}", entry.name, e.message)))?;
    let hash = hex(&Sha256::digest(&package));
    if !hash.eq_ignore_ascii_case(&entry.sha256) {
        return Err(Error::Invalid(format!(
            "the download of {} does not match the catalogue (sha256 {hash}); not installed",
            entry.name
        )));
    }

    let root = user_themes_dir(paths);
    std::fs::create_dir_all(&root).map_err(|e| Error::io(&root, e))?;
    let work = root.join(format!(".install-{}", entry.id));
    let _ = std::fs::remove_dir_all(&work);
    let result = unpack_and_check(&package, &work, entry).and_then(|dir| {
        let target = root.join(&entry.id);
        swap_in(&dir, &target)?;
        Ok(load(&target, ThemeOrigin::User))
    });
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn unpack_and_check(package: &[u8], work: &Path, entry: &CatalogueTheme) -> Result<PathBuf> {
    let bad = |why: String| {
        Error::Invalid(format!(
            "{} is not a usable theme package: {why}",
            entry.name
        ))
    };
    unpack(package, work).map_err(bad)?;
    let dir =
        theme_root(work).ok_or_else(|| bad("no theme.json at its root or in one folder".into()))?;
    let theme = load(&dir, ThemeOrigin::User);
    let manifest = theme
        .manifest
        .as_ref()
        .ok_or_else(|| bad(theme.unavailable.clone().unwrap_or_default()))?;
    if manifest.id != entry.id || manifest.version != entry.version {
        return Err(bad(format!(
            "it holds {} {}, not {} {}",
            manifest.id, manifest.version, entry.id, entry.version
        )));
    }
    if let Some(why) = &theme.unavailable {
        return Err(bad(why.clone()));
    }
    Ok(dir)
}

/// Extracts every file, refusing paths that leave the folder, links, and runaway sizes.
fn unpack(package: &[u8], into: &Path) -> std::result::Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().map_err(|e| e.to_string())?.into_owned();
        let Some(relative) = file.enclosed_name() else {
            return Err(format!("`{name}` points outside the package"));
        };
        if file.is_symlink() {
            return Err(format!("`{name}` is a link"));
        }
        let path = into.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let room = MAX_UNPACKED.saturating_sub(total);
        let mut bytes = Vec::new();
        (&mut file)
            .take(room + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("`{name}`: {e}"))?;
        total += bytes.len() as u64;
        if total > MAX_UNPACKED {
            return Err(format!("it unpacks to more than {} MB", MAX_UNPACKED >> 20));
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The folder holding `theme.json`: the package's root, or its single folder.
fn theme_root(work: &Path) -> Option<PathBuf> {
    if work.join("theme.json").is_file() {
        return Some(work.to_path_buf());
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(work)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    match entries.as_slice() {
        [only] if only.join("theme.json").is_file() => Some(only.clone()),
        _ => None,
    }
}

/// Puts `new` at `target`: the old copy moves aside first and goes once the new one is in, so
/// a failure halfway leaves the old theme where it was.
fn swap_in(new: &Path, target: &Path) -> Result<()> {
    let old = target.with_file_name(format!(
        ".old-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("theme")
    ));
    let _ = std::fs::remove_dir_all(&old);
    let had_old = match std::fs::symlink_metadata(target) {
        Ok(_) => {
            std::fs::rename(target, &old).map_err(|e| Error::io(target, e))?;
            true
        }
        Err(_) => false,
    };
    if let Err(e) = std::fs::rename(new, target) {
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        return Err(Error::io(target, e));
    }
    if had_old {
        let meta = std::fs::symlink_metadata(&old);
        let _ = if meta.is_ok_and(|m| m.file_type().is_symlink()) {
            std::fs::remove_file(&old)
        } else {
            std::fs::remove_dir_all(&old)
        };
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::net::HttpError;
    use crate::test_support::fixture_copy;

    /// Serves one package at any URL.
    struct Serve(Vec<u8>);
    impl Http for Serve {
        fn get(&self, _: &str) -> std::result::Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }
    }

    fn zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            out.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(body.as_bytes()).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    fn manifest(id: &str, version: &str) -> String {
        format!(r#"{{ "id": "{id}", "name": "Neon", "version": "{version}", "sdk": "^1" }}"#)
    }

    fn entry(package: &[u8], version: &str) -> CatalogueTheme {
        CatalogueTheme {
            id: "neon".into(),
            name: "Neon".into(),
            version: version.into(),
            author: None,
            description: None,
            sdk: "^1".into(),
            permissions: vec![],
            previews: vec![],
            download: "https://example.com/neon.zip".into(),
            sha256: hex(&Sha256::digest(package)),
        }
    }

    fn package(version: &str) -> Vec<u8> {
        zip(&[
            ("neon/theme.json", &manifest("neon", version)),
            ("neon/index.html", "<!doctype html>"),
            ("neon/assets/app.js", "1"),
        ])
    }

    #[test]
    fn parses_and_skips_bad_entries_with_the_reason() {
        let hash = "a".repeat(64);
        let body = format!(
            r#"{{ "themes": [
                {{ "id": "neon", "name": "Neon", "version": "1.0.0", "sdk": "^1",
                   "download": "https://x/neon.zip", "sha256": "{hash}" }},
                {{ "id": "Bad Id", "name": "B", "version": "1", "sdk": "^1",
                   "download": "https://x/b.zip", "sha256": "{hash}" }},
                {{ "id": "plain", "name": "P", "version": "1", "sdk": "^1",
                   "download": "http://x/p.zip", "sha256": "{hash}" }},
                {{ "id": "short", "name": "S", "version": "1", "sdk": "^1",
                   "download": "https://x/s.zip", "sha256": "abc" }},
                {{ "id": "neon", "name": "Again", "version": "2", "sdk": "^1",
                   "download": "https://x/n.zip", "sha256": "{hash}" }},
                {{ "name": "no id" }}
            ] }}"#
        );
        let c = parse(body.as_bytes()).unwrap();
        assert_eq!(c.themes.len(), 1);
        assert_eq!(c.skipped.len(), 5, "{:?}", c.skipped);
        assert!(c.skipped[0].contains("kebab-case"));
        assert!(c.skipped[1].contains("https://"));
        assert!(c.skipped[2].contains("64 hex"));
        assert!(c.skipped[3].contains("listed twice"));
        assert!(parse(b"not json").is_err());
    }

    #[test]
    fn installs_a_package_and_updates_it_in_place() {
        let (_dir, paths) = fixture_copy();
        let v1 = package("1.0.0");
        let theme = install(&Serve(v1.clone()), &paths, &entry(&v1, "1.0.0")).unwrap();
        assert!(theme.is_available(), "{:?}", theme.unavailable);
        let dir = user_themes_dir(&paths).join("neon");
        assert!(dir.join("assets/app.js").is_file());

        let v2 = package("1.1.0");
        let catalogue = Catalogue {
            themes: vec![entry(&v2, "1.1.0")],
            skipped: vec![],
        };
        assert_eq!(catalogue.update_for(&theme).unwrap().version, "1.1.0");
        let updated = install(&Serve(v2.clone()), &paths, &catalogue.themes[0]).unwrap();
        assert_eq!(updated.manifest.unwrap().version, "1.1.0");
        let left: Vec<_> = std::fs::read_dir(user_themes_dir(&paths))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert_eq!(left, ["neon"], "no work folders left behind");
    }

    #[test]
    fn refuses_a_package_that_is_not_what_the_catalogue_says() {
        let (_dir, paths) = fixture_copy();
        let good = package("1.0.0");
        install(&Serve(good.clone()), &paths, &entry(&good, "1.0.0")).unwrap();
        let installed = user_themes_dir(&paths).join("neon/theme.json");
        let err = |pkg: Vec<u8>, e: CatalogueTheme| {
            install(&Serve(pkg), &paths, &e).unwrap_err().to_string()
        };

        // A hash that does not match.
        let mut wrong = entry(&good, "1.0.0");
        wrong.sha256 = "0".repeat(64);
        assert!(err(good.clone(), wrong).contains("does not match"));
        // A different theme or version inside.
        let other = zip(&[
            ("theme.json", &manifest("other", "1.0.0")),
            ("index.html", ""),
        ]);
        assert!(err(other.clone(), entry(&other, "1.0.0")).contains("holds other 1.0.0"));
        // A path that climbs out.
        let escape = zip(&[
            ("../evil.txt", "x"),
            ("theme.json", &manifest("neon", "2.0.0")),
        ]);
        assert!(err(escape.clone(), entry(&escape, "2.0.0")).contains("outside the package"));
        // No entry file: the theme could not load.
        let broken = zip(&[("theme.json", &manifest("neon", "2.0.0"))]);
        assert!(err(broken.clone(), entry(&broken, "2.0.0")).contains("does not exist"));
        // Not a zip at all.
        assert!(err(b"nope".to_vec(), entry(b"nope", "2.0.0")).contains("not a usable"));
        // An SDK this Forma does not have.
        let mut future = entry(&good, "9.0.0");
        future.sdk = "^99".into();
        assert!(err(good.clone(), future).contains("needs Forma SDK ^99"));

        // Through all that, the installed copy stayed.
        assert!(
            std::fs::read_to_string(installed)
                .unwrap()
                .contains("1.0.0")
        );
        assert!(!paths.userdata.join("evil.txt").exists());
    }

    #[test]
    fn serves_the_last_catalogue_offline() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let url = "https://example.com/catalogue.json";
        assert!(cached_catalogue(&cache, url).is_none());
        let body = format!(
            r#"{{ "themes": [ {{ "id": "neon", "name": "Neon", "version": "1.0.0", "sdk": "^1",
               "download": "https://x/neon.zip", "sha256": "{}" }} ] }}"#,
            "b".repeat(64)
        );
        let fetched = fetch_catalogue(&Serve(body.into_bytes()), &cache, url).unwrap();
        assert_eq!(fetched.data.themes.len(), 1);
        assert_eq!(cached_catalogue(&cache, url).unwrap().themes.len(), 1);
        assert!(fetch_catalogue(&Serve(vec![]), &cache, "http://insecure").is_err());
    }
}
