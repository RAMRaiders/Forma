//! Which games of a system are versions of one another: the same title with different region,
//! revision or translation tags. Any system can ask; nothing here needs a scraper.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use super::Library;
use crate::error::Result;

/// One file of a game that has several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVersion {
    pub id: i64,
    pub name: String,
    pub path: String,
    /// What its name has in brackets: `Japan`, `Rev 1`, `T-En by Someone v1.0`.
    pub tags: Vec<String>,
    /// The first version in its group with the same game data, when that is known.
    pub identical_to: Option<i64>,
}

/// A title with more than one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionGroup {
    pub title: String,
    pub games: Vec<GameVersion>,
}

/// The title before the first bracket, and what is inside each bracket.
fn split_name(text: &str) -> (String, Vec<String>) {
    let start = text.find(['(', '[']).unwrap_or(text.len());
    let mut tags = Vec::new();
    let mut open = None;
    for (i, c) in text.char_indices().skip_while(|(i, _)| *i < start) {
        match (c, open) {
            ('(' | '[', None) => open = Some(i + 1),
            (')' | ']', Some(from)) => {
                let tag = text[from..i].trim();
                if !tag.is_empty() {
                    tags.push(tag.to_owned());
                }
                open = None;
            }
            _ => {}
        }
    }
    (text[..start].trim().to_owned(), tags)
}

/// Whether a tag names one piece of a game (`Disc 2`), which is not another version of it.
fn is_part(tag: &str) -> bool {
    let lower = tag.to_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next().unwrap_or_default();
    matches!(first, "disc" | "disk" | "cd" | "side" | "tape" | "part") && words.next().is_some()
}

/// What versions of one title share: no articles, spacing, punctuation or case.
fn title_key(title: &str, tags: &[String]) -> String {
    let lower = title.to_lowercase();
    let mut bare = lower.as_str();
    for article in [", the", ", an", ", a"] {
        bare = bare.strip_suffix(article).unwrap_or(bare);
    }
    for article in ["the ", "an ", "a "] {
        bare = bare.strip_prefix(article).unwrap_or(bare);
    }
    let mut key: String = bare.chars().filter(|c| c.is_alphanumeric()).collect();
    for part in tags.iter().filter(|t| is_part(t)) {
        key.push('|');
        key.push_str(&part.to_lowercase());
    }
    key
}

impl Library {
    /// The titles of `system` with more than one version, by title. Named by the file, whose
    /// tags are the reliable ones; a file with no tags (an arcade set's short name) by the
    /// game's name instead.
    pub fn versions(&self, system: &str) -> Result<Vec<VersionGroup>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, path, ra_hash FROM game
             WHERE system = ?1 AND present = 1 AND hidden = 0 ORDER BY path COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([system], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut order = Vec::new();
        let mut groups: HashMap<String, VersionGroup> = HashMap::new();
        let mut hashes: HashMap<(String, String), i64> = HashMap::new();
        for row in rows {
            let (id, name, path, hash) = row?;
            let stem = Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (mut title, mut tags) = split_name(&stem);
            if tags.is_empty() {
                (title, tags) = split_name(&name);
            }
            let key = title_key(&title, &tags);
            if key.is_empty() {
                continue;
            }
            let identical_to = hash.and_then(|h| {
                let first = *hashes.entry((key.clone(), h)).or_insert(id);
                (first != id).then_some(first)
            });
            groups
                .entry(key.clone())
                .or_insert_with(|| {
                    order.push(key);
                    VersionGroup {
                        title,
                        games: Vec::new(),
                    }
                })
                .games
                .push(GameVersion {
                    id,
                    name,
                    path,
                    tags,
                    identical_to,
                });
        }
        let mut found: Vec<VersionGroup> = order
            .iter()
            .filter_map(|key| groups.remove(key))
            .filter(|g| g.games.len() > 1)
            .collect();
        found.sort_by_key(|g| g.title.to_lowercase());
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> String {
        let (title, tags) = split_name(name);
        title_key(&title, &tags)
    }

    #[test]
    fn reads_the_title_and_every_tag() {
        let (title, tags) = split_name("Monster World IV (Japan) [T-En by DeJap v1.02]");
        assert_eq!(title, "Monster World IV");
        assert_eq!(tags, ["Japan", "T-En by DeJap v1.02"]);
    }

    #[test]
    fn versions_of_a_title_share_a_key() {
        assert_eq!(key("Sonic (USA)"), key("sonic (Japan) (Rev 1)"));
        assert_eq!(
            key("Addams Family, The (USA)"),
            key("The Addams Family (Europe)")
        );
        assert_eq!(key("Mega-Lo-Mania (Japan)"), key("Mega Lo Mania (Europe)"));
    }

    #[test]
    fn sequels_and_other_titles_do_not() {
        assert_ne!(
            key("Phantasy Star II (USA)"),
            key("Phantasy Star III (USA)")
        );
        assert_ne!(
            key("Phantasy Star II (USA)"),
            key("Phantasy Star II - Amia's Adventure (Japan)")
        );
    }

    #[test]
    fn the_discs_of_a_game_are_not_versions_of_each_other() {
        assert_ne!(key("Game (USA) (Disc 1)"), key("Game (USA) (Disc 2)"));
        assert_eq!(key("Game (USA) (Disc 1)"), key("Game (Japan) (Disc 1)"));
    }
}
