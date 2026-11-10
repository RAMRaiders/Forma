//! Games of a system that are one game under different names: `Rockman (Japan)` and `Mega Man
//! (USA)`. LaunchBox says which game each one is, by its name and alternate names, from Forma's
//! own copy of its data: nothing is asked of the network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
    let list: Vec<(i64, &str, &str)> = games
        .iter()
        .map(|g| (g.id, g.name.as_str(), g.path.as_str()))
        .collect();
    let Some(found) = launchbox::identify(&file, &system.platforms, &list)? else {
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
