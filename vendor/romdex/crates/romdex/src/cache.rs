//! Hashes already computed, keyed by path, size and modification time: a re-scan reads only
//! what changed. Safe to delete; it is rebuilt by hashing again.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};
use crate::hash::{Hashes, Headerless};

/// What identifies a file's contents without reading them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub mtime_ns: i64,
}

impl Stamp {
    pub fn of(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
        Ok(Self {
            size: meta.len(),
            mtime_ns,
        })
    }
}

pub struct Cache {
    conn: Connection,
}

/// What was learnt about a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Of the ROM: the file itself, or the one file inside its archive.
    pub hashes: Hashes,
    /// The file the hashes are of, when not the file itself: the one inside a single-file
    /// archive, or a CUE sheet's first track.
    pub inner: Option<String>,
    /// The ROM's size: uncompressed, for an archive.
    pub size: u64,
    /// The RetroAchievements hash, and the console it was computed for: a hash for another
    /// console is not this file's, so a changed system mapping recomputes it.
    pub ra: Option<String>,
    pub ra_console: Option<u32>,
}

/// Bump when what is cached changes meaning. A cache is only a shortcut, so an old one is
/// dropped and rebuilt rather than migrated.
const VERSION: i64 = 4;

const SCHEMA: &str = r"
DROP TABLE IF EXISTS hash;
CREATE TABLE hash (
    path      TEXT PRIMARY KEY,
    size      INTEGER NOT NULL,
    mtime_ns  INTEGER NOT NULL,
    crc32     TEXT NOT NULL,
    md5       TEXT NOT NULL,
    sha1      TEXT NOT NULL,
    inner     TEXT,
    rom_size  INTEGER NOT NULL,
    ra        TEXT,
    ra_console INTEGER,
    -- Past a known header: its length and the hashes of what follows it.
    bare_header INTEGER,
    bare_crc32  TEXT,
    bare_md5    TEXT,
    bare_sha1   TEXT
) WITHOUT ROWID;
";

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != VERSION {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", VERSION)?;
        }
        Ok(Self { conn })
    }

    /// What is cached for the file, if it has not changed since.
    pub fn get(&self, path: &Path, stamp: Stamp) -> Result<Option<Entry>> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT crc32, md5, sha1, inner, rom_size, ra, ra_console, bare_header, bare_crc32,
                        bare_md5, bare_sha1 FROM hash
                 WHERE path = ?1 AND size = ?2 AND mtime_ns = ?3",
            )?
            .query_row(params![key(path), size(stamp.size), stamp.mtime_ns], |r| {
                Ok(Entry {
                    hashes: Hashes {
                        crc32: r.get(0)?,
                        md5: r.get(1)?,
                        sha1: r.get(2)?,
                        headerless: match (
                            r.get::<_, Option<i64>>(7)?,
                            r.get::<_, Option<String>>(8)?,
                            r.get::<_, Option<String>>(9)?,
                            r.get::<_, Option<String>>(10)?,
                        ) {
                            (Some(header), Some(crc32), Some(md5), Some(sha1)) => {
                                Some(Headerless {
                                    header: u64::try_from(header).unwrap_or(0),
                                    crc32,
                                    md5,
                                    sha1,
                                })
                            }
                            _ => None,
                        },
                    },
                    inner: r.get(3)?,
                    size: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                    ra: r.get(5)?,
                    ra_console: r.get(6)?,
                })
            })
            .optional()?)
    }

    pub fn put(&self, path: &Path, stamp: Stamp, entry: &Entry) -> Result<()> {
        let bare = entry.hashes.headerless.as_ref();
        self.conn
            .prepare_cached(
                "INSERT INTO hash (path, size, mtime_ns, crc32, md5, sha1, inner, rom_size, ra,
                                   ra_console, bare_header, bare_crc32, bare_md5, bare_sha1)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT (path) DO UPDATE SET size = excluded.size,
                     mtime_ns = excluded.mtime_ns, crc32 = excluded.crc32, md5 = excluded.md5,
                     sha1 = excluded.sha1, inner = excluded.inner, rom_size = excluded.rom_size,
                     ra = excluded.ra, ra_console = excluded.ra_console,
                     bare_header = excluded.bare_header, bare_crc32 = excluded.bare_crc32,
                     bare_md5 = excluded.bare_md5, bare_sha1 = excluded.bare_sha1",
            )?
            .execute(params![
                key(path),
                size(stamp.size),
                stamp.mtime_ns,
                entry.hashes.crc32,
                entry.hashes.md5,
                entry.hashes.sha1,
                entry.inner,
                size(entry.size),
                entry.ra,
                entry.ra_console,
                bare.map(|b| size(b.header)),
                bare.map(|b| &b.crc32),
                bare.map(|b| &b.md5),
                bare.map(|b| &b.sha1),
            ])?;
        Ok(())
    }

    /// Groups writes into one transaction: one fsync for a whole scan, not one per file.
    pub fn batch<T>(&mut self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("BEGIN")?;
        match f(self) {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn size(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(tag: &str) -> Entry {
        Entry {
            hashes: Hashes {
                crc32: tag.into(),
                md5: tag.into(),
                sha1: tag.into(),
                headerless: Some(Headerless {
                    header: 16,
                    crc32: format!("{tag}-bare"),
                    md5: format!("{tag}-bare"),
                    sha1: format!("{tag}-bare"),
                }),
            },
            inner: Some(format!("{tag}.nes")),
            size: 7,
            ra: Some(format!("{tag}-ra")),
            ra_console: Some(7),
        }
    }

    #[test]
    fn an_old_cache_is_dropped_not_misread() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache.db");
        {
            let conn = Connection::open(&file).unwrap();
            conn.execute_batch("CREATE TABLE hash (path TEXT PRIMARY KEY, crc32 TEXT);")
                .unwrap();
            conn.execute("INSERT INTO hash VALUES ('/a', 'old')", [])
                .unwrap();
        }
        let cache = Cache::open(&file).unwrap();
        let stamp = Stamp {
            size: 1,
            mtime_ns: 1,
        };
        assert_eq!(cache.get(Path::new("/a"), stamp).unwrap(), None);
        cache.put(Path::new("/a"), stamp, &hashes("new")).unwrap();
        assert_eq!(
            cache.get(Path::new("/a"), stamp).unwrap(),
            Some(hashes("new"))
        );
    }

    #[test]
    fn returns_hashes_only_while_the_file_is_unchanged() {
        let cache = Cache::in_memory().unwrap();
        let path = Path::new("/roms/nes/a.nes");
        let stamp = Stamp {
            size: 40976,
            mtime_ns: 7,
        };
        cache.put(path, stamp, &hashes("one")).unwrap();
        assert_eq!(cache.get(path, stamp).unwrap(), Some(hashes("one")));
        assert_eq!(
            cache
                .get(
                    path,
                    Stamp {
                        mtime_ns: 8,
                        ..stamp
                    }
                )
                .unwrap(),
            None
        );
        assert_eq!(cache.get(path, Stamp { size: 1, ..stamp }).unwrap(), None);

        cache
            .put(
                path,
                Stamp {
                    mtime_ns: 8,
                    ..stamp
                },
                &hashes("two"),
            )
            .unwrap();
        assert_eq!(
            cache
                .get(
                    path,
                    Stamp {
                        mtime_ns: 8,
                        ..stamp
                    }
                )
                .unwrap(),
            Some(hashes("two"))
        );
    }

    #[test]
    fn persists_on_disk_and_a_failed_batch_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deep/cache.db");
        let stamp = Stamp {
            size: 1,
            mtime_ns: 1,
        };
        {
            let mut cache = Cache::open(&file).unwrap();
            cache
                .batch(|c| c.put(Path::new("/a"), stamp, &hashes("a")))
                .unwrap();
            let failed: Result<()> = cache.batch(|c| {
                c.put(Path::new("/b"), stamp, &hashes("b"))?;
                Err(Error::io(Path::new("/b"), std::io::Error::other("boom")))
            });
            assert!(failed.is_err());
        }
        let cache = Cache::open(&file).unwrap();
        assert!(cache.get(Path::new("/a"), stamp).unwrap().is_some());
        assert!(cache.get(Path::new("/b"), stamp).unwrap().is_none());
    }
}
