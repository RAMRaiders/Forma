//! The library schema and its migrations, tracked by SQLite's `user_version`.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Bump with every migration appended to [`MIGRATIONS`].
pub(crate) const SCHEMA_VERSION: i64 = 7;

const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7];

/// The gamelist importer learned `boxside` (a box's spine): read every gamelist again, once.
const V7: &str = r"
DELETE FROM import_file WHERE path LIKE '%gamelist.xml';
";

/// The user's collections, as EmulationStation's `custom-<name>.cfg` files hold them (see
/// `collections.rs`): the files stay the truth, this is their index for browsing. Paths are
/// absolute, as in the files, so a game that is missing keeps its place.
/// The gamelist importer learned new tags (`boxback`, `boxart`, `wheel`, `titleshot`,
/// `cartridge`, `map`, `bezel`, `mix`): gamelists read before kept none of them, and are only
/// read again when they change. Forgetting when they were read makes the next refresh read
/// each one again, once. Do the same whenever the importer learns to read more.
const V6: &str = r"
DELETE FROM import_file WHERE path LIKE '%gamelist.xml';
";

const V5: &str = r"
CREATE TABLE collection (
    name  TEXT PRIMARY KEY COLLATE NOCASE
);
CREATE TABLE collection_game (
    collection  TEXT NOT NULL COLLATE NOCASE REFERENCES collection (name) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (collection, path)
) WITHOUT ROWID;
";

/// Visible games per system from the index alone: counting them read every game row, 2.6 s on
/// a cold SD card at boot, and far longer while the start-up refresh read the disk too.
const V4: &str = r"
CREATE INDEX game_visible ON game (system) WHERE present = 1 AND hidden = 0;
";

/// Total time played, in seconds, recorded by Forma or imported from Batocera's `gametime`.
const V3: &str = r"
ALTER TABLE game ADD COLUMN game_time INTEGER NOT NULL DEFAULT 0;
";

/// Forma's own settings (boot view, selected theme…), kept with the library so one file holds
/// everything a user would lose on a reinstall.
const V2: &str = r"
CREATE TABLE setting (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
";

const V1: &str = r"
CREATE TABLE system (
    name          TEXT PRIMARY KEY,
    fullname      TEXT NOT NULL,
    manufacturer  TEXT,
    release_year  INTEGER,
    hardware      TEXT,
    rom_path      TEXT NOT NULL,
    sort_order    INTEGER NOT NULL,
    -- JSON: the full definition (extensions, platforms, emulators…) for the launcher.
    definition    TEXT NOT NULL
);
CREATE INDEX system_manufacturer ON system (manufacturer);

-- One row per game file. The metadata columns are materialised from game_field: the value
-- from the highest-precedence source wins. Never write them directly.
CREATE TABLE game (
    id            INTEGER PRIMARY KEY,
    system        TEXT NOT NULL REFERENCES system (name) ON DELETE CASCADE,
    path          TEXT NOT NULL,             -- relative to the system's ROM folder
    present       INTEGER NOT NULL DEFAULT 1, -- 0 while the file is missing (e.g. drive unplugged)
    size          INTEGER,
    mtime_ns      INTEGER,
    name          TEXT NOT NULL,
    description   TEXT,
    developer     TEXT,
    publisher     TEXT,
    genre         TEXT,
    players       TEXT,
    players_max   INTEGER,
    release_date  TEXT,
    release_year  INTEGER,
    rating        REAL,
    region        TEXT,
    lang          TEXT,
    hidden        INTEGER NOT NULL DEFAULT 0,
    favorite      INTEGER NOT NULL DEFAULT 0,
    play_count    INTEGER NOT NULL DEFAULT 0,
    last_played   TEXT,
    ra_game_id    INTEGER,
    ra_hash       TEXT,
    UNIQUE (system, path)
);
-- One index per column a hierarchy view can group or sort by.
CREATE INDEX game_system_name ON game (system, name COLLATE NOCASE);
CREATE INDEX game_genre ON game (genre);
CREATE INDEX game_developer ON game (developer);
CREATE INDEX game_publisher ON game (publisher);
CREATE INDEX game_release_year ON game (release_year);
CREATE INDEX game_players_max ON game (players_max);
CREATE INDEX game_region ON game (region);
CREATE INDEX game_favorite ON game (favorite) WHERE favorite = 1;
CREATE INDEX game_last_played ON game (last_played) WHERE last_played IS NOT NULL;

-- Every source's value for every field. source: 1 = gamelist.xml, 2 = external import,
-- 3 = the user in Forma. A higher number wins.
CREATE TABLE game_field (
    game_id  INTEGER NOT NULL REFERENCES game (id) ON DELETE CASCADE,
    field    TEXT NOT NULL,
    source   INTEGER NOT NULL,
    value    TEXT NOT NULL,
    PRIMARY KEY (game_id, field, source)
) WITHOUT ROWID;

CREATE TABLE media (
    game_id  INTEGER NOT NULL REFERENCES game (id) ON DELETE CASCADE,
    kind     TEXT NOT NULL,   -- image, video, marquee, thumbnail, fanart, manual
    path     TEXT NOT NULL,
    PRIMARY KEY (game_id, kind)
) WITHOUT ROWID;

-- What was imported, and when it last changed, so unchanged files are not re-read.
CREATE TABLE import_file (
    path      TEXT PRIMARY KEY,
    size      INTEGER NOT NULL,
    mtime_ns  INTEGER NOT NULL
);

-- Search index. Contentless (the text lives in `game`) with contentless_delete, so a row is
-- removed by rowid alone. Maintained from code, only for games whose searchable text changed:
-- per-row triggers made a 20k-game import four times slower. The one trigger left covers
-- deletes, including those cascaded from a removed system.
CREATE VIRTUAL TABLE game_fts USING fts5 (
    name, description, developer, publisher,
    content = '', contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER game_fts_delete AFTER DELETE ON game BEGIN
    DELETE FROM game_fts WHERE rowid = old.id;
END;
";

pub(crate) fn configure(conn: &Connection) -> Result<()> {
    // WAL: readers (the UI) never wait on a refresh, and a crash cannot leave a half-written
    // database. In-memory databases ignore it.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

pub(crate) fn migrate(conn: &mut Connection, path: &std::path::Path) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        // Refuse rather than misread: rows written by a newer schema look like data.
        return Err(Error::Parse {
            path: path.to_path_buf(),
            message: format!(
                "library schema v{current} is newer than this Forma understands (v{SCHEMA_VERSION})"
            ),
        });
    }
    let tx = conn.transaction()?;
    for (i, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(usize::try_from(current).unwrap_or(0))
    {
        tx.execute_batch(sql)?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(i + 1).unwrap_or(i64::MAX),
        )?;
    }
    tx.commit()?;
    Ok(())
}
