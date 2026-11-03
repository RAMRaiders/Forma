//! Resolving a view and a path to one page of nodes.

use rusqlite::params_from_iter;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use super::{Filter, GameSort, GamesSpec, GroupKey, GroupSort, Node, NodeKind, Order, View};
use crate::error::{Error, Result};
use crate::library::{GAME_COLUMNS, Game, Library};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub offset: u32,
    pub limit: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 200,
        }
    }
}

/// One step of the path, for a breadcrumb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Crumb {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// What to append to the path to open this group. Empty for "Unknown".
    pub key: String,
    pub label: String,
    pub game_count: i64,
    /// The game whose image represents the group: the best-rated one that has an image.
    /// An id rather than a path, so a host can serve it without exposing the filesystem.
    pub image_game: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub game_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Listing {
    #[serde(rename_all = "camelCase")]
    Groups {
        group_by: GroupKey,
        items: Vec<Group>,
        total: i64,
    },
    Branches {
        items: Vec<Branch>,
    },
    Games {
        items: Vec<Game>,
        total: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Browse {
    pub view: String,
    pub path: Vec<String>,
    pub trail: Vec<Crumb>,
    pub listing: Listing,
}

impl Library {
    /// The page of `view` at `path`. Each path segment is a group key (as returned in
    /// [`Group::key`]) or a branch id, from the root down.
    pub fn browse(&self, view: &View, path: &[String], page: Page) -> Result<Browse> {
        let mut node: &Node = &view.root;
        let mut conds = Conds::visible();
        let mut trail = Vec::with_capacity(path.len());

        for segment in path {
            conds.filter(&node.filter);
            match &node.kind {
                NodeKind::GroupBy { key, children, .. } => {
                    conds.group(*key, segment);
                    trail.push(Crumb {
                        key: segment.clone(),
                        label: self.label(*key, segment)?,
                    });
                    node = children;
                }
                NodeKind::Branches(branches) => {
                    let branch = branches.iter().find(|b| &b.id == segment).ok_or_else(|| {
                        Error::View(format!("view `{}` has no branch `{segment}` here", view.id))
                    })?;
                    trail.push(Crumb {
                        key: branch.id.clone(),
                        label: branch.name.clone(),
                    });
                    node = &branch.child;
                }
                NodeKind::Games(_) => {
                    return Err(Error::View(format!(
                        "path goes below a game list in view `{}`",
                        view.id
                    )));
                }
            }
        }
        conds.filter(&node.filter);

        let listing = match &node.kind {
            NodeKind::GroupBy { key, sort, .. } => self.groups(*key, *sort, &conds, page)?,
            NodeKind::Branches(branches) => {
                let mut items = Vec::with_capacity(branches.len());
                for b in branches {
                    let mut c = conds.clone();
                    c.filter(&b.child.filter);
                    items.push(Branch {
                        id: b.id.clone(),
                        name: b.name.clone(),
                        game_count: self.count(&c)?,
                    });
                }
                Listing::Branches { items }
            }
            NodeKind::Games(spec) => self.game_page(spec, &conds, page)?,
        };
        Ok(Browse {
            view: view.id.clone(),
            path: path.to_vec(),
            trail,
            listing,
        })
    }

    fn groups(&self, key: GroupKey, sort: GroupSort, conds: &Conds, page: Page) -> Result<Listing> {
        let expr = key_expr(key);
        let natural = match key {
            GroupKey::System => "MIN(s.sort_order)",
            k if is_numeric(k) => "k",
            _ => "k COLLATE NOCASE",
        };
        let order = match (sort, key) {
            (GroupSort::Count, _) => "n DESC, k COLLATE NOCASE",
            (GroupSort::Name, GroupKey::System) => "MIN(s.fullname) COLLATE NOCASE",
            (GroupSort::Name | GroupSort::Natural, _) => natural,
        };
        let sql = format!(
            "SELECT {expr} AS k, COUNT(*) AS n, MIN(s.fullname)
             FROM game g JOIN system s ON s.name = g.system WHERE {}
             GROUP BY k ORDER BY (k IS NULL), {order} LIMIT {} OFFSET {}",
            conds.sql(),
            page.limit,
            page.offset
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows: Vec<(String, i64, Option<String>)> = stmt
            .query_map(params_from_iter(&conds.params), |r| {
                Ok((key_string(r.get(0)?), r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut items = Vec::with_capacity(rows.len());
        for (k, n, fullname) in rows {
            let mut c = conds.clone();
            c.group(key, &k);
            let label = match (key, fullname) {
                (GroupKey::System, Some(fullname)) => fullname,
                _ => label(key, &k),
            };
            items.push(Group {
                image_game: self.sample_image(&c)?,
                key: k,
                label,
                game_count: n,
            });
        }

        let total: i64 = self.conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM (SELECT 1 FROM game g JOIN system s ON s.name = g.system
                 WHERE {} GROUP BY {expr})",
                conds.sql()
            ),
            params_from_iter(&conds.params),
            |r| r.get(0),
        )?;
        Ok(Listing::Groups {
            group_by: key,
            items,
            total,
        })
    }

    fn game_page(&self, spec: &GamesSpec, conds: &Conds, page: Page) -> Result<Listing> {
        let cap = spec.limit.map_or(i64::MAX, i64::from);
        let total = self.count(conds)?.min(cap);
        let offset = i64::from(page.offset);
        let limit = i64::from(page.limit).min((total - offset).max(0));

        let default_order = match spec.sort {
            GameSort::Name | GameSort::ReleaseDate => Order::Asc,
            GameSort::Rating | GameSort::LastPlayed | GameSort::PlayCount => Order::Desc,
        };
        let dir = match spec.order.unwrap_or(default_order) {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };
        let sort = match spec.sort {
            GameSort::Name => format!("g.name COLLATE NOCASE {dir}"),
            GameSort::ReleaseDate => format!("g.release_date IS NULL, g.release_date {dir}"),
            GameSort::Rating => format!("g.rating IS NULL, g.rating {dir}"),
            GameSort::LastPlayed => format!("g.last_played IS NULL, g.last_played {dir}"),
            GameSort::PlayCount => format!("g.play_count {dir}"),
        };
        let items = self.query_games(
            &format!(
                "SELECT {GAME_COLUMNS} FROM game g JOIN system s ON s.name = g.system
                 WHERE {} ORDER BY {sort}, g.name COLLATE NOCASE, g.id LIMIT {limit} OFFSET {offset}",
                conds.sql()
            ),
            params_from_iter(&conds.params),
        )?;
        Ok(Listing::Games { items, total })
    }

    fn count(&self, conds: &Conds) -> Result<i64> {
        Ok(self.conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM game g JOIN system s ON s.name = g.system WHERE {}",
                conds.sql()
            ),
            params_from_iter(&conds.params),
            |r| r.get(0),
        )?)
    }

    fn sample_image(&self, conds: &Conds) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT g.id FROM game g JOIN system s ON s.name = g.system
             JOIN media m ON m.game_id = g.id AND m.kind = 'image'
             WHERE {} ORDER BY g.rating IS NULL, g.rating DESC, g.name LIMIT 1",
            conds.sql()
        ))?;
        let mut rows = stmt.query(params_from_iter(&conds.params))?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    fn label(&self, key: GroupKey, value: &str) -> Result<String> {
        if key == GroupKey::System
            && let Some(fullname) = self
                .conn
                .query_row(
                    "SELECT fullname FROM system WHERE name = ?1",
                    [value],
                    |r| r.get::<_, String>(0),
                )
                .ok()
        {
            return Ok(fullname);
        }
        Ok(label(key, value))
    }
}

/// SQL conditions and their parameters, built up along a path.
#[derive(Debug, Clone)]
struct Conds {
    sql: Vec<String>,
    params: Vec<Value>,
}

impl Conds {
    fn visible() -> Self {
        Self {
            sql: vec!["g.present = 1".into(), "g.hidden = 0".into()],
            params: Vec::new(),
        }
    }

    fn sql(&self) -> String {
        self.sql.join(" AND ")
    }

    fn group(&mut self, key: GroupKey, value: &str) {
        let expr = key_expr(key);
        if value.is_empty() {
            self.sql.push(format!("{expr} IS NULL"));
        } else if is_numeric(key) {
            // A key that is not a number cannot match a numeric group.
            match value.parse::<i64>() {
                Ok(n) => {
                    self.sql.push(format!("{expr} = ?"));
                    self.params.push(Value::Integer(n));
                }
                Err(_) => self.sql.push("0".into()),
            }
        } else {
            self.sql.push(format!("{expr} = ?"));
            self.params.push(Value::Text(value.to_owned()));
        }
    }

    fn filter(&mut self, f: &Filter) {
        let text = [
            ("g.system", &f.system),
            ("s.manufacturer", &f.manufacturer),
            ("s.hardware", &f.hardware),
            ("g.genre", &f.genre),
            ("g.developer", &f.developer),
            ("g.publisher", &f.publisher),
            ("g.region", &f.region),
        ];
        for (column, values) in text {
            if let Some(values) = values {
                let values = values.values();
                let marks = vec!["?"; values.len()].join(", ");
                self.sql
                    .push(format!("{column} COLLATE NOCASE IN ({marks})"));
                self.params
                    .extend(values.into_iter().map(|v| Value::Text(v.to_owned())));
            }
        }
        for (column, range) in [("g.release_year", f.year), ("g.players_max", f.players)] {
            let Some(range) = range else { continue };
            if let Some(min) = range.min {
                self.sql.push(format!("{column} >= ?"));
                self.params.push(Value::Integer(min));
            }
            if let Some(max) = range.max {
                self.sql.push(format!("{column} <= ?"));
                self.params.push(Value::Integer(max));
            }
        }
        if let Some(favorite) = f.favorite {
            self.sql.push("g.favorite = ?".into());
            self.params.push(Value::Integer(i64::from(favorite)));
        }
        if let Some(collection) = &f.collection {
            self.sql.push(
                "EXISTS (SELECT 1 FROM collection_game cg WHERE cg.collection = ?
                 AND cg.path = s.rom_path || '/' || g.path)"
                    .into(),
            );
            self.params.push(Value::Text(collection.clone()));
        }
        if let Some(played) = f.played {
            self.sql.push(
                if played {
                    "g.last_played IS NOT NULL"
                } else {
                    "g.last_played IS NULL"
                }
                .into(),
            );
        }
    }
}

/// The SQL value a group key splits by. Empty text counts as unknown, like a missing value.
fn key_expr(key: GroupKey) -> &'static str {
    match key {
        GroupKey::System => "g.system",
        GroupKey::Manufacturer => "NULLIF(s.manufacturer, '')",
        GroupKey::Hardware => "NULLIF(s.hardware, '')",
        GroupKey::Genre => "NULLIF(g.genre, '')",
        GroupKey::Developer => "NULLIF(g.developer, '')",
        GroupKey::Publisher => "NULLIF(g.publisher, '')",
        GroupKey::Region => "NULLIF(g.region, '')",
        GroupKey::Year => "g.release_year",
        GroupKey::Decade => "(g.release_year / 10) * 10",
        GroupKey::Players => "g.players_max",
    }
}

fn is_numeric(key: GroupKey) -> bool {
    matches!(key, GroupKey::Year | GroupKey::Decade | GroupKey::Players)
}

fn key_string(v: Value) -> String {
    match v {
        Value::Null | Value::Blob(_) => String::new(),
        Value::Integer(n) => n.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => s,
    }
}

fn label(key: GroupKey, value: &str) -> String {
    match (key, value) {
        (_, "") => "Unknown".to_owned(),
        (GroupKey::Decade, v) => format!("{v}s"),
        (GroupKey::Players, "1") => "1 player".to_owned(),
        (GroupKey::Players, v) => format!("{v} players"),
        (_, v) => v.to_owned(),
    }
}
