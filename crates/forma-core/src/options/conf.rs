//! `batocera.conf` as the options see it: keys at three levels, `global.ratio`, `snes.ratio` and
//! `snes["Super Mario World (USA).sfc"].ratio`, the last with a file name that may hold `=`.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};

/// The key and value of a setting line, or `None` for comments and anything else.
fn split(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // A game's file name sits between `["` and `"]` and can hold anything, `=` included.
    let from = line.find("\"]").map_or(0, |i| i + 2);
    let eq = from + line[from..].find('=')?;
    Some((line[..eq].trim(), line[eq + 1..].trim()))
}

/// Every set key; an empty value counts as unset, as Batocera's own tools read it.
pub(crate) fn read(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(split)
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Unsets `key` as ES does: its lines are commented out, so `batocera-settings-set` puts a
/// later value back in the same place. Written to a temporary file first, then renamed over.
pub(crate) fn clear(path: &Path, key: &str) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(path, e)),
    };
    if !text
        .lines()
        .any(|l| split(l).is_some_and(|(k, _)| k == key))
    {
        return Ok(());
    }
    let mut out = String::with_capacity(text.len() + 8);
    for line in text.lines() {
        if split(line).is_some_and(|(k, _)| k == key) {
            out.push('#');
            out.push_str(line.trim_start());
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    replace(path, &out)
}

/// Marks the line after it as Forma's own, set for one launch only.
const FOR_LAUNCH: &str = "# forma: only while this game runs";

/// Sets `key` for one launch, marked so that [`clear_for_launch`] can tell it from the user's.
pub(crate) fn set_for_launch(path: &Path, key: &str, value: &str) -> Result<()> {
    let mut text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{FOR_LAUNCH}\n{key}={value}\n"));
    replace(path, &text)
}

/// Takes out what [`set_for_launch`] put in, this launch's or one left by a crash.
pub(crate) fn clear_for_launch(path: &Path) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(path, e)),
    };
    if !text.lines().any(|l| l == FOR_LAUNCH) {
        return Ok(());
    }
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line != FOR_LAUNCH {
            out.push_str(line);
            out.push('\n');
        } else if lines.peek().is_some_and(|next| split(next).is_some()) {
            lines.next();
        }
    }
    replace(path, &out)
}

/// Written to a temporary file first, then renamed over: never a half-written config.
fn replace(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("conf.forma-tmp");
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| Error::io(path, e))
}

/// The key for a game: its system and file name, as ES writes it.
pub(crate) fn game_prefix(system: &str, file_name: &str) -> String {
    format!("{system}[\"{file_name}\"]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_game_keys_whose_file_name_holds_an_equals_sign() {
        assert_eq!(
            split(r#"snes["A = B (USA).sfc"].ratio=16/9"#),
            Some((r#"snes["A = B (USA).sfc"].ratio"#, "16/9"))
        );
        assert_eq!(split("global.ratio = 4/3"), Some(("global.ratio", "4/3")));
        assert_eq!(split("#global.ratio=4/3"), None);
    }

    #[test]
    fn clearing_comments_the_key_out_and_leaves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("batocera.conf");
        std::fs::write(
            &conf,
            "# comment\nsnes.ratio=4/3\nsnes.ratio_extra=1\nnes.ratio=4/3\n",
        )
        .unwrap();
        clear(&conf, "snes.ratio").unwrap();
        assert_eq!(
            std::fs::read_to_string(&conf).unwrap(),
            "# comment\n#snes.ratio=4/3\nsnes.ratio_extra=1\nnes.ratio=4/3\n"
        );
        assert!(!read(&conf).contains_key("snes.ratio"));
    }
}
