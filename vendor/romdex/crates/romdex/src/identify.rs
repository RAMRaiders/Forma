//! Identify every ROM in a set of system folders, from the cache where possible.

use serde::Serialize;

use std::path::Path;

use crate::archive::{self, Contents};
use crate::batocera::Target;
use crate::cache::{Cache, Entry, Stamp};
use crate::error::{Error, Result};
use crate::hash::{Hashes, hash_reader};
use crate::ra::{self, Console};
use crate::scan::scan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identified {
    pub system: String,
    /// Relative to the system folder, as the import format keys it.
    pub path: String,
    /// The file inside the archive that was identified, when `path` is a single-file archive.
    pub inner: Option<String>,
    /// Of the ROM: uncompressed, for an archive.
    pub size: u64,
    pub hashes: Hashes,
    /// The RetroAchievements hash, when the system has an RA console and rcheevos could hash
    /// the file. `None` is "unknown", never "no achievements".
    pub ra: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub files: usize,
    /// Read and hashed this time.
    pub hashed: usize,
    /// Unchanged since a previous run: answered from the cache without reading.
    pub cached: usize,
    /// ROM bytes hashed (uncompressed, for archives).
    pub bytes_read: u64,
    /// Files on systems RetroAchievements supports that got an RA hash, and that did not
    /// (rcheevos declined them: unrecognised data, or a format it cannot read yet, like CHD).
    pub ra_hashed: usize,
    pub ra_unknown: usize,
    /// Passed over, with why (unreadable files, folder games…).
    pub skipped: Vec<String>,
}

/// Hashes every ROM in `targets`, reading only files the cache does not already know.
pub fn identify(targets: &[Target], cache: &mut Cache) -> Result<(Vec<Identified>, Report)> {
    let mut out = Vec::new();
    let mut report = Report::default();
    cache.batch(|cache| {
        for target in targets {
            let (files, skipped) = scan(target);
            report.skipped.extend(skipped);
            let console = ra::console_for(&target.system);
            for file in files {
                report.files += 1;
                let stamp = match Stamp::of(&file.full) {
                    Ok(stamp) => stamp,
                    Err(e) => {
                        report.skipped.push(e.to_string());
                        continue;
                    }
                };
                let cached = cache
                    .get(&file.full, stamp)?
                    .filter(|e| e.ra_console == console.map(|c| c.0));
                let entry = match cached {
                    Some(entry) => {
                        report.cached += 1;
                        entry
                    }
                    None => match learn(&file.full, console, &mut report.skipped) {
                        Some(entry) => {
                            report.hashed += 1;
                            report.bytes_read += entry.size;
                            cache.put(&file.full, stamp, &entry)?;
                            entry
                        }
                        None => continue,
                    },
                };
                if console.is_some() {
                    if entry.ra.is_some() {
                        report.ra_hashed += 1;
                    } else {
                        report.ra_unknown += 1;
                    }
                }
                out.push(Identified {
                    system: file.system,
                    path: file.path,
                    inner: entry.inner,
                    size: entry.size,
                    hashes: entry.hashes,
                    ra: entry.ra,
                });
            }
        }
        Ok(())
    })?;
    Ok((out, report))
}

/// The largest archived ROM read into memory for rcheevos: more than any cartridge.
const RA_IN_MEMORY: u64 = 256 * 1024 * 1024;

/// Hashes a file, or the one ROM inside it when it is a single-file archive, and asks
/// rcheevos for its RA hash. `None` (with the reason in `skipped`) when there is nothing to
/// identify.
fn learn(path: &Path, console: Option<Console>, skipped: &mut Vec<String>) -> Option<Entry> {
    let ra_console = console.map(|c| c.0);
    // Arcade is hashed by name; everything else from the ROM's contents.
    let by_name = console.is_some_and(Console::hashes_by_name);
    let keep = console.filter(|_| !by_name).map(|_| RA_IN_MEMORY);
    let ra_of_file = || console.and_then(|c| ra::hash_file(c, path));
    match archive::inspect(path, keep) {
        Some(Ok(Contents::Single {
            name,
            hashes,
            size,
            data,
        })) => {
            let ra = match (console, data) {
                (Some(c), _) if by_name => ra::hash_file(c, path),
                (Some(c), Some(data)) => ra::hash_buffer(c, &data),
                _ => None,
            };
            return Some(Entry {
                hashes,
                inner: Some(name),
                size,
                ra,
                ra_console,
            });
        }
        Some(Ok(Contents::Empty)) => {
            skipped.push(format!("{}: an empty archive", path.display()));
            return None;
        }
        // Arcade sets always hold several files and are identified by name: nothing to report.
        Some(Ok(Contents::Several(n))) if !by_name => skipped.push(format!(
            "{}: {n} files inside, so it is hashed as an archive, not as a ROM",
            path.display()
        )),
        Some(Ok(Contents::Several(_))) => {}
        Some(Err(e)) => {
            skipped.push(e.to_string());
            return None;
        }
        None => {}
    }
    match std::fs::File::open(path).and_then(|f| hash_reader(std::io::BufReader::new(f))) {
        Ok((hashes, size)) => Some(Entry {
            hashes,
            inner: None,
            size,
            ra: ra_of_file(),
            ra_console,
        }),
        Err(e) => {
            skipped.push(Error::io(path, e).to_string());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn library(dir: &Path) -> Vec<Target> {
        let nes = dir.join("roms/nes");
        std::fs::create_dir_all(&nes).unwrap();
        std::fs::write(
            nes.join("A.nes"),
            b"The quick brown fox jumps over the lazy dog",
        )
        .unwrap();
        std::fs::write(nes.join("B.nes"), b"").unwrap();
        vec![Target {
            system: "nes".into(),
            root: nes,
            extensions: vec![".nes".into()],
        }]
    }

    #[test]
    fn hashes_once_then_answers_from_the_cache_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let targets = library(dir.path());
        let mut cache = Cache::open(&dir.path().join("cache.db")).unwrap();

        let (found, first) = identify(&targets, &mut cache).unwrap();
        assert_eq!((first.files, first.hashed, first.cached), (2, 2, 0));
        assert_eq!(found[0].path, "A.nes");
        assert_eq!(found[0].hashes.crc32, "414fa339");

        let (again, second) = identify(&targets, &mut cache).unwrap();
        assert_eq!((second.hashed, second.cached, second.bytes_read), (0, 2, 0));
        assert_eq!(again, found);
    }

    #[test]
    fn identifies_the_rom_inside_an_archive_and_remembers_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut targets = library(dir.path());
        targets[0].extensions.push(".zip".into());
        let nes = dir.path().join("roms/nes");
        crate::archive::tests::write_zip(
            &nes.join("C.zip"),
            &[("C (USA).nes", crate::archive::tests::ROM)],
        );
        crate::archive::tests::write_zip(&nes.join("Set.zip"), &[("a", b"1"), ("b", b"2")]);
        let mut cache = Cache::in_memory().unwrap();

        let (found, report) = identify(&targets, &mut cache).unwrap();
        let c = found.iter().find(|f| f.path == "C.zip").unwrap();
        assert_eq!(c.inner.as_deref(), Some("C (USA).nes"));
        assert_eq!(c.hashes.crc32, "414fa339", "the ROM's CRC, not the zip's");
        assert_eq!(c.size, 43);
        let set = found.iter().find(|f| f.path == "Set.zip").unwrap();
        assert_eq!(set.inner, None);
        assert!(report.skipped.iter().any(|s| s.contains("2 files inside")));

        let (again, _) = identify(&targets, &mut cache).unwrap();
        assert_eq!(
            again, found,
            "the inner name and size come back from the cache"
        );
    }

    #[test]
    fn computes_ra_hashes_for_files_and_archived_roms_and_counts_the_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let nes = dir.path().join("roms/nes");
        std::fs::create_dir_all(&nes).unwrap();
        let mut rom = vec![b'N', b'E', b'S', 0x1a];
        rom.resize(16, 0);
        rom.extend_from_slice(&[0u8; 32]);
        std::fs::write(nes.join("Plain.nes"), &rom).unwrap();
        crate::archive::tests::write_zip(&nes.join("Zipped.zip"), &[("Zipped.nes", &rom)]);
        let ports = dir.path().join("roms/ports");
        std::fs::create_dir_all(&ports).unwrap();
        std::fs::write(ports.join("Game.sh"), b"#!/bin/sh").unwrap();
        let targets = vec![
            Target {
                system: "nes".into(),
                root: nes,
                extensions: vec![".nes".into(), ".zip".into()],
            },
            Target {
                system: "ports".into(),
                root: ports,
                extensions: vec![".sh".into()],
            },
        ];
        let mut cache = Cache::in_memory().unwrap();
        let (found, report) = identify(&targets, &mut cache).unwrap();

        let ra = |path: &str| found.iter().find(|f| f.path == path).unwrap().ra.clone();
        let header_skipped = Some("70bc8f4b72a86921468bf8e8441dce51".to_owned());
        assert_eq!(ra("Plain.nes"), header_skipped);
        assert_eq!(
            ra("Zipped.zip"),
            header_skipped,
            "the ROM inside, by rcheevos' NES rule"
        );
        assert_eq!(ra("Game.sh"), None, "ports has no RA console");
        assert_eq!((report.ra_hashed, report.ra_unknown), (2, 0));

        let (again, cached) = identify(&targets, &mut cache).unwrap();
        assert_eq!(again, found);
        assert_eq!((cached.cached, cached.ra_hashed), (3, 2));
    }

    #[test]
    fn a_changed_file_is_hashed_again() {
        let dir = tempfile::tempdir().unwrap();
        let targets = library(dir.path());
        let mut cache = Cache::in_memory().unwrap();
        identify(&targets, &mut cache).unwrap();
        std::fs::write(dir.path().join("roms/nes/B.nes"), b"changed").unwrap();
        let (_, report) = identify(&targets, &mut cache).unwrap();
        assert_eq!((report.hashed, report.cached), (1, 1));
    }
}
