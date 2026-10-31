//! The import format: how other tools (RetroWorker first) feed RomDex what `gamelist.xml`
//! cannot carry — ROM hashes above all — without sharing any code with it.
//!
//! JSON Lines, one file per producer, dropped in `/userdata/system/romdex/import/`. The first
//! line is a header naming the format and its version; every other line is one game, keyed by
//! system and ROM path, exactly as the library keys it. The full specification, with an
//! example, is `docs/import-format.md`.
//!
//! Parsing is forgiving the way a hand-fed pipeline needs: a line that is not a record is
//! skipped and reported with its line number; a field that is not valid is dropped and
//! reported, and the rest of its record kept. A file from a newer format version is refused
//! whole, since its records may mean something this RomDex cannot tell.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::Warning;
use crate::field;
use crate::paths::Paths;

pub const FORMAT: &str = "romdex-import";
/// The newest version this RomDex reads.
pub const FORMAT_VERSION: u32 = 1;

/// One game, with its fields already checked and normalised for the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub system: String,
    /// Relative to the system's ROM folder, or `/userdata/...` absolute (rebased later).
    pub path: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
pub struct ImportFile {
    /// Who wrote it, from the header, e.g. `retroworker`.
    pub source: Option<String>,
    pub records: Vec<Record>,
    pub warnings: Vec<Warning>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: String,
    format_version: u32,
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord {
    system: String,
    path: String,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
    #[serde(default)]
    media: BTreeMap<String, Value>,
    #[serde(default)]
    hashes: BTreeMap<String, Value>,
    ra_game_id: Option<Value>,
}

/// Metadata keys a record may set, and the library field each one becomes.
const METADATA: &[(&str, &str)] = &[
    ("name", field::NAME),
    ("description", field::DESCRIPTION),
    ("developer", field::DEVELOPER),
    ("publisher", field::PUBLISHER),
    ("genre", field::GENRE),
    ("players", field::PLAYERS),
    ("releaseDate", field::RELEASE_DATE),
    ("rating", field::RATING),
    ("region", field::REGION),
    ("lang", field::LANG),
];

const MEDIA_KINDS: &[&str] = &["image", "thumbnail", "marquee", "video", "fanart", "manual"];

/// Hash name, its field, and the length of its lowercase hex form.
const HASHES: &[(&str, &str, usize)] = &[
    ("crc32", field::HASH_CRC32, 8),
    ("md5", field::HASH_MD5, 32),
    ("sha1", field::HASH_SHA1, 40),
    ("ra", field::RA_HASH, 32),
];

pub fn parse(text: &str, file: &Path) -> ImportFile {
    let mut out = ImportFile::default();
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty());

    let Some((_, first)) = lines.next() else {
        out.warnings.push(Warning::new(file, "empty import file"));
        return out;
    };
    match serde_json::from_str::<Header>(first) {
        Ok(h) if h.format == FORMAT && h.format_version <= FORMAT_VERSION => out.source = h.source,
        Ok(h) if h.format == FORMAT => {
            out.warnings.push(Warning::new(
                file,
                format!(
                    "format version {} is newer than this RomDex reads ({FORMAT_VERSION}); \
                     file skipped",
                    h.format_version
                ),
            ));
            return out;
        }
        _ => {
            out.warnings.push(Warning::new(
                file,
                format!("line 1 must be a header: {{\"format\":\"{FORMAT}\",\"formatVersion\":1}}"),
            ));
            return out;
        }
    }

    for (n, line) in lines {
        let raw = match serde_json::from_str::<RawRecord>(line) {
            Ok(raw) => raw,
            Err(e) => {
                out.warnings
                    .push(Warning::new(file, format!("line {n}: {e}; skipped")));
                continue;
            }
        };
        if raw.system.trim().is_empty() || raw.path.trim().is_empty() {
            out.warnings.push(Warning::new(
                file,
                format!("line {n}: needs a system and a path"),
            ));
            continue;
        }
        let mut problems = Vec::new();
        let fields = fields(&raw, &mut problems);
        for p in problems {
            out.warnings
                .push(Warning::new(file, format!("line {n}: {p}; field skipped")));
        }
        out.records.push(Record {
            system: raw.system.trim().to_owned(),
            path: raw.path.trim().to_owned(),
            fields,
        });
    }
    out
}

fn fields(raw: &RawRecord, problems: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, value) in &raw.metadata {
        let Some((_, field)) = METADATA.iter().find(|(k, _)| k == key) else {
            problems.push(format!("unknown metadata `{key}`"));
            continue;
        };
        let normalised = match (*field, value) {
            (field::RATING, Value::Number(n)) => n
                .as_f64()
                .filter(|r| (0.0..=1.0).contains(r))
                .map(|r| r.to_string()),
            (field::RELEASE_DATE, Value::String(s)) => release_date(s),
            (_, Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            _ => None,
        };
        match normalised {
            Some(v) => {
                out.insert((*field).to_owned(), v);
            }
            None => problems.push(format!("metadata `{key}` has an invalid value")),
        }
    }
    for (kind, value) in &raw.media {
        match (MEDIA_KINDS.contains(&kind.as_str()), value) {
            (true, Value::String(p)) if !p.trim().is_empty() => {
                out.insert(
                    format!("{}{kind}", field::MEDIA_PREFIX),
                    p.trim().to_owned(),
                );
            }
            (false, _) => problems.push(format!("unknown media kind `{kind}`")),
            _ => problems.push(format!("media `{kind}` must be a path")),
        }
    }
    for (name, value) in &raw.hashes {
        let Some((_, field, len)) = HASHES.iter().find(|(h, _, _)| h == name) else {
            problems.push(format!("unknown hash `{name}`"));
            continue;
        };
        match value.as_str().map(str::to_ascii_lowercase) {
            Some(h) if h.len() == *len && h.chars().all(|c| c.is_ascii_hexdigit()) => {
                out.insert((*field).to_owned(), h);
            }
            _ => problems.push(format!("hash `{name}` must be {len} hex digits")),
        }
    }
    if let Some(id) = &raw.ra_game_id {
        match id.as_u64().filter(|id| *id > 0) {
            Some(id) => {
                out.insert(field::RA_GAME_ID.to_owned(), id.to_string());
            }
            None => problems.push("raGameId must be a positive number".to_owned()),
        }
    }
    out
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, stored as a full date like gamelist dates are.
fn release_date(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    fn num(p: &str, len: usize) -> Option<&str> {
        (p.len() == len && p.chars().all(|c| c.is_ascii_digit())).then_some(p)
    }
    let year = num(parts.first()?, 4)?;
    let month = parts.get(1).map_or(Some("01"), |m| num(m, 2))?;
    let day = parts.get(2).map_or(Some("01"), |d| num(d, 2))?;
    (parts.len() <= 3).then(|| format!("{year}-{month}-{day}"))
}

/// The import files, in name order: later files win where two set the same field.
pub fn files(paths: &Paths) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(paths.import_dir())
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ImportFile {
        let file =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/import/retroworker.jsonl");
        parse(&std::fs::read_to_string(&file).unwrap(), &file)
    }

    #[test]
    fn reads_the_sample_file() {
        let import = sample();
        assert_eq!(import.source.as_deref(), Some("retroworker"));
        let zelda = &import.records[0];
        assert_eq!(
            (zelda.system.as_str(), zelda.path.as_str()),
            ("nes", "The Legend of Zelda (USA).nes")
        );
        let f = &zelda.fields;
        assert_eq!(f[field::NAME], "The Legend of Zelda");
        assert_eq!(f[field::RELEASE_DATE], "1986-02-21");
        assert_eq!(f[field::HASH_CRC32], "3fe272fb");
        assert_eq!(f[field::RA_HASH], "e2a91f8e3e5a7ffc5f8fd9a9dd1b6b1d");
        assert_eq!(f[field::RA_GAME_ID], "1447");
        assert_eq!(f["media.marquee"], "./images/zelda-marquee.png");
    }

    #[test]
    fn reports_bad_lines_and_fields_with_their_line_number_and_keeps_the_rest() {
        let import = sample();
        let messages: Vec<&str> = import.warnings.iter().map(|w| w.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("line 4:") && m.contains("skipped")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("line 5:") && m.contains("hash `crc32`")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("line 5:") && m.contains("rating")),
            "{messages:?}"
        );
        let metroid = import
            .records
            .iter()
            .find(|r| r.path.contains("Metroid"))
            .unwrap();
        assert_eq!(
            metroid.fields[field::NAME],
            "Super Metroid",
            "its good fields are kept"
        );
        assert!(!metroid.fields.contains_key(field::HASH_CRC32));
    }

    #[test]
    fn needs_a_header_and_refuses_a_newer_version() {
        let none = parse(r#"{"system":"nes","path":"a.nes"}"#, Path::new("x.jsonl"));
        assert!(none.records.is_empty());
        assert!(none.warnings[0].message.contains("must be a header"));

        let newer = parse(
            "{\"format\":\"romdex-import\",\"formatVersion\":2}\n{\"system\":\"nes\",\"path\":\"a.nes\"}",
            Path::new("x.jsonl"),
        );
        assert!(newer.records.is_empty());
        assert!(
            newer.warnings[0]
                .message
                .contains("newer than this RomDex reads")
        );

        let empty = parse("\n\n", Path::new("x.jsonl"));
        assert!(empty.warnings[0].message.contains("empty"));
    }

    #[test]
    fn release_dates_accept_partial_forms() {
        assert_eq!(release_date("1994").as_deref(), Some("1994-01-01"));
        assert_eq!(release_date("1994-07").as_deref(), Some("1994-07-01"));
        assert_eq!(release_date("1994-07-12").as_deref(), Some("1994-07-12"));
        for bad in ["94", "1994-7", "1994/07/12", "1994-07-12-01", "soon"] {
            assert_eq!(release_date(bad), None, "{bad}");
        }
    }
}
