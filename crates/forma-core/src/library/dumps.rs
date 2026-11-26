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
    /// A playlist's is its discs' together (see [`of_discs`]), when all were checked; the
    /// playlist file's own is never kept (it is a list of names, in no DAT).
    pub fn set_dumps(&mut self, system: &str, dumps: &[(String, String, Dump)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut changed = 0;
        let dumps = {
            let mut stmt = tx.prepare_cached(
                "SELECT g.path, d.path FROM game_disc d JOIN game g ON g.id = d.game_id
                 WHERE g.system = ?1 ORDER BY g.id, d.n",
            )?;
            let rows: Vec<(String, String)> = stmt
                .query_map([system], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            with_playlists(dumps, &rows)
        };
        let dumps = dumps.as_slice();
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

/// `dumps` with each playlist's (`playlists`: its path and each of its discs', in order) made of
/// its discs' in place of its own file's; none for a playlist with a disc not checked.
fn with_playlists(
    dumps: &[(String, String, Dump)],
    playlists: &[(String, String)],
) -> Vec<(String, String, Dump)> {
    let mut discs: Vec<(&str, Vec<&str>)> = Vec::new();
    for (playlist, disc) in playlists {
        match discs.last_mut() {
            Some((p, list)) if *p == playlist.as_str() => list.push(disc),
            _ => discs.push((playlist, vec![disc])),
        }
    }
    let by_path: std::collections::HashMap<&str, (&str, &Dump)> = dumps
        .iter()
        .map(|(path, sha1, dump)| (path.as_str(), (sha1.as_str(), dump)))
        .collect();
    let mut out: Vec<(String, String, Dump)> = dumps
        .iter()
        .filter(|(path, _, _)| !discs.iter().any(|(p, _)| p == path))
        .cloned()
        .collect();
    for (playlist, list) in discs {
        let checked: Option<Vec<(&str, &Dump)>> =
            list.iter().map(|d| by_path.get(d).copied()).collect();
        let Some(checked) = checked else { continue };
        let Some(&(sha1, _)) = checked.first() else {
            continue;
        };
        let dumps: Vec<&Dump> = checked.iter().map(|(_, d)| *d).collect();
        out.push((playlist.to_owned(), sha1.to_owned(), of_discs(&dumps)));
    }
    out
}

/// A game of several discs' dump check: as good as its worst disc's (a bad dump, then one in no
/// DAT, then one matched on its CRC32 alone); its catalogue, name and region the first disc's,
/// the name without its disc (`Deep Fear (Europe)`).
pub(crate) fn of_discs(discs: &[&Dump]) -> Dump {
    let rank = |status: &str| match status {
        "badDump" => 3,
        "notCatalogued" => 2,
        "crcOnly" => 1,
        _ => 0,
    };
    let worst = discs
        .iter()
        .map(|d| d.status.as_str())
        .max_by_key(|s| rank(s))
        .unwrap_or("notCatalogued");
    let first = discs.first();
    let catalogued = worst != "notCatalogued";
    Dump {
        status: worst.to_owned(),
        catalogue: first
            .and_then(|d| d.catalogue.clone())
            .filter(|_| catalogued),
        name: first
            .and_then(|d| d.name.as_deref())
            .filter(|_| catalogued)
            .map(without_disc),
        region: first.and_then(|d| d.region.clone()).filter(|_| catalogued),
    }
}

/// `name` without its `(Disc 2)`-like tags.
fn without_disc(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(open) = rest.find(" (Disc ") {
        let Some(len) = rest[open..].find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(status: &str, name: Option<&str>) -> Dump {
        Dump {
            status: status.to_owned(),
            catalogue: name.map(|_| "redump".to_owned()),
            name: name.map(str::to_owned),
            region: name.map(|_| "Europe".to_owned()),
        }
    }

    #[test]
    fn a_games_discs_make_one_check_as_good_as_its_worst_disc() {
        let one = dump("verified", Some("Deep Fear (Europe) (Disc 1)"));
        let two = dump("verified", Some("Deep Fear (Europe) (Disc 2)"));
        let both = of_discs(&[&one, &two]);
        assert_eq!(both.status, "verified");
        assert_eq!(both.name.as_deref(), Some("Deep Fear (Europe)"));
        assert_eq!(both.catalogue.as_deref(), Some("redump"));

        let crc = dump("crcOnly", Some("Deep Fear (Europe) (Disc 2)"));
        assert_eq!(of_discs(&[&one, &crc]).status, "crcOnly");
        let none = dump("notCatalogued", None);
        let mixed = of_discs(&[&one, &none]);
        assert_eq!(mixed.status, "notCatalogued");
        assert_eq!(mixed.name, None, "not catalogued, as a whole");
        let bad = dump("badDump", Some("Deep Fear (Europe) (Disc 2)"));
        assert_eq!(of_discs(&[&none, &bad]).status, "badDump");
    }

    #[test]
    fn a_playlist_is_checked_by_its_discs_once_all_are() {
        let entry = |path: &str, d: Dump| (path.to_owned(), format!("sha-{path}"), d);
        let dumps = vec![
            entry("Deep Fear.m3u", dump("notCatalogued", None)),
            entry(
                "Deep Fear (Disc 1).chd",
                dump("verified", Some("Deep Fear (Disc 1)")),
            ),
            entry(
                "Deep Fear (Disc 2).chd",
                dump("verified", Some("Deep Fear (Disc 2)")),
            ),
            entry("Lunacy.m3u", dump("notCatalogued", None)),
            entry(
                "Lunacy (Disc 1).chd",
                dump("verified", Some("Lunacy (Disc 1)")),
            ),
        ];
        let playlists = [
            ("Deep Fear.m3u", "Deep Fear (Disc 1).chd"),
            ("Deep Fear.m3u", "Deep Fear (Disc 2).chd"),
            ("Lunacy.m3u", "Lunacy (Disc 1).chd"),
            ("Lunacy.m3u", "Lunacy (Disc 2).chd"),
        ]
        .map(|(p, d)| (p.to_owned(), d.to_owned()));
        let out = with_playlists(&dumps, &playlists);
        let of = |path: &str| {
            out.iter()
                .find(|(p, _, _)| p == path)
                .map(|(_, s, d)| (s, d))
        };
        let (sha1, deep) = of("Deep Fear.m3u").expect("checked by its discs");
        assert_eq!(deep.status, "verified");
        assert_eq!(deep.name.as_deref(), Some("Deep Fear"));
        assert_eq!(sha1, "sha-Deep Fear (Disc 1).chd");
        assert!(of("Lunacy.m3u").is_none(), "a disc not checked: unknown");
        assert!(
            of("Deep Fear (Disc 1).chd").is_some(),
            "the discs' own kept"
        );
    }
}
