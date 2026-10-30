//! Bringing the library in line with what is on disk.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Library, Source, default_name, materialise};
use crate::error::{Result, Warning};
use crate::gamelist::parse_gamelist;
use crate::paths::Paths;
use crate::scan::{ScannedRom, scan_system};
use crate::systems::{System, load_systems};

#[derive(Debug, Default, Clone, Serialize)]
pub struct RefreshReport {
    pub systems: usize,
    /// Games present on disk after the refresh.
    pub games: usize,
    pub added: usize,
    /// Known games whose file is gone, kept (with the user's data) in case it comes back.
    pub missing: usize,
    pub gamelists_imported: usize,
    pub gamelists_unchanged: usize,
    pub warnings: Vec<Warning>,
}

impl Library {
    /// Re-reads system definitions, scans every ROM folder, and re-imports each `gamelist.xml`
    /// that changed. A gamelist is also re-imported when its system gained a game, in case the
    /// file already described it. One transaction: a reader sees the old or the new library.
    pub fn refresh(&mut self, paths: &Paths) -> Result<RefreshReport> {
        let load = load_systems(paths)?;
        let mut report = RefreshReport {
            systems: load.systems.len(),
            warnings: load.warnings,
            ..RefreshReport::default()
        };

        let tx = self.conn.transaction()?;
        sync_systems(&tx, &load.systems)?;
        for system in &load.systems {
            let (roms, warnings) = scan_system(system);
            report.warnings.extend(warnings);
            refresh_system(&tx, system, &roms, paths, &mut report)?;
        }
        tx.commit()?;
        Ok(report)
    }
}

fn sync_systems(tx: &Transaction<'_>, systems: &[System]) -> Result<()> {
    let mut upsert = tx.prepare_cached(
        "INSERT INTO system (name, fullname, manufacturer, release_year, hardware, rom_path,
                             sort_order, definition)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (name) DO UPDATE SET
             fullname = excluded.fullname, manufacturer = excluded.manufacturer,
             release_year = excluded.release_year, hardware = excluded.hardware,
             rom_path = excluded.rom_path, sort_order = excluded.sort_order,
             definition = excluded.definition",
    )?;
    for (order, s) in systems.iter().enumerate() {
        let definition = serde_json::to_string(s).unwrap_or_default();
        upsert.execute(params![
            s.name,
            s.fullname,
            s.manufacturer,
            s.release_year,
            s.hardware,
            s.rom_path.to_string_lossy(),
            i64::try_from(order).unwrap_or(i64::MAX),
            definition,
        ])?;
    }

    // A system removed from every config takes its games with it (ON DELETE CASCADE).
    let names: Vec<&str> = systems.iter().map(|s| s.name.as_str()).collect();
    let mut known = tx.prepare_cached("SELECT name FROM system")?;
    let stale: Vec<String> = known
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?
        .into_iter()
        .filter(|n| !names.contains(&n.as_str()))
        .collect();
    for name in stale {
        tx.execute("DELETE FROM system WHERE name = ?1", [name])?;
    }
    Ok(())
}

fn refresh_system(
    tx: &Transaction<'_>,
    system: &System,
    roms: &[ScannedRom],
    paths: &Paths,
    report: &mut RefreshReport,
) -> Result<()> {
    let mut ids: HashMap<String, i64> = {
        let mut stmt = tx.prepare_cached("SELECT path, id FROM game WHERE system = ?1")?;
        stmt.query_map([&system.name], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };

    // Files on disk.
    let mut added = Vec::new();
    let mut seen = HashMap::with_capacity(roms.len());
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO game (system, path, size, mtime_ns, name) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut update = tx.prepare_cached(
            "UPDATE game SET present = 1, size = ?2, mtime_ns = ?3 WHERE id = ?1",
        )?;
        for rom in roms {
            let path = rom.path.to_string_lossy().into_owned();
            let size = i64::try_from(rom.size).unwrap_or(i64::MAX);
            let id = match ids.get(&path) {
                Some(&id) => {
                    update.execute(params![id, size, rom.mtime_ns])?;
                    id
                }
                None => {
                    insert.execute(params![
                        system.name,
                        path,
                        size,
                        rom.mtime_ns,
                        default_name(&path)
                    ])?;
                    let id = tx.last_insert_rowid();
                    ids.insert(path.clone(), id);
                    added.push(id);
                    id
                }
            };
            seen.insert(path, id);
        }
    }
    let mut mark_missing = tx.prepare_cached("UPDATE game SET present = 0 WHERE id = ?1")?;
    for (path, &id) in &ids {
        if !seen.contains_key(path) {
            mark_missing.execute([id])?;
            report.missing += 1;
        }
    }
    report.games += roms.len();
    report.added += added.len();

    // The gamelist, only when it (or the set of games it can describe) changed.
    let file = system.rom_path.join("gamelist.xml");
    let affected = if gamelist_changed(tx, &file)? || !added.is_empty() {
        import_gamelist(tx, system, &file, &ids, paths, report)?;
        report.gamelists_imported += usize::from(file.exists());
        ids.values().copied().collect()
    } else {
        report.gamelists_unchanged += usize::from(file.exists());
        added.clone()
    };
    materialise(tx, &affected, &added)
}

/// Compares the file's size and mtime with the last import. A file that disappeared counts
/// as changed, so its fields are dropped.
fn gamelist_changed(tx: &Transaction<'_>, file: &Path) -> Result<bool> {
    let key = file.to_string_lossy();
    let previous: Option<(i64, i64)> = tx
        .query_row(
            "SELECT size, mtime_ns FROM import_file WHERE path = ?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(stat(file) != previous)
}

fn stat(file: &Path) -> Option<(i64, i64)> {
    let meta = std::fs::metadata(file).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((
        i64::try_from(meta.len()).unwrap_or(i64::MAX),
        i64::try_from(mtime).unwrap_or(i64::MAX),
    ))
}

fn import_gamelist(
    tx: &Transaction<'_>,
    system: &System,
    file: &Path,
    ids: &HashMap<String, i64>,
    paths: &Paths,
    report: &mut RefreshReport,
) -> Result<()> {
    // Replace this system's gamelist layer wholesale; other sources are untouched.
    tx.execute(
        "DELETE FROM game_field WHERE source = ?1
         AND game_id IN (SELECT id FROM game WHERE system = ?2)",
        params![Source::Gamelist as i64, system.name],
    )?;
    let key = file.to_string_lossy();

    let Some(size_mtime) = stat(file) else {
        tx.execute("DELETE FROM import_file WHERE path = ?1", [&key])?;
        return Ok(());
    };
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) => {
            report
                .warnings
                .push(Warning::new(file, format!("cannot read: {e}")));
            return Ok(());
        }
    };
    let list = parse_gamelist(
        &String::from_utf8_lossy(&bytes),
        file,
        &system.rom_path,
        paths,
    );
    report.warnings.extend(list.warnings);

    let mut insert = tx.prepare_cached(
        "INSERT OR REPLACE INTO game_field (game_id, field, source, value) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for entry in list.entries {
        let path = entry.path.to_string_lossy();
        let Some(&id) = ids.get(path.as_ref()) else {
            report.warnings.push(Warning::new(
                file,
                format!("`{path}` is listed but not on disk; skipped"),
            ));
            continue;
        };
        for (field, value) in entry.fields {
            insert.execute(params![id, field, Source::Gamelist as i64, value])?;
        }
    }
    tx.execute(
        "INSERT INTO import_file (path, size, mtime_ns) VALUES (?1, ?2, ?3)
         ON CONFLICT (path) DO UPDATE SET size = excluded.size, mtime_ns = excluded.mtime_ns",
        params![key, size_mtime.0, size_mtime.1],
    )?;
    Ok(())
}
