//! The views on offer: the bundled ones and the user's, and which one Forma opens with.

use std::path::PathBuf;

use super::{Branch, Filter, GamesSpec, Node, NodeKind, View, parse_view};
use crate::error::Warning;
use crate::paths::Paths;

/// The setting that names the view Forma boots into.
pub const DEFAULT_VIEW_SETTING: &str = "views.default";

/// The view used when neither the user nor the theme chose one: what EmulationStation shows.
const FACTORY_DEFAULT: &str = "systems";

const BUNDLED: &[(&str, &str)] = &[
    ("systems.json", include_str!("views/systems.json")),
    (
        "manufacturer-system.json",
        include_str!("views/manufacturer-system.json"),
    ),
    ("genre.json", include_str!("views/genre.json")),
    ("all.json", include_str!("views/all.json")),
    ("favorites.json", include_str!("views/favorites.json")),
    ("recent.json", include_str!("views/recent.json")),
    ("home.json", include_str!("views/home.json")),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Bundled,
    User(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ViewEntry {
    pub view: View,
    pub origin: Origin,
}

#[derive(Debug, Default)]
pub struct ViewCatalog {
    pub views: Vec<ViewEntry>,
    pub warnings: Vec<Warning>,
}

/// Bundled views, then every `*.json` in the user's views folder (in name order). A user view
/// with a bundled view's id replaces it; a broken file is reported and skipped.
pub fn load_views(paths: &Paths) -> ViewCatalog {
    let mut catalog = ViewCatalog::default();
    for (file, json) in BUNDLED {
        match parse_view(json) {
            Ok(view) => catalog.views.push(ViewEntry {
                view,
                origin: Origin::Bundled,
            }),
            Err(e) => catalog
                .warnings
                .push(Warning::new(std::path::Path::new(file), e)),
        }
    }

    catalog.views.push(ViewEntry {
        view: collections_view(&collection_names(paths)),
        origin: Origin::Bundled,
    });

    let mut files: Vec<PathBuf> = std::fs::read_dir(paths.views_dir())
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    let mut user_ids: Vec<String> = Vec::new();
    for file in files {
        let parsed = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|json| parse_view(&json));
        let view = match parsed {
            Ok(view) => view,
            Err(e) => {
                catalog.warnings.push(Warning::new(&file, e));
                continue;
            }
        };
        if user_ids.contains(&view.id) {
            catalog.warnings.push(Warning::new(
                &file,
                format!("another file already defines view `{}`; skipped", view.id),
            ));
            continue;
        }
        user_ids.push(view.id.clone());
        let entry = ViewEntry {
            view,
            origin: Origin::User(file),
        };
        match catalog
            .views
            .iter_mut()
            .find(|e| e.view.id == entry.view.id)
        {
            Some(existing) => *existing = entry,
            None => catalog.views.push(entry),
        }
    }
    catalog
}

/// The collections' names, from their files (see `library/collections.rs`), as EmulationStation
/// names them: `custom-<name>.cfg`.
fn collection_names(paths: &Paths) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(paths.collections_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let file = e.file_name().to_string_lossy().into_owned();
                    let name = file.strip_prefix("custom-")?.strip_suffix(".cfg")?.trim();
                    (!name.is_empty() && !name.starts_with('.')).then(|| name.to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// `collections`: a branch per collection, its games by name. Made from the collections there
/// are, so it changes when they do; with none yet, it is an empty list of games.
pub(crate) fn collections_view(names: &[String]) -> View {
    let games = |collection: Option<String>| Node {
        filter: Filter {
            collection,
            ..Filter::default()
        },
        kind: NodeKind::Games(GamesSpec::default()),
    };
    let mut branches: Vec<Branch> = Vec::new();
    for name in names {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while branches.iter().any(|b| b.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        branches.push(Branch {
            id,
            name: name.clone(),
            child: games(Some(name.clone())),
        });
    }
    View {
        id: "collections".into(),
        name: "Collections".into(),
        description: Some("Your own lists of games.".into()),
        root: if branches.is_empty() {
            // Matches nothing: no collection has an empty name.
            games(Some(String::new()))
        } else {
            Node {
                filter: Filter::default(),
                kind: NodeKind::Branches(branches),
            }
        },
    }
}

/// A branch id from a name: lower-case letters and digits, dashes between.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "collection".into()
    } else {
        out
    }
}

impl ViewCatalog {
    pub fn get(&self, id: &str) -> Option<&View> {
        self.views.iter().map(|e| &e.view).find(|v| v.id == id)
    }

    /// The view Forma opens with: the user's setting, else the theme's suggestion, else the
    /// bundled `systems` view. Notes say why a choice was passed over, for the user to see.
    pub fn boot_view(
        &self,
        setting: Option<&str>,
        theme: Option<&str>,
    ) -> (Option<&View>, Vec<String>) {
        let mut notes = Vec::new();
        let candidates = [
            ("your boot view setting", setting),
            ("the theme's suggested view", theme),
            ("the bundled default", Some(FACTORY_DEFAULT)),
        ];
        for (source, id) in candidates {
            let Some(id) = id else { continue };
            match self.get(id) {
                Some(view) => return (Some(view), notes),
                None => notes.push(format!("{source} `{id}` is not available")),
            }
        }
        // Only reachable if the bundled default itself was replaced by a broken user file.
        (self.views.first().map(|e| &e.view), notes)
    }
}
