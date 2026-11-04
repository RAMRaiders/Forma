//! The ROM files in a system's folder, by the same rules as Forma: matching extensions, hidden
//! entries skipped, and the discs an `.m3u` lists left out so a multi-disc game counts once.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::batocera::Target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomFile {
    pub system: String,
    /// Relative to the system folder, `/`-separated: how the import format keys a game.
    pub path: String,
    pub full: PathBuf,
}

/// Deep enough for `system/collection/series/game`, shallow enough that a symlink loop ends.
const MAX_DEPTH: usize = 8;

/// Files to identify, and what was passed over (with why).
pub fn scan(target: &Target) -> (Vec<RomFile>, Vec<String>) {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    if target.root.is_dir() {
        walk(target, &target.root, 0, &mut files, &mut skipped);
    }
    let in_playlists = playlist_members(target, &files);
    files.retain(|f| !in_playlists.contains(&f.full));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    (files, skipped)
}

fn walk(
    target: &Target,
    dir: &Path,
    depth: usize,
    files: &mut Vec<RomFile>,
    skipped: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        skipped.push(format!("{}: cannot list folder", dir.display()));
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let full = entry.path();
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        if target.accepts(&name) {
            if meta.is_dir() {
                // A folder game (e.g. `Game.ps3`) has no single file to hash.
                skipped.push(format!("{}: a folder game, not hashed", full.display()));
                continue;
            }
            if let Some(path) = relative(&target.root, &full) {
                files.push(RomFile {
                    system: target.system.clone(),
                    path,
                    full,
                });
            }
        } else if meta.is_dir() && depth < MAX_DEPTH {
            walk(target, &full, depth + 1, files, skipped);
        }
    }
}

fn playlist_members(target: &Target, files: &[RomFile]) -> HashSet<PathBuf> {
    let mut members = HashSet::new();
    for m3u in files
        .iter()
        .filter(|f| f.path.to_lowercase().ends_with(".m3u"))
    {
        let Ok(text) = std::fs::read_to_string(&m3u.full) else {
            continue;
        };
        let base = m3u.full.parent().unwrap_or(&target.root);
        for line in text.lines().map(str::trim) {
            if !line.is_empty() && !line.starts_with('#') {
                members.insert(normalise(&base.join(line)));
            }
        }
    }
    members
}

fn relative(root: &Path, full: &Path) -> Option<String> {
    let rest = full.strip_prefix(root).ok()?;
    let parts: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_roms_skips_hidden_and_folder_games_and_counts_multi_disc_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("psx");
        std::fs::create_dir_all(root.join("FF7")).unwrap();
        std::fs::create_dir_all(root.join("Folder Game.cue")).unwrap();
        for f in [
            "FF7/Disc 1.cue",
            "FF7/Disc 2.cue",
            "Other.cue",
            ".hidden.cue",
            "notes.txt",
        ] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        std::fs::write(
            root.join("FF7.m3u"),
            "FF7/Disc 1.cue\n# comment\n./FF7/Disc 2.cue\n",
        )
        .unwrap();
        let target = Target {
            system: "psx".into(),
            root,
            extensions: vec![".cue".into(), ".m3u".into()],
        };
        let (files, skipped) = scan(&target);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["FF7.m3u", "Other.cue"]);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("folder game"));
    }

    #[test]
    fn a_missing_folder_yields_nothing() {
        let target = Target {
            system: "nes".into(),
            root: "/definitely/not/here".into(),
            extensions: vec![".nes".into()],
        };
        assert_eq!(scan(&target), (vec![], vec![]));
    }
}
