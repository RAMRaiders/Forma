//! The library database: Forma's primary store.
//!
//! Batocera's files and external imports feed it; the UI reads only from it. Metadata is kept
//! per source in `game_field`, and the winning value is materialised onto the `game` row, so a
//! re-import from `gamelist.xml` can never overwrite what the user set in Forma.

mod collections;
mod computed;
mod dumps;
mod imports;
mod names;
mod recommend;
mod refresh;
mod schema;
mod versions;

pub use dumps::DumpReport;
pub use names::{DEFAULT_PREFIXES, NAME_STYLE_SETTING, NameRule, NameStyle};
pub use versions::{GameVersion, VersionGroup};

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::field;
use crate::paths::Paths;
use crate::systems::System;

pub use collections::Collection;
pub use computed::RaMatch;
pub use refresh::RefreshReport;

/// Where a field value came from. A higher variant wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// Worked out by Forma: RetroAchievements hashes and matches.
    Forma = 0,
    Gamelist = 1,
    Import = 2,
    User = 3,
}

pub struct Library {
    pub(crate) conn: Connection,
    /// The last recommendations, until the library changes (see `recommend.rs`).
    pub(crate) recommended: std::sync::Mutex<Option<recommend::Cached>>,
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
    /// Its name as shown and sorted: in the user's [`NameRule`].
    pub name: String,
    /// Its name as the gamelist, the scraper or the user gave it, before the [`NameRule`]: what
    /// the scraper searches with and writes back.
    #[serde(skip)]
    pub given_name: String,
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
    /// As the gamelist gives it, e.g. `us`, `eu`, `jp`, `wor`.
    pub region: Option<String>,
    /// Languages, as the gamelist gives them, e.g. `en,fr,de`.
    pub lang: Option<String>,
    /// Age ratings, `BOARD:RATING` comma-separated, e.g. `ESRB:M,PEGI:18`.
    pub age_rating: Option<String>,
    /// On the list of the games its system is remembered for (see [`crate::iconic`]).
    pub iconic: bool,
    /// Media kind (`image`, `video`, `marquee`…) to absolute path.
    pub media: BTreeMap<String, String>,
    /// Whether its file is a catalogued dump; `None` while unknown (not checked yet, or no
    /// catalogue covers its system). A playlist's, its discs' together.
    pub dump: Option<Dump>,
    /// A playlist's discs, in its order, relative to the system's folder (as `path` is); empty
    /// for any other game.
    pub discs: Vec<String>,
}

/// What the No-Intro or Redump DATs say of a game's file (see [`crate::dumps`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dump {
    /// `verified`: size, CRC32 and SHA1 as the DAT lists them. `crcOnly`: size and CRC32,
    /// the DAT having no SHA1. `badDump`: a dump the DAT marks bad. `notCatalogued`: in no
    /// DAT of its system (a hack, a translation, homebrew, a changed or bad file).
    pub status: String,
    /// `no-intro`, `redump` or `tosec`; `None` when not catalogued.
    pub catalogue: Option<String>,
    /// The DAT's name for it, e.g. `Super Mario Bros. (World)`.
    pub name: Option<String>,
    pub region: Option<String>,
}

pub(crate) const GAME_COLUMNS: &str = "g.id, g.system, g.path, g.name, g.description, g.developer, \
     g.publisher, g.genre, g.players, g.release_date, g.rating, g.favorite, g.hidden, \
     g.play_count, g.last_played, g.ra_game_id, g.game_time, g.region, g.lang, g.age_rating, \
     COALESCE((SELECT f.value FROM game_field f WHERE f.game_id = g.id AND f.field = 'name' \
               ORDER BY f.source DESC LIMIT 1), g.name)";

/// A game's id, name and file: all a whole system's list needs when nothing else is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFile {
    pub id: i64,
    pub name: String,
    pub path: String,
}

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
        Ok(Self {
            conn,
            recommended: std::sync::Mutex::default(),
        })
    }

    /// Systems with at least one visible game, in the order the user set
    /// ([`SYSTEMS_ORDER_SETTING`]): by maker, then year, unless they chose otherwise.
    pub fn systems_in_order(&self) -> Result<Vec<SystemSummary>> {
        let mut systems = self.systems()?;
        let order = self.setting_text(SYSTEMS_ORDER_SETTING)?;
        sort_systems(&mut systems, SystemsOrder::parse(order.as_deref()));
        Ok(systems)
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

    /// [`Library::games`], in the same order, with only their ids, names and files: one query,
    /// none per game for its media and dump, so about ten times quicker on a big system.
    pub fn game_files(&self, system: &str) -> Result<Vec<GameFile>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT g.id, g.name, g.path FROM game g
             WHERE g.system = ?1 AND g.present = 1 AND g.hidden = 0
             ORDER BY g.name COLLATE NOCASE",
        )?;
        let files = stmt
            .query_map(params![system], |r| {
                Ok(GameFile {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(files)
    }

    pub fn game(&self, id: i64) -> Result<Option<Game>> {
        let games = self.query_games(
            &format!("SELECT {GAME_COLUMNS} FROM game g WHERE g.id = ?1"),
            params![id],
        )?;
        Ok(games.into_iter().next())
    }

    /// The file of one kind of a game's media (`image`, `video`…), if it has one: one row by
    /// its key, for the `forma://` protocol, which asks for every picture a theme shows.
    pub fn media_file(&self, game_id: i64, kind: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT path FROM media WHERE game_id = ?1 AND kind = ?2")?
            .query_row(params![game_id, kind], |r| r.get(0))
            .optional()?)
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

    /// Up to `limit` visible games in a random order, each with at least one of the `media`
    /// kinds (any game when it is empty); only favourites, or one system's, when asked. The
    /// screensaver's slideshow and videos come from here.
    pub fn random_games(
        &self,
        media: &[&str],
        limit: usize,
        favorites_only: bool,
        system: Option<&str>,
    ) -> Result<Vec<Game>> {
        let kinds = serde_json::to_string(media).unwrap_or_else(|_| "[]".into());
        self.query_games(
            &format!(
                "SELECT {GAME_COLUMNS} FROM game g
                 WHERE g.present = 1 AND g.hidden = 0
                   AND (?1 = 0 OR g.favorite = 1)
                   AND (?2 IS NULL OR g.system = ?2)
                   AND (json_array_length(?3) = 0 OR EXISTS (
                     SELECT 1 FROM media m
                     WHERE m.game_id = g.id AND m.kind IN (SELECT value FROM json_each(?3))))
                 ORDER BY random() LIMIT ?4"
            ),
            params![
                favorites_only,
                system,
                kinds,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
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

    /// Writes the game's favourite, play count and last played to its system's
    /// `gamelist.xml`, when the user turned [`GAMELIST_WRITE_BACK_SETTING`] on, so
    /// EmulationStation and other tools see them. Returns whether the file changed.
    ///
    /// The re-import this causes through the watcher is harmless: the values came from the
    /// user's own fields, which outrank the gamelist, and only a change made in Forma writes.
    pub fn write_back_to_gamelist(&self, paths: &Paths, id: i64) -> Result<bool> {
        if self.setting(GAMELIST_WRITE_BACK_SETTING)?.as_deref() != Some("true") {
            return Ok(false);
        }
        let Some((game, system)) = self.launch_target(id)? else {
            return Ok(false);
        };
        crate::gamelist::write_back(
            &system.rom_path,
            paths,
            Path::new(&game.path),
            &crate::gamelist::PlayStats {
                name: &game.given_name,
                favorite: game.favorite,
                play_count: game.play_count,
                last_played: game.last_played.as_deref(),
            },
        )
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

    /// How games' names are shown and sorted ([`NAME_STYLE_SETTING`]).
    pub fn name_rule(&self) -> Result<NameRule> {
        Ok(NameRule::load(self.setting(NAME_STYLE_SETTING)?.as_deref()))
    }

    /// Shows and sorts every game's name in `rule` from now on: each game is worked out again.
    pub fn set_name_rule(&mut self, rule: &NameRule) -> Result<()> {
        let raw = serde_json::to_string(rule).map_err(|e| Error::Invalid(e.to_string()))?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![NAME_STYLE_SETTING, raw],
        )?;
        let ids: Vec<i64> = tx
            .prepare("SELECT id FROM game")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        materialise(&tx, &ids, &[])?;
        tx.commit()?;
        Ok(())
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
                    region: r.get(17)?,
                    lang: r.get(18)?,
                    age_rating: r.get(19)?,
                    given_name: r.get(20)?,
                    iconic: false,
                    media: BTreeMap::new(),
                    dump: None,
                    discs: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        for game in &mut games {
            game.iconic = crate::iconic::is_iconic(&game.system, &game.given_name, &game.path);
        }

        let mut media = self
            .conn
            .prepare_cached("SELECT kind, path FROM media WHERE game_id = ?1")?;
        for game in &mut games {
            game.media = media
                .query_map([game.id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
        }
        let mut dump = self.conn.prepare_cached(
            "SELECT status, catalogue, name, region FROM game_dump WHERE game_id = ?1",
        )?;
        for game in &mut games {
            game.dump = dump
                .query_row([game.id], |r| {
                    Ok(Dump {
                        status: r.get(0)?,
                        catalogue: r.get(1)?,
                        name: r.get(2)?,
                        region: r.get(3)?,
                    })
                })
                .optional()?;
        }
        let mut discs = self
            .conn
            .prepare_cached("SELECT path FROM game_disc WHERE game_id = ?1 ORDER BY n")?;
        for game in &mut games {
            game.discs = discs
                .query_map([game.id], |r| r.get(0))?
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
    let names = NameRule::load(
        tx.query_row(
            "SELECT value FROM setting WHERE key = ?1",
            [NAME_STYLE_SETTING],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .as_deref(),
    );
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
             last_played = ?17, ra_game_id = ?18, ra_hash = ?19, game_time = ?20,
             age_rating = ?21
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
        // In the user's style; the file's name (no name found yet) as it is.
        let name = get(field::NAME).map_or_else(|| default_name(&path), |n| names.apply(&n));
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
            get(field::AGE_RATING),
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

/// Forma's setting (`true` or `false`, off by default) for writing favourites and play stats
/// back to `gamelist.xml`: see [`Library::write_back_to_gamelist`].
pub const GAMELIST_WRITE_BACK_SETTING: &str = "library.gamelistWriteBack";

/// Forma's setting for the order systems are listed in, for every theme: `maker` (by
/// manufacturer, then oldest first: the default), `name` (A to Z) or `year` (oldest first).
pub const SYSTEMS_ORDER_SETTING: &str = "library.systemsOrder";

/// How systems are listed ([`SYSTEMS_ORDER_SETTING`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemsOrder {
    Name,
    #[default]
    Maker,
    Year,
}

impl SystemsOrder {
    /// The setting's value; anything else (or none) is by maker.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("name") => Self::Name,
            Some("year") => Self::Year,
            _ => Self::Maker,
        }
    }
}

/// Sorts systems as the user sees them: by full name, ignoring case; by maker or year with the
/// name breaking ties. A system without a maker or year comes after those with one.
pub fn sort_systems(systems: &mut [SystemSummary], order: SystemsOrder) {
    let name = |s: &SystemSummary| s.fullname.to_lowercase();
    let year = |s: &SystemSummary| s.release_year.unwrap_or(i64::MAX);
    let maker = |s: &SystemSummary| {
        s.manufacturer
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_lowercase)
    };
    match order {
        SystemsOrder::Name => systems.sort_by_cached_key(|s| (name(s), s.name.clone())),
        SystemsOrder::Year => systems.sort_by_cached_key(|s| (year(s), name(s))),
        SystemsOrder::Maker => systems.sort_by_cached_key(|s| {
            let m = maker(s);
            (m.is_none(), m, year(s), name(s))
        }),
    }
}

fn players_max(players: &str) -> Option<i64> {
    players
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse::<i64>().ok())
        .max()
}

#[cfg(test)]
mod tests;
