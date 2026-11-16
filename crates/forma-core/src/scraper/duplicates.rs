//! Games of a system that are one game under different names: `Rockman (Japan)` and `Mega Man
//! (USA)`. LaunchBox says which game each one is, by its name and alternate names, from Forma's
//! own copy of its data: nothing is asked of the network.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use serde::Serialize;

use super::sources::launchbox;
use super::text::title_key;
use crate::library::Game;
use crate::paths::Paths;
use crate::systems::System;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGame {
    pub id: i64,
    pub name: String,
    pub path: String,
}

/// Games LaunchBox says are one, under its name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub title: String,
    pub games: Vec<DuplicateGame>,
}

/// The games of `system` that are another of its games under a different name; `None` without
/// LaunchBox's data. A group whose files all have one title is left out: those are versions of
/// one another, which Game versions lists.
pub fn duplicates(
    paths: &Paths,
    system: &System,
    games: &[Game],
) -> Result<Option<Vec<DuplicateGroup>>, String> {
    let file = paths.cache_dir().join("launchbox.db");
    let Some(found) = identified(&file, &system.platforms, games)? else {
        return Ok(None);
    };
    let mut groups: BTreeMap<i64, (String, Vec<&Game>)> = BTreeMap::new();
    for game in games {
        if let Some((id, title)) = found.get(&game.id) {
            groups
                .entry(*id)
                .or_insert_with(|| (title.clone(), Vec::new()))
                .1
                .push(game);
        }
    }
    let title_of = |game: &Game| {
        let stem = Path::new(&game.path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let key = title_key(&stem);
        if key.is_empty() {
            title_key(&game.name)
        } else {
            key
        }
    };
    let mut out: Vec<DuplicateGroup> = groups
        .into_values()
        .filter(|(_, games)| {
            games
                .iter()
                .map(|g| title_of(g))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .map(|(title, games)| DuplicateGroup {
            title,
            games: games
                .into_iter()
                .map(|g| DuplicateGame {
                    id: g.id,
                    name: g.name.clone(),
                    path: g.path.clone(),
                })
                .collect(),
        })
        .collect();
    out.sort_by_key(|g| g.title.to_lowercase());
    Ok(Some(out))
}

/// What LaunchBox said of each game, by its id, for one copy of LaunchBox's data.
struct Known {
    /// The data's size and time: rebuilt, it is another copy and everything is asked again.
    stamp: (u64, Option<SystemTime>),
    games: HashMap<i64, Said>,
}

/// LaunchBox's game for one of ours, kept while the game's name, file and system's platforms
/// are what it was asked with.
struct Said {
    asked: (String, String, Vec<String>),
    found: Option<(i64, String)>,
}

/// By the data's file. A game's siblings are asked each time Select opens on it: matching a
/// whole system again (2,000 games on the Pi: 200 ms) only for the games it has not seen.
static KNOWN: Mutex<BTreeMap<PathBuf, Known>> = Mutex::new(BTreeMap::new());

/// [`launchbox::identify`] for `games`, asking LaunchBox's data only of those it has not been
/// asked about as they are now. `None` without the data.
fn identified(
    file: &Path,
    platforms: &[String],
    games: &[Game],
) -> Result<Option<launchbox::Identified>, String> {
    let Ok(meta) = std::fs::metadata(file) else {
        return Ok(None);
    };
    let stamp = (meta.len(), meta.modified().ok());
    let mut all = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let known = all.entry(file.to_path_buf()).or_insert_with(|| Known {
        stamp,
        games: HashMap::new(),
    });
    if known.stamp != stamp {
        known.stamp = stamp;
        known.games.clear();
    }
    let new: Vec<(i64, &str, &str)> = games
        .iter()
        .filter(|g| {
            known.games.get(&g.id).is_none_or(|s| {
                let (name, path, on) = &s.asked;
                *name != g.name || *path != g.path || on != platforms
            })
        })
        .map(|g| (g.id, g.name.as_str(), g.path.as_str()))
        .collect();
    if !new.is_empty() {
        let Some(mut found) = launchbox::identify(file, platforms, &new)? else {
            return Ok(None);
        };
        let ids: HashSet<i64> = new.iter().map(|n| n.0).collect();
        for g in games.iter().filter(|g| ids.contains(&g.id)) {
            known.games.insert(
                g.id,
                Said {
                    asked: (g.name.clone(), g.path.clone(), platforms.to_vec()),
                    found: found.remove(&g.id),
                },
            );
        }
    }
    Ok(Some(
        games
            .iter()
            .filter_map(|g| Some((g.id, known.games.get(&g.id)?.found.clone()?)))
            .collect(),
    ))
}
