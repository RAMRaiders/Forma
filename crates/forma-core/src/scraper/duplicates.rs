//! Games of a system that are one game under different names: `Rockman (Japan)` and `Mega Man
//! (USA)`. LaunchBox says which game each one is, by its name and alternate names, from Forma's
//! own copy of its data: nothing is asked of the network.
//!
//! LaunchBox sometimes holds one game twice: on PlayStation `Descent Maximum`, called
//! `Descent 2` in Europe, and a `Descent II` of its own. Two of its games are taken for one when
//! one gives the other's name as its name in a region, a file of the other is from that region,
//! and they came out within a year of each other. Names alone are not enough: LaunchBox's
//! regional names often belong to another game of the series (`Adventures of Lolo II` is Japan's
//! name for `Adventures of Lolo 3`, and a library's `Adventures of Lolo 2` is from the USA).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use serde::Serialize;

use super::regions;
use super::sources::launchbox;
use super::text::title_key;
use crate::library::GameFile;
use crate::paths::Paths;
use crate::systems::System;

/// Games LaunchBox says are one, under its name for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub title: String,
    pub games: Vec<GameFile>,
}

/// The games of `system` that are another of its games under a different name; `None` without
/// LaunchBox's data. A group whose files all have one title is left out: those are versions of
/// one another, which Game versions lists.
pub fn duplicates(
    paths: &Paths,
    system: &System,
    games: &[GameFile],
) -> Result<Option<Vec<DuplicateGroup>>, String> {
    let file = paths.cache_dir().join("launchbox.db");
    let Some(found) = identified(&file, &system.platforms, games)? else {
        return Ok(None);
    };
    let entries: BTreeMap<i64, &Found> = found.values().map(|f| (f.id, f)).collect();
    let mut from: HashMap<i64, HashSet<&str>> = HashMap::new();
    for game in games {
        if let Some(lb) = found.get(&game.id) {
            let stem = Path::new(&game.path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let theirs: Vec<String> = regions::in_name(&stem)
                .into_iter()
                .map(str::to_owned)
                .collect();
            from.entry(lb.id)
                .or_default()
                .extend(launchbox::regions_of(&theirs));
        }
    }
    let one = same_game(&entries, &from);
    let mut groups: BTreeMap<i64, (String, Vec<&GameFile>)> = BTreeMap::new();
    for game in games {
        if let Some(lb) = found.get(&game.id) {
            let id = one[&lb.id];
            groups
                .entry(id)
                .or_insert_with(|| (entries[&id].title.clone(), Vec::new()))
                .1
                .push(game);
        }
    }
    let title_of = |game: &GameFile| {
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
            games: games.into_iter().cloned().collect(),
        })
        .collect();
    out.sort_by_key(|g| g.title.to_lowercase());
    Ok(Some(out))
}

/// For each LaunchBox game in `entries` (by its id), the one it is taken for: the first of
/// those it is one game with (see the module), through any chain of them. `from` holds the
/// LaunchBox regions of the library's files of each.
fn same_game(
    entries: &BTreeMap<i64, &Found>,
    from: &HashMap<i64, HashSet<&str>>,
) -> HashMap<i64, i64> {
    let mut root: HashMap<i64, i64> = entries.keys().map(|&id| (id, id)).collect();
    fn top(root: &HashMap<i64, i64>, mut id: i64) -> i64 {
        while root[&id] != id {
            id = root[&id];
        }
        id
    }
    let mut by_name: HashMap<&str, Vec<i64>> = HashMap::new();
    for (id, f) in entries {
        by_name.entry(f.own.as_str()).or_default().push(*id);
    }
    for (a, f) in entries {
        for (name, region) in &f.regional {
            for b in by_name.get(name.as_str()).into_iter().flatten() {
                let other = entries[b];
                let near = match (f.year, other.year) {
                    (Some(x), Some(y)) => (x - y).abs() <= 1,
                    _ => true,
                };
                let there = from.get(b).is_some_and(|r| r.contains(region.as_str()));
                if b != a && near && there {
                    let (x, y) = (top(&root, *a), top(&root, *b));
                    root.insert(x.max(y), x.min(y));
                }
            }
        }
    }
    entries.keys().map(|&id| (id, top(&root, id))).collect()
}

/// LaunchBox's game for one of ours: its id and name, and, as compared, its name and its
/// regional names with their regions; the year it came out.
#[derive(Clone)]
struct Found {
    id: i64,
    title: String,
    own: String,
    regional: Vec<(String, String)>,
    year: Option<i32>,
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
    found: Option<Found>,
}

/// By the data's file. A game's siblings are asked each time Select opens on it: matching a
/// whole system again (2,000 games on the Pi: 200 ms) only for the games it has not seen.
static KNOWN: Mutex<BTreeMap<PathBuf, Known>> = Mutex::new(BTreeMap::new());

/// [`launchbox::identify`] for `games`, with each LaunchBox game's names, asking LaunchBox's
/// data only of those it has not been asked about as they are now. `None` without the data.
fn identified(
    file: &Path,
    platforms: &[String],
    games: &[GameFile],
) -> Result<Option<HashMap<i64, Found>>, String> {
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
        let lb: Vec<i64> = found
            .values()
            .map(|f| f.0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let names = launchbox::names(file, &lb)?;
        let ids: HashSet<i64> = new.iter().map(|n| n.0).collect();
        for g in games.iter().filter(|g| ids.contains(&g.id)) {
            known.games.insert(
                g.id,
                Said {
                    asked: (g.name.clone(), g.path.clone(), platforms.to_vec()),
                    found: found.remove(&g.id).map(|(id, title)| {
                        let named = names.get(&id);
                        Found {
                            id,
                            own: title_key(named.map_or(title.as_str(), |n| n.name.as_str())),
                            regional: named
                                .map(|n| {
                                    n.regional
                                        .iter()
                                        .map(|(name, region)| (title_key(name), region.clone()))
                                        .filter(|(name, _)| !name.is_empty())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            year: named.and_then(|n| n.year),
                            title,
                        }
                    }),
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
