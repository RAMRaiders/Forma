//! Applying import files (see [`crate::import`]) as the library's `import` layer.
//!
//! The layer is rebuilt whole whenever an import file changes, appears or goes, and whenever
//! the library gains games (a record may describe a file that only just arrived). Rebuilding
//! whole keeps deletion simple: remove a file and everything it said is gone, with the
//! gamelist's values showing through again.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::{Transaction, params};

use super::refresh::{RefreshReport, file_stamp};
use super::{Source, materialise};
use crate::error::{Result, Warning};
use crate::field;
use crate::gamelist::relative_to;
use crate::import;
use crate::paths::Paths;

pub(super) fn apply(
    tx: &Transaction<'_>,
    paths: &Paths,
    force: bool,
    report: &mut RefreshReport,
) -> Result<()> {
    let files = import::files(paths);
    let dir = paths.import_dir().to_string_lossy().into_owned();
    let current: BTreeSet<(String, i64, i64)> = files
        .iter()
        .filter_map(|f| {
            file_stamp(f).map(|(size, mtime)| (f.to_string_lossy().into_owned(), size, mtime))
        })
        .collect();
    let previous: BTreeSet<(String, i64, i64)> = {
        let mut stmt = tx.prepare_cached(
            "SELECT path, size, mtime_ns FROM import_file WHERE substr(path, 1, length(?1)) = ?1",
        )?;
        stmt.query_map([&dir], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    if !force && current == previous {
        return Ok(());
    }

    let mut affected: BTreeSet<i64> = {
        let mut stmt =
            tx.prepare_cached("SELECT DISTINCT game_id FROM game_field WHERE source = ?1")?;
        stmt.query_map([Source::Import as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    tx.execute(
        "DELETE FROM game_field WHERE source = ?1",
        [Source::Import as i64],
    )?;

    let roms: HashMap<String, PathBuf> = {
        let mut stmt = tx.prepare_cached("SELECT name, rom_path FROM system")?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                PathBuf::from(r.get::<_, String>(1)?),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let ids: HashMap<(String, String), i64> = {
        let mut stmt = tx.prepare_cached("SELECT system, path, id FROM game")?;
        stmt.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };

    let mut insert = tx.prepare_cached(
        "INSERT OR REPLACE INTO game_field (game_id, field, source, value) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for file in &files {
        let text = match std::fs::read(file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                report
                    .warnings
                    .push(Warning::new(file, format!("cannot read: {e}")));
                continue;
            }
        };
        let parsed = import::parse(&text, file);
        report.warnings.extend(parsed.warnings);
        report.imports_applied += 1;

        for record in parsed.records {
            let Some(rom_dir) = roms.get(&record.system) else {
                report.warnings.push(Warning::new(
                    file,
                    format!(
                        "system `{}` is not defined; `{}` skipped",
                        record.system, record.path
                    ),
                ));
                continue;
            };
            let Some(rel) = library_path(rom_dir, &record.path, paths) else {
                report.warnings.push(Warning::new(
                    file,
                    format!(
                        "`{}` is outside the {} folder; skipped",
                        record.path, record.system
                    ),
                ));
                continue;
            };
            let Some(&id) = ids.get(&(record.system.clone(), rel.clone())) else {
                report.warnings.push(Warning::new(
                    file,
                    format!(
                        "{}/{rel} is not in the library yet; it applies once the file appears",
                        record.system
                    ),
                ));
                continue;
            };
            for (key, value) in record.fields {
                let value = if key.starts_with(field::MEDIA_PREFIX) {
                    media_path(rom_dir, &value, paths)
                } else {
                    value
                };
                insert.execute(params![id, key, Source::Import as i64, value])?;
            }
            affected.insert(id);
            report.import_records += 1;
        }
    }

    tx.execute(
        "DELETE FROM import_file WHERE substr(path, 1, length(?1)) = ?1",
        [&dir],
    )?;
    for (path, size, mtime) in &current {
        tx.execute(
            "INSERT INTO import_file (path, size, mtime_ns) VALUES (?1, ?2, ?3)",
            params![path, size, mtime],
        )?;
    }

    let affected: Vec<i64> = affected.into_iter().collect();
    materialise(tx, &affected, &[])?;
    let mut systems = tx.prepare_cached("SELECT system FROM game WHERE id = ?1")?;
    for id in &affected {
        let system: String = systems.query_row([id], |r| r.get(0))?;
        if !report.changed_systems.contains(&system) {
            report.changed_systems.push(system);
        }
    }
    Ok(())
}

/// A record's path as the library keys it: relative to the system's ROM folder.
fn library_path(rom_dir: &Path, path: &str, paths: &Paths) -> Option<String> {
    let p = Path::new(path);
    let full = if p.is_absolute() {
        paths.rebase(p)
    } else {
        rom_dir.join(p)
    };
    relative_to(rom_dir, &full).map(|rel| rel.to_string_lossy().into_owned())
}

/// Media as the library stores it: an absolute path, rebased for development, with `./`
/// segments folded away.
fn media_path(rom_dir: &Path, path: &str, paths: &Paths) -> String {
    let p = Path::new(path);
    let full = if p.is_absolute() {
        paths.rebase(p)
    } else {
        relative_to(rom_dir, &rom_dir.join(p))
            .map_or_else(|| rom_dir.join(p), |rel| rom_dir.join(rel))
    };
    full.to_string_lossy().into_owned()
}
