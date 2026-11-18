//! Identify every ROM in a set of system folders, from the cache where possible.

use serde::Serialize;

use std::path::Path;

use crate::archive::{self, Contents};
use crate::batocera::Target;
use crate::cache::{Cache, Entry, Stamp};
use crate::chd;
use crate::error::{Error, Result};
use crate::hash::{Hashes, hash_reader};
use crate::ra::{self, Console};
use crate::scan::scan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identified {
    pub system: String,
    /// Relative to the system folder, as the import format keys it.
    pub path: String,
    /// The file that was identified, when not `path` itself: the one inside a single-file
    /// archive, a CUE sheet's first track, or a CHD's (`Track 01`).
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
    /// (rcheevos declined them: unrecognised data, or a disc it cannot find the boot files of).
    pub ra_hashed: usize,
    pub ra_unknown: usize,
    /// Passed over, with why (unreadable files, folder games…).
    pub skipped: Vec<String>,
}

/// A file about to be read (not one the cache answers): of a system's files to read, which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'a> {
    pub system: &'a str,
    /// Relative to the system folder.
    pub path: &'a str,
    /// Files of this system read before this one, and how many it has to read.
    pub done: usize,
    pub total: usize,
}

/// Hashes every ROM in `targets`, reading only files the cache does not already know.
pub fn identify(targets: &[Target], cache: &mut Cache) -> Result<(Vec<Identified>, Report)> {
    identify_with(targets, cache, &mut |_| {})
}

/// [`identify`], told of each file as it is read: the cache's answers come first, so
/// `total` is known before the first file read.
pub fn identify_with(
    targets: &[Target],
    cache: &mut Cache,
    on_read: &mut dyn FnMut(Progress<'_>),
) -> Result<(Vec<Identified>, Report)> {
    let mut out = Vec::new();
    let mut report = Report::default();
    cache.batch(|cache| {
        for target in targets {
            let (files, skipped) = scan(target);
            report.skipped.extend(skipped);
            let console = ra::console_for(&target.system);
            // Each file with what the cache knows of it, to count the ones to read first.
            let mut known = Vec::with_capacity(files.len());
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
                    .filter(|e| e.ra_console == console.map(|c| c.0))
                    // Cached before CUE sheets and CHDs were read to their first track.
                    .filter(|e| {
                        !((is_cue(&file.full) || chd::is_chd(&file.full)) && e.inner.is_none())
                    });
                known.push((file, stamp, cached));
            }
            let total = known.iter().filter(|(_, _, c)| c.is_none()).count();
            let mut done = 0;
            for (file, stamp, cached) in known {
                let entry = match cached {
                    Some(entry) => {
                        report.cached += 1;
                        entry
                    }
                    None => {
                        on_read(Progress {
                            system: &target.system,
                            path: &file.path,
                            done,
                            total,
                        });
                        done += 1;
                        let Some(entry) = learn(&file.full, console, &mut report.skipped) else {
                            continue;
                        };
                        report.hashed += 1;
                        report.bytes_read += entry.size;
                        cache.put(&file.full, stamp, &entry)?;
                        entry
                    }
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
    // A CHD is a compressed disc: its first track, uncompressed, is the `.bin` DATs list.
    if chd::is_chd(path) {
        return match chd::hash_first_track(path) {
            Ok((hashes, size, name)) => Some(Entry {
                hashes,
                inner: Some(name),
                size,
                ra: ra_of_file(),
                ra_console,
            }),
            Err(e) => {
                skipped.push(format!("{}: {e}", path.display()));
                None
            }
        };
    }
    // A CUE sheet is a few lines of text: the disc is its tracks, and DATs list the first.
    if let Some(track) = is_cue(path).then(|| first_track(path)).flatten() {
        return match std::fs::File::open(&track)
            .and_then(|f| hash_reader(std::io::BufReader::new(f)))
        {
            Ok((hashes, size)) => Some(Entry {
                hashes,
                inner: track.file_name().map(|n| n.to_string_lossy().into_owned()),
                size,
                ra: ra_of_file(),
                ra_console,
            }),
            Err(e) => {
                skipped.push(Error::io(&track, e).to_string());
                None
            }
        };
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

fn is_cue(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cue"))
}

/// The first `FILE` a CUE sheet names, beside it: the first track, data on every disc a DAT
/// lists. `None` for a sheet with no file in it.
fn first_track(cue: &Path) -> Option<std::path::PathBuf> {
    let text = std::fs::read(cue).ok()?;
    let text = String::from_utf8_lossy(&text);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.len() > 5 && l[..5].eq_ignore_ascii_case("FILE "))?;
    let rest = line[5..].trim();
    let name = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        // Unquoted: the name, then the file type.
        rest.rsplit_once(char::is_whitespace)
            .map_or(rest, |(n, _)| n.trim())
    };
    // Only beside the sheet: a name never reaches out of its folder.
    let file = Path::new(name).file_name()?;
    Some(cue.with_file_name(file))
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
    fn tells_of_each_file_read_counting_only_those_the_cache_does_not_answer() {
        let dir = tempfile::tempdir().unwrap();
        let targets = library(dir.path());
        let mut cache = Cache::in_memory().unwrap();
        let mut seen = Vec::new();
        identify_with(&targets, &mut cache, &mut |p| {
            seen.push((p.system.to_owned(), p.path.to_owned(), p.done, p.total));
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                ("nes".to_owned(), "A.nes".to_owned(), 0, 2),
                ("nes".to_owned(), "B.nes".to_owned(), 1, 2)
            ]
        );
        // All in the cache now: nothing read, nothing told.
        seen.clear();
        identify_with(&targets, &mut cache, &mut |p| {
            seen.push((String::new(), String::new(), p.done, p.total))
        })
        .unwrap();
        assert!(seen.is_empty());
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

#[cfg(test)]
mod cue_tests {
    use super::*;

    #[test]
    fn a_cue_sheet_is_identified_by_its_first_track() {
        let dir = tempfile::tempdir().unwrap();
        let psx = dir.path().join("roms/psx");
        std::fs::create_dir_all(&psx).unwrap();
        std::fs::write(
            psx.join("Game (Europe).cue"),
            "FILE \"Game (Europe) (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\nFILE \"Game (Europe) (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n",
        )
        .unwrap();
        std::fs::write(
            psx.join("Game (Europe) (Track 1).bin"),
            b"The quick brown fox jumps over the lazy dog",
        )
        .unwrap();
        std::fs::write(psx.join("Game (Europe) (Track 2).bin"), b"audio").unwrap();
        let targets = vec![Target {
            system: "psx".into(),
            root: psx,
            extensions: vec![".cue".into()],
        }];
        let mut cache = Cache::in_memory().unwrap();
        let (found, _) = identify(&targets, &mut cache).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].hashes.crc32, "414fa339");
        assert_eq!(found[0].size, 43);
        assert_eq!(
            found[0].inner.as_deref(),
            Some("Game (Europe) (Track 1).bin")
        );
    }

    #[test]
    fn a_chd_is_identified_by_its_first_track_with_the_ra_hash_of_its_cue_sheet() {
        use crate::chd::tests::{TestTrack, mode1, write_disc};
        let dir = tempfile::tempdir().unwrap();
        let saturn = dir.path().join("roms/saturn");
        std::fs::create_dir_all(&saturn).unwrap();
        let sectors = (0..20u32)
            .map(|lba| {
                let mut data = vec![lba as u8; 2048];
                data[..16].copy_from_slice(b"SEGA SEGASATURN ");
                mode1(lba, &data)
            })
            .collect();
        let track = TestTrack {
            cue_mode: "MODE1/2352",
            chd_type: "MODE1_RAW",
            sectors,
            pregap: 0,
        };
        let (cue, _) = write_disc(&saturn, "Game", &[track]);
        let targets = |ext: &str| {
            vec![Target {
                system: "saturn".into(),
                root: saturn.clone(),
                extensions: vec![ext.into()],
            }]
        };
        let mut cache = Cache::in_memory().unwrap();
        let (chd, report) = identify(&targets(".chd"), &mut cache).unwrap();
        let (from_cue, _) = identify(&targets(".cue"), &mut cache).unwrap();
        assert_eq!((chd.len(), report.ra_hashed), (1, 1));
        assert_eq!(chd[0].inner.as_deref(), Some("Track 01"));
        assert_eq!(chd[0].size, 20 * 2352);
        assert_eq!(
            chd[0].hashes, from_cue[0].hashes,
            "the first track, as its .bin"
        );
        assert_eq!(chd[0].ra, from_cue[0].ra);
        assert!(cue.is_file());
    }

    #[test]
    fn reads_unquoted_names_and_stays_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("a.cue");
        std::fs::write(&cue, "REM x\nfile track1.bin BINARY\n").unwrap();
        assert_eq!(first_track(&cue), Some(dir.path().join("track1.bin")));
        std::fs::write(&cue, "FILE \"../../etc/passwd\" BINARY\n").unwrap();
        assert_eq!(first_track(&cue), Some(dir.path().join("passwd")));
        std::fs::write(&cue, "TRACK 01 AUDIO\n").unwrap();
        assert_eq!(first_track(&cue), None);
    }
}
