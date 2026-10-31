//! The library database: Forma's primary store.
//!
//! Batocera's files and external imports feed it; the UI reads only from it. Metadata is kept
//! per source in `game_field`, and the winning value is materialised onto the `game` row, so a
//! re-import from `gamelist.xml` can never overwrite what the user set in Forma.

mod imports;
mod refresh;
mod schema;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::field;
use crate::systems::System;

pub use refresh::RefreshReport;

/// Where a field value came from. A higher variant wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Gamelist = 1,
    Import = 2,
    User = 3,
}

pub struct Library {
    pub(crate) conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSummary {
    pub name: String,
    pub fullname: String,
    pub manufacturer: Option<String>,
    pub release_year: Option<i64>,
    pub hardware: Option<String>,
    /// Visible games: present on disk and not hidden.
    pub game_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub system: String,
    pub path: String,
    pub name: String,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub players: Option<String>,
    pub release_date: Option<String>,
    pub rating: Option<f64>,
    pub favorite: bool,
    pub hidden: bool,
    pub play_count: i64,
    pub last_played: Option<String>,
    /// Total seconds played.
    pub game_time: i64,
    pub ra_game_id: Option<i64>,
    /// Media kind (`image`, `video`, `marquee`…) to absolute path.
    pub media: BTreeMap<String, String>,
}

pub(crate) const GAME_COLUMNS: &str = "g.id, g.system, g.path, g.name, g.description, g.developer, \
     g.publisher, g.genre, g.players, g.release_date, g.rating, g.favorite, g.hidden, \
     g.play_count, g.last_played, g.ra_game_id, g.game_time";

/// Systems with visible games, and how many; counted from the `game_visible` index alone.
const SYSTEMS_SQL: &str =
    "SELECT s.name, s.fullname, s.manufacturer, s.release_year, s.hardware, COUNT(g.id)
     FROM system s JOIN game g ON g.system = s.name AND g.present = 1 AND g.hidden = 0
     GROUP BY s.name ORDER BY s.sort_order";

impl Library {
    /// Opens (creating if needed) the database at `path` and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        Self::init(Connection::open(path)?, path)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, Path::new(":memory:"))
    }

    fn init(mut conn: Connection, path: &Path) -> Result<Self> {
        schema::configure(&conn)?;
        schema::migrate(&mut conn, path)?;
        Ok(Self { conn })
    }

    /// Systems with at least one visible game, in definition order.
    pub fn systems(&self) -> Result<Vec<SystemSummary>> {
        let mut stmt = self.conn.prepare_cached(SYSTEMS_SQL)?;
        let rows = stmt.query_map([], |r| {
            Ok(SystemSummary {
                name: r.get(0)?,
                fullname: r.get(1)?,
                manufacturer: r.get(2)?,
                release_year: r.get(3)?,
                hardware: r.get(4)?,
                game_count: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Visible games of one system, by name.
    pub fn games(&self, system: &str) -> Result<Vec<Game>> {
        self.query_games(
            &format!(
                "SELECT {GAME_COLUMNS} FROM game g
                 WHERE g.system = ?1 AND g.present = 1 AND g.hidden = 0
                 ORDER BY g.name COLLATE NOCASE"
            ),
            params![system],
        )
    }

    pub fn game(&self, id: i64) -> Result<Option<Game>> {
        let games = self.query_games(
            &format!("SELECT {GAME_COLUMNS} FROM game g WHERE g.id = ?1"),
            params![id],
        )?;
        Ok(games.into_iter().next())
    }

    /// Full-text search over name, description, developer and publisher. Every word matches
    /// as a prefix, so `super mar` finds Super Mario Bros.; accents are ignored.
    pub fn search(&self, text: &str, limit: usize) -> Result<Vec<Game>> {
        let query: Vec<String> = text
            .split_whitespace()
            .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
            .collect();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        self.query_games(
            &format!(
                "SELECT {GAME_COLUMNS} FROM game_fts f JOIN game g ON g.id = f.rowid
                 WHERE game_fts MATCH ?1 AND g.present = 1 AND g.hidden = 0
                 ORDER BY f.rank LIMIT ?2"
            ),
            params![query.join(" "), i64::try_from(limit).unwrap_or(i64::MAX)],
        )
    }

    /// Records the user's choice, which outranks anything a gamelist or import says.
    pub fn set_favorite(&mut self, id: i64, favorite: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO game_field (game_id, field, source, value) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (game_id, field, source) DO UPDATE SET value = excluded.value",
            params![
                id,
                field::FAVORITE,
                Source::User as i64,
                if favorite { "1" } else { "0" }
            ],
        )?;
        materialise(&tx, &[id], &[])?;
        tx.commit()?;
        Ok(())
    }

    /// A game and its system, for launching. Includes games that are hidden or whose file is
    /// missing: the launcher reports the missing file itself, more clearly than "not found".
    pub fn launch_target(&self, id: i64) -> Result<Option<(Game, System)>> {
        let Some(game) = self.game(id)? else {
            return Ok(None);
        };
        let definition: String = self.conn.query_row(
            "SELECT definition FROM system WHERE name = ?1",
            [&game.system],
            |r| r.get(0),
        )?;
        let system = serde_json::from_str(&definition).map_err(|e| Error::Parse {
            path: Path::new("library.db").to_path_buf(),
            message: format!("system `{}` definition is unreadable: {e}", game.system),
        })?;
        Ok(Some((game, system)))
    }

    /// Adds one play of `played` length ending at `ended`. Stored as the user's own data, so a
    /// gamelist re-import cannot reset it.
    pub fn record_play(&mut self, id: i64, ended: SystemTime, played: Duration) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (count, time): (i64, i64) = tx.query_row(
            "SELECT play_count, game_time FROM game WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let seconds = i64::try_from(played.as_secs()).unwrap_or(i64::MAX);
        let mut set = tx.prepare_cached(
            "INSERT INTO game_field (game_id, field, source, value) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (game_id, field, source) DO UPDATE SET value = excluded.value",
        )?;
        let user = Source::User as i64;
        set.execute(params![
            id,
            field::PLAY_COUNT,
            user,
            (count + 1).to_string()
        ])?;
        set.execute(params![
            id,
            field::LAST_PLAYED,
            user,
            crate::time::iso_utc(ended)
        ])?;
        set.execute(params![
            id,
            field::GAME_TIME,
            user,
            time.saturating_add(seconds).to_string()
        ])?;
        drop(set);
        materialise(&tx, &[id], &[])?;
        tx.commit()?;
        Ok(())
    }

    /// A Forma setting, e.g. [`crate::hierarchy::DEFAULT_VIEW_SETTING`].
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// A setting as text, whether it was stored raw or as a JSON string (as the SDK stores
    /// every value). `views.default` set from the UI and from a theme read the same.
    pub fn setting_text(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .setting(key)?
            .map(|raw| match serde_json::from_str::<String>(&raw) {
                Ok(text) => text,
                Err(_) => raw,
            }))
    }

    /// Sets a Forma setting; `None` clears it.
    pub fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(value) => self.conn.execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?,
            None => self
                .conn
                .execute("DELETE FROM setting WHERE key = ?1", [key])?,
        };
        Ok(())
    }

    pub(crate) fn query_games(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut games: Vec<Game> = stmt
            .query_map(params, |r| {
                Ok(Game {
                    id: r.get(0)?,
                    system: r.get(1)?,
                    path: r.get(2)?,
                    name: r.get(3)?,
                    description: r.get(4)?,
                    developer: r.get(5)?,
                    publisher: r.get(6)?,
                    genre: r.get(7)?,
                    players: r.get(8)?,
                    release_date: r.get(9)?,
                    rating: r.get(10)?,
                    favorite: r.get(11)?,
                    hidden: r.get(12)?,
                    play_count: r.get(13)?,
                    last_played: r.get(14)?,
                    ra_game_id: r.get(15)?,
                    game_time: r.get(16)?,
                    media: BTreeMap::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut media = self
            .conn
            .prepare_cached("SELECT kind, path FROM media WHERE game_id = ?1")?;
        for game in &mut games {
            game.media = media
                .query_map([game.id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(games)
    }
}

/// Recomputes the `game` columns and `media` rows of `ids` from their fields, highest source
/// first. Called after any change to `game_field`. `fresh` are games inserted in this
/// transaction, which are not in the search index yet.
fn materialise(tx: &Transaction<'_>, ids: &[i64], fresh: &[i64]) -> Result<()> {
    let fresh: HashSet<i64> = fresh.iter().copied().collect();
    let mut fields = tx.prepare_cached(
        "SELECT field, value FROM game_field WHERE game_id = ?1 ORDER BY source DESC",
    )?;
    let mut current = tx.prepare_cached(
        "SELECT path, name, description, developer, publisher FROM game WHERE id = ?1",
    )?;
    let mut unindex = tx.prepare_cached("DELETE FROM game_fts WHERE rowid = ?1")?;
    let mut index = tx.prepare_cached(
        "INSERT INTO game_fts (rowid, name, description, developer, publisher)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut update = tx.prepare_cached(
        "UPDATE game SET name = ?2, description = ?3, developer = ?4, publisher = ?5, genre = ?6,
             players = ?7, players_max = ?8, release_date = ?9, release_year = ?10, rating = ?11,
             region = ?12, lang = ?13, hidden = ?14, favorite = ?15, play_count = ?16,
             last_played = ?17, ra_game_id = ?18, ra_hash = ?19, game_time = ?20
         WHERE id = ?1",
    )?;
    let mut clear_media = tx.prepare_cached("DELETE FROM media WHERE game_id = ?1")?;
    let mut add_media =
        tx.prepare_cached("INSERT INTO media (game_id, kind, path) VALUES (?1, ?2, ?3)")?;

    for &id in ids {
        let mut won: BTreeMap<String, String> = BTreeMap::new();
        for row in fields.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))? {
            let (key, value) = row?;
            won.entry(key).or_insert(value);
        }
        let Some((path, old_text)) = current
            .query_row([id], |r| {
                let text: Searchable = (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
                Ok((r.get::<_, String>(0)?, text))
            })
            .optional()?
        else {
            continue;
        };

        let get = |key: &str| won.get(key).cloned();
        let flag = |key: &str| won.get(key).is_some_and(|v| v == "1");
        let name = get(field::NAME).unwrap_or_else(|| default_name(&path));
        let players = get(field::PLAYERS);
        let max_players = players.as_deref().and_then(players_max);
        let release_date = get(field::RELEASE_DATE);
        let release_year = release_date
            .as_deref()
            .and_then(|d| d.get(..4)?.parse::<i64>().ok());

        let text: Searchable = (
            name,
            get(field::DESCRIPTION),
            get(field::DEVELOPER),
            get(field::PUBLISHER),
        );
        if fresh.contains(&id) || text != old_text {
            if !fresh.contains(&id) {
                unindex.execute([id])?;
            }
            index.execute(params![id, text.0, text.1, text.2, text.3])?;
        }

        update.execute(params![
            id,
            text.0,
            text.1,
            text.2,
            text.3,
            get(field::GENRE),
            players,
            max_players,
            release_date,
            release_year,
            get(field::RATING).and_then(|r| r.parse::<f64>().ok()),
            get(field::REGION),
            get(field::LANG),
            flag(field::HIDDEN),
            flag(field::FAVORITE),
            get(field::PLAY_COUNT)
                .and_then(|n| n.parse::<i64>().ok())
                .unwrap_or(0),
            get(field::LAST_PLAYED),
            get(field::RA_GAME_ID).and_then(|n| n.parse::<i64>().ok()),
            get(field::RA_HASH),
            get(field::GAME_TIME)
                .and_then(|n| n.parse::<i64>().ok())
                .unwrap_or(0),
        ])?;

        clear_media.execute([id])?;
        for (key, value) in &won {
            if let Some(kind) = key.strip_prefix(field::MEDIA_PREFIX) {
                add_media.execute(params![id, kind, value])?;
            }
        }
    }
    Ok(())
}

/// The columns the search index covers: name, description, developer, publisher.
type Searchable = (String, Option<String>, Option<String>, Option<String>);

/// What EmulationStation shows for a game with no metadata: the file name without extension.
fn default_name(path: &str) -> String {
    let file = Path::new(path);
    file.file_stem()
        .unwrap_or(file.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The highest number in a players field: `1-2` → 2, `1` → 1, `2+` → 2.
fn players_max(players: &str) -> Option<i64> {
    players
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse::<i64>().ok())
        .max()
}

#[cfg(test)]
mod tests;
