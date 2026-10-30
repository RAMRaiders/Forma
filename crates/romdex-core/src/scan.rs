//! Finding the games in a system's ROM folder.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::Warning;
use crate::gamelist::relative_to;
use crate::systems::System;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedRom {
    /// Relative to the system's ROM folder.
    pub path: PathBuf,
    pub size: u64,
    /// Modification time, nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
}

/// Deep enough for `system/collection/series/game`, shallow enough that a symlink loop ends.
const MAX_DEPTH: usize = 8;

/// Walks a system folder for games, the way EmulationStation decides what is a game:
///
/// - a file whose name has one of the system's extensions;
/// - a **folder** whose name has one (e.g. `Game.ps3`), which is one game and is not entered;
/// - hidden entries (`.name`) are skipped.
///
/// Files listed by an `.m3u` playlist are left out, so a multi-disc game appears once (as its
/// playlist) rather than once per disc. A missing folder yields no games and no warning: a
/// system without ROMs is normal.
pub fn scan_system(system: &System) -> (Vec<ScannedRom>, Vec<Warning>) {
    let mut roms = Vec::new();
    let mut warnings = Vec::new();
    if system.rom_path.is_dir() {
        walk(system, &system.rom_path, 0, &mut roms, &mut warnings);
    }

    let in_playlists = playlist_members(system, &roms);
    roms.retain(|r| !in_playlists.contains(&r.path));
    roms.sort_by(|a, b| a.path.cmp(&b.path));
    (roms, warnings)
}

fn walk(
    system: &System,
    dir: &Path,
    depth: usize,
    roms: &mut Vec<ScannedRom>,
    warnings: &mut Vec<Warning>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warnings.push(Warning::new(dir, format!("cannot list folder: {e}")));
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            warnings.push(Warning::new(
                &entry.path(),
                "file name is not valid UTF-8; skipped",
            ));
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // Follows symlinks: Batocera users commonly link ROM folders from other drives.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if system.accepts(name) {
            let Some(rel) = relative_to(&system.rom_path, &path) else {
                continue;
            };
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
            roms.push(ScannedRom {
                path: rel,
                size: if meta.is_dir() { 0 } else { meta.len() },
                mtime_ns,
            });
        } else if meta.is_dir() && depth < MAX_DEPTH {
            walk(system, &path, depth + 1, roms, warnings);
        }
    }
}

fn playlist_members(system: &System, roms: &[ScannedRom]) -> HashSet<PathBuf> {
    let mut members = HashSet::new();
    for rom in roms {
        let is_m3u = rom
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("m3u"));
        if !is_m3u {
            continue;
        }
        let full = system.rom_path.join(&rom.path);
        let Ok(text) = std::fs::read_to_string(&full) else {
            continue;
        };
        let base = full.parent().unwrap_or(&system.rom_path);
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rel) = relative_to(&system.rom_path, &base.join(line)) {
                members.insert(rel);
            }
        }
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::load_systems;
    use crate::test_support::fixture_paths;

    fn scan(name: &str) -> Vec<PathBuf> {
        let load = load_systems(&fixture_paths()).unwrap();
        let system = load.systems.iter().find(|s| s.name == name).unwrap();
        let (roms, warnings) = scan_system(system);
        assert!(warnings.is_empty(), "{warnings:?}");
        roms.into_iter().map(|r| r.path).collect()
    }

    #[test]
    fn finds_files_with_the_system_extensions_only() {
        assert_eq!(
            scan("nes"),
            [
                "Homebrew Demo.nes",
                "Super Mario Bros. (World).nes",
                "The Legend of Zelda (USA).nes",
                "Tom & Jerry (USA).nes"
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn a_multi_disc_game_appears_once_as_its_playlist() {
        assert_eq!(scan("psx"), [PathBuf::from("Final Fantasy VII.m3u")]);
    }

    #[test]
    fn a_folder_named_like_a_game_is_one_game() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Game.ps3")).unwrap();
        std::fs::write(dir.path().join("Game.ps3/EBOOT.BIN"), b"").unwrap();
        let system = System {
            name: "ps3".into(),
            fullname: "PS3".into(),
            manufacturer: None,
            release_year: None,
            hardware: None,
            rom_path: dir.path().to_path_buf(),
            extensions: vec![".ps3".into()],
            command: None,
            platforms: vec![],
            theme: None,
            emulators: vec![],
        };
        let (roms, _) = scan_system(&system);
        assert_eq!(roms.len(), 1);
        assert_eq!(roms[0].path, Path::new("Game.ps3"));
    }

    #[test]
    fn a_missing_folder_is_not_an_error() {
        assert!(scan("megadrive").len() == 2);
        let load = load_systems(&fixture_paths()).unwrap();
        let mut gone = load.systems[0].clone();
        gone.rom_path = PathBuf::from("/definitely/not/here");
        let (roms, warnings) = scan_system(&gone);
        assert!(roms.is_empty() && warnings.is_empty());
    }
}
