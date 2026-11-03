//! Recommended games: the ones not played yet that look most like what the user plays.
//!
//! A taste profile is made from every game played or starred. Each counts by how much it was
//! played (time, then sessions), how recently, and whether it is a favourite. The profile
//! weighs what those games share: genre, system, developer, decade, and whether they are for
//! more than one player. Each unplayed game scores by how much of the profile it matches, plus
//! a little for its rating. Games already starred are left out (the user knows them), and a
//! game on several systems comes once. The list is then spread out, so one system or genre
//! does not fill it. With nothing played yet, the best-rated games come first, spread the same
//! way.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::Library;
use crate::error::Result;

/// How much each kind of likeness counts.
const GENRE: f64 = 3.0;
const SYSTEM: f64 = 2.0;
const DEVELOPER: f64 = 1.5;
const DECADE: f64 = 1.0;
const PLAYERS: f64 = 0.5;
const RATING: f64 = 1.0;
/// Each further pick from the same system, or genre, counts this much less.
const SAME_SYSTEM: f64 = 0.8;
const SAME_GENRE: f64 = 0.9;
/// Only the best this many are spread out: enough for any list, cheap to re-rank.
const POOL: usize = 600;

#[derive(Default)]
struct Profile {
    genre: HashMap<String, f64>,
    system: HashMap<String, f64>,
    developer: HashMap<String, f64>,
    decade: HashMap<i64, f64>,
    multiplayer: HashMap<bool, f64>,
}

impl Profile {
    fn is_empty(&self) -> bool {
        self.system.is_empty()
    }
}

/// What a game is like, for matching.
struct Traits {
    system: String,
    genres: Vec<String>,
    developer: Option<String>,
    decade: Option<i64>,
    multiplayer: Option<bool>,
}

impl Traits {
    fn read(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Self> {
        let genre: Option<String> = r.get(at + 1)?;
        let developer: Option<String> = r.get(at + 2)?;
        let year: Option<i64> = r.get(at + 3)?;
        let players: Option<i64> = r.get(at + 4)?;
        Ok(Self {
            system: r.get(at)?,
            genres: genres(genre.as_deref()),
            developer: developer
                .map(|d| d.trim().to_lowercase())
                .filter(|d| !d.is_empty()),
            decade: year.filter(|y| *y > 1900).map(|y| y / 10 * 10),
            multiplayer: players.map(|p| p > 1),
        })
    }
}

/// "Platform / Action, Shooter" → `platform`, `action`, `shooter`.
fn genres(text: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in text.unwrap_or_default().split(['/', ',', ';', '-', '&']) {
        let g = g.trim().to_lowercase();
        if !g.is_empty() && !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

/// A name with its region and version tags dropped: `tiny toon adventures 2`.
fn title(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            c if depth == 0 && c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ if depth == 0 && !out.ends_with(' ') && !out.is_empty() => out.push(' '),
            _ => {}
        }
    }
    out.trim().to_owned()
}

/// The share of the profile's weight on `key`: 0 to 1.
fn share<K: std::hash::Hash + Eq>(map: &HashMap<K, f64>, key: &K) -> f64 {
    let total: f64 = map.values().sum();
    if total <= 0.0 {
        return 0.0;
    }
    map.get(key).copied().unwrap_or(0.0) / total
}

/// Days since 1970-01-01 of `YYYY-MM-DD…`.
fn days(date: &str) -> Option<i64> {
    let mut parts = date.get(..10)?.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

impl Library {
    /// The ids of up to `limit` recommended games among those `conds` keep (a view's filters,
    /// as SQL over `game g JOIN system s`), best first.
    pub(crate) fn recommended(
        &self,
        conds: &str,
        params: &[Value],
        limit: usize,
    ) -> Result<Vec<i64>> {
        let profile = self.taste()?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT g.id, g.rating, g.system, g.genre, g.developer, g.release_year, g.players_max,
                    g.name
             FROM game g JOIN system s ON s.name = g.system
             WHERE {conds} AND g.play_count = 0 AND g.last_played IS NULL AND g.favorite = 0"
        ))?;
        let rows = stmt.query_map(params_from_iter(params), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<f64>>(1)?,
                Traits::read(r, 2)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        let mut scored: Vec<(f64, i64, Traits, String)> = Vec::new();
        for row in rows {
            let (id, rating, traits, name) = row?;
            // Ratings are 0–1; an unrated game is taken as middling.
            let mut score = RATING * rating.unwrap_or(0.5);
            if !profile.is_empty() {
                let genre = traits
                    .genres
                    .iter()
                    .map(|g| share(&profile.genre, g))
                    .fold(0.0, f64::max);
                score += GENRE * genre + SYSTEM * share(&profile.system, &traits.system);
                if let Some(d) = &traits.developer {
                    score += DEVELOPER * share(&profile.developer, d);
                }
                if let Some(d) = traits.decade {
                    score += DECADE * share(&profile.decade, &d);
                }
                if let Some(m) = traits.multiplayer {
                    score += PLAYERS * share(&profile.multiplayer, &m);
                }
            }
            scored.push((score, id, traits, title(&name)));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        // The same game on several systems (Game Boy, Super Game Boy…): its best place only.
        let mut seen = std::collections::HashSet::new();
        scored.retain(|s| s.3.is_empty() || seen.insert(s.3.clone()));
        scored.truncate(POOL);

        // Spread out: each pick lowers the others from its system and genres.
        let mut systems: HashMap<String, i32> = HashMap::new();
        let mut genres: HashMap<String, i32> = HashMap::new();
        let mut picked = Vec::with_capacity(limit.min(scored.len()));
        while picked.len() < limit && !scored.is_empty() {
            let adjusted = |(score, _, t, _): &(f64, i64, Traits, String)| {
                let same_genre = t
                    .genres
                    .iter()
                    .map(|g| genres.get(g).copied().unwrap_or(0))
                    .max();
                score
                    * SAME_SYSTEM.powi(systems.get(&t.system).copied().unwrap_or(0))
                    * SAME_GENRE.powi(same_genre.unwrap_or(0))
            };
            let best = scored
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| adjusted(a).total_cmp(&adjusted(b)).then(b.1.cmp(&a.1)))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let (_, id, t, _) = scored.swap_remove(best);
            *systems.entry(t.system).or_default() += 1;
            for g in t.genres {
                *genres.entry(g).or_default() += 1;
            }
            picked.push(id);
        }
        Ok(picked)
    }

    /// What the user plays, from every game played or starred.
    fn taste(&self) -> Result<Profile> {
        let today = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs() / 86_400).unwrap_or(0));
        let mut stmt = self.conn.prepare_cached(
            "SELECT g.system, g.genre, g.developer, g.release_year, g.players_max,
                    g.game_time, g.play_count, g.last_played, g.favorite
             FROM game g WHERE g.present = 1
               AND (g.play_count > 0 OR g.last_played IS NOT NULL OR g.favorite = 1)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                Traits::read(r, 0)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, bool>(8)?,
            ))
        })?;
        let mut p = Profile::default();
        for row in rows {
            let (t, seconds, sessions, last, favorite) = row?;
            // Time played counts most, but slowly: an hour is not sixty times a minute.
            #[allow(clippy::cast_precision_loss)]
            let mut w = (1.0 + seconds.max(0) as f64 / 60.0).ln() + 0.5 * (sessions.min(20) as f64);
            if let Some(age) = last.as_deref().and_then(days).map(|d| today - d) {
                w *= match age {
                    ..=30 => 1.5,
                    31..=180 => 1.0,
                    _ => 0.6,
                };
            }
            // A star says the user likes it, but stars come by the hundred: less than an hour.
            if favorite {
                w += 1.0;
            }
            let w = w.max(0.5);
            for g in &t.genres {
                *p.genre.entry(g.clone()).or_default() += w;
            }
            *p.system.entry(t.system).or_default() += w;
            if let Some(d) = t.developer {
                *p.developer.entry(d).or_default() += w;
            }
            if let Some(d) = t.decade {
                *p.decade.entry(d).or_default() += w;
            }
            if let Some(m) = t.multiplayer {
                *p.multiplayer.entry(m).or_default() += w;
            }
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_genres_and_dates() {
        assert_eq!(
            genres(Some("Platform / Action, Shooter")),
            ["platform", "action", "shooter"]
        );
        assert!(genres(None).is_empty());
        assert_eq!(days("1970-01-02T00:00:00"), Some(1));
        assert_eq!(days("2026-11-03"), Some(20_760));
        assert_eq!(days("garbage"), None);
        assert_eq!(
            title("Tiny Toon Adventures 2 : Montana's (USA) [!]"),
            "tiny toon adventures 2 montana s"
        );
    }
}
