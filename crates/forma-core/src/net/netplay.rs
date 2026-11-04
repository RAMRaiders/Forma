//! Netplay rooms: the libretro lobby's, and RetroArch hosts on the local network (its LAN
//! discovery), each matched to a local game and to the installed core that can join it, with
//! where to connect: straight to the host, or through its relay.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::time::{Duration, Instant};

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
    /// Found on the local network rather than in the lobby.
    pub lan: bool,
    /// Where to connect to play in it: the host, or its relay with the room's session.
    /// `None` when the room gives no way in.
    pub join: Option<JoinAt>,
    /// The local game this room is playing, when Forma can tell.
    pub local_game_id: Option<i64>,
    /// The installed core that can join it (Batocera's name, e.g. `fceumm`), when there is one.
    pub core: Option<String>,
    /// How long connecting took, when measured (see [`measure_pings`]).
    pub ping_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinAt {
    pub host: String,
    pub port: u16,
    /// A relay's session, when joining through a relay.
    pub session: Option<String>,
}

/// Which rooms to keep.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoomFilter {
    /// Only rooms playing a game on this box.
    pub owned: bool,
    /// Only rooms an installed core can join.
    pub joinable: bool,
    /// Only rooms without a password.
    pub open: bool,
    /// Only rooms with this core (the room's name for it, e.g. `FCEUmm`), any case.
    pub core_name: Option<String>,
    /// Only rooms answering within this many milliseconds; measures each room's ping.
    pub max_ping_ms: Option<u32>,
}

impl RoomFilter {
    /// What can be decided without the network.
    fn keeps(&self, room: &Room) -> bool {
        (!self.owned || room.local_game_id.is_some())
            && (!self.joinable || (room.core.is_some() && room.join.is_some()))
            && (!self.open || !room.has_password)
            && self
                .core_name
                .as_deref()
                .is_none_or(|c| room.core_name.eq_ignore_ascii_case(c))
    }

    /// Keeps the rooms that pass; with `max_ping_ms`, measures those first (`probe` connects).
    pub fn apply(
        &self,
        rooms: &mut Vec<Room>,
        probe: &(dyn Fn(&JoinAt) -> Option<Duration> + Sync),
    ) {
        rooms.retain(|r| self.keeps(r));
        if let Some(max) = self.max_ping_ms {
            measure_pings(rooms, probe);
            rooms.retain(|r| r.ping_ms.is_some_and(|p| p <= max));
        }
    }
}

/// Times a connection to each room (the first [`PINGS`], in parallel), into `ping_ms`.
pub fn measure_pings(rooms: &mut [Room], probe: &(dyn Fn(&JoinAt) -> Option<Duration> + Sync)) {
    std::thread::scope(|scope| {
        for room in rooms.iter_mut().take(PINGS) {
            scope.spawn(move || {
                room.ping_ms = room
                    .join
                    .as_ref()
                    .and_then(probe)
                    .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX));
            });
        }
    });
}

/// Rooms measured at most at once.
pub const PINGS: usize = 40;

/// A TCP connection to where a room is joined, timed; nothing is sent.
pub fn tcp_ping(at: &JoinAt) -> Option<Duration> {
    let addr = (at.host.as_str(), at.port).to_socket_addrs().ok()?.next()?;
    let start = Instant::now();
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(900)).ok()?;
    Some(start.elapsed())
}

/// Installed libretro cores by the name RetroArch announces them with (the `corename` of
/// their `.info`, lower-cased), e.g. `fceumm` for "FCEUmm".
#[derive(Debug, Clone, Default)]
pub struct Cores(HashMap<String, String>);

impl Cores {
    /// Batocera keeps cores in `/usr/lib/libretro/<core>_libretro.so` and their `.info` in
    /// `/usr/share/libretro/info`.
    pub fn batocera() -> Self {
        Self::load(
            Path::new("/usr/lib/libretro"),
            Path::new("/usr/share/libretro/info"),
        )
    }

    pub fn load(cores: &Path, info: &Path) -> Self {
        let mut out = HashMap::new();
        let Ok(dir) = std::fs::read_dir(cores) else {
            return Self(out);
        };
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(core) = name.strip_suffix("_libretro.so") else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(info.join(format!("{core}_libretro.info")))
            else {
                continue;
            };
            let corename = text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == "corename").then(|| v.trim().trim_matches('"').to_lowercase())
            });
            if let Some(corename) = corename {
                out.entry(corename).or_insert_with(|| core.to_owned());
            }
        }
        Self(out)
    }

    /// Cores by RetroArch's name for them, `(corename, core)`: for tests and the mock.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(name, core)| (name.to_lowercase(), core.to_owned()))
                .collect(),
        )
    }

    fn get(&self, corename: &str) -> Option<&String> {
        self.0.get(&corename.trim().to_lowercase())
    }
}

/// Sets each room's `core` from the installed ones.
pub fn match_cores(rooms: &mut [Room], cores: &Cores) {
    for room in rooms {
        room.core = cores.get(&room.core_name).cloned();
    }
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

/// How RetroArch hosts are reached (`host_method`): 3 is through a relay ("MITM").
const THROUGH_RELAY: i64 = 3;

fn room(f: Fields) -> Option<Room> {
    let port = |p: Option<i64>| p.and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0);
    let relay_session = f.mitm_session.clone().filter(|s| !s.is_empty());
    let join = if f.host_method == Some(THROUGH_RELAY) {
        match (
            f.mitm_ip.clone().filter(|ip| !ip.is_empty()),
            port(f.mitm_port),
        ) {
            (Some(host), Some(port)) => Some(JoinAt {
                host,
                port,
                session: relay_session.clone(),
            }),
            _ => None,
        }
    } else {
        match (f.ip.clone().filter(|ip| !ip.is_empty()), port(f.port)) {
            (Some(host), Some(port)) => Some(JoinAt {
                host,
                port,
                session: None,
            }),
            _ => None,
        }
    };
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
        lan: false,
        join,
        local_game_id: None,
        core: None,
        ping_ms: None,
    })
}

/// RetroArch's LAN discovery (`network/netplay/netplay_frontend.c`): a 4-byte query, `RANQ`,
/// broadcast to UDP 55435; every host on the network not behind a relay answers with an
/// `ad_packet`.
const DISCOVERY_PORT: u16 = 55435;
const QUERY: u32 = 0x5241_4E51;
const RESPONSE: u32 = 0x5241_4E53;
/// `ad_packet`: four 32-bit words, a 32-byte nick, four 32-byte strings, two 256-byte ones.
const AD_LEN: usize = 16 + 32 + 4 * 32 + 2 * 256;

/// Hosts on the local network, asked for `wait`. Empty, not an error, when nothing answers
/// or the network refuses broadcasts.
pub fn discover_lan(wait: Duration) -> Vec<Room> {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
        return Vec::new();
    };
    if socket.set_broadcast(true).is_err()
        || socket
            .send_to(&QUERY.to_be_bytes(), ("255.255.255.255", DISCOVERY_PORT))
            .is_err()
    {
        return Vec::new();
    }
    let until = Instant::now() + wait;
    let mut rooms: Vec<Room> = Vec::new();
    let mut buf = [0u8; AD_LEN + 1];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(room) = parse_ad(&buf[..n], from)
                    && !rooms.iter().any(|r| r.join == room.join)
                {
                    rooms.push(room);
                }
            }
            Err(_) => break,
        }
    }
    rooms
}

/// One host's answer, from `from`.
fn parse_ad(packet: &[u8], from: SocketAddr) -> Option<Room> {
    if packet.len() != AD_LEN {
        return None;
    }
    let word = |at: usize| u32::from_be_bytes(packet[at..at + 4].try_into().unwrap_or_default());
    if word(0) != RESPONSE {
        return None;
    }
    let text = |at: usize, len: usize| {
        let raw = &packet[at..at + len];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(len);
        String::from_utf8_lossy(&raw[..end]).trim().to_owned()
    };
    let crc = word(4);
    let port = u16::try_from(word(8)).ok().filter(|p| *p > 0)?;
    let has_password = word(12);
    let nick = text(16, 32);
    let frontend = text(48, 32);
    let core_name = text(80, 32);
    let core_version = text(112, 32);
    let retroarch_version = text(144, 32);
    let content = text(176, 256);
    if content.is_empty() || content == "N/A" {
        return None;
    }
    // The file's name, extension and all; the lobby names games without it.
    let game_name = Path::new(&content)
        .file_stem()
        .map_or(content.clone(), |s| s.to_string_lossy().into_owned());
    // RetroArch answers over IPv4 only.
    let std::net::IpAddr::V4(ip) = from.ip() else {
        return None;
    };
    let host = ip.to_string();
    let some = |s: String| Some(s).filter(|s| !s.is_empty() && s != "N/A");
    Some(Room {
        // Not a lobby id: the host's address and port, negative so it never meets one.
        id: -(i64::from(u32::from(ip)) * 65_536 + i64::from(port)),
        username: nick,
        country: None,
        game_name,
        game_crc: (crc != 0).then(|| format!("{crc:08X}")),
        core_name,
        core_version: some(core_version),
        retroarch_version: some(retroarch_version),
        frontend: some(frontend),
        host: host.clone(),
        port: Some(i64::from(port)),
        relay_host: None,
        relay_port: None,
        has_password: has_password & 1 != 0,
        has_spectate_password: has_password & 2 != 0,
        connectable: true,
        created: None,
        lan: true,
        join: Some(JoinAt {
            host,
            port,
            session: None,
        }),
        local_game_id: None,
        core: None,
        ping_ms: None,
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
    mitm_session: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    host_method: Option<i64>,
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

    fn fixture_rooms() -> Vec<Room> {
        let dir = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().fixture("lobby.libretro.com", "lobby.json");
        lobby(&http, &Cache::new(dir.path().into())).unwrap().data
    }

    #[test]
    fn a_room_behind_a_relay_is_joined_through_it() {
        let rooms = fixture_rooms();
        assert_eq!(
            rooms[0].join,
            Some(JoinAt {
                host: "192.0.2.10".into(),
                port: 55435,
                session: None
            })
        );
        let relayed = &rooms[1];
        let at = relayed.join.as_ref().unwrap();
        assert_eq!(at.host, "198.51.100.200");
        assert!(at.session.is_some(), "the relay's session for the room");
    }

    fn cores(dir: &Path) -> Cores {
        let (so, info) = (dir.join("lib"), dir.join("info"));
        std::fs::create_dir_all(&so).unwrap();
        std::fs::create_dir_all(&info).unwrap();
        for (core, name) in [("fceumm", "FCEUmm"), ("genesisplusgx", "Genesis Plus GX")] {
            std::fs::write(so.join(format!("{core}_libretro.so")), "").unwrap();
            std::fs::write(
                info.join(format!("{core}_libretro.info")),
                format!("display_name = \"x\"\ncorename = \"{name}\"\n"),
            )
            .unwrap();
        }
        // An .info without its core is not installed.
        std::fs::write(info.join("snes9x_libretro.info"), "corename = \"Snes9x\"\n").unwrap();
        Cores::load(&so, &info)
    }

    #[test]
    fn rooms_are_matched_to_installed_cores_and_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        lib.refresh(&fixture_paths()).unwrap();
        let mut rooms = fixture_rooms();
        match_local(&mut rooms, &lib).unwrap();
        match_cores(&mut rooms, &cores(dir.path()));
        assert_eq!(rooms[0].core.as_deref(), Some("fceumm"));
        assert_eq!(rooms[1].core, None, "Snes9x is not installed");

        let never = |_: &JoinAt| -> Option<Duration> { panic!("no ping without max_ping_ms") };
        let kept = |filter: RoomFilter| {
            let mut r = rooms.clone();
            filter.apply(&mut r, &never);
            r.into_iter().map(|r| r.id).collect::<Vec<_>>()
        };
        let all = kept(RoomFilter::default());
        assert_eq!(all.len(), 2);
        let first = vec![rooms[0].id];
        assert_eq!(
            kept(RoomFilter {
                owned: true,
                ..Default::default()
            }),
            first
        );
        assert_eq!(
            kept(RoomFilter {
                joinable: true,
                ..Default::default()
            }),
            first
        );
        assert_eq!(
            kept(RoomFilter {
                open: true,
                ..Default::default()
            }),
            first
        );
        assert_eq!(
            kept(RoomFilter {
                core_name: Some("snes9X".into()),
                ..Default::default()
            }),
            vec![rooms[1].id]
        );

        let mut r = rooms.clone();
        let probe = |at: &JoinAt| (at.host == "192.0.2.10").then(|| Duration::from_millis(42));
        RoomFilter {
            max_ping_ms: Some(100),
            ..Default::default()
        }
        .apply(&mut r, &probe);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].ping_ms, Some(42));
    }

    fn ad(content: &str, port: u32, password: u32) -> Vec<u8> {
        let mut p = Vec::new();
        for w in [RESPONSE, 0x3337_EC46, port, password] {
            p.extend(w.to_be_bytes());
        }
        let mut text = |s: &str, len: usize| {
            let mut f = s.as_bytes().to_vec();
            f.resize(len, 0);
            p.extend(f);
        };
        text("Player2", 32);
        text("Linux aarch64", 32);
        text("FCEUmm", 32);
        text("7d2f0b6", 32);
        text("1.22.2", 32);
        text(content, 256);
        text("N/A", 256);
        p
    }

    #[test]
    fn reads_a_lan_hosts_answer() {
        let from: SocketAddr = "192.168.1.50:55435".parse().unwrap();
        let room = parse_ad(&ad("Super Mario Bros. (World).nes", 55435, 2), from).unwrap();
        assert!(room.lan);
        assert_eq!(room.username, "Player2");
        assert_eq!(room.game_name, "Super Mario Bros. (World)");
        assert_eq!(room.game_crc.as_deref(), Some("3337EC46"));
        assert_eq!(room.core_name, "FCEUmm");
        assert!(!room.has_password && room.has_spectate_password);
        assert_eq!(room.join.as_ref().unwrap().host, "192.168.1.50");
        assert!(room.id < 0);
        assert_eq!(parse_ad(&ad("N/A", 55435, 0), from), None, "no game loaded");
        assert_eq!(parse_ad(&ad("x.nes", 0, 0), from), None, "no port");
        assert_eq!(parse_ad(&ad("x.nes", 1, 0)[..100], from), None, "short");
    }

    #[test]
    fn discovery_asks_by_broadcast_and_keeps_each_answer_once() {
        // A host on this machine: answers the query (to wherever it came from) twice.
        let host = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(_) => return, // the port is taken here (a RetroArch running): nothing to test
        };
        let answer = std::thread::spawn(move || {
            let mut buf = [0u8; 16];
            host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let Ok((n, from)) = host.recv_from(&mut buf) else {
                return false;
            };
            let ok = buf[..n] == QUERY.to_be_bytes();
            for _ in 0..2 {
                host.send_to(&ad("Zelda.nes", 55435, 0), from).unwrap();
            }
            ok
        });
        let rooms = discover_lan(Duration::from_millis(800));
        let asked = answer.join().unwrap();
        if !asked {
            return; // broadcasts don't reach this machine's own sockets here
        }
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].game_name, "Zelda");
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
