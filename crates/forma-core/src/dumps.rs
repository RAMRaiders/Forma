//! Telling whether each game's file is a catalogued dump (#115): its hashes (romdex, in the
//! background hasher) looked up in the No-Intro, Redump and TOSEC DATs of its system.
//!
//! The DATs are libretro-database's copies, fetched for the systems present only, kept in
//! `system/forma/dats/` and fetched again when a month old; one that cannot be fetched keeps
//! the copy there is. Nothing is redistributed.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub use romdex::Identified;
use romdex::dat::{self, Confidence, DatFile, Index};

use crate::error::{Error, Result};
use crate::library::Dump;
use crate::net::Http;
use crate::paths::Paths;

/// Forma's setting: check games against the DATs (`true`, the default), fetching them.
pub const SETTING: &str = "library.verifyDumps";

/// A DAT is fetched again after this long.
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// More than the largest DAT libretro-database has (No-Intro's Nintendo DS is about 20 MB).
const DAT_LIMIT: u64 = 64 * 1024 * 1024;

/// Where DATs are kept.
pub fn dat_dir(paths: &Paths) -> PathBuf {
    paths.forma_dir().join("dats")
}

fn dat_path(paths: &Paths, file: &DatFile) -> PathBuf {
    dat_dir(paths).join(file.cache_name())
}

/// Whether a system has any DAT to be checked against.
pub fn has_dats(system: &str) -> bool {
    !dat::dats_for(system).is_empty()
}

/// Makes sure `system`'s DATs are here and less than a month old, fetching what is not.
/// Returns how many were fetched. A DAT that cannot be fetched or read is skipped, with the
/// copy there is kept; the error says which when none is usable.
pub fn fetch(http: &dyn Http, paths: &Paths, system: &str) -> Result<usize> {
    let mut fetched = 0;
    let mut failures = Vec::new();
    for file in dat::dats_for(system) {
        let path = dat_path(paths, &file);
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|at| {
                SystemTime::now()
                    .duration_since(at)
                    .unwrap_or(Duration::ZERO)
                    < MAX_AGE
            });
        if fresh {
            continue;
        }
        match http.download(&file.url(), DAT_LIMIT) {
            Ok(body) => {
                let text = String::from_utf8_lossy(&body);
                if let Err(e) = dat::parse(&text) {
                    failures.push(format!("{}: {e}", file.name));
                    continue;
                }
                write_atomically(&path, &body)?;
                fetched += 1;
            }
            Err(e) => failures.push(format!("{}: {}", file.name, e.message)),
        }
    }
    let usable = dat::dats_for(system)
        .iter()
        .any(|f| dat_path(paths, f).is_file());
    if !usable && !failures.is_empty() {
        return Err(Error::Network(format!(
            "no DAT for {system}: {}",
            failures.join("; ")
        )));
    }
    Ok(fetched)
}

fn write_atomically(path: &std::path::Path, body: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(path);
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let tmp = path.with_extension("dat.part");
    std::fs::write(&tmp, body).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// `system`'s DATs, from those fetched; `None` when there is none to check against.
pub fn index(paths: &Paths, system: &str) -> Option<Index> {
    let mut index = Index::new();
    for file in dat::dats_for(system) {
        let Ok(bytes) = std::fs::read(dat_path(paths, &file)) else {
            continue;
        };
        match dat::parse(&String::from_utf8_lossy(&bytes)) {
            Ok(parsed) => index.add(file.catalogue, parsed),
            Err(e) => eprintln!("forma: {}: {e}", file.name),
        }
    }
    (!index.is_empty()).then_some(index)
}

/// Each identified file's dump check, by path relative to its system's folder, with its SHA1.
pub fn check(index: &Index, found: &[Identified]) -> Vec<(String, String, Dump)> {
    found
        .iter()
        .map(|item| {
            let dump = match index.lookup(item.size, &item.hashes) {
                Some(m) => Dump {
                    status: if m.bad {
                        "badDump"
                    } else if m.confidence == Confidence::Crc32 {
                        "crcOnly"
                    } else {
                        "verified"
                    }
                    .to_owned(),
                    catalogue: Some(m.catalogue.as_str().to_owned()),
                    name: Some(m.game.to_owned()),
                    region: m.region.map(str::to_owned),
                },
                None => Dump {
                    status: "notCatalogued".to_owned(),
                    catalogue: None,
                    name: None,
                    region: None,
                },
            };
            (item.path.clone(), item.hashes.sha1.clone(), dump)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;
    use romdex::hash::Hashes;

    fn paths(dir: &std::path::Path) -> Paths {
        Paths {
            userdata: dir.to_path_buf(),
            ..crate::test_support::fixture_paths()
        }
    }

    fn item(path: &str, crc32: &str, sha1: &str, size: u64) -> Identified {
        Identified {
            system: "nes".into(),
            path: path.into(),
            inner: None,
            size,
            hashes: Hashes {
                crc32: crc32.into(),
                md5: String::new(),
                sha1: sha1.into(),
                headerless: None,
            },
            ra: None,
        }
    }

    #[test]
    fn fetches_a_systems_dats_once_then_checks_its_games() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        // No-Intro's and TOSEC's (the same dumps here: No-Intro, asked first, names them).
        let http = FakeHttp::default()
            .fixture(
                "no-intro/Nintendo%20-%20Nintendo%20Entertainment%20System.dat",
                "dats/nes.dat",
            )
            .fixture(
                "tosec/Nintendo%20-%20Nintendo%20Entertainment%20System.dat",
                "dats/nes.dat",
            );
        assert_eq!(fetch(&http, &paths, "nes").unwrap(), 2);
        assert_eq!(
            fetch(&http, &paths, "nes").unwrap(),
            0,
            "a month before it is old"
        );
        assert_eq!(http.calls(), 2);

        let index = index(&paths, "nes").unwrap();
        let checked = check(
            &index,
            &[
                item(
                    "Super Mario Bros. (World).nes",
                    "414fa339",
                    "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12",
                    43,
                ),
                item("Hack.nes", "deadbeef", &"0".repeat(40), 43),
            ],
        );
        assert_eq!(checked[0].2.status, "verified");
        assert_eq!(checked[0].2.catalogue.as_deref(), Some("no-intro"));
        assert_eq!(
            checked[0].2.name.as_deref(),
            Some("Super Mario Bros. (World)")
        );
        assert_eq!(checked[1].2.status, "notCatalogued");
        assert_eq!(checked[1].2.catalogue, None);
    }

    #[test]
    fn a_dat_that_cannot_be_had_is_an_error_only_without_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let http = FakeHttp::default().fail("libretro-database", None);
        assert!(fetch(&http, &paths, "nes").is_err());
        assert!(index(&paths, "nes").is_none());
        assert_eq!(fetch(&http, &paths, "mame").unwrap(), 0, "no DAT to fetch");
        assert!(!has_dats("mame"));
    }
}
