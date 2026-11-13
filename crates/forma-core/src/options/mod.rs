//! Emulator, core and game options, as Batocera defines them and EmulationStation offers them.
//!
//! `es_features.cfg` lists every option per emulator and core; `es_systems.cfg` lists each
//! system's emulators and cores; `batocera.conf` holds the choices at three levels, global,
//! system and game, a game's value overriding its system's, which overrides the global one.
//! configgen applies them when `emulatorlauncher` starts a game, so a value set here is used by
//! the next launch, and ES shows it too.
//!
//! Only options that apply to the level (and to the chosen emulator and core) can be set, each
//! checked against its choices or range. Values go through `batocera-settings-set`; unsetting
//! comments the key out, as ES does.

pub(crate) mod conf;
mod features;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host::Shell;
use crate::systems::System;
use features::{Feature, Features, Preset};

const SHADERS: &str = "/usr/share/batocera/shaders/configs";
const DECORATIONS: &str = "/usr/share/batocera/datainit/decorations";

/// Where an option is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every system and game, unless they say otherwise.
    Global,
    System(String),
    /// One game, by its system and file name (what ES keys it by).
    Game {
        system: String,
        file: String,
    },
}

impl Scope {
    pub(crate) fn system(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::System(s) | Self::Game { system: s, .. } => Some(s),
        }
    }

    /// This level's key prefix, then each level it inherits from.
    fn prefixes(&self) -> Vec<(String, &'static str)> {
        match self {
            Self::Global => vec![("global".into(), "global")],
            Self::System(s) => vec![(s.clone(), "system"), ("global".into(), "global")],
            Self::Game { system, file } => vec![
                (conf::game_prefix(system, file), "game"),
                (system.clone(), "system"),
                ("global".into(), "global"),
            ],
        }
    }
}

/// The options of one level, ready to draw.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionsPage {
    pub options: Vec<OptionItem>,
    /// What the chosen emulator supports: `netplay`, `cheevos`, `padtokeyboard`.
    pub flags: Vec<String>,
    /// Every emulator and core the system can run, as one list (a system's and a game's page;
    /// empty globally): one choice sets both `emulator` and `core`.
    pub cores: Vec<CoreChoice>,
}

/// One emulator and one of its cores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreChoice {
    pub emulator: String,
    pub core: String,
    /// `pcsx_rearmed` for a RetroArch core, `duckstation (standalone)`, `mupen64plus · rice`.
    pub label: String,
    /// What the system runs when nothing is set.
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionItem {
    /// `emulator`, `core`, or the option's `batocera.conf` key after the level prefix.
    pub key: String,
    pub label: String,
    /// Where ES files it: a section of the page (`group`) or a page of its own (`submenu`).
    pub group: Option<String>,
    pub submenu: Option<String>,
    pub description: Option<String>,
    /// `choice`, `number` (`min`…`max` by `step`) or `text`.
    pub kind: &'static str,
    pub choices: Vec<OptionChoice>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// What applies when no level sets it, when Batocera says.
    pub default: Option<String>,
    /// Set at this level.
    pub value: Option<String>,
    /// Set at a level this one inherits from, when this one does not.
    pub inherited: Option<Inherited>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionChoice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inherited {
    pub value: String,
    /// `global` or `system`.
    pub from: &'static str,
}

pub struct Options {
    shell: Box<dyn Shell>,
    conf: PathBuf,
    features_file: PathBuf,
    shaders: Vec<PathBuf>,
    decorations: Vec<PathBuf>,
    features: OnceLock<Features>,
}

impl Options {
    /// `features` is `es_features.cfg`, next to `es_systems.cfg`; `userdata` holds the user's
    /// own shaders and decorations beside Batocera's.
    pub fn new(shell: Box<dyn Shell>, conf: &Path, features: &Path, userdata: &Path) -> Self {
        Self {
            shell,
            conf: conf.to_path_buf(),
            features_file: features.to_path_buf(),
            shaders: vec![SHADERS.into(), userdata.join("shaders/configs")],
            decorations: vec![DECORATIONS.into(), userdata.join("decorations")],
            features: OnceLock::new(),
        }
    }

    /// For tests and the mock: where shader sets and decorations are.
    pub fn with_preset_dirs(mut self, shaders: Vec<PathBuf>, decorations: Vec<PathBuf>) -> Self {
        self.shaders = shaders;
        self.decorations = decorations;
        self
    }

    /// Batocera's file ships in the image and never changes while Forma runs: read once.
    fn features(&self) -> &Features {
        self.features.get_or_init(|| {
            let xml = std::fs::read_to_string(&self.features_file).unwrap_or_default();
            let features = Features::parse(&xml);
            if let Some(e) = &features.error {
                eprintln!("forma: {}: {e}", self.features_file.display());
            }
            features
        })
    }

    /// The options at `scope`. `system` is the scope's system (none for global).
    pub fn page(&self, scope: &Scope, system: Option<&System>) -> Result<OptionsPage> {
        let conf = conf::read(&self.conf);
        let prefixes = scope.prefixes();
        let lookup = |key: &str| {
            let mut levels = prefixes
                .iter()
                .map(|(p, from)| (conf.get(&format!("{p}.{key}")), *from));
            let value = levels.next().and_then(|(v, _)| v.cloned());
            let inherited = levels
                .find_map(|(v, from)| Some((v?, from)))
                .map(|(v, from)| Inherited {
                    value: v.clone(),
                    from,
                });
            (value, inherited)
        };

        let mut options = Vec::new();
        let mut flags = Vec::new();
        let mut cores = Vec::new();
        match (scope.system(), system) {
            (None, _) => {
                for f in &self.features().global {
                    options.push(self.item(f, &lookup));
                }
            }
            (Some(name), Some(system)) if system.name == name => {
                let chosen = |key: &str| {
                    let (value, inherited) = lookup(key);
                    // A global emulator or core means nothing to a system that lacks it.
                    value.or(inherited.filter(|i| i.from != "global").map(|i| i.value))
                };
                let emulator = chosen("emulator")
                    .filter(|e| system.emulators.iter().any(|em| &em.name == e))
                    .or_else(|| default_emulator(system).map(str::to_owned));
                cores = core_choices(system);
                if let Some(emulator) = &emulator {
                    options.push(emulator_item(system, &lookup));
                    let em = system.emulators.iter().find(|em| &em.name == emulator);
                    let core = chosen("core")
                        .filter(|c| em.is_some_and(|em| em.cores.iter().any(|k| &k.name == c)))
                        .or_else(|| em.and_then(default_core).map(str::to_owned));
                    if let Some(em) = em.filter(|em| !em.cores.is_empty()) {
                        options.push(core_item(em, &lookup));
                    }
                    let applicable = self.features().for_system(name, emulator, core.as_deref());
                    flags = applicable.flags;
                    for f in applicable.features {
                        options.push(self.item(f, &lookup));
                    }
                }
            }
            (Some(name), _) => return Err(Error::Invalid(format!("no system `{name}`"))),
        }
        Ok(OptionsPage {
            options,
            flags,
            cores,
        })
    }

    /// Sets an option at `scope`, or unsets it with `None` (the level then inherits).
    pub fn set(
        &self,
        scope: &Scope,
        system: Option<&System>,
        key: &str,
        value: Option<&str>,
    ) -> Result<()> {
        let page = self.page(scope, system)?;
        let item = page
            .options
            .iter()
            .find(|o| o.key == key)
            .ok_or_else(|| Error::Invalid(format!("`{key}` is not an option here")))?;
        let prefix = &scope.prefixes()[0].0;
        let full = format!("{prefix}.{key}");
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            conf::clear(&self.conf, &full)?;
            if key == "emulator" {
                conf::clear(&self.conf, &format!("{prefix}.core"))?;
            }
            return Ok(());
        };
        check(item, value)?;
        self.shell
            .run("batocera-settings-set", &[full.as_str(), value])?;
        // A core belongs to its emulator: the one set here may not exist in the new one.
        if key == "emulator" && item.value.as_deref() != Some(value) {
            conf::clear(&self.conf, &format!("{prefix}.core"))?;
        }
        Ok(())
    }

    fn item(
        &self,
        f: &Feature,
        lookup: &impl Fn(&str) -> (Option<String>, Option<Inherited>),
    ) -> OptionItem {
        let (value, inherited) = lookup(&f.key);
        let mut item = OptionItem {
            key: f.key.clone(),
            label: f.label.clone(),
            group: f.group.clone(),
            submenu: f.submenu.clone(),
            description: f.description.clone(),
            kind: "choice",
            choices: f
                .choices
                .iter()
                .map(|(value, label)| OptionChoice {
                    value: value.clone(),
                    label: label.clone(),
                })
                .collect(),
            min: None,
            max: None,
            step: None,
            default: None,
            value,
            inherited,
        };
        match &f.preset {
            None => {}
            Some(Preset::VideoModes) => item.choices = self.video_modes(),
            Some(Preset::ShaderSet) => item.choices = folders(&self.shaders),
            Some(Preset::Bezel) => item.choices = folders(&self.decorations),
            Some(Preset::Switch) => {
                item.choices = vec![choice("1", "On"), choice("0", "Off")];
            }
            Some(Preset::Slider {
                min,
                max,
                step,
                default,
            }) => {
                item.kind = "number";
                (item.min, item.max, item.step) = (Some(*min), Some(*max), Some(*step));
                item.default = default.map(|d| d.to_string());
            }
            Some(Preset::Text) => item.kind = "text",
        }
        item
    }

    /// `batocera-resolution listModes`: `id:label` lines, e.g. `1920x1080.60000:1920x1080 60 Hz`.
    fn video_modes(&self) -> Vec<OptionChoice> {
        self.shell
            .run("batocera-resolution", &["listModes"])
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(id, label)| choice(id.trim(), &video_mode_label(label)))
            .filter(|c| !c.value.is_empty())
            .collect()
    }
}

/// `1920x1080 59.939999 Hz` as `1920x1080 · 59.94 Hz`, `maximum 1920x1080` as
/// `Maximum 1920x1080`.
pub(crate) fn video_mode_label(label: &str) -> String {
    let label = label.trim();
    if let Some((size, rate)) = label
        .strip_suffix(" Hz")
        .and_then(|l| l.rsplit_once(' '))
        .and_then(|(size, rate)| Some((size, rate.parse::<f64>().ok()?)))
    {
        let rate = format!("{rate:.2}");
        let rate = rate.trim_end_matches('0').trim_end_matches('.');
        return format!("{size} · {rate} Hz");
    }
    let mut chars = label.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn choice(value: &str, label: &str) -> OptionChoice {
    OptionChoice {
        value: value.to_owned(),
        label: label.to_owned(),
    }
}

/// `none`, then every folder in `dirs` (Batocera's first, then the user's), by name.
fn folders(dirs: &[PathBuf]) -> Vec<OptionChoice> {
    let mut names: Vec<String> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    std::iter::once(choice("none", "None"))
        .chain(names.iter().map(|n| choice(n, &n.replace('_', " "))))
        .collect()
}

/// Every emulator's cores, the system's default first among them as Batocera lists them.
fn core_choices(system: &System) -> Vec<CoreChoice> {
    let default = default_emulator(system).map(|em| {
        let core = system
            .emulators
            .iter()
            .find(|e| e.name == em)
            .and_then(default_core);
        (em.to_owned(), core.map(str::to_owned))
    });
    let mut out = Vec::new();
    for em in &system.emulators {
        let names: Vec<&str> = if em.cores.is_empty() {
            vec![em.name.as_str()]
        } else {
            em.cores.iter().map(|c| c.name.as_str()).collect()
        };
        for core in names {
            let label = if em.name == "libretro" {
                core.to_owned()
            } else if core == em.name {
                format!("{core} (standalone)")
            } else {
                format!("{} · {core}", em.name)
            };
            out.push(CoreChoice {
                default: default
                    .as_ref()
                    .is_some_and(|(e, c)| *e == em.name && c.as_deref().is_none_or(|c| c == core)),
                emulator: em.name.clone(),
                core: core.to_owned(),
                label,
            });
        }
    }
    out
}

fn default_emulator(system: &System) -> Option<&str> {
    system
        .emulators
        .iter()
        .find(|em| em.cores.iter().any(|c| c.default))
        .or(system.emulators.first())
        .map(|em| em.name.as_str())
}

fn default_core(em: &crate::systems::Emulator) -> Option<&str> {
    em.cores
        .iter()
        .find(|c| c.default)
        .or(em.cores.first())
        .map(|c| c.name.as_str())
}

fn emulator_item(
    system: &System,
    lookup: &impl Fn(&str) -> (Option<String>, Option<Inherited>),
) -> OptionItem {
    let (value, inherited) = lookup("emulator");
    OptionItem {
        key: "emulator".into(),
        label: "Emulator".into(),
        group: None,
        submenu: None,
        description: Some("The emulator that runs this system's games.".into()),
        kind: "choice",
        choices: system
            .emulators
            .iter()
            .map(|em| choice(&em.name, &em.name))
            .collect(),
        min: None,
        max: None,
        step: None,
        default: default_emulator(system).map(str::to_owned),
        value,
        inherited: inherited.filter(|i| i.from != "global"),
    }
}

fn core_item(
    em: &crate::systems::Emulator,
    lookup: &impl Fn(&str) -> (Option<String>, Option<Inherited>),
) -> OptionItem {
    let (value, inherited) = lookup("core");
    OptionItem {
        key: "core".into(),
        label: "Core".into(),
        group: None,
        submenu: None,
        description: Some("The emulator's core for this system.".into()),
        kind: "choice",
        choices: em.cores.iter().map(|c| choice(&c.name, &c.name)).collect(),
        min: None,
        max: None,
        step: None,
        default: default_core(em).map(str::to_owned),
        value,
        inherited: inherited.filter(|i| i.from != "global"),
    }
}

fn check(item: &OptionItem, value: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::Invalid(format!("`{}`: {why}", item.key)));
    if value.contains(['\n', '\r']) {
        return bad("must be on one line");
    }
    match item.kind {
        "number" => {
            let Ok(n) = value.parse::<f64>() else {
                return bad("must be a number");
            };
            let (min, max) = (item.min.unwrap_or(f64::MIN), item.max.unwrap_or(f64::MAX));
            if n < min || n > max {
                return bad(&format!("must be from {min} to {max}"));
            }
        }
        "text" => {}
        _ if !item.choices.iter().any(|c| c.value == value) => {
            return bad(&format!("`{value}` is not one of its choices"));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
