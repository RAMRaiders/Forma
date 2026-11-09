//! Matching games to RetroAchievements by hash (#41): romdex hashes each ROM as rcheevos does
//! (reading a file again only when its size or time changed), and each hash is looked up in
//! RetroAchievements' list of the console's games and hashes. Imported hashes and ids win
//! over these (see [`crate::library::Source::Forma`]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use romdex::batocera::Target;

use crate::error::Result;
use crate::library::Library;
use crate::net::Cache;
use crate::net::Http;
use crate::net::achievements::hash_index;
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
pub fn look_up(http: &dyn Http, cache: &Cache, unmatched: Unmatched) -> Result<Vec<(i64, i64)>> {
    let mut matched = Vec::new();
    for (console, games) in unmatched {
        let index = hash_index(http, cache, console)?.data;
        matched.extend(
            games
                .into_iter()
                .filter_map(|(id, hash)| index.get(&hash).map(|ra| (id, *ra))),
        );
    }
    Ok(matched)
}

/// [`unmatched`], [`look_up`], then stores the matches. Returns how many were matched.
pub fn resolve(lib: &mut Library, http: &dyn Http, cache: &Cache) -> Result<usize> {
    let matched = look_up(http, cache, unmatched(lib)?)?;
    lib.set_ra_game_ids(&matched)?;
    Ok(matched.len())
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
        let net = Cache::new(dir.path().join("net"));
        // One fixture game has an imported RA id already.
        let before = lib.ra_match_report(&["nes".into()]).unwrap()[0].matched;
        assert_eq!(resolve(&mut lib, &http, &net).unwrap(), 1);
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
        resolve(&mut lib, &http, &net).unwrap();
        assert_eq!(http.calls(), 1);
    }
}
