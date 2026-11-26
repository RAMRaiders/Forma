//! Deleting a game for good: its ROM (with the files a multi-file game is made of: an `.m3u`'s
//! discs, their tracks), its entry in `gamelist.xml`, and the media scraped for it; a playlist's
//! discs' own entries and media too, from before they were in it. The library drops it through
//! the watcher, as for any file that goes.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::gamelist::remove_game;
use crate::library::Game;
use crate::paths::Paths;
use crate::scraper::MediaKind;
use crate::systems::System;

/// The extensions a scraped picture, video or manual is saved with.
const MEDIA_EXTENSIONS: [&str; 9] = [
    "png", "jpg", "jpeg", "gif", "webp", "mp4", "webm", "mkv", "pdf",
];

/// Whether `relative` stays inside the folder it is relative to.
fn stays_inside(relative: &Path) -> bool {
    relative.components().next().is_some()
        && relative
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The files a `.cue`, `.gdi` or `.m3u` names: next to it, below it, or a full path (some tools
/// write an `.m3u`'s discs so; [`delete_game`] keeps to the system's folder).
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
        .filter(|n| n.is_absolute() || stays_inside(n))
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
    let (inside, target, folder) = match (full.canonicalize(), system.rom_path.canonicalize()) {
        (Ok(target), Ok(folder)) => (
            target != folder && target.starts_with(&folder),
            target,
            folder,
        ),
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
        // A playlist's discs, each once a game of its own (its tracks are not).
        let discs: BTreeSet<PathBuf> = if is_playlist(&full) {
            parts_of(&full)
                .iter()
                .filter_map(|d| d.canonicalize().ok())
                .collect()
        } else {
            BTreeSet::new()
        };
        for file in with_parts(full.clone()) {
            // Never out of the system's folder: a part named `/etc/…` or reached by a link.
            let Ok(real) = file.canonicalize() else {
                continue; // A part another entry already took, or a name that was never there.
            };
            let Ok(relative) = real.strip_prefix(&folder).map(Path::to_path_buf) else {
                continue;
            };
            if relative.as_os_str().is_empty() || real.is_dir() {
                continue;
            }
            match std::fs::remove_file(&real) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => problems.push(format!("{}: {e}", file.display())),
            }
            // A playlist's disc: its own entry and media, if it had them before the playlist.
            if discs.contains(&real) {
                if let Err(e) = remove_game(&system.rom_path, paths, &relative) {
                    problems.push(e.to_string());
                }
                remove_scraped(&system.rom_path, &relative, &mut problems);
            }
        }
    }
    remove_scraped(&system.rom_path, rom, &mut problems);

    // Only what was scraped for this game: named after its ROM, in the system's folder.
    let stem = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefix = format!("{stem}-");
    for file in game.media.values().map(Path::new) {
        // `<rom>-<kind>.png` as ES and Forma save it, or `<rom>.png` as other scrapers do.
        let its_own = file.starts_with(&system.rom_path)
            && (file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                || file
                    .file_stem()
                    .is_some_and(|n| n.to_string_lossy() == stem));
        if its_own
            && let Err(e) = std::fs::remove_file(file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            problems.push(format!("{}: {e}", file.display()));
        }
    }
    problems.into_iter().next().map_or(Ok(()), Err)
}

fn is_playlist(file: &Path) -> bool {
    file.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
}

/// The media scraped for `rom` (relative to the system's folder) as ES and Forma save it:
/// `<folder>/<rom's name>-<kind>.<ext>`, beside the system's ROMs.
fn remove_scraped(system_dir: &Path, rom: &Path, problems: &mut Vec<String>) {
    let Some(stem) = rom.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return;
    };
    for kind in MediaKind::ALL {
        for ext in MEDIA_EXTENSIONS {
            let file = system_dir
                .join(kind.folder())
                .join(format!("{stem}-{}.{ext}", kind.suffix()));
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => problems.push(format!("{}: {e}", file.display())),
            }
        }
    }
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
