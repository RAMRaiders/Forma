//! Bringing the library in line with what is on disk.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use super::{Library, Source, default_name, materialise};

use crate::error::{Result, Warning};
use crate::field;
use crate::gamelist::parse_gamelist;
use crate::paths::Paths;
use crate::scan::{ScannedRom, scan_system};
use crate::scraper::regions;
use crate::systems::{System, load_systems};

/// Counts cover the systems that were refreshed (all of them, or the ones asked for).
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub systems: usize,
    /// Systems whose visible content changed: games added, gone or back, metadata re-imported,
    /// or the system itself added or removed. Empty means a UI has nothing to redraw.
    pub changed_systems: Vec<String>,
    /// Games present on disk after the refresh.
    pub games: usize,
    pub added: usize,
    /// Known games whose file is gone, kept (with the user's data) in case it comes back.
    pub missing: usize,
    pub gamelists_imported: usize,
    pub gamelists_unchanged: usize,
    /// Import files read this refresh (0 when none changed).
    pub imports_applied: usize,
    /// Import records that matched a game.
    pub import_records: usize,
    pub warnings: Vec<Warning>,
}

impl Library {
    /// Re-reads system definitions, scans every ROM folder, and re-imports each `gamelist.xml`
    /// that changed. A gamelist is also re-imported when its system gained a game, in case the
    /// file already described it. One transaction per system, with its folder scanned before the
    /// transaction opens: a reader sees each system old or new, and the UI's own writes (a
    /// favourite, a setting) wait for one system, not for the whole library, which on a cold SD
    /// card at boot took longer than SQLite's busy timeout.
    pub fn refresh(&mut self, paths: &Paths) -> Result<RefreshReport> {
        self.refresh_where(paths, |_| true)
    }

    /// [`Library::refresh`] limited to the named systems' folders, for when a watcher knows
    /// where the change was. System definitions are always re-read: they are one small file.
    pub fn refresh_systems(&mut self, paths: &Paths, names: &[String]) -> Result<RefreshReport> {
        self.refresh_where(paths, |s| names.contains(&s.name))
    }

    fn refresh_where(
        &mut self,
        paths: &Paths,
        include: impl Fn(&System) -> bool,
    ) -> Result<RefreshReport> {
        let load = load_systems(paths)?;
        let mut report = RefreshReport {
            warnings: load.warnings,
            ..RefreshReport::default()
        };

        let tx = self.conn.transaction()?;
        report.changed_systems = sync_systems(&tx, &load.systems)?;
        tx.commit()?;
        for system in load.systems.iter().filter(|s| include(s)) {
            report.systems += 1;
            let (roms, warnings) = scan_system(system);
            report.warnings.extend(warnings);
            let tx = self.conn.transaction()?;
            let changed = refresh_system(&tx, system, &roms, paths, &mut report)?;
            tx.commit()?;
            if changed && !report.changed_systems.contains(&system.name) {
                report.changed_systems.push(system.name.clone());
            }
        }
        // After the scan: a record may describe a game added just now.
        let added = report.added > 0;
        let tx = self.conn.transaction()?;
        super::imports::apply(&tx, paths, added, &mut report)?;
        tx.commit()?;
        self.load_collections(paths)?;
        Ok(report)
    }
}

/// Writes the system definitions; returns the names of systems added or removed.
fn sync_systems(tx: &Transaction<'_>, systems: &[System]) -> Result<Vec<String>> {
    let known: HashSet<String> = {
        let mut stmt = tx.prepare_cached("SELECT name FROM system")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut changed: Vec<String> = systems
        .iter()
        .filter(|s| !known.contains(&s.name))
        .map(|s| s.name.clone())
        .collect();

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
    for name in known {
        if !systems.iter().any(|s| s.name == name) {
            tx.execute("DELETE FROM system WHERE name = ?1", [&name])?;
            changed.push(name);
        }
    }
    Ok(changed)
}

fn refresh_system(
    tx: &Transaction<'_>,
    system: &System,
    roms: &[ScannedRom],
    paths: &Paths,
    report: &mut RefreshReport,
) -> Result<bool> {
    let (mut ids, was_present): (HashMap<String, i64>, HashSet<i64>) = {
        let mut stmt = tx.prepare_cached("SELECT path, id, present FROM game WHERE system = ?1")?;
        let rows: Vec<(String, i64, bool)> = stmt
            .query_map([&system.name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let present = rows.iter().filter(|r| r.2).map(|r| r.1).collect();
        (
            rows.into_iter().map(|(p, id, _)| (p, id)).collect(),
            present,
        )
    };
    let mut changed = false;

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
                    changed |= !was_present.contains(&id);
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
            changed |= was_present.contains(&id);
        }
    }
    report.games += roms.len();
    report.added += added.len();
    changed |= !added.is_empty();
    let regions = playlist_regions(tx, roms, &seen)?;
    changed |= !regions.is_empty();

    // The gamelist, only when it (or the set of games it can describe) changed.
    let file = system.rom_path.join("gamelist.xml");
    let affected = if gamelist_changed(tx, &file)? || !added.is_empty() {
        import_gamelist(tx, system, &file, &ids, paths, report)?;
        report.gamelists_imported += usize::from(file.exists());
        changed = true;
        ids.values().copied().collect()
    } else {
        report.gamelists_unchanged += usize::from(file.exists());
        added.clone()
    };
    let mut affected = affected;
    for id in regions {
        if !affected.contains(&id) {
            affected.push(id);
        }
    }
    materialise(tx, &affected, &added)?;
    Ok(changed)
}

/// Gives each playlist whose name has no region its discs' (see [`regions::of_playlist`]), as
/// Forma's own value: a gamelist's, an import's or the user's still wins. Returns the games
/// whose value changed.
fn playlist_regions(
    tx: &Transaction<'_>,
    roms: &[ScannedRom],
    ids: &HashMap<String, i64>,
) -> Result<Vec<i64>> {
    let mut old = tx.prepare_cached(
        "SELECT value FROM game_field WHERE game_id = ?1 AND field = ?2 AND source = ?3",
    )?;
    let mut put = tx.prepare_cached(
        "INSERT OR REPLACE INTO game_field (game_id, field, source, value) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut drop = tx.prepare_cached(
        "DELETE FROM game_field WHERE game_id = ?1 AND field = ?2 AND source = ?3",
    )?;
    let forma = Source::Forma as i64;
    let mut changed = Vec::new();
    for rom in roms.iter().filter(|r| !r.discs.is_empty()) {
        let Some(&id) = ids.get(rom.path.to_string_lossy().as_ref()) else {
            continue;
        };
        let stem = |p: &Path| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let discs: Vec<String> = rom.discs.iter().map(|d| stem(d)).collect();
        let discs: Vec<&str> = discs.iter().map(String::as_str).collect();
        let region = regions::of_playlist(&stem(&rom.path), &discs);
        let was: Option<String> = old
            .query_row(params![id, field::REGION, forma], |r| r.get(0))
            .optional()?;
        if was == region {
            continue;
        }
        match &region {
            Some(region) => put.execute(params![id, field::REGION, forma, region])?,
            None => drop.execute(params![id, field::REGION, forma])?,
        };
        changed.push(id);
    }
    Ok(changed)
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
    Ok(file_stamp(file) != previous)
}

/// A file's size and modification time (ns), to tell whether it changed since last read.
pub(super) fn file_stamp(file: &Path) -> Option<(i64, i64)> {
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

    let Some(size_mtime) = file_stamp(file) else {
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
