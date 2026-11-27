//! The LaunchBox Games Database (gamesdb.launchbox-app.com). It has no API: its data is one
//! file, `Metadata.zip`, updated daily, about 110 MB with half a gigabyte of XML inside. Forma
//! fetches it once, keeps what it needs in a small database of its own, and answers from that;
//! after [`MAX_AGE`] it fetches it again. Only the pictures come from the network per game.

use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use quick_xml::Reader;
use quick_xml::events::Event;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::super::text::{search_name, title_key};
use super::super::{
    Credentials, Found, LookupError, MediaKind, Metadata, Query, Source, platforms,
};
use crate::net::Http;
use crate::paths::Paths;
use crate::scraper::{MediaNote, Notes};
use crate::xml::resolve_ref;

const DATA_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";
const IMAGES_URL: &str = "https://images.launchbox-app.com/";
/// Well over the file's size, should it grow.
const DATA_LIMIT: u64 = 512 << 20;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// What the database holds; one built by an older Forma lacks the alternate names as written.
const SCHEMA: i64 = 2;
/// After a failed download, how long until another is tried.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
const PER_KIND: usize = 6;

/// LaunchBox's image types Forma uses; for each kind of media, the best first.
const TYPES: [(&str, MediaKind); 21] = [
    ("Box - Front", MediaKind::Thumbnail),
    ("Box - Front - Reconstructed", MediaKind::Thumbnail),
    ("Fanart - Box - Front", MediaKind::Thumbnail),
    ("Box - Back", MediaKind::Boxback),
    ("Box - Back - Reconstructed", MediaKind::Boxback),
    ("Fanart - Box - Back", MediaKind::Boxback),
    ("Box - Spine", MediaKind::Boxside),
    ("Box - 3D", MediaKind::Boxart),
    ("Screenshot - Gameplay", MediaKind::Image),
    ("Screenshot - Game Select", MediaKind::Image),
    ("Screenshot - Game Title", MediaKind::Titleshot),
    ("Clear Logo", MediaKind::Wheel),
    ("Fanart - Background", MediaKind::Fanart),
    ("Cart - Front", MediaKind::Cartridge),
    ("Disc", MediaKind::Cartridge),
    ("Fanart - Cart - Front", MediaKind::Cartridge),
    ("Fanart - Disc", MediaKind::Cartridge),
    ("Banner", MediaKind::Banner),
    ("Arcade - Marquee", MediaKind::Marquee),
    ("Arcade - Cabinet", MediaKind::Cabinet),
    ("Advertisement Flyer - Front", MediaKind::Flyer),
];

/// One download and build at a time, however many times the sources are made.
static BUILDING: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct State {
    db: Option<Connection>,
    /// When the file open was written: another (see [`upgrade`]) is opened in its place.
    opened: Option<SystemTime>,
    failed: Option<(Instant, String)>,
}

pub struct LaunchBox {
    file: PathBuf,
    state: Mutex<State>,
}

impl Default for LaunchBox {
    fn default() -> Self {
        Self::at(Paths::from_env().cache_dir().join("launchbox.db"))
    }
}

impl LaunchBox {
    pub(super) fn at(file: PathBuf) -> Self {
        Self {
            file,
            state: Mutex::new(State::default()),
        }
    }

    fn is_stale(&self) -> bool {
        fs::metadata(&self.file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_none_or(|age| age > MAX_AGE)
    }

    /// Downloads and builds when there is nothing yet or it has aged. An old database goes on
    /// answering if a new one cannot be had.
    fn prepare(&self, http: &dyn Http, state: &mut State) -> Result<(), LookupError> {
        let may_try = state
            .failed
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= RETRY_AFTER);
        if self.is_stale() && may_try {
            let _one = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
            if self.is_stale() {
                match fetch(http, &self.file) {
                    Ok(()) => {
                        state.db = None;
                        state.failed = None;
                    }
                    Err(e) => state.failed = Some((Instant::now(), e)),
                }
            }
        }
        let written = fs::metadata(&self.file).and_then(|m| m.modified()).ok();
        if state.opened != written {
            state.db = None;
        }
        if state.db.is_none() && self.file.exists() {
            let db = Connection::open_with_flags(&self.file, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| LookupError::Failed(e.to_string()))?;
            state.db = Some(db);
            state.opened = written;
        }
        if state.db.is_some() {
            return Ok(());
        }
        // Quota, not Failed: without its data it has no answers for any game of this run.
        let why = state.failed.as_ref().map_or("not available", |(_, e)| e);
        Err(LookupError::Quota(format!("LaunchBox's data: {why}")))
    }
}

impl Source for LaunchBox {
    fn id(&self) -> &str {
        "launchbox"
    }
    fn name(&self) -> &str {
        "LaunchBox"
    }
    fn about(&self) -> &str {
        "Boxes, spines, cartridges, logos, fan art, screenshots and details, matched by name. \
         Nothing to set up; the first scrape fetches its data, about 110 MB"
    }
    fn gives(&self) -> &[MediaKind] {
        &[
            MediaKind::Thumbnail,
            MediaKind::Boxback,
            MediaKind::Boxside,
            MediaKind::Boxart,
            MediaKind::Image,
            MediaKind::Titleshot,
            MediaKind::Wheel,
            MediaKind::Fanart,
            MediaKind::Cartridge,
            MediaKind::Banner,
            MediaKind::Marquee,
            MediaKind::Cabinet,
            MediaKind::Flyer,
        ]
    }
    fn covers(&self, query: &Query<'_>) -> bool {
        !platforms::launchbox(query.platforms).is_empty()
    }

    fn lookup(
        &self,
        http: &dyn Http,
        _: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let on = platforms::launchbox(query.platforms);
        if on.is_empty() {
            return Ok(None);
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.prepare(http, &mut state)?;
        let Some(db) = &state.db else {
            return Ok(None);
        };
        answer(db, &on, query).map_err(|e| LookupError::Failed(e.to_string()))
    }
}

fn fetch(http: &dyn Http, file: &Path) -> Result<(), String> {
    let zip = http.download(DATA_URL, DATA_LIMIT).map_err(|e| e.message)?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    build(&zip, file)
}

/// Writes the database beside `file` and moves it into place only once it is whole.
fn build(zip: &[u8], file: &Path) -> Result<(), String> {
    let temp = file.with_extension("building");
    let _ = fs::remove_file(&temp);
    let built = fill(zip, &temp).and_then(|()| fs::rename(&temp, file).map_err(|e| e.to_string()));
    if built.is_err() {
        let _ = fs::remove_file(&temp);
    }
    built
}

fn fill(zip: &[u8], temp: &Path) -> Result<(), String> {
    fn text(e: impl std::fmt::Display) -> String {
        e.to_string()
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(text)?;
    let mut db = Connection::open(temp).map_err(text)?;
    db.execute_batch(
        "PRAGMA journal_mode = OFF;
         PRAGMA synchronous = OFF;
         CREATE TABLE game (id INTEGER PRIMARY KEY, platform TEXT NOT NULL, name TEXT NOT NULL,
             overview TEXT, developer TEXT, publisher TEXT, genres TEXT, players TEXT,
             released TEXT, rating REAL, esrb TEXT, votes INTEGER NOT NULL);
         CREATE TABLE title (key TEXT NOT NULL, game INTEGER NOT NULL, alternate INTEGER NOT NULL);
         CREATE TABLE alias (game INTEGER NOT NULL, name TEXT NOT NULL, region TEXT);
         CREATE TABLE image (game INTEGER NOT NULL, type INTEGER NOT NULL, region TEXT,
             file TEXT NOT NULL);
         CREATE TABLE mame (file TEXT PRIMARY KEY, name TEXT NOT NULL) WITHOUT ROWID;",
    )
    .map_err(text)?;
    let tx = db.transaction().map_err(text)?;
    let mut games = 0usize;
    {
        let mut game = tx
            .prepare("INSERT OR REPLACE INTO game VALUES (?,?,?,?,?,?,?,?,?,?,?,?)")
            .map_err(text)?;
        let mut title = tx
            .prepare("INSERT INTO title VALUES (?,?,?)")
            .map_err(text)?;
        let mut image = tx
            .prepare("INSERT INTO image VALUES (?,?,?,?)")
            .map_err(text)?;
        let mut alias = tx
            .prepare("INSERT INTO alias VALUES (?,?,?)")
            .map_err(text)?;
        records(
            archive.by_name("Metadata.xml").map_err(text)?,
            |tag, fields| {
                let id = || field(fields, "DatabaseID")?.parse::<i64>().ok();
                match tag {
                    "Game" => {
                        let (Some(id), Some(name), Some(platform)) =
                            (id(), field(fields, "Name"), field(fields, "Platform"))
                        else {
                            return Ok(());
                        };
                        let votes = field(fields, "CommunityRatingCount")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(0);
                        let rating = field(fields, "CommunityRating")
                            .and_then(|v| v.parse::<f64>().ok())
                            .filter(|_| votes > 0)
                            .map(|v| (v / 5.0).clamp(0.0, 1.0));
                        game.execute(params![
                            id,
                            platform,
                            name,
                            field(fields, "Overview"),
                            field(fields, "Developer"),
                            field(fields, "Publisher"),
                            field(fields, "Genres").map(genres),
                            field(fields, "MaxPlayers").map(players),
                            field(fields, "ReleaseDate")
                                .or_else(|| field(fields, "ReleaseYear"))
                                .and_then(|d| d.get(..d.len().min(10))),
                            rating,
                            field(fields, "ESRB").and_then(esrb),
                            votes,
                        ])
                        .map_err(text)?;
                        title
                            .execute(params![title_key(name), id, 0])
                            .map_err(text)?;
                        games += 1;
                    }
                    "GameAlternateName" => {
                        if let (Some(id), Some(name)) = (id(), field(fields, "AlternateName")) {
                            title
                                .execute(params![title_key(name), id, 1])
                                .map_err(text)?;
                            alias
                                .execute(params![id, name, field(fields, "Region")])
                                .map_err(text)?;
                        }
                    }
                    "GameImage" => {
                        let kind = field(fields, "Type")
                            .and_then(|t| TYPES.iter().position(|(name, _)| *name == t));
                        if let (Some(id), Some(kind), Some(file)) =
                            (id(), kind, field(fields, "FileName"))
                        {
                            image
                                .execute(params![id, kind as i64, field(fields, "Region"), file])
                                .map_err(text)?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
        )?;
        let mut mame = tx
            .prepare("INSERT OR IGNORE INTO mame VALUES (?,?)")
            .map_err(text)?;
        records(archive.by_name("Mame.xml").map_err(text)?, |tag, fields| {
            if tag == "MameFile"
                && let (Some(file), Some(name)) = (field(fields, "FileName"), field(fields, "Name"))
            {
                mame.execute(params![file, name]).map_err(text)?;
            }
            Ok(())
        })?;
    }
    tx.commit().map_err(text)?;
    if games == 0 {
        return Err("it lists no games".to_owned());
    }
    db.execute_batch(&format!(
        "CREATE INDEX title_key ON title (key);
             CREATE INDEX image_game ON image (game);
             CREATE INDEX alias_game ON alias (game);
             PRAGMA user_version = {SCHEMA};"
    ))
    .map_err(text)
}

/// What a database file holds ([`SCHEMA`]); 0 for none, or one from before it was counted.
fn schema(file: &Path) -> i64 {
    Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|db| db.pragma_query_value(None, "user_version", |r| r.get(0)))
        .unwrap_or(0)
}

/// Builds LaunchBox's data again when an older Forma built what is there: it answers on, but
/// lacks what this one reads. `Ok(true)` when it did. Nothing is fetched when there is no data:
/// the first lookup does it.
pub(in crate::scraper) fn upgrade(http: &dyn Http, file: &Path) -> Result<bool, String> {
    if !file.exists() || schema(file) >= SCHEMA {
        return Ok(false);
    }
    let _one = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
    if schema(file) >= SCHEMA {
        return Ok(false);
    }
    fetch(http, file)?;
    Ok(true)
}

/// Each game's LaunchBox game, and LaunchBox's name for it, by the game's id.
pub(in crate::scraper) type Identified = HashMap<i64, (i64, String)>;

/// Which LaunchBox game each of `games` (id, name, path) is, with LaunchBox's name for it, as a
/// lookup would match it; `None` without the data.
pub(in crate::scraper) fn identify(
    file: &Path,
    platforms: &[String],
    games: &[(i64, &str, &str)],
) -> Result<Option<Identified>, String> {
    if !file.exists() {
        return Ok(None);
    }
    let fail = |e: rusqlite::Error| e.to_string();
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail)?;
    let on = platforms::launchbox(platforms);
    let mut found = HashMap::new();
    if on.is_empty() {
        return Ok(Some(found));
    }
    let mut titles: HashMap<i64, String> = HashMap::new();
    for (id, name, path) in games {
        let rom = Path::new(path);
        let part = |p: Option<&std::ffi::OsStr>| {
            p.map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (file_name, stem) = (part(rom.file_name()), part(rom.file_stem()));
        let name = search_name(name);
        let query = Query {
            system: "",
            platforms,
            file_name: &file_name,
            stem: &stem,
            name: &name,
            size: 0,
            hashes: None,
            wanted: &[],
            wants_metadata: false,
            regions: &[],
        };
        let Some(game) = matched(&db, &on, &query).map_err(fail)? else {
            continue;
        };
        let title = match titles.entry(game) {
            std::collections::hash_map::Entry::Occupied(known) => known.get().clone(),
            std::collections::hash_map::Entry::Vacant(new) => new
                .insert(
                    db.query_row("SELECT name FROM game WHERE id = ?", [game], |r| r.get(0))
                        .map_err(fail)?,
                )
                .clone(),
        };
        found.insert(*id, (game, title));
    }
    Ok(Some(found))
}

/// What LaunchBox says one of its games is called, and when it came out.
pub(in crate::scraper) struct Names {
    pub name: String,
    /// Its other names that say where they are used, with that region as LaunchBox writes it.
    pub regional: Vec<(String, String)>,
    pub year: Option<i32>,
}

/// [`Names`] for each of `games` (LaunchBox's ids). A game not in the data has none.
pub(in crate::scraper) fn names(file: &Path, games: &[i64]) -> Result<HashMap<i64, Names>, String> {
    let fail = |e: rusqlite::Error| e.to_string();
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail)?;
    let mut own = db
        .prepare_cached("SELECT name, released FROM game WHERE id = ?")
        .map_err(fail)?;
    let mut out = HashMap::new();
    for &game in games {
        let Some((name, released)) = own
            .query_row([game], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .optional()
            .map_err(fail)?
        else {
            continue;
        };
        let regional = aliases(&db, game)
            .into_iter()
            .filter_map(|(name, region)| Some((name, region.filter(|r| !r.trim().is_empty())?)))
            .collect();
        let year = released.and_then(|r| r.get(..4)?.parse().ok());
        out.insert(
            game,
            Names {
                name,
                regional,
                year,
            },
        );
    }
    Ok(out)
}

/// The game's other names, with the region each is used in, as written. None from a database
/// an older Forma built.
fn aliases(db: &Connection, game: i64) -> Vec<(String, Option<String>)> {
    let Ok(mut stmt) =
        db.prepare_cached("SELECT name, region FROM alias WHERE game = ? ORDER BY rowid")
    else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    stmt.query_map([game], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter(|(name, _)| seen.insert(name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k.as_str() == name)
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// Calls `on` with each child of the document element and that child's fields, as it goes.
fn records(
    input: impl Read,
    mut on: impl FnMut(&str, &[(String, String)]) -> Result<(), String>,
) -> Result<(), String> {
    let mut xml = Reader::from_reader(BufReader::with_capacity(1 << 16, input));
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut record = String::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let name = |e: &quick_xml::events::BytesStart<'_>| e.name().0.to_owned();
    let push = |fields: &mut Vec<(String, String)>, text: &str| {
        if let Some((_, value)) = fields.last_mut() {
            value.push_str(text);
        }
    };
    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) => {
                depth += 1;
                if depth == 2 {
                    record = name(&e);
                    fields.clear();
                } else if depth == 3 {
                    fields.push((name(&e), String::new()));
                }
            }
            Event::End(_) => {
                if depth == 2 {
                    on(&record, &fields)?;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Text(t) if depth == 3 => push(&mut fields, &t.xml10_content()),
            Event::CData(t) if depth == 3 => {
                push(&mut fields, &t.xml10_content());
            }
            Event::GeneralRef(r) if depth == 3 => push(&mut fields, &resolve_ref(&r)),
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

/// `Action; Adventure` as the other sources give them.
fn genres(text: &str) -> String {
    text.split(';')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn players(most: &str) -> String {
    match most.parse::<u32>() {
        Ok(n) if n > 1 => format!("1-{n}"),
        _ => most.to_owned(),
    }
}

/// `E10+ - Everyone 10+` to `ESRB:E10+`; none for `Not Rated` or one still pending.
fn esrb(text: &str) -> Option<String> {
    let (rating, _) = text.split_once(" - ")?;
    (rating != "RP").then(|| format!("ESRB:{rating}"))
}

/// The game the ROM is: by its name or an alternate one, on its platform, else by its MAME name.
fn matched(db: &Connection, on: &[&str], query: &Query<'_>) -> rusqlite::Result<Option<i64>> {
    let mame: Option<String> = db
        .query_row("SELECT name FROM mame WHERE file = ?", [query.stem], |r| {
            r.get(0)
        })
        .optional()?;
    let names = [Some(query.name), Some(query.stem), mame.as_deref()];
    let mut find = db.prepare_cached(
        "SELECT g.id FROM title t JOIN game g ON g.id = t.game
         WHERE t.key = ?1 AND g.platform = ?2 ORDER BY t.alternate, g.votes DESC LIMIT 1",
    )?;
    for name in names.into_iter().flatten() {
        let key = title_key(name);
        if key.is_empty() {
            continue;
        }
        for platform in on {
            let id = find
                .query_row(params![key, platform], |r| r.get(0))
                .optional()?;
            if id.is_some() {
                return Ok(id);
            }
        }
    }
    Ok(None)
}

/// LaunchBox's regions for `regions`, the profile's order.
pub(in crate::scraper) fn regions_of(regions: &[String]) -> Vec<&'static str> {
    const NAMES: [(&str, &[&str]); 10] = [
        ("USA", &["North America", "United States"]),
        ("Europe", &["Europe"]),
        ("Japan", &["Japan"]),
        ("World", &["World"]),
        ("UK", &["United Kingdom"]),
        ("Germany", &["Germany"]),
        ("France", &["France"]),
        ("Spain", &["Spain"]),
        ("Italy", &["Italy"]),
        ("Australia", &["Australia"]),
    ];
    regions
        .iter()
        .filter_map(|r| NAMES.iter().find(|(n, _)| n == r))
        .flat_map(|(_, names)| names.iter().copied())
        .collect()
}

/// Lower is better: the ROM's own regions, then ones anybody can read.
fn region_rank(wanted: &[&str], region: Option<&str>) -> usize {
    const THEN: [&str; 5] = ["World", "", "North America", "United States", "Europe"];
    let region = region.unwrap_or("");
    wanted
        .iter()
        .position(|r| *r == region)
        .or_else(|| THEN.iter().position(|r| *r == region).map(|i| i + 20))
        .unwrap_or(40)
}

fn answer(db: &Connection, on: &[&str], query: &Query<'_>) -> rusqlite::Result<Option<Found>> {
    let Some(id) = matched(db, on, query)? else {
        return Ok(None);
    };
    let mut metadata = db.query_row(
        "SELECT name, overview, developer, publisher, genres, players, released, rating, esrb
         FROM game WHERE id = ?",
        [id],
        |r| {
            Ok(Metadata {
                name: r.get(0)?,
                description: r.get(1)?,
                developer: r.get(2)?,
                publisher: r.get(3)?,
                genre: r.get(4)?,
                players: r.get(5)?,
                release_date: r.get(6)?,
                rating: r.get(7)?,
                age_rating: r.get(8)?,
            })
        },
    )?;
    let mut stmt = db.prepare_cached("SELECT type, region, file FROM image WHERE game = ?")?;
    let mut images: Vec<(i64, Option<String>, String)> = stmt
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let wanted = regions_of(query.regions);
    images.sort_by_key(|(kind, region, _)| (*kind, region_rank(&wanted, region.as_deref())));
    let mut media: Vec<(MediaKind, String)> = Vec::new();
    let mut notes = Notes::default();
    for (kind, region, file) in images {
        let Some((_, kind)) = TYPES.get(kind as usize) else {
            continue;
        };
        if media.iter().filter(|(k, _)| k == kind).count() < PER_KIND {
            let url = format!("{IMAGES_URL}{file}");
            if let Some(region) = region.filter(|r| !r.is_empty()) {
                let note = MediaNote {
                    region: Some(region),
                    ..MediaNote::default()
                };
                notes.media.insert(url.clone(), note);
            }
            media.push((*kind, url));
        }
    }
    // LaunchBox's own name is North America's: a name for a region the profile puts first
    // takes its place, and it joins the other names.
    let mut names = aliases(db, id);
    let main_rank = ["North America", "United States"]
        .iter()
        .map(|r| region_rank(&wanted, Some(r)))
        .min()
        .unwrap_or(usize::MAX);
    let best = names
        .iter()
        .enumerate()
        .filter(|(_, (_, region))| region.as_deref().is_some_and(|r| !r.is_empty()))
        .min_by_key(|(_, (_, region))| region_rank(&wanted, region.as_deref()))
        .filter(|(_, (_, region))| region_rank(&wanted, region.as_deref()) < main_rank)
        .map(|(i, _)| i);
    if let Some(i) = best {
        let (name, region) = names.remove(i);
        if let Some(main) = metadata.name.replace(name) {
            names.insert(0, (main, Some("North America".to_owned())));
        }
        if let Some(region) = region {
            notes.text.insert("name", region);
        }
    }
    let own = metadata.name.clone();
    let mut seen = std::collections::HashSet::new();
    notes.names = names
        .into_iter()
        .filter(|(name, _)| Some(name) != own.as_ref() && seen.insert(name.clone()))
        .collect();
    Ok(Some(Found {
        metadata,
        media,
        notes,
    }))
}
