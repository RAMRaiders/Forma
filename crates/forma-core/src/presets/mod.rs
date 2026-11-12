//! Presets: ready-made setups for a kind of machine and screen. One is active at a time, or none.
//!
//! A preset holds Batocera's emulator options (for every system, and for some systems) and
//! Forma's CRT shader (on or off, the look it starts from, its settings, for every system and for
//! some). Its values sit under the user's own at each level, so what the user sets always wins:
//! a game's, then a system's own, then the preset's for that system, then the user's global,
//! then the preset's global. Nothing of it is written into `batocera.conf` for good: its options
//! are set for each launch only (where the user set nothing), and taken out after, so choosing
//! another preset, or none, leaves the user's settings as they were.
//!
//! Forma ships its presets; which is active is Forma's own file.

#[cfg(test)]
pub(crate) mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::options::conf;
use crate::shaders::Level;

/// Forma's presets, in the menu's order.
const BUILT_IN: &[&str] = &[include_str!("rpi5-monitor.json")];

/// A preset as Forma ships it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    /// What it does, a line each, for its page.
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub options: PresetOptions,
    #[serde(default)]
    pub(crate) shader: Option<PresetShader>,
}

/// Batocera's options, by their key after the level's prefix (`ratio`, `smooth`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetOptions {
    #[serde(default)]
    pub global: BTreeMap<String, String>,
    /// By system.
    #[serde(default)]
    pub systems: BTreeMap<String, BTreeMap<String, String>>,
}

impl PresetOptions {
    /// What it gives `key` for `system` (or for every system with `None`), and from which level.
    pub(crate) fn get(&self, system: Option<&str>, key: &str) -> Option<(&str, &'static str)> {
        system
            .and_then(|s| self.systems.get(s))
            .and_then(|o| o.get(key))
            .map(|v| (v.as_str(), "system"))
            .or_else(|| self.global.get(key).map(|v| (v.as_str(), "global")))
    }
}

/// Forma's CRT shader: switched on or not, and its levels, under the user's.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresetShader {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) global: Level,
    #[serde(default)]
    pub(crate) systems: BTreeMap<String, Level>,
}

/// A preset for the menu.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub notes: Vec<String>,
    pub active: bool,
    /// Every option it sets, as `global.smooth = 0` or `nds.shaderset = none`.
    pub options: Vec<String>,
    /// It switches Forma's CRT shader on.
    pub shader: bool,
}

/// Forma's file: the active preset.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Chosen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
}

/// Forma's presets and the one active, kept in `file`.
#[derive(Debug, Clone)]
pub struct Presets {
    file: PathBuf,
    presets: Vec<Preset>,
}

impl Presets {
    pub fn new(file: &Path) -> Self {
        Self::with(file, Self::built_in())
    }

    /// Other presets than Forma's (for tests).
    pub(crate) fn with(file: &Path, presets: Vec<Preset>) -> Self {
        Self {
            file: file.to_path_buf(),
            presets,
        }
    }

    /// Every preset Forma ships.
    pub fn built_in() -> Vec<Preset> {
        BUILT_IN
            .iter()
            .map(|json| serde_json::from_str(json).expect("Forma's presets are valid"))
            .collect()
    }

    fn chosen(&self) -> Option<String> {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str::<Chosen>(&t).ok())
            .and_then(|c| c.active)
    }

    /// The active preset, if any (one Forma no longer has counts as none).
    pub fn active(&self) -> Option<Preset> {
        let id = self.chosen()?;
        self.presets.iter().find(|p| p.id == id).cloned()
    }

    pub fn list(&self) -> Vec<PresetSummary> {
        let active = self.chosen();
        self.presets
            .iter()
            .cloned()
            .map(|p| {
                let mut options: Vec<String> = p
                    .options
                    .global
                    .iter()
                    .map(|(k, v)| format!("global.{k} = {v}"))
                    .collect();
                for (system, keys) in &p.options.systems {
                    options.extend(keys.iter().map(|(k, v)| format!("{system}.{k} = {v}")));
                }
                PresetSummary {
                    active: active.as_deref() == Some(p.id.as_str()),
                    shader: p.shader.as_ref().is_some_and(|s| s.enabled),
                    id: p.id,
                    name: p.name,
                    description: p.description,
                    notes: p.notes,
                    options,
                }
            })
            .collect()
    }

    /// Makes `id` the active preset, or none with `None`.
    pub fn set_active(&self, id: Option<&str>) -> Result<()> {
        if let Some(id) = id
            && !self.presets.iter().any(|p| p.id == id)
        {
            return Err(Error::Invalid(format!("no preset `{id}`")));
        }
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let chosen = Chosen {
            active: id.map(str::to_owned),
        };
        let text = serde_json::to_string_pretty(&chosen).expect("serialises");
        std::fs::write(&self.file, text).map_err(|e| Error::io(&self.file, e))
    }

    /// Before a launch: the active preset's options for `system`, each set for this launch only
    /// where the user set nothing that comes before it (the game's or the system's own value,
    /// and, for one the preset gives every system, the user's global too). After the game,
    /// [`conf::clear_for_launch`] takes them out with the rest of the launch's settings.
    pub fn before_launch(&self, conf_file: &Path, system: &str, file: &str) -> Result<()> {
        let Some(preset) = self.active() else {
            return Ok(());
        };
        let settings = conf::read(conf_file);
        let name: String = file.chars().filter(|c| *c != '=' && *c != '#').collect();
        let game = conf::game_prefix(system, &name);
        let set = |prefix: &str, key: &str| settings.contains_key(&format!("{prefix}.{key}"));
        let mut keys: BTreeMap<&str, &str> = BTreeMap::new();
        keys.extend(preset.options.global.keys().map(|k| (k.as_str(), "global")));
        if let Some(own) = preset.options.systems.get(system) {
            keys.extend(own.keys().map(|k| (k.as_str(), "system")));
        }
        for (key, level) in keys {
            if set(&game, key) || set(system, key) || (level == "global" && set("global", key)) {
                continue;
            }
            let (value, _) = preset
                .options
                .get(Some(system), key)
                .expect("listed from the preset");
            let prefix = if level == "system" { system } else { "global" };
            conf::set_for_launch(conf_file, &format!("{prefix}.{key}"), value)?;
        }
        Ok(())
    }
}
