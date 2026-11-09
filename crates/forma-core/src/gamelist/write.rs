//! Writing favourites and play stats back to `gamelist.xml`, for EmulationStation and other
//! tools that read it (off unless the user turns it on: see
//! [`crate::library::GAMELIST_WRITE_BACK_SETTING`]).
//!
//! The file belongs to ES and scrapers, so Forma edits it in place: only `<favorite>`,
//! `<playcount>` and `<lastplayed>` of the game's entry change, and every other byte (other
//! tags, attributes, comments, layout) stays as it was. A file that does not parse is left
//! alone rather than rewritten from what could be read. The new file replaces the old one in a
//! single rename, so a crash never leaves half a gamelist.

use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;

use super::{relative_to, resolve};
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::xml;

/// What Forma knows about a game that ES keeps in its gamelist.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayStats<'a> {
    /// Written only when the game has no entry yet, as ES needs a name.
    pub name: &'a str,
    pub favorite: bool,
    pub play_count: i64,
    /// As the library stores it: `YYYY-MM-DDTHH:MM:SS`.
    pub last_played: Option<&'a str>,
}

/// What to do with one tag of a game's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    /// Write this value (escaped for XML).
    Set(String),
    /// Take the tag out.
    Remove,
}

/// Updates the entry for `rom` (relative to `system_dir`) in `system_dir/gamelist.xml`,
/// adding the entry, or the file, if there is none. Returns whether the file changed: when it
/// already says the same, it is not touched, so the watcher sees nothing.
pub fn write_back(
    system_dir: &Path,
    paths: &Paths,
    rom: &Path,
    stats: &PlayStats<'_>,
) -> Result<bool> {
    set_tags(system_dir, paths, rom, stats.name, &stats_tags(stats))
}

/// The tags ES keeps for play stats, in the order it writes them.
fn stats_tags(stats: &PlayStats<'_>) -> Vec<(&'static str, Tag)> {
    let mut tags = vec![(
        "favorite",
        if stats.favorite {
            Tag::Set("true".into())
        } else {
            // ES writes nothing for false, so the tag goes.
            Tag::Remove
        },
    )];
    if stats.play_count > 0 {
        tags.push(("playcount", Tag::Set(stats.play_count.to_string())));
    }
    if let Some(at) = stats.last_played {
        tags.push(("lastplayed", Tag::Set(es_date_time(at))));
    }
    tags
}

/// Sets (or removes) `tags` in the entry for `rom`, leaving every other byte of the file as it
/// was; adds the entry, named `name`, if there is none. Returns whether the file changed.
pub fn set_tags(
    system_dir: &Path,
    paths: &Paths,
    rom: &Path,
    name: &str,
    tags: &[(&str, Tag)],
) -> Result<bool> {
    let file = system_dir.join("gamelist.xml");
    let old = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            "<?xml version=\"1.0\"?>\n<gameList>\n</gameList>\n".to_owned()
        }
        Err(e) => return Err(Error::io(&file, e)),
    };
    let new =
        updated(&old, system_dir, paths, rom, name, tags).map_err(|message| Error::Parse {
            path: file.clone(),
            message: format!("{message}; not written"),
        })?;
    if new == old && file.exists() {
        return Ok(false);
    }
    let temp = system_dir.join(".gamelist.xml.forma.tmp");
    std::fs::write(&temp, &new).map_err(|e| Error::io(&temp, e))?;
    std::fs::rename(&temp, &file).map_err(|e| Error::io(&file, e))?;
    Ok(true)
}

/// Takes `rom`'s entries out of `system_dir/gamelist.xml`, leaving every other byte as it was.
/// Returns whether the file changed.
pub fn remove_game(system_dir: &Path, paths: &Paths, rom: &Path) -> Result<bool> {
    let file = system_dir.join("gamelist.xml");
    let old = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io(&file, e)),
    };
    let (games, _) = spans(&old).map_err(|message| Error::Parse {
        path: file.clone(),
        message: format!("{message}; not written"),
    })?;
    let mut new = old.clone();
    for game in games.iter().rev() {
        if game_path(&old, game, system_dir, paths).as_deref() != Some(rom) {
            continue;
        }
        let Some(start) = old[..game.open_end].rfind("<game") else {
            continue;
        };
        let end = game.close + "</game>".len();
        new.replace_range(trim_end_at(&old, start)..end, "");
    }
    if new == old {
        return Ok(false);
    }
    let temp = system_dir.join(".gamelist.xml.forma.tmp");
    std::fs::write(&temp, &new).map_err(|e| Error::io(&temp, e))?;
    std::fs::rename(&temp, &file).map_err(|e| Error::io(&file, e))?;
    Ok(true)
}

/// A direct child of a `<game>`: its tag and its bytes, start and end tags included.
struct Child {
    name: String,
    start: usize,
    end: usize,
}

struct GameSpan {
    /// Just after `<game …>`.
    open_end: usize,
    /// Where `</game>` starts.
    close: usize,
    children: Vec<Child>,
}

fn updated(
    xml_text: &str,
    system_dir: &Path,
    paths: &Paths,
    rom: &Path,
    name: &str,
    tags: &[(&str, Tag)],
) -> std::result::Result<String, String> {
    let (games, root_close) = spans(xml_text)?;

    // (start, end, replacement), applied from the end so earlier offsets stay valid.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut found = false;
    for game in &games {
        if game_path(xml_text, game, system_dir, paths).as_deref() != Some(rom) {
            continue;
        }
        found = true;
        let indent = child_indent(xml_text, game);
        for (tag, what) in tags {
            let existing: Vec<&Child> = game.children.iter().filter(|c| c.name == *tag).collect();
            match (what, existing.first()) {
                (Tag::Set(value), Some(child)) => {
                    let element = format!("<{tag}>{}</{tag}>", escape(value.as_str()));
                    if xml_text[child.start..child.end] != element {
                        edits.push((child.start, child.end, element));
                    }
                }
                (Tag::Set(value), None) => {
                    let at = trim_end_at(xml_text, game.close);
                    edits.push((
                        at,
                        at,
                        format!("{indent}<{tag}>{}</{tag}>", escape(value.as_str())),
                    ));
                }
                (Tag::Remove, Some(child)) => {
                    edits.push((trim_end_at(xml_text, child.start), child.end, String::new()));
                }
                (Tag::Remove, None) => {}
            }
            // A second copy of the tag would contradict the first: drop it.
            for extra in existing.iter().skip(1) {
                edits.push((trim_end_at(xml_text, extra.start), extra.end, String::new()));
            }
        }
    }
    if !found {
        let mut entry = format!(
            "\n\t<game>\n\t\t<path>./{}</path>",
            escape(rom.to_string_lossy())
        );
        if !tags.iter().any(|(t, _)| *t == "name") {
            entry.push_str(&format!("\n\t\t<name>{}</name>", escape(name)));
        }
        for (tag, what) in tags {
            if let Tag::Set(value) = what {
                entry.push_str(&format!("\n\t\t<{tag}>{}</{tag}>", escape(value.as_str())));
            }
        }
        entry.push_str("\n\t</game>");
        let at = trim_end_at(xml_text, root_close);
        edits.push((at, at, entry));
    }

    // Last first; at one spot, the later edit first, so inserts there keep the order above.
    let mut ordered: Vec<_> = edits.into_iter().enumerate().collect();
    ordered.sort_by(|(i, a), (j, b)| b.0.cmp(&a.0).then(j.cmp(i)));
    let mut out = xml_text.to_owned();
    for (_, (start, end, text)) in ordered {
        out.replace_range(start..end, &text);
    }
    Ok(out)
}

/// Every `<game>` under the document element, and where the document element closes. Any
/// error, including a file cut short, is returned: such a file is never rewritten.
fn spans(xml_text: &str) -> std::result::Result<(Vec<GameSpan>, usize), String> {
    // The reader passes over a byte-order mark without counting it, and every offset here is
    // into the whole file: read after the mark, and add it back.
    let body = xml_text.strip_prefix('\u{feff}').unwrap_or(xml_text);
    let mark = xml_text.len() - body.len();
    let mut reader = Reader::from_str(body);
    let mut depth = 0usize;
    let mut games = Vec::new();
    let mut game: Option<GameSpan> = None;
    let mut child: Option<(String, usize)> = None;
    let mut root_close = None;
    loop {
        let before = mark + offset(reader.buffer_position());
        let event = reader
            .read_event()
            .map_err(|e| format!("{e} at byte {}", reader.error_position()))?;
        let after = mark + offset(reader.buffer_position());
        match event {
            Event::Start(e) => {
                let name = e.name().as_ref().to_owned();
                match depth {
                    1 if name == "game" => {
                        game = Some(GameSpan {
                            open_end: after,
                            close: after,
                            children: Vec::new(),
                        });
                    }
                    2 if game.is_some() => child = Some((name, before)),
                    _ => {}
                }
                depth += 1;
            }
            Event::Empty(e) => {
                if depth == 2
                    && let Some(g) = game.as_mut()
                {
                    g.children.push(Child {
                        name: e.name().as_ref().to_owned(),
                        start: before,
                        end: after,
                    });
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or("an end tag with no start")?;
                match depth {
                    2 => {
                        if let (Some(g), Some((name, start))) = (game.as_mut(), child.take()) {
                            g.children.push(Child {
                                name,
                                start,
                                end: after,
                            });
                        }
                    }
                    1 => {
                        if let Some(mut g) = game.take() {
                            g.close = before;
                            games.push(g);
                        }
                    }
                    0 => root_close = Some(before),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err("file ends inside an element".to_owned());
    }
    let root_close = root_close.ok_or("no document element")?;
    Ok((games, root_close))
}

fn offset(position: u64) -> usize {
    usize::try_from(position).unwrap_or(usize::MAX)
}

/// The game's ROM relative to the system folder, read the way the importer reads it.
fn game_path(xml_text: &str, game: &GameSpan, system_dir: &Path, paths: &Paths) -> Option<PathBuf> {
    let child = game.children.iter().find(|c| c.name == "path")?;
    let parsed = xml::parse(&xml_text[child.start..child.end]).root?;
    let raw = parsed.text.trim();
    if raw.is_empty() {
        return None;
    }
    relative_to(system_dir, &resolve(raw, system_dir, paths))
}

/// The whitespace before the game's first child, so an added tag lines up with the others.
fn child_indent<'a>(xml_text: &'a str, game: &GameSpan) -> &'a str {
    game.children
        .first()
        .map(|c| &xml_text[game.open_end..c.start])
        .filter(|ws| !ws.is_empty() && ws.chars().all(char::is_whitespace))
        .unwrap_or("\n\t\t")
}

/// `at`, moved back over the whitespace before it.
fn trim_end_at(xml_text: &str, at: usize) -> usize {
    xml_text[..at].trim_end().len()
}

/// `2026-01-01T12:00:00` to ES's `20260101T120000`.
fn es_date_time(iso: &str) -> String {
    iso.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_paths;

    /// The fixture tree stands in for `/userdata`, so absolute gamelist paths rebase onto it.
    fn dir() -> PathBuf {
        fixture_paths().roms().join("nes")
    }

    fn stats(favorite: bool, plays: i64, last: Option<&'static str>) -> PlayStats<'static> {
        PlayStats {
            name: "Game & Co",
            favorite,
            play_count: plays,
            last_played: last,
        }
    }

    fn run(xml_text: &str, rom: &str, s: &PlayStats<'_>) -> String {
        updated(
            xml_text,
            &dir(),
            &fixture_paths(),
            Path::new(rom),
            s.name,
            &stats_tags(s),
        )
        .unwrap()
    }

    const LIST: &str = "<?xml version=\"1.0\"?>\n<gameList>\n\t<game id=\"7\" source=\"ss\">\n\
        \t\t<path>./a.nes</path>\n\t\t<name>A</name>\n\t\t<!-- kept -->\n\
        \t\t<playcount>2</playcount>\n\t\t<unknown x=\"1\">kept</unknown>\n\t</game>\n\
        \t<game>\n\t\t<path>./b.nes</path>\n\t\t<favorite>true</favorite>\n\t</game>\n</gameList>\n";

    #[test]
    fn changes_only_the_three_tags_of_that_game() {
        let out = run(LIST, "a.nes", &stats(true, 3, Some("2026-11-04T10:20:30")));
        let expected = LIST
            .replace("<playcount>2</playcount>", "<playcount>3</playcount>")
            .replace(
                "kept</unknown>\n\t</game>",
                "kept</unknown>\n\t\t<favorite>true</favorite>\
                 \n\t\t<lastplayed>20261104T102030</lastplayed>\n\t</game>",
            );
        assert_eq!(out, expected);
    }

    #[test]
    fn unfavouriting_removes_the_tag_and_its_line() {
        let out = run(LIST, "b.nes", &stats(false, 0, None));
        assert!(out.contains("<path>./b.nes</path>\n\t</game>"), "{out}");
        assert_eq!(out.matches("<favorite>").count(), 0);
    }

    #[test]
    fn a_byte_order_mark_stays_and_moves_nothing() {
        let marked = format!("\u{feff}{LIST}");
        // A game with no entry yet, added after the last one; and one that has an entry.
        for (rom, count) in [("sub/c.nes", 1), ("b.nes", 0)] {
            let out = run(&marked, rom, &stats(true, 1, None));
            assert!(out.starts_with('\u{feff}'), "{out}");
            assert!(crate::xml::parse(&out).error.is_none(), "{out}");
            assert_eq!(
                out.matches("</game>").count(),
                LIST.matches("</game>").count() + count
            );
            assert!(out.contains("<favorite>true</favorite>"), "{out}");
        }

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gamelist.xml");
        std::fs::write(&file, &marked).unwrap();
        assert!(remove_game(dir.path(), &fixture_paths(), Path::new("b.nes")).unwrap());
        let out = std::fs::read_to_string(&file).unwrap();
        assert!(crate::xml::parse(&out).error.is_none(), "{out}");
        assert!(!out.contains("b.nes"), "{out}");
    }

    #[test]
    fn the_same_values_change_nothing() {
        assert_eq!(run(LIST, "b.nes", &stats(true, 0, None)), LIST);
    }

    #[test]
    fn a_game_without_an_entry_gets_one() {
        let out = run(
            LIST,
            "sub/c & d.nes",
            &stats(true, 1, Some("2026-01-02T03:04:05")),
        );
        assert!(out.starts_with(&LIST[..LIST.len() - "</gameList>\n".len() - 1]));
        assert!(out.ends_with(
            "\t</game>\n\t<game>\n\t\t<path>./sub/c &amp; d.nes</path>\n\t\t<name>Game &amp; Co</name>\
             \n\t\t<favorite>true</favorite>\n\t\t<playcount>1</playcount>\
             \n\t\t<lastplayed>20260102T030405</lastplayed>\n\t</game>\n</gameList>\n"
        ));
        assert!(xml::parse(&out).error.is_none());
    }

    #[test]
    fn matches_absolute_and_bare_paths_and_entities() {
        let list = "<gameList><game><path>/userdata/roms/nes/Tom &amp; Jerry.nes</path>\
                    </game></gameList>";
        let out = run(list, "Tom & Jerry.nes", &stats(false, 5, None));
        assert!(
            out.contains("Jerry.nes</path>\n\t\t<playcount>5</playcount></game>"),
            "{out}"
        );
        assert_eq!(out.matches("<game>").count(), 1);
    }

    #[test]
    fn a_truncated_file_is_refused() {
        let cut = &LIST[..LIST.len() - 20];
        let err = updated(
            cut,
            &dir(),
            &fixture_paths(),
            Path::new("a.nes"),
            "A",
            &stats_tags(&stats(true, 1, None)),
        );
        assert!(err.is_err());
    }

    #[test]
    fn writes_through_a_rename_and_creates_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = fixture_paths();
        let s = stats(true, 1, None);
        assert!(write_back(dir.path(), &paths, Path::new("x.nes"), &s).unwrap());
        let text = std::fs::read_to_string(dir.path().join("gamelist.xml")).unwrap();
        assert!(text.contains("<path>./x.nes</path>"));
        assert!(!write_back(dir.path(), &paths, Path::new("x.nes"), &s).unwrap());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, ["gamelist.xml"]);
    }
}
