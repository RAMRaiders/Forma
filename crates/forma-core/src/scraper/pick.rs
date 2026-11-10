//! Scraping one game by hand: what every source offers for each section, side by side, for
//! the user to pick from, and editing a game's details directly.
//!
//! A media offer's address stays here. Some carry the user's account (ScreenScraper's do), so
//! a page only ever gets an offer's number and, if it asks, a small preview.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use serde::Serialize;

use super::engine::{HASH_LIMIT, es_date, has_media, hash, save};
use super::{Config, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source};
use crate::error::Result;
use crate::gamelist::{Tag, set_tags};
use crate::library::Game;
use crate::net::Http;
use crate::paths::Paths;
use crate::systems::System;

/// At most this many pictures of one kind from one source: a source can have dozens.
const OFFERS_PER_SOURCE: usize = 4;
/// How wide a preview is, in pixels.
const PREVIEW_WIDTH: u32 = 480;

/// One source's value for a text section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextOffer {
    pub source: String,
    pub value: String,
    /// The country or language it is in, when the source says.
    pub note: Option<String>,
}

/// A text section of a game (its description, say) and what the sources have for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSection {
    pub field: &'static str,
    pub label: &'static str,
    /// What the game has now.
    pub current: Option<String>,
    pub offers: Vec<TextOffer>,
}

/// One source's picture, video or manual: `id` names it to [`preview`] and [`apply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaOffer {
    pub id: usize,
    pub source: String,
    pub region: Option<String>,
    /// What sets it apart from others of its kind: `Small file`.
    pub variant: Option<String>,
    pub format: Option<String>,
    pub size: Option<u64>,
}

/// A kind of media and what the sources have for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSection {
    pub kind: MediaKind,
    pub label: &'static str,
    /// Whether the game has one now.
    pub current: bool,
    pub offers: Vec<MediaOffer>,
}

/// Everything the sources offer for a game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offers {
    pub text: Vec<TextSection>,
    pub media: Vec<MediaSection>,
    /// Sources that could not answer, and why.
    pub problems: Vec<String>,
}

/// The offers for one game, with the addresses of its media, which stay on this side.
#[derive(Debug, Clone)]
pub struct Picking {
    pub game: i64,
    pub offers: Offers,
    urls: Vec<(MediaKind, String)>,
}

impl Picking {
    /// The address of offer `id`, and its kind.
    fn offer(&self, id: usize) -> Option<&(MediaKind, String)> {
        self.urls.get(id)
    }

    /// A small picture of offer `id` as a `data:` URL; `None` when it is not a picture, or
    /// cannot be fetched.
    pub fn preview(&self, http: &dyn Http, id: usize) -> Option<String> {
        let (kind, url) = self.offer(id)?;
        if matches!(kind, MediaKind::Video | MediaKind::Manual) {
            return None;
        }
        preview(&http.download(url, kind.limit()).ok()?)
    }
}

/// The text sections, each with how to read it from a [`Metadata`] and from a [`Game`].
fn sections(age_rating: bool) -> Vec<(&'static str, &'static str)> {
    let mut all = vec![
        ("name", "Name"),
        ("description", "Description"),
        ("developer", "Developer"),
        ("publisher", "Publisher"),
        ("genre", "Genre"),
        ("players", "Players"),
        ("releaseDate", "Release date"),
        ("rating", "Rating"),
    ];
    if age_rating {
        all.push(("ageRating", "Age ratings"));
    }
    all
}

fn found_text(found: &Metadata, field: &str) -> Option<String> {
    match field {
        "name" => found.name.clone(),
        "description" => found.description.clone(),
        "developer" => found.developer.clone(),
        "publisher" => found.publisher.clone(),
        "genre" => found.genre.clone(),
        "players" => found.players.clone(),
        "releaseDate" => found.release_date.clone(),
        "rating" => found.rating.map(|r| format!("{:.2}", r.clamp(0.0, 1.0))),
        "ageRating" => found.age_rating.clone(),
        _ => None,
    }
    .map(|v| v.trim().to_owned())
    .filter(|v| !v.is_empty())
}

fn game_text(game: &Game, stem: &str, field: &str) -> Option<String> {
    match field {
        // A name that is only the file's is no name yet.
        "name" => Some(game.name.clone()).filter(|n| n != stem),
        "description" => game.description.clone(),
        "developer" => game.developer.clone(),
        "publisher" => game.publisher.clone(),
        "genre" => game.genre.clone(),
        "players" => game.players.clone(),
        "releaseDate" => game.release_date.clone(),
        "rating" => game.rating.map(|r| format!("{r:.2}")),
        "ageRating" => game.age_rating.clone(),
        _ => None,
    }
}

/// Asks every source that is on and set up about `game`, whatever the game already has and
/// however many have answered already, for the sections `config` wants.
pub fn gather(
    http: &dyn Http,
    sources: &[Box<dyn Source>],
    config: &Config,
    credentials: &BTreeMap<String, Credentials>,
    game: &Game,
    system: &System,
) -> Picking {
    let rom = Path::new(&game.path);
    let file_name = rom
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = rom
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let full = system.rom_path.join(rom);
    let size = std::fs::metadata(&full)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len());
    let name = super::text::search_name(&game.name);
    let kinds = config.all_media();
    let empty = Credentials::new();

    let mut hashes = None;
    let mut answers: Vec<(String, Found)> = Vec::new();
    let mut problems = Vec::new();
    for choice in config.sources.iter().filter(|c| c.enabled) {
        let Some(source) = sources.iter().find(|s| s.id() == choice.id) else {
            continue;
        };
        let creds = credentials.get(source.id()).unwrap_or(&empty);
        if !source.ready(creds) {
            continue;
        }
        if source.id() == "screenscraper" && hashes.is_none() {
            hashes = Some(
                (size > 0 && size <= HASH_LIMIT)
                    .then(|| hash(&full))
                    .flatten(),
            );
        }
        let query = Query {
            system: &system.name,
            platforms: &system.platforms,
            file_name: &file_name,
            stem: &stem,
            name: &name,
            size,
            hashes: hashes.as_ref().and_then(Option::as_ref),
            wanted: &kinds,
            wants_metadata: true,
        };
        if !source.covers(&query) {
            continue;
        }
        match source.lookup(http, creds, &query) {
            Ok(Some(found)) => answers.push((source.name().to_owned(), found)),
            Ok(None) => {}
            Err(LookupError::Credentials(e) | LookupError::Quota(e) | LookupError::Failed(e)) => {
                problems.push(format!("{}: {e}", source.name()))
            }
        }
    }

    let text = sections(config.wants_age_rating())
        .into_iter()
        .map(|(field, label)| TextSection {
            field,
            label,
            current: game_text(game, &stem, field),
            offers: answers
                .iter()
                .flat_map(|(source, found)| {
                    let best = found_text(&found.metadata, field).map(|value| TextOffer {
                        source: source.clone(),
                        value,
                        note: found.notes.text.get(field).cloned(),
                    });
                    let others = found
                        .notes
                        .names
                        .iter()
                        .filter(move |_| field == "name")
                        .map(|(value, note)| TextOffer {
                            source: source.clone(),
                            value: value.clone(),
                            note: note.clone(),
                        });
                    best.into_iter().chain(others)
                })
                .collect(),
        })
        .filter(|s| !s.offers.is_empty())
        .collect();

    let mut urls = Vec::new();
    let mut media = Vec::new();
    for kind in kinds {
        let mut offers = Vec::new();
        let mut seen = HashSet::new();
        for (source, found) in &answers {
            let theirs = found.media.iter().filter(|(k, _)| *k == kind);
            for (_, url) in theirs.take(OFFERS_PER_SOURCE) {
                if seen.insert(url.clone()) {
                    let note = found.notes.media.get(url).cloned().unwrap_or_default();
                    offers.push(MediaOffer {
                        id: urls.len(),
                        source: source.clone(),
                        region: note.region,
                        variant: note.variant,
                        format: note.format.or_else(|| extension(url)),
                        size: note.size,
                    });
                    urls.push((kind, url.clone()));
                }
            }
        }
        if !offers.is_empty() {
            media.push(MediaSection {
                kind,
                label: kind.label(),
                current: has_media(game, kind),
                offers,
            });
        }
    }

    Picking {
        game: game.id,
        offers: Offers {
            text,
            media,
            problems,
        },
        urls,
    }
}

/// The file type in an address, for sources that do not say.
fn extension(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let (_, ext) = path.rsplit_once('/')?.1.rsplit_once('.')?;
    (ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

/// `bytes` as a small JPEG in a `data:` URL, if they are a picture.
fn preview(bytes: &[u8]) -> Option<String> {
    let picture = image::load_from_memory(bytes).ok()?;
    let small = if picture.width() > PREVIEW_WIDTH {
        picture.thumbnail(PREVIEW_WIDTH, PREVIEW_WIDTH.saturating_mul(4))
    } else {
        picture
    };
    let mut jpeg = Vec::new();
    small
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64(&jpeg)))
}

pub(super) fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(LETTERS[(n >> (18 - 6 * i)) as usize & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// A section's gamelist tag and what to write in it; `Err` says why `value` will not do.
fn text_tag(field: &str, value: &str) -> std::result::Result<(&'static str, String), String> {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    let value = value.trim();
    Ok(match field {
        "name" => ("name", value.to_owned()),
        "description" => ("desc", value.to_owned()),
        "developer" => ("developer", value.to_owned()),
        "publisher" => ("publisher", value.to_owned()),
        "genre" => ("genre", value.to_owned()),
        "players" => ("players", value.to_owned()),
        "ageRating" => ("agerating", value.to_owned()),
        "releaseDate" => {
            if !is_date(value) {
                return Err(format!(
                    "`{value}` is not a date: write it as YYYY, YYYY-MM or YYYY-MM-DD"
                ));
            }
            ("releasedate", es_date(value))
        }
        "rating" => match value.parse::<f64>() {
            Ok(r) if (0.0..=1.0).contains(&r) => ("rating", format!("{r:.2}")),
            _ => return Err(format!("`{value}` is not a rating from 0 to 1")),
        },
        _ => return Err(format!("no field `{field}`")),
    })
}

fn is_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    let year = parts.next().is_some_and(|y| digits(y, 4));
    let rest: Vec<&str> = parts.collect();
    year && rest.len() <= 2 && rest.iter().all(|p| digits(p, 2))
}

/// Writes what the user picked: text as given, and each media kind's chosen offer downloaded.
/// Returns what could not be saved.
pub fn apply(
    http: &dyn Http,
    paths: &Paths,
    picking: &Picking,
    game: &Game,
    system: &System,
    text: &BTreeMap<String, String>,
    media: &[usize],
) -> Result<Vec<String>> {
    let rom = Path::new(&game.path);
    let stem = rom
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut problems = Vec::new();
    let mut tags: Vec<(&str, Tag)> = Vec::new();
    for (field, value) in text {
        match text_tag(field, value) {
            Ok((_, v)) if v.is_empty() => {}
            Ok((tag, v)) => tags.push((tag, Tag::Set(v))),
            Err(e) => problems.push(e),
        }
    }
    for id in media {
        let Some((kind, url)) = picking.offer(*id) else {
            problems.push(format!("no offer {id}"));
            continue;
        };
        match save(http, &system.rom_path, &stem, *kind, url) {
            Ok(path) => tags.push((kind.tag(), Tag::Set(path))),
            Err(e) => problems.push(format!("{}: {e}", kind.label())),
        }
    }
    if !tags.is_empty() {
        set_tags(&system.rom_path, paths, rom, &game.name, &tags)?;
    }
    Ok(problems)
}

/// Changes a game's details by hand: an empty value clears the field (a game keeps its name).
pub fn edit(
    paths: &Paths,
    game: &Game,
    system: &System,
    fields: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    let mut tags: Vec<(&str, Tag)> = Vec::new();
    for (field, value) in fields {
        if value.trim().is_empty() {
            if field == "name" {
                return Err("a game needs a name".to_owned());
            }
            // Through `text_tag` for the tag's name; any valid value does.
            let (tag, _) = text_tag(field, if field == "rating" { "0" } else { "2000" })?;
            tags.push((tag, Tag::Remove));
        } else {
            let (tag, v) = text_tag(field, value)?;
            tags.push((tag, Tag::Set(v)));
        }
    }
    if !tags.is_empty() {
        set_tags(
            &system.rom_path,
            paths,
            Path::new(&game.path),
            &game.name,
            &tags,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_ratings_are_checked() {
        assert!(is_date("1994"));
        assert!(is_date("1994-03"));
        assert!(is_date("1994-03-19"));
        assert!(!is_date("94"));
        assert!(!is_date("1994-3"));
        assert!(!is_date("March 1994"));
        assert_eq!(
            text_tag("releaseDate", "1994-03").unwrap(),
            ("releasedate", "19940301T000000".to_owned())
        );
        assert_eq!(
            text_tag("rating", "0.5").unwrap(),
            ("rating", "0.50".to_owned())
        );
        assert!(text_tag("rating", "85").is_err());
        assert!(text_tag("hologram", "x").is_err());
    }

    #[test]
    fn base64_pads_as_it_should() {
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
    }
}
