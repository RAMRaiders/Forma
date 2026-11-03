//! Hierarchy views: how the library is organised for browsing.
//!
//! A view is a JSON tree of nodes. EmulationStation has one fixed tree (system → games); in
//! Forma the tree is data, so users and themes define their own: manufacturer → system,
//! genre, a home menu of favourites and recents, anything the filters can express. The engine
//! resolves a view and a path to one page of nodes, so a theme only renders what it is given.
//!
//! The format is documented in `docs/hierarchy.md`; this module is its reference.

mod browse;
mod catalog;

use serde::{Deserialize, Serialize};

pub use browse::{Branch as BranchItem, Browse, Crumb, Group, Listing, Page};
pub use catalog::{DEFAULT_VIEW_SETTING, Origin, ViewCatalog, ViewEntry, load_views};

/// Deep enough for any real menu; a limit keeps a typo from recursing without end.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(skip)]
    pub(crate) root: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub filter: Filter,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeKind {
    GroupBy {
        key: GroupKey,
        sort: GroupSort,
        children: Box<Node>,
    },
    Branches(Vec<Branch>),
    Games(GamesSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Branch {
    pub id: String,
    pub name: String,
    pub child: Node,
}

/// What a `groupBy` node splits games by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKey {
    System,
    Manufacturer,
    Hardware,
    Genre,
    Developer,
    Publisher,
    Year,
    Decade,
    Players,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupSort {
    /// Systems in definition order, numbers ascending, everything else by name.
    #[default]
    Natural,
    Name,
    /// Largest group first.
    Count,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct GamesSpec {
    #[serde(default)]
    pub sort: GameSort,
    pub order: Option<Order>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GameSort {
    #[default]
    Name,
    ReleaseDate,
    Rating,
    LastPlayed,
    PlayCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Order {
    Asc,
    Desc,
}

/// Every condition must hold. Text conditions match case-insensitively, against one value or
/// any of a list.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Filter {
    pub system: Option<OneOrMany>,
    pub manufacturer: Option<OneOrMany>,
    pub hardware: Option<OneOrMany>,
    pub genre: Option<OneOrMany>,
    pub developer: Option<OneOrMany>,
    pub publisher: Option<OneOrMany>,
    pub region: Option<OneOrMany>,
    pub year: Option<Range>,
    pub players: Option<Range>,
    pub favorite: Option<bool>,
    /// Played at least once (`true`) or never (`false`).
    pub played: Option<bool>,
    /// In the user's collection of this name (any case).
    pub collection: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn values(&self) -> Vec<&str> {
        match self {
            Self::One(v) => vec![v.as_str()],
            Self::Many(vs) => vs.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Range {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

// ---- Parsing: a permissive shape, then validation into the types above. -------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ViewDef {
    /// Lets editors find `docs/view.schema.json`; not used by Forma.
    #[serde(rename = "$schema")]
    _schema: Option<String>,
    id: String,
    name: String,
    description: Option<String>,
    root: NodeDef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NodeDef {
    group_by: Option<GroupKey>,
    branches: Option<Vec<BranchDef>>,
    games: Option<GamesSpec>,
    filter: Option<Filter>,
    sort: Option<GroupSort>,
    children: Option<Box<NodeDef>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BranchDef {
    id: String,
    name: String,
    filter: Option<Filter>,
    children: Option<Box<NodeDef>>,
}

/// Parses and validates a view. The error says where the problem is (`root.children: …`),
/// because the person reading it is editing a JSON file by hand.
pub fn parse_view(json: &str) -> Result<View, String> {
    let def: ViewDef = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if !is_slug(&def.id) {
        return Err(format!(
            "id `{}` must be lower-case letters, digits and dashes",
            def.id
        ));
    }
    Ok(View {
        id: def.id,
        name: def.name,
        description: def.description,
        root: node(def.root, "root", 0)?,
    })
}

fn node(def: NodeDef, at: &str, depth: usize) -> Result<Node, String> {
    if depth > MAX_DEPTH {
        return Err(format!("{at}: deeper than {MAX_DEPTH} levels"));
    }
    let kinds = [
        def.group_by.is_some(),
        def.branches.is_some(),
        def.games.is_some(),
    ];
    if kinds.iter().filter(|k| **k).count() != 1 {
        return Err(format!(
            "{at}: needs exactly one of `groupBy`, `branches` or `games`"
        ));
    }
    let filter = def.filter.unwrap_or_default();
    check_filter(&filter, at)?;

    let kind = if let Some(key) = def.group_by {
        let children = def
            .children
            .ok_or_else(|| format!("{at}: `groupBy` needs `children` (what each group holds)"))?;
        NodeKind::GroupBy {
            key,
            sort: def.sort.unwrap_or_default(),
            children: Box::new(node(*children, &format!("{at}.children"), depth + 1)?),
        }
    } else if let Some(branches) = def.branches {
        if def.children.is_some() || def.sort.is_some() {
            return Err(format!(
                "{at}: `branches` takes `children` per branch, not on the node"
            ));
        }
        if branches.is_empty() {
            return Err(format!("{at}: `branches` is empty"));
        }
        let mut out: Vec<Branch> = Vec::with_capacity(branches.len());
        for (i, b) in branches.into_iter().enumerate() {
            let here = format!("{at}.branches[{i}]");
            if !is_slug(&b.id) {
                return Err(format!("{here}: id `{}` must be a slug", b.id));
            }
            if out.iter().any(|o| o.id == b.id) {
                return Err(format!("{here}: id `{}` is used twice", b.id));
            }
            let children = b
                .children
                .ok_or_else(|| format!("{here}: needs `children`"))?;
            let mut child = node(*children, &format!("{here}.children"), depth + 1)?;
            let filter = b.filter.unwrap_or_default();
            check_filter(&filter, &here)?;
            child.filter = merge(filter, child.filter);
            out.push(Branch {
                id: b.id,
                name: b.name,
                child,
            });
        }
        NodeKind::Branches(out)
    } else {
        if def.children.is_some() || def.sort.is_some() {
            return Err(format!("{at}: `games` is a leaf; put sorting inside it"));
        }
        NodeKind::Games(def.games.unwrap_or_default())
    };
    Ok(Node { filter, kind })
}

fn check_filter(f: &Filter, at: &str) -> Result<(), String> {
    for (name, range) in [("year", f.year), ("players", f.players)] {
        if let Some(Range {
            min: Some(min),
            max: Some(max),
        }) = range
            && min > max
        {
            return Err(format!(
                "{at}: filter `{name}` has min {min} above max {max}"
            ));
        }
    }
    Ok(())
}

/// A branch's own filter and its child node's filter both apply; the child's wins where both
/// set the same field, since it is the more specific.
fn merge(outer: Filter, inner: Filter) -> Filter {
    Filter {
        system: inner.system.or(outer.system),
        manufacturer: inner.manufacturer.or(outer.manufacturer),
        hardware: inner.hardware.or(outer.hardware),
        genre: inner.genre.or(outer.genre),
        developer: inner.developer.or(outer.developer),
        publisher: inner.publisher.or(outer.publisher),
        region: inner.region.or(outer.region),
        year: inner.year.or(outer.year),
        players: inner.players.or(outer.players),
        favorite: inner.favorite.or(outer.favorite),
        played: inner.played.or(outer.played),
        collection: inner.collection.or(outer.collection),
    }
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests;
