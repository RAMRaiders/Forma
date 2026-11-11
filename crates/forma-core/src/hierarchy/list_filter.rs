//! The user's filter on the game list on screen: by words in the name, first letter and what
//! the game is. Forma's Select menu sets it and every theme's list follows, as `nav.browse`
//! answers with the filtered games.

use std::collections::BTreeMap;

use rusqlite::params_from_iter;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use super::browse::{Conds, key_string};
use super::{NodeKind, View};
use crate::error::{Error, Result};
use crate::library::Library;

/// Everything set must hold. Empty: every game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListFilter {
    /// Words the name contains.
    pub name: Option<String>,
    /// `A` to `Z`, or `#` for a name starting with anything else.
    pub letter: Option<String>,
    pub genre: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub region: Option<String>,
    pub year: Option<i64>,
    /// At least this many players at once.
    pub players: Option<i64>,
    pub favorites: bool,
    /// Only these games: one game's versions and siblings (other names), from its Select menu.
    pub games: Option<Vec<i64>>,
}

impl ListFilter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One value to filter by, and how many games it would leave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterChoice {
    pub value: String,
    pub count: i64,
}

/// What the menu offers for a list. Each property's choices take the other properties'
/// filters into account, so none of them leaves an empty list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub filter: ListFilter,
    /// Games the filter leaves.
    pub total: i64,
    /// Games in the list without it.
    pub all: i64,
    pub letters: Vec<FilterChoice>,
    pub genres: Vec<FilterChoice>,
    pub developers: Vec<FilterChoice>,
    pub publishers: Vec<FilterChoice>,
    pub regions: Vec<FilterChoice>,
    pub years: Vec<FilterChoice>,
    pub players: Vec<FilterChoice>,
}

const FIRST_LETTER: &str = "UPPER(SUBSTR(LTRIM(g.name), 1, 1))";
/// A game's genres, comma-separated whichever way the scraper wrote them.
const GENRES: &str =
    "',' || REPLACE(REPLACE(REPLACE(g.genre, ' / ', ','), '/', ','), ', ', ',') || ','";

/// `text` taken literally inside a `LIKE ... ESCAPE '\'`.
fn literal(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Conds {
    pub(super) fn list_filter(&mut self, f: &ListFilter) {
        if let Some(name) = f.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            self.sql.push("g.name LIKE ? ESCAPE '\\'".into());
            self.params
                .push(Value::Text(format!("%{}%", literal(name))));
        }
        if let Some(letter) = &f.letter {
            if letter == "#" {
                self.sql
                    .push(format!("{FIRST_LETTER} NOT BETWEEN 'A' AND 'Z'"));
            } else {
                self.sql.push(format!("{FIRST_LETTER} = ?"));
                self.params.push(Value::Text(letter.to_uppercase()));
            }
        }
        if let Some(genre) = &f.genre {
            self.sql.push(format!("{GENRES} LIKE ? ESCAPE '\\'"));
            self.params
                .push(Value::Text(format!("%,{},%", literal(genre))));
        }
        for (column, value) in [
            ("g.developer", &f.developer),
            ("g.publisher", &f.publisher),
            ("g.region", &f.region),
        ] {
            if let Some(value) = value {
                self.sql.push(format!("{column} = ? COLLATE NOCASE"));
                self.params.push(Value::Text(value.clone()));
            }
        }
        if let Some(year) = f.year {
            self.sql.push("g.release_year = ?".into());
            self.params.push(Value::Integer(year));
        }
        if let Some(players) = f.players {
            self.sql.push("g.players_max >= ?".into());
            self.params.push(Value::Integer(players));
        }
        if f.favorites {
            self.sql.push("g.favorite = 1".into());
        }
        if let Some(games) = &f.games {
            if games.is_empty() {
                self.sql.push("0".into());
            } else {
                let marks = vec!["?"; games.len()].join(", ");
                self.sql.push(format!("g.id IN ({marks})"));
                self.params
                    .extend(games.iter().map(|&id| Value::Integer(id)));
            }
        }
    }
}

impl Library {
    /// The filter's result for the game list at `path`, and the choices to refine it.
    pub fn filter_options(
        &self,
        view: &View,
        path: &[String],
        filter: &ListFilter,
    ) -> Result<FilterOptions> {
        let (node, base, _) = self.node_at(view, path)?;
        if !matches!(node.kind, NodeKind::Games(_)) {
            return Err(Error::View(format!(
                "`{}` is not a game list in view `{}`",
                path.join("/"),
                view.id
            )));
        }
        let with = |clear: fn(&mut ListFilter)| {
            let mut f = filter.clone();
            clear(&mut f);
            let mut conds = base.clone();
            conds.list_filter(&f);
            conds
        };
        let values = |clear: fn(&mut ListFilter), expr: &str| self.facet(expr, &with(clear));

        let mut letters = BTreeMap::new();
        for c in values(|f| f.letter = None, FIRST_LETTER)? {
            let upper = c.value.chars().next().filter(char::is_ascii_uppercase);
            let key = upper.map_or_else(|| "#".to_owned(), String::from);
            *letters.entry(key).or_insert(0) += c.count;
        }

        // A game of several genres counts under each; spelled as first seen.
        let mut genres: BTreeMap<String, FilterChoice> = BTreeMap::new();
        for c in values(|f| f.genre = None, "NULLIF(g.genre, '')")? {
            for genre in c.value.split([',', '/']).map(str::trim) {
                if !genre.is_empty() {
                    genres
                        .entry(genre.to_lowercase())
                        .or_insert_with(|| FilterChoice {
                            value: genre.to_owned(),
                            count: 0,
                        })
                        .count += c.count;
                }
            }
        }

        // "At least n players": a game for four counts for two as well.
        let mut players = values(|f| f.players = None, "g.players_max")?;
        players.sort_by_key(|c| std::cmp::Reverse(c.value.parse::<i64>().unwrap_or(0)));
        let mut sum = 0;
        for c in &mut players {
            sum += c.count;
            c.count = sum;
        }
        players.reverse();
        players.retain(|c| c.value.parse::<i64>().is_ok_and(|n| n >= 2));

        Ok(FilterOptions {
            filter: filter.clone(),
            total: self.filtered_count(&with(|_| {}))?,
            all: self.filtered_count(&base)?,
            letters: letters
                .into_iter()
                .map(|(value, count)| FilterChoice { value, count })
                .collect(),
            genres: genres.into_values().collect(),
            developers: values(|f| f.developer = None, "NULLIF(g.developer, '')")?,
            publishers: values(|f| f.publisher = None, "NULLIF(g.publisher, '')")?,
            regions: values(|f| f.region = None, "NULLIF(g.region, '')")?,
            years: values(|f| f.year = None, "g.release_year")?,
            players,
        })
    }

    fn filtered_count(&self, conds: &Conds) -> Result<i64> {
        Ok(self.conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM game g JOIN system s ON s.name = g.system WHERE {}",
                conds.sql()
            ),
            params_from_iter(&conds.params),
            |r| r.get(0),
        )?)
    }

    fn facet(&self, expr: &str, conds: &Conds) -> Result<Vec<FilterChoice>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {expr} AS k, COUNT(*) FROM game g JOIN system s ON s.name = g.system
             WHERE {} AND {expr} IS NOT NULL GROUP BY k ORDER BY k COLLATE NOCASE",
            conds.sql()
        ))?;
        let rows = stmt.query_map(params_from_iter(&conds.params), |r| {
            Ok(FilterChoice {
                value: key_string(r.get::<_, Value>(0)?),
                count: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
