//! Scraping one game: asking the sources in order, downloading what they point to, and
//! writing it all to the system's folders and `gamelist.xml`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Config, Credentials, Found, Hashes, LookupError, MediaKind, Metadata, Query, Source};
use crate::error::{Error, Result};
use crate::gamelist::{Tag, set_tags};
use crate::library::Game;
use crate::net::Http;
use crate::paths::Paths;
use crate::systems::System;

/// Files above this are not hashed (it would take too long on a Pi); sources then match by
/// name and size.
pub(super) const HASH_LIMIT: u64 = 256 << 20;

/// What scraping a game did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Nothing was wanted: it had everything asked for.
    pub skipped: bool,
    /// The sources that gave something, in order.
    pub sources: Vec<String>,
    /// The media saved.
    pub media: Vec<MediaKind>,
    /// Whether any metadata was written.
    pub metadata: bool,
    /// Sources that failed, and why.
    pub problems: Vec<String>,
}

impl Outcome {
    pub fn found(&self) -> bool {
        self.metadata || !self.media.is_empty()
    }
}

/// Scrapes `game` of `system` with `sources` as `config` says. Sources in `out_of_service`
/// (refused credentials, over quota) are not asked; one that fails that way here is added to
/// it, so a run stops asking it.
#[allow(clippy::too_many_arguments)]
pub fn scrape_game(
    http: &dyn Http,
    paths: &Paths,
    sources: &[Box<dyn Source>],
    config: &Config,
    credentials: &BTreeMap<String, Credentials>,
    game: &Game,
    system: &System,
    out_of_service: &mut HashSet<String>,
) -> Result<Outcome> {
    let rom = Path::new(&game.path);
    let file_name = rom
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = rom
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut outcome = Outcome::default();
    if config.skip_scraped && scraped(game) {
        outcome.skipped = true;
        return Ok(outcome);
    }

    let mut want: Vec<MediaKind> = config
        .all_media()
        .into_iter()
        .filter(|k| config.replace || !has_media(game, *k))
        .collect();
    // A theme's backups: asked for along with the rest, saved only if what each stands in for
    // cannot be had.
    let backups: Vec<(MediaKind, MediaKind)> = config
        .needs
        .backups
        .iter()
        .copied()
        .filter(|(backup, _)| config.replace || !has_media(game, *backup))
        .collect();
    let age = config.wants_age_rating();
    let mut want_metadata = config.replace || lacks_metadata(game, &stem, age);
    if want.is_empty() && !want_metadata {
        outcome.skipped = true;
        return Ok(outcome);
    }
    let regions = config.regions_for(&stem);
    // Each source's media that was not saved, in the order asked, for the backups.
    let mut spare: Vec<(MediaKind, String)> = Vec::new();

    let full = system.rom_path.join(rom);
    let size = std::fs::metadata(&full)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len());
    let mut hashes: Option<Option<Hashes>> = None;
    let name = super::text::search_name(&game.name);
    let mut metadata = Metadata::default();
    // Kind → the path written in the gamelist.
    let mut saved: BTreeMap<MediaKind, String> = BTreeMap::new();
    let empty = Credentials::new();

    for choice in config.sources.iter().filter(|c| c.enabled) {
        if want.is_empty() && !want_metadata {
            break;
        }
        let Some(source) = sources.iter().find(|s| s.id() == choice.id) else {
            continue;
        };
        let creds = credentials.get(source.id()).unwrap_or(&empty);
        let useful = (want_metadata && source.has_metadata())
            || source.gives().iter().any(|k| want.contains(k));
        if !useful || out_of_service.contains(source.id()) || !source.ready(creds) {
            continue;
        }
        if source.id() == "screenscraper" && hashes.is_none() {
            hashes = Some(
                (size > 0 && size <= HASH_LIMIT)
                    .then(|| hash(&full))
                    .flatten(),
            );
        }
        let asked: Vec<MediaKind> = want
            .iter()
            .copied()
            .chain(backups.iter().map(|(b, _)| *b))
            .collect();
        let query = Query {
            system: &system.name,
            platforms: &system.platforms,
            file_name: &file_name,
            stem: &stem,
            name: &name,
            size,
            hashes: hashes.as_ref().and_then(Option::as_ref),
            wanted: &asked,
            wants_metadata: want_metadata,
            regions: &regions,
        };
        if !source.covers(&query) {
            continue;
        }
        let mut found: Found = match source.lookup(http, creds, &query) {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(e @ (LookupError::Credentials(_) | LookupError::Quota(_))) => {
                out_of_service.insert(source.id().to_owned());
                outcome.problems.push(format!("{}: {e}", source.name()));
                continue;
            }
            Err(LookupError::Failed(e)) => {
                outcome.problems.push(format!("{}: {e}", source.name()));
                continue;
            }
        };
        found.metadata.decode_entities();

        let mut gave = false;
        if want_metadata && found.metadata != Metadata::default() {
            metadata.fill_from(found.metadata);
            gave = true;
            want_metadata = !metadata.is_complete(age);
        }
        for kind in want.clone() {
            for (_, url) in found.media.iter().filter(|(k, _)| *k == kind) {
                match save(http, &system.rom_path, &stem, kind, url) {
                    Ok(path) => {
                        outcome.media.push(kind);
                        want.retain(|k| *k != kind);
                        saved.insert(kind, path);
                        gave = true;
                        break;
                    }
                    // A guessed URL that is not there, or a broken file: try the next.
                    Err(_) => continue,
                }
            }
        }
        spare.extend(
            found
                .media
                .into_iter()
                .filter(|(k, _)| backups.iter().any(|(b, _)| b == k)),
        );
        if gave {
            outcome.sources.push(source.id().to_owned());
        }
    }

    for (backup, main) in &backups {
        if saved.contains_key(main) || has_media(game, *main) {
            continue;
        }
        for (_, url) in spare.iter().filter(|(k, _)| k == backup) {
            if let Ok(path) = save(http, &system.rom_path, &stem, *backup, url) {
                outcome.media.push(*backup);
                saved.insert(*backup, path);
                break;
            }
        }
    }

    let mut tags: Vec<(&str, Tag)> = saved
        .into_iter()
        .map(|(kind, path)| (kind.tag(), Tag::Set(path)))
        .collect();
    let text_tags = metadata_tags(&metadata, game, &stem, config.replace);
    outcome.metadata = !text_tags.is_empty();
    tags.extend(text_tags);
    if !tags.is_empty() {
        set_tags(&system.rom_path, paths, rom, &game.name, &tags)?;
    }
    Ok(outcome)
}

/// Whether the game has been scraped: a description, or any media.
fn scraped(game: &Game) -> bool {
    game.description.is_some() || MediaKind::ALL.iter().any(|k| has_media(game, *k))
}

pub(super) fn has_media(game: &Game, kind: MediaKind) -> bool {
    game.media
        .get(kind.tag())
        .is_some_and(|p| Path::new(p).exists())
}

/// Whether any field is missing; a name that is only the file's counts as missing. The age
/// rating counts only when it is wanted (`age`).
fn lacks_metadata(game: &Game, stem: &str, age: bool) -> bool {
    game.name == stem
        || game.description.is_none()
        || game.developer.is_none()
        || game.publisher.is_none()
        || game.genre.is_none()
        || game.players.is_none()
        || game.release_date.is_none()
        || game.rating.is_none()
        || (age && game.age_rating.is_none())
}

/// The text tags to write: what was found, where the game has nothing (or everything, when
/// replacing).
fn metadata_tags(
    found: &Metadata,
    game: &Game,
    stem: &str,
    replace: bool,
) -> Vec<(&'static str, Tag)> {
    let mut tags = Vec::new();
    let mut put = |tag: &'static str, value: Option<String>, has: bool| {
        if let Some(value) = value.filter(|v| !v.trim().is_empty())
            && (replace || !has)
        {
            tags.push((
                tag,
                Tag::Set(
                    value
                        .replace("\r\n", "\n")
                        .replace('\r', "\n")
                        .trim()
                        .to_owned(),
                ),
            ));
        }
    };
    put("name", found.name.clone(), game.name != stem);
    put(
        "desc",
        found.description.clone(),
        game.description.is_some(),
    );
    put(
        "developer",
        found.developer.clone(),
        game.developer.is_some(),
    );
    put(
        "publisher",
        found.publisher.clone(),
        game.publisher.is_some(),
    );
    put("genre", found.genre.clone(), game.genre.is_some());
    put("players", found.players.clone(), game.players.is_some());
    put(
        "releasedate",
        found.release_date.as_deref().map(es_date),
        game.release_date.is_some(),
    );
    put(
        "rating",
        found.rating.map(|r| format!("{:.2}", r.clamp(0.0, 1.0))),
        game.rating.is_some(),
    );
    put(
        "agerating",
        found.age_rating.clone(),
        game.age_rating.is_some(),
    );
    tags
}

/// `1994-03-19` (or `1994`, `1994-03`) as ES writes dates: `19940319T000000`.
pub(super) fn es_date(date: &str) -> String {
    let digits: String = date.chars().filter(char::is_ascii_digit).collect();
    let pad = |s: Option<&str>| s.filter(|s| s.len() == 2).unwrap_or("01").to_owned();
    format!(
        "{}{}{}T000000",
        &digits[..digits.len().min(4)],
        pad(digits.get(4..6)),
        pad(digits.get(6..8))
    )
}

pub(super) fn hash(file: &Path) -> Option<Hashes> {
    let reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let (h, _) = romdex::hash::hash_reader(reader).ok()?;
    Some(Hashes {
        crc32: h.crc32,
        md5: h.md5,
        sha1: h.sha1,
    })
}

/// Downloads `url` as `kind` into `system_dir/<folder>/<stem>-<suffix>.<ext>`, checking it is
/// what it should be (a picture, a video, a PDF). Returns the path to put in the gamelist.
pub(super) fn save(
    http: &dyn Http,
    system_dir: &Path,
    stem: &str,
    kind: MediaKind,
    url: &str,
) -> std::result::Result<String, String> {
    let bytes = http.download(url, kind.limit()).map_err(|e| e.message)?;
    let ext = sniff(&bytes, kind).ok_or("not the kind of file expected")?;
    let dir = system_dir.join(kind.folder());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("{stem}-{}.{ext}", kind.suffix());
    let file = dir.join(&name);
    let temp: PathBuf = dir.join(format!(".{name}.forma.tmp"));
    std::fs::write(&temp, &bytes).map_err(|e| Error::io(&temp, e).to_string())?;
    std::fs::rename(&temp, &file).map_err(|e| Error::io(&file, e).to_string())?;
    // An older copy in another format would be found first by some tools.
    for other in ["png", "jpg", "gif", "webp", "mp4", "webm", "mkv", "pdf"] {
        if other != ext {
            let _ = std::fs::remove_file(dir.join(format!("{stem}-{}.{other}", kind.suffix())));
        }
    }
    Ok(format!("./{}/{name}", kind.folder()))
}

/// The file's extension from its first bytes, if it is something `kind` can be.
fn sniff(bytes: &[u8], kind: MediaKind) -> Option<&'static str> {
    let image = if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    };
    match kind {
        MediaKind::Video => {
            if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
                Some("mp4")
            } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
                Some("webm")
            } else {
                None
            }
        }
        MediaKind::Manual => bytes.starts_with(b"%PDF").then_some("pdf"),
        _ => image,
    }
}
