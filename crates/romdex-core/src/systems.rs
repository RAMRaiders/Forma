//! System definitions from `es_systems.cfg` and the user's `es_systems_*.cfg` overlays.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result, Warning};
use crate::paths::Paths;
use crate::xml::{self, Element};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct System {
    /// Short id, e.g. `nes`. Unique; also the folder name under `roms/`.
    pub name: String,
    pub fullname: String,
    pub manufacturer: Option<String>,
    pub release_year: Option<i64>,
    /// `console`, `portable`, `computer`, `arcade`…
    pub hardware: Option<String>,
    /// Already rebased onto the configured `/userdata`.
    pub rom_path: PathBuf,
    /// Lower-case, with the leading dot, de-duplicated: matching is case-insensitive.
    pub extensions: Vec<String>,
    pub command: Option<String>,
    pub platforms: Vec<String>,
    pub theme: Option<String>,
    pub emulators: Vec<Emulator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Emulator {
    pub name: String,
    pub cores: Vec<Core>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Core {
    pub name: String,
    pub default: bool,
}

impl System {
    /// Whether a file or folder name has one of this system's extensions.
    pub fn accepts(&self, file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        self.extensions
            .iter()
            .any(|ext| lower.ends_with(ext.as_str()))
    }
}

#[derive(Debug, Default)]
pub struct SystemsLoad {
    pub systems: Vec<System>,
    pub warnings: Vec<Warning>,
}

/// Loads the base definitions, then applies every `es_systems_*.cfg` overlay in name order.
///
/// An overlay system with an existing name **replaces** that system whole, keeping its position;
/// a new name is appended. A missing or unreadable base file is an error, since there would be
/// nothing to show; a broken overlay is only a warning.
pub fn load_systems(paths: &Paths) -> Result<SystemsLoad> {
    let mut load = SystemsLoad::default();

    let base = read(&paths.systems_cfg)?;
    load.systems = parse_systems(&base, &paths.systems_cfg, paths, &mut load.warnings);
    if load.systems.is_empty() {
        return Err(Error::Parse {
            path: paths.systems_cfg.clone(),
            message: "no systems defined".to_owned(),
        });
    }

    for overlay in overlay_files(&paths.systems_overlay_dir()) {
        let xml = match read(&overlay) {
            Ok(xml) => xml,
            Err(e) => {
                load.warnings.push(Warning::new(&overlay, e.to_string()));
                continue;
            }
        };
        for system in parse_systems(&xml, &overlay, paths, &mut load.warnings) {
            match load.systems.iter_mut().find(|s| s.name == system.name) {
                Some(existing) => *existing = system,
                None => load.systems.push(system),
            }
        }
    }
    Ok(load)
}

fn read(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn overlay_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("es_systems_") && n.ends_with(".cfg"))
        })
        .collect();
    files.sort();
    files
}

fn parse_systems(
    xml: &str,
    file: &Path,
    paths: &Paths,
    warnings: &mut Vec<Warning>,
) -> Vec<System> {
    let parsed = xml::parse(xml);
    if let Some(error) = parsed.error {
        warnings.push(Warning::new(file, error));
    }
    let Some(root) = parsed.root else {
        return Vec::new();
    };
    root.children_named("system")
        .filter_map(|el| match system(el, paths) {
            Ok(s) => Some(s),
            Err(message) => {
                warnings.push(Warning::new(file, message));
                None
            }
        })
        .collect()
}

fn system(el: &Element, paths: &Paths) -> std::result::Result<System, String> {
    let name = el
        .child_text("name")
        .ok_or("a <system> without a <name> was skipped")?
        .to_owned();
    let rom_path = el
        .child_text("path")
        .ok_or_else(|| format!("system `{name}` has no <path> and was skipped"))?;

    let mut extensions: Vec<String> = Vec::new();
    for ext in el
        .child_text("extension")
        .unwrap_or_default()
        .split_whitespace()
    {
        let ext = ext.to_lowercase();
        let ext = if ext.starts_with('.') {
            ext
        } else {
            format!(".{ext}")
        };
        if !extensions.contains(&ext) {
            extensions.push(ext);
        }
    }

    let emulators = el
        .child("emulators")
        .map(|e| {
            e.children_named("emulator")
                .filter_map(|em| {
                    let cores = em
                        .child("cores")
                        .map(|c| {
                            c.children_named("core")
                                .filter(|core| !core.text.trim().is_empty())
                                .map(|core| Core {
                                    name: core.text.trim().to_owned(),
                                    default: core.attr("default") == Some("true"),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Emulator {
                        name: em.attr("name")?.to_owned(),
                        cores,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(System {
        fullname: el.child_text("fullname").unwrap_or(&name).to_owned(),
        manufacturer: el.child_text("manufacturer").map(str::to_owned),
        release_year: el
            .child_text("release")
            .and_then(|r| r.get(..4)?.parse().ok()),
        hardware: el.child_text("hardware").map(str::to_owned),
        rom_path: paths.rebase(Path::new(rom_path)),
        extensions,
        command: el.child_text("command").map(str::to_owned),
        platforms: el
            .child_text("platform")
            .unwrap_or_default()
            .split([',', ' '])
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect(),
        theme: el.child_text("theme").map(str::to_owned),
        emulators,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_paths;

    #[test]
    fn loads_the_fixture_with_its_overlay_applied() {
        let paths = fixture_paths();
        let load = load_systems(&paths).unwrap();
        let names: Vec<&str> = load.systems.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["nes", "snes", "megadrive", "psx", "homebrew"]);
        assert!(load.warnings.is_empty(), "{:?}", load.warnings);

        let nes = &load.systems[0];
        assert_eq!(nes.fullname, "Nintendo Entertainment System");
        assert_eq!(nes.manufacturer.as_deref(), Some("Nintendo"));
        assert_eq!(nes.release_year, Some(1985));
        assert_eq!(nes.rom_path, paths.userdata.join("roms/nes"));
        // `.nes .NES` collapse to one lower-case entry.
        assert_eq!(nes.extensions, [".nes", ".unif", ".zip", ".7z"]);
        assert_eq!(nes.emulators[0].name, "libretro");
        assert_eq!(nes.emulators[0].cores.len(), 3);
        assert!(nes.emulators[0].cores[0].default);
    }

    #[test]
    fn an_overlay_replaces_a_system_by_name_in_place() {
        let load = load_systems(&fixture_paths()).unwrap();
        let snes = &load.systems[1];
        assert_eq!(snes.fullname, "Super Famicom");
        assert_eq!(snes.emulators[0].cores[0].name, "bsnes");
    }

    #[test]
    fn a_system_without_optional_fields_still_loads() {
        let load = load_systems(&fixture_paths()).unwrap();
        let homebrew = load.systems.iter().find(|s| s.name == "homebrew").unwrap();
        assert_eq!(homebrew.manufacturer, None);
        assert!(homebrew.emulators.is_empty());
    }

    #[test]
    fn splits_a_comma_separated_platform_list() {
        let load = load_systems(&fixture_paths()).unwrap();
        let md = load.systems.iter().find(|s| s.name == "megadrive").unwrap();
        assert_eq!(md.platforms, ["megadrive", "genesis"]);
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        let load = load_systems(&fixture_paths()).unwrap();
        let nes = &load.systems[0];
        assert!(nes.accepts("Game.NES"));
        assert!(!nes.accepts("readme.txt"));
    }

    #[test]
    fn skips_a_system_without_a_name_or_path_and_says_so() {
        let mut warnings = Vec::new();
        let xml = "<systemList><system><path>/x</path></system>\
                   <system><name>nopath</name></system>\
                   <system><name>ok</name><path>/userdata/roms/ok</path></system></systemList>";
        let systems = parse_systems(xml, Path::new("t.cfg"), &fixture_paths(), &mut warnings);
        assert_eq!(systems.len(), 1);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn a_missing_base_file_is_an_error() {
        let mut paths = fixture_paths();
        paths.systems_cfg = paths.userdata.join("does-not-exist.cfg");
        assert!(matches!(load_systems(&paths), Err(Error::Io { .. })));
    }
}
