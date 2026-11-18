//! What is inside a `.zip` or `.7z`: DATs and RetroAchievements list the ROM, not its archive.
//!
//! A single-file archive is identified as the file inside it. An archive with several files is
//! identified as the one it is named after (`Game (USA).zip` holding `Game (USA).nes` and the
//! game's other versions, as merged sets have it); otherwise (an arcade set, a ROM with a readme)
//! it is not guessed at: the caller hashes the archive itself and reports why. macOS metadata (`__MACOSX/`, `.DS_Store`) does not count as a file.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{Error, Result};
use crate::hash::{Hashes, hash_reader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contents {
    /// One ROM inside: its name, hashes and uncompressed size.
    Single {
        name: String,
        hashes: Hashes,
        size: u64,
        /// The ROM's bytes, when asked for and no larger than the limit given.
        data: Option<Vec<u8>>,
    },
    /// More than one file: which one is the ROM is not ours to guess.
    Several(usize),
    Empty,
}

/// `None` when `path` is not an archive romdex reads. With `keep`, a single ROM no larger than
/// that many bytes is also returned in memory, read once (for rcheevos, which needs it whole).
pub fn inspect(path: &Path, keep: Option<u64>) -> Option<Result<Contents>> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "zip" => Some(zip(path, keep)),
        "7z" => Some(seven_z(path, keep)),
        _ => None,
    }
}

/// A ROM read out of an archive: its hashes, size, and its bytes when kept.
type ReadRom = (Hashes, u64, Option<Vec<u8>>);

/// Hashes `reader`, keeping its bytes when `keep_up_to` allows `size` of them.
fn read_rom(reader: impl Read, size: u64, keep_up_to: Option<u64>) -> std::io::Result<ReadRom> {
    if keep_up_to.is_some_and(|max| size <= max) {
        let mut data = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        let mut reader = reader;
        reader.read_to_end(&mut data)?;
        let (hashes, n) = hash_reader(data.as_slice())?;
        Ok((hashes, n, Some(data)))
    } else {
        let (hashes, n) = hash_reader(reader)?;
        Ok((hashes, n, None))
    }
}

/// Of several files, the one the archive is named after, if just one is: its name without the
/// extension (and without any folder) the archive's own.
fn named_after(path: &Path, names: &[&str]) -> Option<usize> {
    let own = path.file_stem()?.to_str()?;
    let stem = |name: &str| {
        let file = name.rsplit('/').next().unwrap_or(name);
        file.rsplit_once('.')
            .map_or(file, |(stem, _)| stem)
            .to_owned()
    };
    let mut found = names.iter().enumerate().filter(|(_, n)| stem(n) == own);
    let (at, _) = found.next()?;
    found.next().is_none().then_some(at)
}

fn is_junk(name: &str) -> bool {
    name.starts_with("__MACOSX/") || name.rsplit('/').next() == Some(".DS_Store")
}

fn zip(path: &Path, keep: Option<u64>) -> Result<Contents> {
    let file = File::open(path).map_err(|e| Error::io(path, e))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| bad(path, e))?;
    let mut roms = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| bad(path, e))?;
        let name = entry.name().map_err(|e| bad(path, e))?.into_owned();
        if entry.is_file() && !is_junk(&name) {
            roms.push((i, name));
        }
    }
    if roms.len() > 1 {
        let names: Vec<&str> = roms.iter().map(|(_, n)| n.as_str()).collect();
        if let Some(at) = named_after(path, &names) {
            roms = vec![roms.swap_remove(at)];
        }
    }
    match roms.as_slice() {
        [] => Ok(Contents::Empty),
        [(index, name)] => {
            let entry = archive.by_index(*index).map_err(|e| bad(path, e))?;
            let expected = entry.crc32();
            let declared = entry.size();
            let (hashes, size, data) =
                read_rom(entry, declared, keep).map_err(|e| Error::io(path, e))?;
            // The directory's CRC32 is the archive's own claim; a mismatch is a damaged file.
            if hashes.crc32 != format!("{expected:08x}") {
                return Err(bad(
                    path,
                    format!("{name}: CRC32 does not match the zip directory"),
                ));
            }
            Ok(Contents::Single {
                name: name.clone(),
                hashes,
                size,
                data,
            })
        }
        several => Ok(Contents::Several(several.len())),
    }
}

fn seven_z(path: &Path, keep: Option<u64>) -> Result<Contents> {
    let mut reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
        .map_err(|e| bad(path, e))?;
    let mut roms: Vec<String> = reader
        .archive()
        .files
        .iter()
        .filter(|f| f.has_stream() && !f.is_directory() && !is_junk(f.name()))
        .map(|f| f.name().to_owned())
        .collect();
    if roms.len() > 1 {
        let names: Vec<&str> = roms.iter().map(String::as_str).collect();
        if let Some(at) = named_after(path, &names) {
            roms = vec![roms.swap_remove(at)];
        }
    }
    let [name] = roms.as_slice() else {
        return Ok(if roms.is_empty() {
            Contents::Empty
        } else {
            Contents::Several(roms.len())
        });
    };
    let mut found: Option<std::io::Result<ReadRom>> = None;
    reader
        .for_each_entries(|entry, data: &mut dyn Read| {
            if entry.name() == name {
                found = Some(read_rom(data, entry.size(), keep));
                return Ok(false);
            }
            // In a solid archive the entries share one stream: read past the ones not wanted.
            std::io::copy(data, &mut std::io::sink())?;
            Ok(true)
        })
        .map_err(|e| bad(path, e))?;
    match found {
        Some(Ok((hashes, size, data))) => Ok(Contents::Single {
            name: name.clone(),
            hashes,
            size,
            data,
        }),
        Some(Err(e)) => Err(Error::io(path, e)),
        None => Err(bad(path, format!("{name}: listed but not found"))),
    }
}

fn bad(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::io(
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub const ROM: &[u8] = b"The quick brown fox jumps over the lazy dog";

    pub fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write_7z(path: &Path, files: &[(&str, &[u8])]) {
        let mut writer = sevenz_rust2::ArchiveWriter::create(path).unwrap();
        for (name, body) in files {
            writer
                .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(*body))
                .unwrap();
        }
        writer.finish().unwrap();
    }

    fn single(contents: Contents) -> (String, Hashes, u64) {
        match contents {
            Contents::Single {
                name, hashes, size, ..
            } => (name, hashes, size),
            other => panic!("expected one ROM, got {other:?}"),
        }
    }

    #[test]
    fn a_zip_with_one_rom_is_identified_as_the_rom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Game.zip");
        write_zip(
            &path,
            &[("Game (USA).nes", ROM), ("__MACOSX/._Game (USA).nes", b"x")],
        );
        let (name, hashes, size) = single(inspect(&path, None).unwrap().unwrap());
        assert_eq!(name, "Game (USA).nes");
        assert_eq!(hashes.crc32, "414fa339");
        assert_eq!(size, ROM.len() as u64);
    }

    #[test]
    fn a_7z_with_one_rom_is_identified_as_the_rom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Game.7z");
        write_7z(&path, &[("Game (USA).sfc", ROM)]);
        let (name, hashes, _) = single(inspect(&path, None).unwrap().unwrap());
        assert_eq!(name, "Game (USA).sfc");
        assert_eq!(hashes.md5, "9e107d9d372bb6826bd81d3542a419d6");
    }

    #[test]
    fn of_several_versions_the_one_the_archive_is_named_after_is_the_rom() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("Game (USA).zip");
        write_zip(
            &zip,
            &[
                ("Game (Europe).nes", b"eu"),
                ("Game (USA).nes", ROM),
                ("Game (Japan).nes", b"jp"),
            ],
        );
        let (name, hashes, _) = single(inspect(&zip, None).unwrap().unwrap());
        assert_eq!(name, "Game (USA).nes");
        assert_eq!(hashes.crc32, "414fa339");
        let seven = dir.path().join("Game (USA).7z");
        write_7z(
            &seven,
            &[("Game (Japan).sfc", b"jp"), ("Game (USA).sfc", ROM)],
        );
        let (name, _, _) = single(inspect(&seven, None).unwrap().unwrap());
        assert_eq!(name, "Game (USA).sfc");
        // Two files by that name (a ROM and its readme): still not guessed.
        let two = dir.path().join("Game.zip");
        write_zip(&two, &[("Game.nes", b"1"), ("Game.txt", b"2")]);
        assert_eq!(inspect(&two, None).unwrap().unwrap(), Contents::Several(2));
    }

    #[test]
    fn several_files_are_counted_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("set.zip");
        write_zip(&zip, &[("a.bin", b"1"), ("b.bin", b"2")]);
        assert_eq!(inspect(&zip, None).unwrap().unwrap(), Contents::Several(2));
        let seven = dir.path().join("set.7z");
        write_7z(&seven, &[("a.bin", b"1"), ("b.bin", b"2"), ("c.bin", b"3")]);
        assert_eq!(
            inspect(&seven, None).unwrap().unwrap(),
            Contents::Several(3)
        );
    }

    #[test]
    fn a_damaged_or_fake_archive_is_an_error_and_others_are_not_archives() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.zip");
        std::fs::write(&fake, b"not a zip").unwrap();
        assert!(inspect(&fake, None).unwrap().is_err());
        assert!(inspect(Path::new("Game.nes"), None).is_none());
    }

    #[test]
    fn keeps_the_rom_in_memory_only_when_asked_and_small_enough() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Game.zip");
        write_zip(&path, &[("Game.nes", ROM)]);
        let data = |keep| match inspect(&path, keep).unwrap().unwrap() {
            Contents::Single { data, .. } => data,
            other => panic!("{other:?}"),
        };
        assert_eq!(data(Some(1024)).as_deref(), Some(ROM));
        assert_eq!(
            data(Some(10)),
            None,
            "larger than the limit: streamed, not kept"
        );
        assert_eq!(data(None), None);
    }
}
