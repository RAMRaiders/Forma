//! The libretro netplay lobby, read-only: rooms open right now, matched to local games.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Cache, Fetched, Http, fetch, lenient};
use crate::error::Result;
use crate::library::Library;

const LOBBY: &str = "https://lobby.libretro.com/list/";
/// Rooms come and go quickly.
const MAX_AGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: i64,
    pub username: String,
    pub country: Option<String>,
    pub game_name: String,
    pub game_crc: Option<String>,
    pub core_name: String,
    pub core_version: Option<String>,
    pub retroarch_version: Option<String>,
    pub frontend: Option<String>,
    pub host: String,
    pub port: Option<i64>,
    /// Relay server, for hosts that cannot be reached directly.
    pub relay_host: Option<String>,
    pub relay_port: Option<i64>,
    pub has_password: bool,
    pub has_spectate_password: bool,
    pub connectable: bool,
    pub created: Option<String>,
    /// The local game this room is playing, when Forma can tell.
    pub local_game_id: Option<i64>,
}

/// Open rooms, those you can join first. Needs no library, so callers need not hold one
/// across the request; [`match_local`] then links rooms to local games.
pub fn lobby(http: &dyn Http, cache: &Cache) -> Result<Fetched<Vec<Room>>> {
    let mut fetched = fetch(http, cache, "netplay-lobby", LOBBY, MAX_AGE, |body| {
        let raw: Vec<Entry> = serde_json::from_slice(body).map_err(|e| e.to_string())?;
        Ok(raw
            .into_iter()
            .filter_map(|e| room(e.fields))
            .collect::<Vec<Room>>())
    })?;
    fetched.data.sort_by(|a, b| {
        b.connectable
            .cmp(&a.connectable)
            .then_with(|| a.game_name.cmp(&b.game_name))
    });
    Ok(fetched)
}

/// Sets each room's `local_game_id`: by CRC when a local game's hash is known (imported, see
/// `docs/import-format.md`), else by name.
pub fn match_local(rooms: &mut [Room], lib: &Library) -> Result<()> {
    let by_name = local_games(lib)?;
    let by_crc = local_crcs(lib)?;
    for room in rooms {
        room.local_game_id = room
            .game_crc
            .as_deref()
            .and_then(|crc| by_crc.get(&crc.to_ascii_lowercase()))
            .or_else(|| by_name.get(&normalise(&room.game_name)))
            .copied();
    }
    Ok(())
}

fn local_crcs(lib: &Library) -> Result<HashMap<String, i64>> {
    let mut stmt = lib.conn.prepare_cached(
        "SELECT f.value, f.game_id FROM game_field f JOIN game g ON g.id = f.game_id
         WHERE f.field = ?1 AND g.present = 1",
    )?;
    let rows = stmt.query_map([crate::field::HASH_CRC32], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Rooms name games by their ROM file name (no-intro style, without extension), which is also
/// what Batocera users' files are called. CRC matching arrives with imported hashes (#57).
fn local_games(lib: &Library) -> Result<HashMap<String, i64>> {
    let mut stmt = lib
        .conn
        .prepare_cached("SELECT id, path FROM game WHERE present = 1")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, path) = row?;
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.entry(normalise(&stem)).or_insert(id);
    }
    Ok(out)
}

fn normalise(name: &str) -> String {
    name.trim().to_lowercase()
}

fn room(f: Fields) -> Option<Room> {
    Some(Room {
        id: f.id?,
        username: f.username.unwrap_or_default(),
        country: f.country.filter(|c| !c.is_empty()),
        game_name: f.game_name.filter(|g| !g.is_empty())?,
        game_crc: f.game_crc.filter(|c| !c.is_empty() && c != "00000000"),
        core_name: f.core_name.unwrap_or_default(),
        core_version: f.core_version,
        retroarch_version: f.retroarch_version,
        frontend: f.frontend,
        host: f.ip.unwrap_or_default(),
        port: f.port,
        relay_host: f.mitm_ip.filter(|ip| !ip.is_empty()),
        relay_port: f.mitm_port.filter(|p| *p > 0),
        has_password: f.has_password.unwrap_or(false),
        has_spectate_password: f.has_spectate_password.unwrap_or(false),
        connectable: f.connectable.unwrap_or(false),
        created: f.created,
        local_game_id: None,
    })
}

#[derive(Deserialize)]
struct Entry {
    fields: Fields,
}

#[derive(Deserialize)]
struct Fields {
    #[serde(default, deserialize_with = "lenient::int")]
    id: Option<i64>,
    username: Option<String>,
    country: Option<String>,
    game_name: Option<String>,
    game_crc: Option<String>,
    core_name: Option<String>,
    core_version: Option<String>,
    retroarch_version: Option<String>,
    frontend: Option<String>,
    ip: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    port: Option<i64>,
    mitm_ip: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    mitm_port: Option<i64>,
    has_password: Option<bool>,
    has_spectate_password: Option<bool>,
    connectable: Option<bool>,
    created: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::UreqHttp;
    use crate::net::fake::FakeHttp;
    use crate::test_support::fixture_paths;

    #[test]
    fn lists_rooms_joinable_first_and_matched_to_local_games() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        lib.refresh(&fixture_paths()).unwrap();
        let http = FakeHttp::default().fixture("lobby.libretro.com", "lobby.json");

        let mut rooms = lobby(&http, &Cache::new(dir.path().into())).unwrap().data;
        match_local(&mut rooms, &lib).unwrap();
        assert_eq!(rooms.len(), 2);
        let mario = &rooms[0];
        assert!(mario.connectable);
        assert_eq!(mario.game_name, "Super Mario Bros. (World)");
        let local = lib.search("super mario", 1).unwrap()[0].id;
        assert_eq!(mario.local_game_id, Some(local));

        let sf2 = &rooms[1];
        assert_eq!(sf2.port, Some(55435), "a string port");
        assert_eq!(sf2.relay_host.as_deref(), Some("198.51.100.200"));
        assert!(sf2.has_password);
        assert_eq!(sf2.local_game_id, None);
    }

    /// The real lobby, to check the format the fixture was written from.
    #[test]
    #[ignore = "needs the network"]
    fn live_lobby_parses() {
        let dir = tempfile::tempdir().unwrap();
        let rooms = lobby(&UreqHttp::default(), &Cache::new(dir.path().into())).unwrap();
        assert!(!rooms.stale);
        println!(
            "{} rooms; first: {:?}",
            rooms.data.len(),
            rooms.data.first()
        );
    }
}

#[cfg(test)]
mod crc_tests {
    use super::*;
    use crate::net::fake::FakeHttp;
    use crate::test_support::fixture_copy;

    #[test]
    fn an_imported_crc_matches_a_room_whose_name_does_not() {
        let (dir, paths) = fixture_copy();
        std::fs::create_dir_all(paths.import_dir()).unwrap();
        // The lobby fixture's second room plays CRC B2D8F6B9 under another name.
        std::fs::write(
            paths.import_dir().join("hashes.jsonl"),
            "{\"format\":\"romdex-import\",\"formatVersion\":1}\n\
             {\"system\":\"nes\",\"path\":\"Homebrew Demo.nes\",\"hashes\":{\"crc32\":\"B2D8F6B9\"}}\n",
        )
        .unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        lib.refresh(&paths).unwrap();
        let http = FakeHttp::default().fixture("lobby.libretro.com", "lobby.json");
        let mut rooms = lobby(&http, &Cache::new(dir.path().join("cache")))
            .unwrap()
            .data;
        match_local(&mut rooms, &lib).unwrap();

        let demo = lib.search("homebrew demo", 1).unwrap()[0].id;
        let sf2 = rooms
            .iter()
            .find(|r| r.game_name.contains("Street Fighter"))
            .unwrap();
        assert_eq!(sf2.local_game_id, Some(demo));
    }
}
