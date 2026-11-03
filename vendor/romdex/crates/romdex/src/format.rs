//! The import format, as romdex writes it. Specified in `docs/import-format.md`.

use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::identify::Identified;

/// The `format` value of a header line.
pub const FORMAT: &str = "romdex-import";
/// The version this romdex writes.
pub const FORMAT_VERSION: u32 = 1;

/// The header line, without its newline.
pub fn header(source: &str, generated_at: &str) -> String {
    json!({
        "format": FORMAT,
        "formatVersion": FORMAT_VERSION,
        "source": source,
        "generatedAt": generated_at,
    })
    .to_string()
}

/// One record line for an identified ROM: its key and hashes. Names and metadata come with
/// DAT matching.
pub fn record(item: &Identified) -> String {
    let mut hashes = Map::new();
    hashes.insert("crc32".into(), item.hashes.crc32.clone().into());
    hashes.insert("md5".into(), item.hashes.md5.clone().into());
    hashes.insert("sha1".into(), item.hashes.sha1.clone().into());
    if let Some(ra) = &item.ra {
        hashes.insert("ra".into(), ra.clone().into());
    }
    json!({ "system": item.system, "path": item.path, "hashes": Value::Object(hashes) }).to_string()
}

/// A whole file: the header, then one record per item.
pub fn write(mut out: impl Write, generated_at: &str, items: &[Identified]) -> std::io::Result<()> {
    writeln!(out, "{}", header("romdex", generated_at))?;
    for item in items {
        writeln!(out, "{}", record(item))?;
    }
    out.flush()
}

/// Writes `path` whole or not at all: a temporary file beside it, renamed into place, so a
/// reader never sees half a file.
pub fn write_file(path: &Path, generated_at: &str, items: &[Identified]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(|| "import".into(), |n| n.to_string_lossy().into_owned());
    // Not `.jsonl`, so a reader watching the folder ignores it until the rename.
    let temp = dir.join(format!(".{name}.tmp"));
    let file = std::fs::File::create(&temp)?;
    let mut buffered = std::io::BufWriter::new(file);
    write(&mut buffered, generated_at, items)?;
    buffered
        .into_inner()
        .map_err(|e| e.into_error())?
        .sync_all()?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hashes;

    fn item(ra: Option<&str>) -> Identified {
        Identified {
            system: "nes".into(),
            path: "Zelda (USA).zip".into(),
            inner: Some("Zelda (USA).nes".into()),
            size: 131_088,
            hashes: Hashes {
                crc32: "3fe272fb".into(),
                md5: "337bd6f1a1163df31bf2633665589ab0".into(),
                sha1: "dab79c84934f9aa5db4e7dad390e5d0c12443fa2".into(),
            },
            ra: ra.map(str::to_owned),
        }
    }

    #[test]
    fn the_header_is_one_json_line_naming_the_format_and_version() {
        let line = header("romdex", "2026-10-31T12:00:00Z");
        assert!(!line.contains('\n'));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["format"], "romdex-import");
        assert_eq!(v["formatVersion"], 1);
        assert_eq!(v["source"], "romdex");
    }

    #[test]
    fn the_sample_file_declares_this_format() {
        let sample = include_str!("../../../fixtures/import/retroworker.jsonl");
        let first: Value = serde_json::from_str(sample.lines().next().unwrap()).unwrap();
        assert_eq!(first["format"], FORMAT);
        assert_eq!(first["formatVersion"], FORMAT_VERSION);
    }

    #[test]
    fn records_carry_the_key_and_hashes_and_the_ra_hash_only_when_known() {
        let with: Value = serde_json::from_str(&record(&item(Some("abc")))).unwrap();
        assert_eq!(with["system"], "nes");
        assert_eq!(with["path"], "Zelda (USA).zip");
        assert_eq!(with["hashes"]["crc32"], "3fe272fb");
        assert_eq!(with["hashes"]["ra"], "abc");
        let without: Value = serde_json::from_str(&record(&item(None))).unwrap();
        assert!(without["hashes"].get("ra").is_none());
    }

    #[test]
    fn writes_a_whole_file_and_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("import/romdex.jsonl");
        write_file(
            &path,
            "2026-10-31T12:00:00Z",
            &[item(None), item(Some("abc"))],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "only the finished file");
    }
}
