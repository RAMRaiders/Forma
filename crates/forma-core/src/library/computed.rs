//! What Forma works out for itself: RetroAchievements hashes (from romdex) and the RA game ids
//! they match. Stored as the lowest source, so an imported or user value always wins and a
//! hash imported from elsewhere is never replaced by one computed here.

use std::collections::HashMap;

use rusqlite::params;
use serde::Serialize;

use super::{Library, Source, materialise};
use crate::error::Result;
use crate::field;

/// A system's match rate: games on disk, those with an RA hash, those matched to an RA game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaMatch {
    pub system: String,
    pub games: i64,
    pub hashed: i64,
    pub matched: i64,
}

impl Library {
    /// Stores computed RA hashes for `system`'s games, by path relative to its folder. Returns
    /// how many changed.
    pub fn set_ra_hashes(&mut self, system: &str, hashes: &[(String, String)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut changed = Vec::new();
        {
            let mut find =
                tx.prepare_cached("SELECT id FROM game WHERE system = ?1 AND path = ?2")?;
            let mut old = tx.prepare_cached(
                "SELECT value FROM game_field WHERE game_id = ?1 AND field = ?2 AND source = ?3",
            )?;
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO game_field (game_id, field, source, value)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (path, hash) in hashes {
                let Some(id) = find
                    .query_row(params![system, path], |r| r.get::<_, i64>(0))
                    .ok()
                else {
                    continue;
                };
                let was: Option<String> = old
                    .query_row(params![id, field::RA_HASH, Source::Forma as i64], |r| {
                        r.get(0)
                    })
                    .ok();
                if was.as_deref() != Some(hash.as_str()) {
                    put.execute(params![id, field::RA_HASH, Source::Forma as i64, hash])?;
                    // A new hash may be another game: its old match goes with it.
                    tx.execute(
                        "DELETE FROM game_field WHERE game_id = ?1 AND field = ?2 AND source = ?3",
                        params![id, field::RA_GAME_ID, Source::Forma as i64],
                    )?;
                    changed.push(id);
                }
            }
        }
        materialise(&tx, &changed, &[])?;
        tx.commit()?;
        Ok(changed.len())
    }

    /// Games on disk with an RA hash but no RA game id: `(id, system, hash)`.
    pub fn unmatched_ra_hashes(&self) -> Result<Vec<(i64, String, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, system, ra_hash FROM game
             WHERE present = 1 AND ra_hash IS NOT NULL AND ra_game_id IS NULL",
        )?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Stores the RA game ids matched to games' hashes: `(game id, RA game id)`.
    pub fn set_ra_game_ids(&mut self, ids: &[(i64, i64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO game_field (game_id, field, source, value)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (game, ra) in ids {
                put.execute(params![
                    game,
                    field::RA_GAME_ID,
                    Source::Forma as i64,
                    ra.to_string()
                ])?;
            }
        }
        let games: Vec<i64> = ids.iter().map(|(g, _)| *g).collect();
        materialise(&tx, &games, &[])?;
        tx.commit()?;
        Ok(())
    }

    /// Per system, of `systems`: games on disk, hashed, matched.
    pub fn ra_match_report(&self, systems: &[String]) -> Result<Vec<RaMatch>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT system, COUNT(*), COUNT(ra_hash), COUNT(ra_game_id) FROM game
             WHERE present = 1 GROUP BY system ORDER BY system",
        )?;
        let all: HashMap<String, RaMatch> = stmt
            .query_map([], |r| {
                Ok(RaMatch {
                    system: r.get(0)?,
                    games: r.get(1)?,
                    hashed: r.get(2)?,
                    matched: r.get(3)?,
                })
            })?
            .map(|m| m.map(|m| (m.system.clone(), m)))
            .collect::<rusqlite::Result<_>>()?;
        Ok(systems.iter().filter_map(|s| all.get(s).cloned()).collect())
    }
}
