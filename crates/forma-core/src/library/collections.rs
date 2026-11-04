//! The user's own lists of games, kept as EmulationStation keeps its custom collections: one
//! `custom-<name>.cfg` per collection in `configs/emulationstation/collections`, a game's full
//! path per line. So collections made in EmulationStation carry over, and those made in Forma
//! show in EmulationStation. The files are the truth: Forma reads them on every refresh and
//! writes them on every change, and the `collection` tables index them for browsing.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::Library;
use crate::error::{Error, Result};
use crate::paths::Paths;

const PREFIX: &str = "custom-";
const SUFFIX: &str = ".cfg";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub name: String,
    /// Games in it that are on the box now (a missing one keeps its place in the file).
    pub games: i64,
}

impl Library {
    /// Reads every collection file again; the index then says what the files say.
    pub fn load_collections(&mut self, paths: &Paths) -> Result<()> {
        let mut found: Vec<(String, Vec<String>)> = Vec::new();
        if let Ok(dir) = std::fs::read_dir(paths.collections_dir()) {
            for entry in dir.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(name) = file
                    .strip_prefix(PREFIX)
                    .and_then(|f| f.strip_suffix(SUFFIX))
                    .filter(|n| checked(n).is_ok())
                else {
                    continue;
                };
                let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                found.push((name.to_owned(), lines(&text)));
            }
        }
        found.sort_by_key(|(name, _)| name.to_lowercase());
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM collection", [])?;
        {
            let mut add =
                tx.prepare_cached("INSERT OR IGNORE INTO collection (name) VALUES (?1)")?;
            let mut game = tx.prepare_cached(
                "INSERT OR IGNORE INTO collection_game (collection, path) VALUES (?1, ?2)",
            )?;
            for (name, games) in &found {
                add.execute([name])?;
                for path in games {
                    game.execute([name, path])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every collection, by name, with how many of its games are here.
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT c.name,
                    (SELECT COUNT(*) FROM collection_game cg
                     JOIN game g ON g.present = 1
                     JOIN system s ON s.name = g.system AND s.rom_path || '/' || g.path = cg.path
                     WHERE cg.collection = c.name)
             FROM collection c ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Collection {
                name: r.get(0)?,
                games: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The collections a game is in.
    pub fn collections_of(&self, game_id: i64) -> Result<Vec<String>> {
        let path = self.full_path(game_id)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT collection FROM collection_game WHERE path = ?1
             ORDER BY collection COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([path], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A new, empty collection. Its name must be free.
    pub fn create_collection(&mut self, paths: &Paths, name: &str) -> Result<String> {
        let name = checked(name)?.to_owned();
        if self.collection_name(&name)?.is_some() {
            return Err(Error::Invalid(format!(
                "there is already a collection called {name}"
            )));
        }
        write(paths, &name, &[])?;
        self.load_collections(paths)?;
        Ok(name)
    }

    /// Deletes a collection: its file goes, its games stay where they are.
    pub fn delete_collection(&mut self, paths: &Paths, name: &str) -> Result<()> {
        let name = self.existing(name)?;
        let file = file(paths, &name);
        std::fs::remove_file(&file).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(Error::io(&file, e)),
        })?;
        self.load_collections(paths)
    }

    /// Puts a game in a collection, or takes it out.
    pub fn set_in_collection(
        &mut self,
        paths: &Paths,
        name: &str,
        game_id: i64,
        member: bool,
    ) -> Result<()> {
        let name = self.existing(name)?;
        let path = self.full_path(game_id)?;
        let mut games: Vec<String> =
            lines(&std::fs::read_to_string(file(paths, &name)).unwrap_or_default());
        let has = games.contains(&path);
        if member == has {
            return Ok(());
        }
        if member {
            games.push(path);
        } else {
            games.retain(|g| *g != path);
        }
        write(paths, &name, &games)?;
        self.load_collections(paths)
    }

    /// The name as stored (its own case), when the collection exists.
    fn collection_name(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT name FROM collection WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .optional()?)
    }

    fn existing(&self, name: &str) -> Result<String> {
        self.collection_name(name)?
            .ok_or_else(|| Error::NotFound(format!("no collection called {name}")))
    }

    /// The game's file as EmulationStation writes it: its system's folder and its path.
    fn full_path(&self, game_id: i64) -> Result<String> {
        self.conn
            .query_row(
                "SELECT s.rom_path || '/' || g.path FROM game g JOIN system s ON s.name = g.system
                 WHERE g.id = ?1",
                params![game_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("no game with id {game_id}")))
    }
}

/// A name EmulationStation can keep as a file name: not empty, not too long, no path in it.
fn checked(name: &str) -> Result<&str> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.chars().count() <= 64
        && !name.starts_with('.')
        && !name.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        });
    if ok {
        Ok(name)
    } else {
        Err(Error::Invalid(format!(
            "`{name}` cannot be a collection's name: up to 64 letters, without / \\ : * ? \" < > |"
        )))
    }
}

fn lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !out.iter().any(|o| o == line) {
            out.push(line.to_owned());
        }
    }
    out
}

fn file(paths: &Paths, name: &str) -> PathBuf {
    paths
        .collections_dir()
        .join(format!("{PREFIX}{name}{SUFFIX}"))
}

/// Writes a collection's file whole, through a temporary file, so a crash leaves the old one.
fn write(paths: &Paths, name: &str, games: &[String]) -> Result<()> {
    let dir = paths.collections_dir();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let target = file(paths, name);
    let temp = dir.join(format!(".{PREFIX}{name}{SUFFIX}.tmp"));
    let mut text = games.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    std::fs::write(&temp, text).map_err(|e| Error::io(&temp, e))?;
    std::fs::rename(&temp, &target).map_err(|e| Error::io(&target, e))?;
    Ok(())
}
