//! Matching games to RetroAchievements by hash (#41): romdex hashes each ROM as rcheevos does
//! (reading a file again only when its size or time changed), and each hash is looked up in
//! RetroAchievements' list of the console's games and hashes. Imported hashes and ids win
//! over these (see [`crate::library::Source::Forma`]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use romdex::batocera::Target;

use crate::error::Result;
use crate::library::{Game, Library};
use crate::net::Cache;
use crate::net::Http;
use crate::net::achievements::{Account, games_with_achievements, has_achievements, hash_index};
use crate::options::conf;
use crate::paths::Paths;

/// romdex's cache of what it read: path, size and time to hashes.
pub fn cache_file(paths: &Paths) -> PathBuf {
    paths.forma_dir().join("romdex.sqlite")
}

/// romdex's hash cache, opened (created) where [`cache_file`] says.
pub type HashCache = romdex::cache::Cache;

pub fn open_cache(paths: &Paths) -> Result<HashCache> {
    let file = cache_file(paths);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|source| crate::Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    Ok(HashCache::open(&file)?)
}

/// Formats rcheevos cannot hash yet: reading them (whole disc images, gigabytes each) would
/// find nothing. CHD comes with romdex R2.
const NOT_HASHABLE: &[&str] = &[".chd"];

/// The systems RetroAchievements supports, as romdex reads them from Batocera's systems,
/// without the formats rcheevos cannot hash yet.
pub fn targets(paths: &Paths) -> Vec<Target> {
    let (targets, _) = romdex::batocera::targets(&paths.userdata, &paths.systems_cfg);
    targets
        .into_iter()
        .filter(|t| romdex::ra::console_for(&t.system).is_some())
        .map(|mut t| {
            t.extensions.retain(|e| !NOT_HASHABLE.contains(&e.as_str()));
            t
        })
        .filter(|t| !t.extensions.is_empty())
        .collect()
}

/// One system's RA hashes, by path relative to its folder. Reads only changed files.
pub fn hash_system(
    target: &Target,
    cache: &mut HashCache,
) -> Result<(Vec<(String, String)>, romdex::Report)> {
    let (found, report) = romdex::identify(std::slice::from_ref(target), cache)?;
    let hashes = found
        .into_iter()
        .filter_map(|i| Some((i.path, i.ra?.to_ascii_lowercase())))
        .collect();
    Ok((hashes, report))
}

/// Games hashed but not matched yet, by RA console: `(game id, hash)`.
pub type Unmatched = BTreeMap<u32, Vec<(i64, String)>>;

pub fn unmatched(lib: &Library) -> Result<Unmatched> {
    let mut by_console = Unmatched::new();
    for (id, system, hash) in lib.unmatched_ra_hashes()? {
        if let Some(console) = romdex::ra::console_for(&system) {
            by_console
                .entry(console.0)
                .or_default()
                .push((id, hash.to_ascii_lowercase()));
        }
    }
    Ok(by_console)
}

/// Looks `unmatched` up, one request per console (cached for a week): `(game id, RA id)`.
/// Needs no library, so it can run without holding it.
pub fn look_up(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    unmatched: Unmatched,
) -> Result<Vec<(i64, i64)>> {
    let mut matched = Vec::new();
    for (console, games) in unmatched {
        let index = hash_index(http, cache, console)?.data;
        let with = games_with_achievements(http, cache, account, console)?.data;
        matched.extend(games.into_iter().filter_map(|(id, hash)| {
            let ra = *index.get(&hash)?;
            with.contains(&ra).then_some((id, ra))
        }));
    }
    Ok(matched)
}

/// [`unmatched`], [`look_up`], then stores the matches. Returns how many were matched.
pub fn resolve(
    lib: &mut Library,
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
) -> Result<usize> {
    let matched = look_up(http, cache, account, unmatched(lib)?)?;
    lib.set_ra_game_ids(&matched)?;
    Ok(matched.len())
}

/// Before a launch: turns achievements off for a game RetroAchievements' lists say has none,
/// so the emulator does not sign in to ask and show a warning. Only on a clear answer: with
/// the lists missing or old nothing changes. A value the user set for the game is kept.
pub fn before_launch(lib: &Library, paths: &Paths, cache: &Cache, game: &Game) -> Result<()> {
    let file = paths.batocera_conf();
    conf::clear_for_launch(&file)?;
    let settings = conf::read(&file);
    if settings.get("global.retroachievements").map(String::as_str) != Some("1")
        || game.ra_game_id.is_some()
    {
        return Ok(());
    }
    let Some(console) = romdex::ra::console_for(&game.system) else {
        return Ok(());
    };
    let Some(hash) = lib.ra_hash(game.id)? else {
        return Ok(());
    };
    if has_achievements(cache, console.0, &hash) != Some(false) {
        return Ok(());
    }
    // As configgen reads it: the file name alone, without these two characters.
    let name = game.path.rsplit('/').next().unwrap_or(&game.path);
    let name = name.replace(['=', '#'], "");
    let key = format!(
        "{}.retroachievements",
        conf::game_prefix(&game.system, &name)
    );
    if settings.contains_key(&key) {
        return Ok(());
    }
    conf::set_for_launch(&file, &key, "0")
}

/// After the game: the setting's work is done.
pub fn after_launch(paths: &Paths) -> Result<()> {
    conf::clear_for_launch(&paths.batocera_conf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;
    use crate::test_support::fixture_copy;

    #[test]
    fn hashes_a_system_matches_through_retroachievements_and_reports_the_rate() {
        let (dir, paths) = fixture_copy();
        // The fixture ROMs are empty, so alike: each gets contents of its own.
        let roms = paths.userdata.join("roms/nes");
        for (n, entry) in std::fs::read_dir(&roms).unwrap().flatten().enumerate() {
            if entry.path().extension().is_some_and(|e| e == "nes") {
                std::fs::write(entry.path(), vec![n as u8; 16 * 1024]).unwrap();
            }
        }
        let mut lib = Library::open_in_memory().unwrap();
        lib.refresh(&paths).unwrap();

        let conf = paths.batocera_conf();
        let mut text = std::fs::read_to_string(&conf).unwrap();
        text.push_str("global.retroachievements.token=t0k\n");
        std::fs::write(&conf, text).unwrap();
        let account = crate::net::achievements::account(&lib, &paths).unwrap();

        let nes = targets(&paths)
            .into_iter()
            .find(|t| t.system == "nes")
            .expect("nes has an RA console");
        assert!(
            targets(&paths).iter().all(|t| !t.accepts("Disc.chd")),
            "no CHD yet"
        );
        let mut cache = romdex::cache::Cache::in_memory().unwrap();
        let (hashes, report) = hash_system(&nes, &mut cache).unwrap();
        assert_eq!(report.ra_hashed, hashes.len());
        assert!(!hashes.is_empty());
        assert_eq!(lib.set_ra_hashes("nes", &hashes).unwrap(), hashes.len());
        assert_eq!(
            lib.set_ra_hashes("nes", &hashes).unwrap(),
            0,
            "nothing new the second time"
        );

        // RetroAchievements knows the first one.
        let (path, hash) = &hashes[0];
        let list = format!(
            r#"{{"Success": true, "MD5List": {{"{}": 4242}}}}"#,
            hash.to_uppercase()
        );
        let mut http = FakeHttp::default();
        http.routes.insert("hashlibrary", Ok(list.into_bytes()));
        // A game on the list with no achievements does not count as matched.
        http.routes.insert(
            "allprogress",
            Ok(br#"{"Success": true, "Response": {"4242": {"Achievements": 0}}}"#.to_vec()),
        );
        let empty = Cache::new(dir.path().join("empty"));
        assert_eq!(resolve(&mut lib, &http, &empty, &account).unwrap(), 0);
        http.routes.insert(
            "allprogress",
            Ok(br#"{"Success": true, "Response": {"4242": {"Achievements": 12}}}"#.to_vec()),
        );
        http.calls.lock().unwrap().clear();
        let net = Cache::new(dir.path().join("net"));
        // One fixture game has an imported RA id already.
        let before = lib.ra_match_report(&["nes".into()]).unwrap()[0].matched;
        assert_eq!(resolve(&mut lib, &http, &net, &account).unwrap(), 1);
        let game = lib
            .games("nes")
            .unwrap()
            .into_iter()
            .find(|g| &g.path == path)
            .unwrap();
        assert_eq!(game.ra_game_id, Some(4242));

        let rate = lib
            .ra_match_report(&["nes".into(), "nowhere".into()])
            .unwrap();
        assert_eq!(rate.len(), 1);
        assert_eq!(
            (rate[0].hashed, rate[0].matched),
            (hashes.len() as i64, before + 1)
        );
        // The list is cached: matching again asks nothing.
        resolve(&mut lib, &http, &net, &account).unwrap();
        assert_eq!(http.calls(), 2);
    }

    /// `(library, account, a game with a hash but no RA id, its file's key in batocera.conf)`.
    fn launch_fixture() -> (tempfile::TempDir, Paths, Library, Account, Game, String) {
        let (dir, paths) = fixture_copy();
        let roms = paths.userdata.join("roms/nes");
        for (n, entry) in std::fs::read_dir(&roms).unwrap().flatten().enumerate() {
            if entry.path().extension().is_some_and(|e| e == "nes") {
                std::fs::write(entry.path(), vec![n as u8; 16 * 1024]).unwrap();
            }
        }
        let mut lib = Library::open_in_memory().unwrap();
        lib.refresh(&paths).unwrap();
        let conf = paths.batocera_conf();
        let mut text = std::fs::read_to_string(&conf).unwrap();
        text.push_str("global.retroachievements=1\nglobal.retroachievements.token=t0k\n");
        std::fs::write(&conf, text).unwrap();
        let account = crate::net::achievements::account(&lib, &paths).unwrap();
        let nes = targets(&paths)
            .into_iter()
            .find(|t| t.system == "nes")
            .unwrap();
        let mut hashes = romdex::cache::Cache::in_memory().unwrap();
        let (found, _) = hash_system(&nes, &mut hashes).unwrap();
        lib.set_ra_hashes("nes", &found).unwrap();
        let game = lib
            .games("nes")
            .unwrap()
            .into_iter()
            .find(|g| g.ra_game_id.is_none() && found.iter().any(|(p, _)| *p == g.path))
            .unwrap();
        let key = format!("nes[\"{}\"].retroachievements", game.path);
        (dir, paths, lib, account, game, key)
    }

    /// The lists on disk, as matching leaves them: only `listed` has achievements.
    fn lists(dir: &std::path::Path, account: &Account, listed: &str) -> Cache {
        let mut http = FakeHttp::default();
        let body = format!(r#"{{"Success": true, "MD5List": {{"{listed}": 7}}}}"#);
        http.routes.insert("hashlibrary", Ok(body.into_bytes()));
        http.routes.insert(
            "allprogress",
            Ok(br#"{"Success": true, "Response": {"7": {"Achievements": 5}}}"#.to_vec()),
        );
        let cache = Cache::new(dir.join("lists"));
        hash_index(&http, &cache, 7).unwrap();
        games_with_achievements(&http, &cache, account, 7).unwrap();
        cache
    }

    #[test]
    fn a_game_the_lists_rule_out_runs_without_achievements_and_the_setting_goes_after() {
        let (dir, paths, lib, account, game, key) = launch_fixture();
        let before = std::fs::read_to_string(paths.batocera_conf()).unwrap();
        let cache = lists(dir.path(), &account, "00000000000000000000000000000000");

        before_launch(&lib, &paths, &cache, &game).unwrap();
        assert_eq!(
            conf::read(&paths.batocera_conf())
                .get(&key)
                .map(String::as_str),
            Some("0")
        );
        after_launch(&paths).unwrap();
        assert_eq!(
            std::fs::read_to_string(paths.batocera_conf()).unwrap(),
            before
        );

        // A crash left it behind: the next launch of a game with achievements takes it out.
        before_launch(&lib, &paths, &cache, &game).unwrap();
        let hash = lib.ra_hash(game.id).unwrap().unwrap();
        let cache = lists(&dir.path().join("newer"), &account, &hash);
        before_launch(&lib, &paths, &cache, &game).unwrap();
        assert!(!conf::read(&paths.batocera_conf()).contains_key(&key));
    }

    #[test]
    fn without_the_lists_nothing_changes() {
        let (dir, paths, lib, _, game, _) = launch_fixture();
        let before = std::fs::read_to_string(paths.batocera_conf()).unwrap();
        let none = Cache::new(dir.path().join("none"));
        before_launch(&lib, &paths, &none, &game).unwrap();
        assert_eq!(
            std::fs::read_to_string(paths.batocera_conf()).unwrap(),
            before
        );
    }

    #[test]
    fn the_users_own_setting_for_the_game_stays() {
        let (dir, paths, lib, account, game, key) = launch_fixture();
        let conf_file = paths.batocera_conf();
        let mut text = std::fs::read_to_string(&conf_file).unwrap();
        text.push_str(&format!("{key}=1\n"));
        std::fs::write(&conf_file, &text).unwrap();
        let cache = lists(dir.path(), &account, "00000000000000000000000000000000");

        before_launch(&lib, &paths, &cache, &game).unwrap();
        after_launch(&paths).unwrap();
        assert_eq!(std::fs::read_to_string(&conf_file).unwrap(), text);
    }
}
