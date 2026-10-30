//! Reading `gamelist.xml`: an import source written by EmulationStation and scrapers.
//!
//! RomDex never treats this file as its store. Each entry becomes a set of normalised fields
//! (see [`crate::field`]) that the library records under the `gamelist` source.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::error::Warning;
use crate::field;
use crate::paths::Paths;
use crate::xml::{self, Element};

#[derive(Debug, Clone, PartialEq)]
pub struct GamelistEntry {
    /// Relative to the system's ROM folder, normalised (`./a/b.nes` becomes `a/b.nes`).
    pub path: PathBuf,
    pub fields: BTreeMap<&'static str, String>,
}

#[derive(Debug, Default)]
pub struct Gamelist {
    pub entries: Vec<GamelistEntry>,
    pub warnings: Vec<Warning>,
}

/// Tag in `gamelist.xml` → field, and how its value is normalised.
const TEXT_FIELDS: &[(&str, &str, Kind)] = &[
    ("name", field::NAME, Kind::Text),
    ("desc", field::DESCRIPTION, Kind::Text),
    ("developer", field::DEVELOPER, Kind::Text),
    ("publisher", field::PUBLISHER, Kind::Text),
    ("genre", field::GENRE, Kind::Text),
    ("players", field::PLAYERS, Kind::Text),
    ("region", field::REGION, Kind::Text),
    ("lang", field::LANG, Kind::Text),
    ("releasedate", field::RELEASE_DATE, Kind::Date),
    ("lastplayed", field::LAST_PLAYED, Kind::DateTime),
    ("rating", field::RATING, Kind::Rating),
    ("playcount", field::PLAY_COUNT, Kind::Count),
    ("gametime", field::GAME_TIME, Kind::Count),
    ("favorite", field::FAVORITE, Kind::Bool),
    ("hidden", field::HIDDEN, Kind::Bool),
    ("cheevosId", field::RA_GAME_ID, Kind::Count),
    ("cheevosHash", field::RA_HASH, Kind::Text),
    ("image", field::MEDIA_IMAGE, Kind::Media),
    ("thumbnail", field::MEDIA_THUMBNAIL, Kind::Media),
    ("marquee", field::MEDIA_MARQUEE, Kind::Media),
    ("video", field::MEDIA_VIDEO, Kind::Media),
    ("fanart", field::MEDIA_FANART, Kind::Media),
    ("manual", field::MEDIA_MANUAL, Kind::Media),
];

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Date,
    DateTime,
    Rating,
    Count,
    Bool,
    Media,
}

/// Parses one system's gamelist. `system_dir` is that system's ROM folder, which relative
/// paths in the file are written against.
pub fn parse_gamelist(xml: &str, file: &Path, system_dir: &Path, paths: &Paths) -> Gamelist {
    let mut out = Gamelist::default();
    let parsed = xml::parse(xml);
    if let Some(error) = parsed.error {
        out.warnings.push(Warning::new(
            file,
            format!("{error}; entries before it were kept"),
        ));
    }
    let Some(root) = parsed.root else {
        return out;
    };

    for game in root.children_named("game") {
        let Some(raw_path) = game.child_text("path") else {
            let name = game.child_text("name").unwrap_or("(unnamed)");
            out.warnings.push(Warning::new(
                file,
                format!("`{name}` has no <path>; skipped"),
            ));
            continue;
        };
        let Some(path) = relative_to(system_dir, &resolve(raw_path, system_dir, paths)) else {
            out.warnings.push(Warning::new(
                file,
                format!("`{raw_path}` is outside the system folder; skipped"),
            ));
            continue;
        };
        out.entries.push(GamelistEntry {
            path,
            fields: fields(game, system_dir, paths),
        });
    }
    out
}

fn fields(game: &Element, system_dir: &Path, paths: &Paths) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    for &(tag, key, kind) in TEXT_FIELDS {
        let Some(raw) = game.child_text(tag) else {
            continue;
        };
        let value = match kind {
            Kind::Text => Some(raw.to_owned()),
            Kind::Date => es_date(raw).map(|(date, _)| date),
            Kind::DateTime => es_date(raw).map(|(date, time)| format!("{date}T{time}")),
            Kind::Rating => raw
                .parse::<f64>()
                .ok()
                .filter(|r| (0.0..=1.0).contains(r))
                .map(|r| r.to_string()),
            Kind::Count => raw.parse::<u64>().ok().map(|n| n.to_string()),
            Kind::Bool => Some(
                if raw.eq_ignore_ascii_case("true") {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            Kind::Media => Some(
                resolve(raw, system_dir, paths)
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        if let Some(value) = value {
            out.insert(key, value);
        }
    }
    out
}

/// ES writes dates as `YYYYMMDDTHHMMSS`. Returns (`YYYY-MM-DD`, `HH:MM:SS`). A year-only or
/// otherwise partial date keeps the parts it has, padded with `01`/`00`; garbage is dropped.
fn es_date(raw: &str) -> Option<(String, String)> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |range: std::ops::Range<usize>, default: &str| {
        digits
            .get(range)
            .filter(|p| *p != "00")
            .unwrap_or(default)
            .to_owned()
    };
    let year = &digits[..4];
    let (month, day) = (part(4..6, "01"), part(6..8, "01"));
    let (hh, mm, ss) = (
        digits.get(8..10).unwrap_or("00"),
        digits.get(10..12).unwrap_or("00"),
        digits.get(12..14).unwrap_or("00"),
    );
    Some((format!("{year}-{month}-{day}"), format!("{hh}:{mm}:{ss}")))
}

/// A path as written in a gamelist: `./x` relative to the system folder, `/userdata/...`
/// absolute (rebased for development), or bare relative.
fn resolve(raw: &str, system_dir: &Path, paths: &Paths) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        paths.rebase(p)
    } else {
        system_dir.join(p)
    }
}

/// `full` relative to `base`, with `.` and `..` folded. `None` if it escapes `base`.
pub(crate) fn relative_to(base: &Path, full: &Path) -> Option<PathBuf> {
    let rest = full.strip_prefix(base).ok()?;
    let mut out = PathBuf::new();
    for c in rest.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_paths;

    fn fixture(system: &str) -> (Gamelist, PathBuf) {
        let paths = fixture_paths();
        let dir = paths.roms().join(system);
        let file = dir.join("gamelist.xml");
        let xml = std::fs::read_to_string(&file).unwrap();
        (parse_gamelist(&xml, &file, &dir, &paths), dir)
    }

    #[test]
    fn reads_and_normalises_a_full_entry() {
        let (list, dir) = fixture("nes");
        let mario = &list.entries[0];
        assert_eq!(mario.path, Path::new("Super Mario Bros. (World).nes"));
        let f = &mario.fields;
        assert_eq!(f[field::NAME], "Super Mario Bros.");
        assert_eq!(
            f[field::DESCRIPTION],
            "Mario & Luigi save the Mushroom Kingdom. Café included."
        );
        assert_eq!(f[field::RELEASE_DATE], "1985-09-13");
        assert_eq!(f[field::LAST_PLAYED], "2026-01-01T12:00:00");
        assert_eq!(f[field::RATING], "0.9");
        assert_eq!(f[field::FAVORITE], "1");
        assert_eq!(f[field::PLAY_COUNT], "12");
        assert_eq!(f[field::RA_GAME_ID], "1446");
        assert_eq!(
            Path::new(&f[field::MEDIA_IMAGE]),
            dir.join("images/Super Mario Bros. (World)-image.png")
        );
    }

    #[test]
    fn keeps_cdata_and_tolerates_undefined_entities() {
        let (list, _) = fixture("nes");
        assert_eq!(
            list.entries[1].fields[field::DESCRIPTION],
            "Link explores <Hyrule>."
        );
        let tom = &list.entries[2];
        assert_eq!(tom.path, Path::new("Tom & Jerry (USA).nes"));
        assert!(tom.fields[field::DESCRIPTION].contains("\u{a0}here"));
        assert_eq!(tom.fields[field::HIDDEN], "1");
    }

    #[test]
    fn skips_an_entry_without_a_path_and_ignores_folders() {
        let (list, _) = fixture("nes");
        // Mario, Zelda, Tom & Jerry, and the entry for a file not on disk (the library decides).
        assert_eq!(list.entries.len(), 4);
        assert_eq!(list.warnings.len(), 1);
        assert!(list.warnings[0].message.contains("no <path>"));
    }

    #[test]
    fn a_truncated_gamelist_keeps_the_complete_entries() {
        let (list, _) = fixture("snes");
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].fields[field::DEVELOPER], "Nintendo R&D1");
        assert_eq!(list.warnings.len(), 1);
    }

    #[test]
    fn es_dates_tolerate_partial_values() {
        assert_eq!(es_date("1994").unwrap().0, "1994-01-01");
        assert_eq!(es_date("19940000T000000").unwrap().0, "1994-01-01");
        assert_eq!(es_date("not a date"), None);
    }

    #[test]
    fn rejects_out_of_range_ratings_and_bad_counts() {
        let paths = fixture_paths();
        let xml = "<gameList><game><path>./a.nes</path><rating>7</rating>\
                   <playcount>-1</playcount></game></gameList>";
        let list = parse_gamelist(
            xml,
            Path::new("g.xml"),
            Path::new("/userdata/roms/nes"),
            &paths,
        );
        assert!(!list.entries[0].fields.contains_key(field::RATING));
        assert!(!list.entries[0].fields.contains_key(field::PLAY_COUNT));
    }

    #[test]
    fn a_path_escaping_the_system_folder_is_skipped() {
        let paths = fixture_paths();
        let xml = "<gameList><game><path>../snes/x.sfc</path></game></gameList>";
        let list = parse_gamelist(xml, Path::new("g.xml"), Path::new("/r/nes"), &paths);
        assert!(list.entries.is_empty());
        assert_eq!(list.warnings.len(), 1);
    }
}
