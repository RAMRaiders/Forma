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
/// playlist) rather than once per disc. So are the discs of a game with a playlist that the
/// playlist forgot (one disc listed twice, another not at all). A missing folder yields no
/// games and no warning: a system without ROMs is normal.
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
    // The games that have a playlist: each disc's folder and name without its disc.
    let mut games: HashSet<(PathBuf, String)> = HashSet::new();
    for rom in roms {
        if !is_m3u(&rom.path) {
            continue;
        }
        let name = stem(&rom.path);
        let folder = rom.path.parent().unwrap_or(Path::new("")).to_path_buf();
        games.insert((folder, without_disc(&name).unwrap_or_else(|| fold(&name))));
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
                if let Some(game) = without_disc(&stem(&rel)) {
                    games.insert((rel.parent().unwrap_or(Path::new("")).to_path_buf(), game));
                }
                members.insert(rel);
            }
        }
    }
    for rom in roms.iter().filter(|r| !is_m3u(&r.path)) {
        let Some(game) = without_disc(&stem(&rom.path)) else {
            continue;
        };
        let folder = rom.path.parent().unwrap_or(Path::new("")).to_path_buf();
        if games.contains(&(folder, game)) {
            members.insert(rom.path.clone());
        }
    }
    members
}

fn is_m3u(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lower case, single spaces.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a tag names one disc of a game: `Disc 2`, `Disk 1 of 3`, `CD2`, `Side B`, `Tape 1`.
fn is_disc(tag: &str) -> bool {
    let tag = tag.trim().to_lowercase();
    ["disc", "disk", "cd", "side", "tape", "part"]
        .iter()
        .any(|word| {
            let Some(rest) = tag.strip_prefix(word) else {
                return false;
            };
            let first = rest.split_whitespace().next().unwrap_or_default();
            !first.is_empty()
                && (first.chars().all(|c| c.is_ascii_digit())
                    || (first.len() == 1 && first.chars().all(|c| c.is_ascii_alphabetic())))
        })
}

/// A disc's file name without its disc, folded (`Game (USA) (Disc 2)` and `Game (US) - CD2`
/// are `game (usa)` and `game (us)`): what its game's other discs and playlist share. `None`
/// when the name names no disc.
fn without_disc(stem: &str) -> Option<String> {
    let mut kept = String::new();
    let mut found = false;
    let mut rest = stem;
    // Bracketed: `(Disc 2)`, `[CD 2]`.
    while let Some(open) = rest.find(['(', '[']) {
        let close = if rest[open..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(len) = rest[open..].find(close) else {
            break;
        };
        let tag = &rest[open + 1..open + len];
        kept.push_str(&rest[..open]);
        if is_disc(tag) {
            found = true;
        } else {
            kept.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    kept.push_str(rest);
    // After a dash at the end: `Game - CD2`.
    if let Some((before, last)) = kept.rsplit_once(" - ")
        && is_disc(last)
    {
        kept = before.to_owned();
        found = true;
    }
    found.then(|| fold(&kept))
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
    fn a_disc_its_playlist_forgot_is_left_out_too() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "Metal Gear Solid (US) - CD1.chd",
            "Metal Gear Solid (US) - CD2.chd",
            "Metal Gear Solid - VR Missions (USA).chd",
            "X-Files, The (USA) (Disc 1).chd",
            "X-Files, The (USA) (Disc 2).chd",
            "Lone (Japan) (Disc 1).chd",
            "Lone (Japan) (Disc 2).chd",
        ] {
            std::fs::write(dir.path().join(f), b"").unwrap();
        }
        // CD1 twice and a CD3 there is not; Disc 1 twice. The lone game has no playlist.
        std::fs::write(
            dir.path().join("Metal Gear Solid (US).m3u"),
            "Metal Gear Solid (US) - CD1.chd\r\nMetal Gear Solid (US) - CD1.chd\r\n\
             Metal Gear Solid (VR Missions) (US) - CD3.chd",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("X-Files, The (USA).m3u"),
            "X-Files, The (USA) (Disc 1).chd\nX-Files, The (USA) (Disc 1).chd\n",
        )
        .unwrap();
        let system = System {
            name: "psx".into(),
            fullname: "PlayStation".into(),
            manufacturer: None,
            release_year: None,
            hardware: None,
            rom_path: dir.path().to_path_buf(),
            extensions: vec![".chd".into(), ".m3u".into()],
            command: None,
            platforms: vec![],
            theme: None,
            emulators: vec![],
        };
        let (roms, _) = scan_system(&system);
        let names: Vec<_> = roms
            .iter()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Lone (Japan) (Disc 1).chd",
                "Lone (Japan) (Disc 2).chd",
                "Metal Gear Solid (US).m3u",
                "Metal Gear Solid - VR Missions (USA).chd",
                "X-Files, The (USA).m3u",
            ]
        );
    }

    #[test]
    fn disc_tags_are_told_from_other_tags() {
        assert_eq!(
            without_disc("Game (USA) (Disc 2)").as_deref(),
            Some("game (usa)")
        );
        assert_eq!(
            without_disc("Game (US) - CD2").as_deref(),
            Some("game (us)")
        );
        assert_eq!(
            without_disc("Game (Japan) (Disk 1 of 2) [T-En]").as_deref(),
            Some("game (japan) [t-en]")
        );
        assert_eq!(without_disc("Game (Side B)").as_deref(), Some("game"));
        assert_eq!(without_disc("Game (USA)"), None);
        assert_eq!(without_disc("CD-i Games (Europe)"), None);
        assert_eq!(without_disc("Game - Discworld"), None);
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
