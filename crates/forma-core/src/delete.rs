//! Deleting a game for good: its ROM (with the files a multi-file game is made of), its entry
//! in `gamelist.xml`, and the media scraped for it. The library drops it through the watcher,
//! as for any file that goes.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::gamelist::remove_game;
use crate::library::Game;
use crate::paths::Paths;
use crate::systems::System;

/// Whether `relative` stays inside the folder it is relative to.
fn stays_inside(relative: &Path) -> bool {
    relative.components().next().is_some()
        && relative
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The files a `.cue`, `.gdi` or `.m3u` names, next to it, as far as they stay in its folder.
fn parts_of(file: &Path) -> Vec<PathBuf> {
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !matches!(extension.as_str(), "cue" | "gdi" | "m3u") {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let unquote = |s: &str| s.trim().trim_matches('"').to_owned();
    let names: Vec<String> = match extension.as_str() {
        "cue" => text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line
                    .get(..5)
                    .filter(|s| s.eq_ignore_ascii_case("FILE "))
                    .map(|_| &line[5..])?;
                Some(match rest.strip_prefix('"') {
                    Some(quoted) => quoted.split('"').next().unwrap_or("").to_owned(),
                    None => rest.split_whitespace().next().unwrap_or("").to_owned(),
                })
            })
            .collect(),
        // `1 0 4 2352 "track 01.bin" 0`: after the first line, the file is the fifth column.
        "gdi" => text
            .lines()
            .skip(1)
            .filter_map(|line| {
                let line = line.trim();
                match line.split_once('"') {
                    Some((_, rest)) => rest.split('"').next().map(str::to_owned),
                    None => line.split_whitespace().nth(4).map(str::to_owned),
                }
            })
            .collect(),
        _ => text
            .lines()
            .map(unquote)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect(),
    };
    let folder = file.parent().unwrap_or(Path::new(""));
    names
        .iter()
        .map(|n| Path::new(n.as_str()))
        .filter(|n| stays_inside(n))
        .map(|n| folder.join(n))
        .collect()
}

/// `rom` and everything it names, however deep (an `.m3u` of `.cue`s of `.bin`s).
fn with_parts(rom: PathBuf) -> BTreeSet<PathBuf> {
    let mut all = BTreeSet::new();
    let mut waiting = vec![rom];
    while let Some(file) = waiting.pop() {
        if all.insert(file.clone()) {
            waiting.extend(parts_of(&file));
        }
    }
    all
}

/// Deletes `game` of `system`. The gamelist goes first: if it cannot be changed, nothing is
/// lost. `Err` says what could not be deleted.
pub fn delete_game(paths: &Paths, game: &Game, system: &System) -> std::result::Result<(), String> {
    let rom = Path::new(&game.path);
    if !stays_inside(rom) {
        return Err(format!("`{}` is not inside the system's folder", game.path));
    }
    let full = system.rom_path.join(rom);
    let (inside, target) = match (full.canonicalize(), system.rom_path.canonicalize()) {
        (Ok(target), Ok(folder)) => (target != folder && target.starts_with(&folder), target),
        (Err(e), _) | (_, Err(e)) => return Err(format!("{}: {e}", game.path)),
    };
    if !inside {
        return Err(format!("`{}` is not inside the system's folder", game.path));
    }
    remove_game(&system.rom_path, paths, rom).map_err(|e| e.to_string())?;

    let mut problems = Vec::new();
    if target.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(&target) {
            problems.push(format!("{}: {e}", game.path));
        }
    } else {
        for file in with_parts(full.clone()) {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                // A part another entry already took, or a name that was never there.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => problems.push(format!("{}: {e}", file.display())),
            }
        }
    }

    // Only what was scraped for this game: named after its ROM, in the system's folder.
    let stem = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = format!("{stem}-");
    for file in game.media.values().map(Path::new) {
        let its_own = file.starts_with(&system.rom_path)
            && file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix));
        if its_own
            && let Err(e) = std::fs::remove_file(file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            problems.push(format!("{}: {e}", file.display()));
        }
    }
    problems.into_iter().next().map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_found_and_stay_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("a.cue");
        std::fs::write(
            &cue,
            "FILE \"a (Track 1).bin\" BINARY\n  TRACK 01 MODE1/2352\nfile ../../evil.bin BINARY\n",
        )
        .unwrap();
        assert_eq!(parts_of(&cue), [dir.path().join("a (Track 1).bin")]);
        let gdi = dir.path().join("b.gdi");
        std::fs::write(
            &gdi,
            "2\n1 0 4 2352 track01.bin 0\n2 600 0 2352 \"track 02.raw\" 0\n",
        )
        .unwrap();
        assert_eq!(
            parts_of(&gdi),
            [
                dir.path().join("track01.bin"),
                dir.path().join("track 02.raw")
            ]
        );
    }
}
