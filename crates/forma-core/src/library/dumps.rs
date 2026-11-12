//! Each game's dump check (#115), as [`crate::dumps`] works it out: stored apart from the
//! gamelist's fields, replaced whole per system on every check.

use rusqlite::params;
use serde::Serialize;

use super::{Dump, Library};
use crate::error::Result;

/// A system's dump check: games on disk, those checked, and how they came out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpReport {
    pub system: String,
    pub games: i64,
    pub checked: i64,
    pub verified: i64,
    /// Matched on size and CRC32 only, or matched a dump the DAT marks bad.
    pub doubtful: i64,
    pub not_catalogued: i64,
}

impl Library {
    /// Replaces `system`'s dump checks with `dumps`, by path relative to its folder, with the
    /// file's SHA1. Games not in `dumps` go back to unknown. Returns how many changed.
    pub fn set_dumps(&mut self, system: &str, dumps: &[(String, String, Dump)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut changed = 0;
        {
            let mut id_of =
                tx.prepare_cached("SELECT id FROM game WHERE system = ?1 AND path = ?2")?;
            let mut current = tx.prepare_cached(
                "SELECT status, catalogue, name, region, sha1 FROM game_dump WHERE game_id = ?1",
            )?;
            let mut put = tx.prepare_cached(
                "INSERT INTO game_dump (game_id, status, catalogue, name, region, sha1)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (game_id) DO UPDATE SET status = excluded.status,
                     catalogue = excluded.catalogue, name = excluded.name,
                     region = excluded.region, sha1 = excluded.sha1",
            )?;
            let mut kept = Vec::new();
            for (path, sha1, dump) in dumps {
                let Some(id) = id_of
                    .query_row(params![system, path], |r| r.get::<_, i64>(0))
                    .ok()
                else {
                    continue;
                };
                kept.push(id);
                let before = current
                    .query_row([id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, String>(4)?,
                        ))
                    })
                    .ok();
                let now = (
                    dump.status.clone(),
                    dump.catalogue.clone(),
                    dump.name.clone(),
                    dump.region.clone(),
                    sha1.clone(),
                );
                if before.as_ref() != Some(&now) {
                    put.execute(params![
                        id,
                        dump.status,
                        dump.catalogue,
                        dump.name,
                        dump.region,
                        sha1
                    ])?;
                    changed += 1;
                }
            }
            // Gone from the check (a file no longer read, a DAT no longer there): unknown.
            let mut stale = tx.prepare_cached(
                "SELECT d.game_id FROM game_dump d JOIN game g ON g.id = d.game_id
                 WHERE g.system = ?1",
            )?;
            let stale: Vec<i64> = stale
                .query_map([system], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<i64>>>()?
                .into_iter()
                .filter(|id| !kept.contains(id))
                .collect();
            let mut forget = tx.prepare_cached("DELETE FROM game_dump WHERE game_id = ?1")?;
            for id in stale {
                forget.execute([id])?;
                changed += 1;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The dump check of each of `systems` with games, in that order.
    pub fn dump_report(&self, systems: &[String]) -> Result<Vec<DumpReport>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT COUNT(g.id),
                    COUNT(d.game_id),
                    COUNT(*) FILTER (WHERE d.status = 'verified'),
                    COUNT(*) FILTER (WHERE d.status IN ('crcOnly', 'badDump')),
                    COUNT(*) FILTER (WHERE d.status = 'notCatalogued')
             FROM game g LEFT JOIN game_dump d ON d.game_id = g.id
             WHERE g.system = ?1 AND g.present = 1",
        )?;
        let mut out = Vec::new();
        for system in systems {
            let report = stmt.query_row([system], |r| {
                Ok(DumpReport {
                    system: system.clone(),
                    games: r.get(0)?,
                    checked: r.get(1)?,
                    verified: r.get(2)?,
                    doubtful: r.get(3)?,
                    not_catalogued: r.get(4)?,
                })
            })?;
            if report.games > 0 {
                out.push(report);
            }
        }
        Ok(out)
    }
}
