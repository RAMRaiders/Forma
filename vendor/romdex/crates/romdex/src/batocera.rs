//! What to scan on a Batocera box: each system's ROM folder and file extensions, from
//! `es_systems.cfg` and the user's `es_systems_*.cfg` overlays.
//!
//! Forma has its own, fuller reader and passes [`Target`]s straight in; this one serves the
//! `romdex` command, so it reads only what scanning needs and never stops on a bad file.

use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;

/// One system to scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub system: String,
    pub root: PathBuf,
    /// Lower-case, with the leading dot.
    pub extensions: Vec<String>,
}

impl Target {
    pub fn accepts(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.extensions.iter().any(|e| lower.ends_with(e.as_str()))
    }
}

/// Every system in `systems_cfg`, then the overlays in `<userdata>/system/configs/
/// emulationstation/`, an overlay replacing a system of the same name. Paths written as
/// `/userdata/...` are moved under `userdata`, so a copied tree works anywhere.
pub fn targets(userdata: &Path, systems_cfg: &Path) -> (Vec<Target>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut targets = read(systems_cfg, userdata, &mut warnings);

    let overlay_dir = userdata.join("system/configs/emulationstation");
    let mut overlays: Vec<PathBuf> = std::fs::read_dir(&overlay_dir)
        .map(|e| e.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    overlays.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("es_systems_") && n.ends_with(".cfg"))
    });
    overlays.sort();
    for overlay in overlays {
        for target in read(&overlay, userdata, &mut warnings) {
            match targets.iter_mut().find(|t| t.system == target.system) {
                Some(existing) => *existing = target,
                None => targets.push(target),
            }
        }
    }
    (targets, warnings)
}

fn read(file: &Path, userdata: &Path, warnings: &mut Vec<String>) -> Vec<Target> {
    let text = match std::fs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            warnings.push(format!("{}: {e}", file.display()));
            return Vec::new();
        }
    };
    let mut reader = Reader::from_str(&text);
    let mut out = Vec::new();
    let mut tag = String::new();
    let (mut name, mut path, mut exts) = (String::new(), String::new(), String::new());
    let mut in_system = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                tag = String::from_utf8_lossy(e.name().as_ref().as_bytes()).into_owned();
                if tag == "system" {
                    in_system = true;
                    (name, path, exts) = (String::new(), String::new(), String::new());
                }
            }
            Ok(Event::Text(t)) if in_system => {
                let text = t.xml10_content();
                match tag.as_str() {
                    "name" => name.push_str(&text),
                    "path" => path.push_str(&text),
                    "extension" => exts.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == "system" {
                    in_system = false;
                    let (name, path) = (name.trim(), path.trim());
                    if name.is_empty() || path.is_empty() {
                        warnings.push(format!("{}: a system without name or path", file.display()));
                    } else {
                        out.push(Target {
                            system: name.to_owned(),
                            root: rebase(Path::new(path), userdata),
                            extensions: extensions(&exts),
                        });
                    }
                }
                tag.clear();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                warnings.push(format!(
                    "{}: {e}; systems before it were kept",
                    file.display()
                ));
                break;
            }
        }
    }
    out
}

fn extensions(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ext in raw.split_whitespace().map(str::to_lowercase) {
        let ext = if ext.starts_with('.') {
            ext
        } else {
            format!(".{ext}")
        };
        if !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
}

fn rebase(path: &Path, userdata: &Path) -> PathBuf {
    path.strip_prefix("/userdata")
        .map_or_else(|_| path.to_path_buf(), |rest| userdata.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_systems_and_applies_overlays_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let userdata = dir.path().join("userdata");
        let cfg = dir.path().join("es_systems.cfg");
        write(
            &cfg,
            "<systemList>\
               <system><name>nes</name><path>/userdata/roms/nes</path>\
                 <extension>.nes .NES .zip</extension></system>\
               <system><name>snes</name><path>/userdata/roms/snes</path>\
                 <extension>.sfc</extension></system>\
               <system><name>broken</name></system>\
             </systemList>",
        );
        write(
            &userdata.join("system/configs/emulationstation/es_systems_mine.cfg"),
            "<systemList><system><name>snes</name><path>/media/usb/snes</path>\
             <extension>.smc .sfc</extension></system></systemList>",
        );
        let (targets, warnings) = targets(&userdata, &cfg);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].root, userdata.join("roms/nes"));
        assert_eq!(targets[0].extensions, [".nes", ".zip"]);
        assert!(targets[0].accepts("Game.NES"));
        assert_eq!(
            targets[1].root,
            Path::new("/media/usb/snes"),
            "overlay replaced snes"
        );
    }

    #[test]
    fn a_truncated_file_keeps_the_systems_before_the_break() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("es_systems.cfg");
        write(
            &cfg,
            "<systemList><system><name>nes</name><path>/r/nes</path><extension>.nes</extension>\
             </system><system><name>sn",
        );
        let (targets, _) = targets(dir.path(), &cfg);
        assert_eq!(targets.len(), 1);
    }
}
