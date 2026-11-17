//! Forma's CRT shader for games: koko-aio (Batocera ships it), tuned from Forma's menus at three
//! levels, global, system and game, as the emulator options are.
//!
//! koko-aio is a RetroArch slang shader with some 300 settings (`#pragma parameter` lines in its
//! `shaders-ng/config.inc`, in sections each switched on by a `★ … ==>` setting) and presets
//! that `#reference` one another and set some of them. Forma keeps what is chosen at each level
//! (a preset to start from, and the settings changed) in one file of its own, and from it
//! writes a preset for each level that has anything: the preset it starts from, then every
//! value set on the way down (global, then the system, then the game), flattened. A level that
//! picks its own preset starts over from it; a handheld system starts from its own screen's
//! preset, not from the global CRT.
//!
//! Batocera finds them through a shader set of Forma's (`shaders/configs/forma`: global, and
//! each system with a preset) and, for a game, `<system>["<file>"]-renderer.shader` in
//! `batocera.conf`. Slang shaders need RetroArch on Vulkan (or glcore): switched on, Forma sets
//! `global.gfxbackend` to Vulkan unless it is set, and puts both back as they were when
//! switched off. Everything is used by the next launch.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::host::Shell;
use crate::options::conf;
use crate::options::{OptionChoice, Scope};

const BATOCERA_SHADERS: &str = "/usr/share/batocera/shaders";
/// koko-aio, under a shaders folder.
const KOKO: &str = "bezel/koko-aio";
/// Its settings, every one, in its menu's order.
const CONFIG: &str = "shaders-ng/config.inc";
/// Folders of presets to start from, under koko-aio.
const PRESET_DIRS: &[&str] = &["Presets-ng", "Presets_Handhelds-ng"];
/// What global starts from unless another is chosen: a CRT monitor, no bezel.
const DEFAULT_PRESET: &str = "Presets-ng/Monitor-Balanced";
/// Handhelds start from their own screen, not from the CRT.
const HANDHELDS: &[(&str, &str)] = &[
    ("gb", "Presets_Handhelds-ng/GameboyMono"),
    ("gbc", "Presets_Handhelds-ng/GameboyColor"),
    ("gba", "Presets_Handhelds-ng/GameboyAdvance"),
    ("gamegear", "Presets_Handhelds-ng/GameGear"),
    ("lynx", "Presets_Handhelds-ng/Lynx"),
    ("psp", "Presets_Handhelds-ng/PSP"),
];
/// The settings worth having at hand, in this order (those this koko-aio has).
const ESSENTIALS: &[&str] = &[
    "GAMMA_OUT",
    "DO_PIXELGRID",
    "DO_PIXELGRID_H",
    "DO_PIXELGRID_W",
    "PIXELGRID_Y_MASK",
    "DO_IN_GLOW",
    "DO_HALO",
    "DO_BLOOM",
    "BLOOM_MIX",
    "DO_CURVATURE",
    "GEOM_WARP_X",
    "GEOM_WARP_Y",
    "DO_VIGNETTE",
    "V_SIZE",
    "DO_BEZEL",
    "DO_AMBILIGHT",
];
/// koko-aio's own bezel: on at a level, Batocera's decoration is off there.
const BEZEL: &str = "DO_BEZEL";
/// Forma's shader set, its presets and its own file, under the user's shaders folder.
const SET: &str = "forma";
/// The global choice that writes no `global.shaderset`: Batocera's own default applies.
const DEFAULT_CHOICE: &str = "default";
/// Where Batocera keeps its defaults, under its folder (`shaderset:` among them).
const CONFIGGEN_DEFAULTS: &str = "configgen/configgen-defaults.yml";
/// How deep presets may reference one another before Forma stops following.
const MAX_DEPTH: usize = 12;

/// One of koko-aio's settings.
#[derive(Debug, Clone, PartialEq)]
struct Param {
    key: String,
    label: String,
    default: f64,
    min: f64,
    max: f64,
    step: f64,
    /// The section it is in (its index in `sections`).
    section: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct Catalog {
    params: Vec<Param>,
    /// Each section's title and the key that switches it on, if it has one.
    sections: Vec<(String, Option<String>)>,
}

impl Catalog {
    fn param(&self, key: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.key == key)
    }
}

/// What is chosen at one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Level {
    /// On or off here, over the level above (a system's or a game's; global's is `enabled`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    /// A preset to start from (under koko-aio, without `.slangp`), instead of the level above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
    /// Off here with one of Batocera's shader sets instead of none (global's too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    set: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    values: BTreeMap<String, f64>,
}

impl Level {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.preset.is_none()
            && self.set.is_none()
            && self.values.is_empty()
    }
}

/// Forma's file: whether it is on, what was there before, and every level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    enabled: bool,
    /// The `shaderset` keys in `batocera.conf` were taken in as choices (Forma owns them since).
    #[serde(default)]
    adopted: bool,
    /// `global.shaderset` and `global.gfxbackend` before Forma's were set, to put back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_shaderset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_backend: Option<String>,
    /// Whether Forma set the backend (else it was the user's, and stays).
    #[serde(default)]
    set_backend: bool,
    #[serde(default)]
    global: Level,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    systems: BTreeMap<String, Level>,
    /// By system, then by the game's file name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    games: BTreeMap<String, BTreeMap<String, Level>>,
    /// Batocera's decoration keys Forma changed (by their level's prefix, `global`, `snes`,
    /// `snes["file"]`): what was there, and what Forma wrote.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    decorations: BTreeMap<String, Decoration>,
    /// The `gfxbackend` keys Forma set for a level with its shader (by the full key): what was
    /// there, and what Forma wrote.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    switches: BTreeMap<String, Decoration>,
}

impl State {
    /// On at `scope`: the nearest level that says, else the global switch.
    fn on_at(&self, scope: &Scope) -> bool {
        let system = |s: &str| self.systems.get(s).and_then(|l| l.enabled);
        match scope {
            Scope::Global => self.enabled,
            Scope::System(s) => system(s).unwrap_or(self.enabled),
            Scope::Game { system: s, file } => self
                .games
                .get(s)
                .and_then(|g| g.get(file))
                .and_then(|l| l.enabled)
                .or_else(|| system(s))
                .unwrap_or(self.enabled),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Decoration {
    before: Option<String>,
    wrote: Option<String>,
}

/// A level's page: whether the shader is there and on, its preset, and every setting.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderPage {
    /// koko-aio is installed.
    pub available: bool,
    /// Forma's shader is on at this level.
    pub enabled: bool,
    /// The shader at this level, as one choice: `none`, `crt:<preset>` (Forma's koko-aio from
    /// that preset) or `set:<name>` (one of Batocera's shader sets). Since SDK 1.35.0.
    pub shader: ShaderChoice,
    /// Keys of the settings worth having at hand, in order.
    pub essentials: Vec<String>,
    pub sections: Vec<ShaderSection>,
    /// koko-aio draws its own bezel at this level, so Batocera's decoration is off here (and
    /// back as it was when the bezel or the shader goes off).
    pub draws_bezel: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderChoice {
    /// Chosen at this level (always, globally); `None` inherits.
    pub value: Option<String>,
    /// What this level has when it chooses nothing.
    pub inherited: ShaderInherited<String>,
    pub choices: Vec<OptionChoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderSection {
    pub title: String,
    /// The setting that switches it on, if it has one (also the first of `params`).
    pub switch: Option<String>,
    pub params: Vec<ShaderParam>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderParam {
    pub key: String,
    pub label: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Set at this level.
    pub value: Option<f64>,
    /// What applies when this level sets nothing: a level above, or the preset.
    pub inherited: ShaderInherited<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShaderInherited<T> {
    pub value: T,
    /// `global`, `system`, `preset` (the preset's own value) or `shader` (koko-aio's default).
    pub from: &'static str,
}

pub struct Shaders {
    shell: Box<dyn Shell>,
    conf: PathBuf,
    /// Batocera's shaders folder, and the user's (where Forma writes).
    batocera: PathBuf,
    user: PathBuf,
    catalog: OnceLock<Option<Catalog>>,
}

impl Shaders {
    pub fn new(shell: Box<dyn Shell>, conf: &Path, userdata: &Path) -> Self {
        Self::with_dirs(
            shell,
            conf,
            Path::new(BATOCERA_SHADERS),
            &userdata.join("shaders"),
        )
    }

    /// For tests and the mock: where Batocera's shaders are, and the user's.
    pub fn with_dirs(shell: Box<dyn Shell>, conf: &Path, batocera: &Path, user: &Path) -> Self {
        Self {
            shell,
            conf: conf.to_path_buf(),
            batocera: batocera.to_path_buf(),
            user: user.to_path_buf(),
            catalog: OnceLock::new(),
        }
    }

    fn koko(&self) -> PathBuf {
        self.batocera.join(KOKO)
    }

    /// koko-aio's settings, read once (Batocera's image does not change while Forma runs).
    fn catalog(&self) -> Option<&Catalog> {
        self.catalog
            .get_or_init(|| {
                let text = std::fs::read_to_string(self.koko().join(CONFIG)).ok()?;
                let catalog = parse_config(&text);
                (!catalog.params.is_empty()).then_some(catalog)
            })
            .as_ref()
    }

    fn state_file(&self) -> PathBuf {
        self.user.join(SET).join("forma.json")
    }

    fn state(&self) -> State {
        let mut state: State = std::fs::read_to_string(self.state_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        if !state.adopted {
            adopt(&mut state, &conf::read(&self.conf));
        }
        state
    }

    /// Takes Batocera's `shaderset` keys in as the levels' choices and writes them as Forma's,
    /// once (from the first start that owns them); nothing after, nor without koko-aio.
    pub fn adopt(&self) -> Result<()> {
        let state = self.state();
        if state.adopted || self.catalog().is_none() {
            return Ok(());
        }
        self.apply(state)
    }

    /// Batocera's own shader sets a level can have instead of Forma's (folders of its
    /// `shaders/configs`): not the user's folder's, where sets made by hand or by tests pile up,
    /// nor Forma's own, nor its stock koko-aio, which Forma's is.
    fn sets(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.batocera.join("configs"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.') && n != SET && n != "koko-aio")
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// Batocera's default shader set (`shaderset:` in its defaults), for the default's label.
    fn batocera_default(&self) -> Option<String> {
        let path = self.batocera.parent()?.join(CONFIGGEN_DEFAULTS);
        std::fs::read_to_string(path)
            .ok()?
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("shaderset:")
                    .map(|v| v.trim().to_owned())
            })
            .filter(|v| !v.is_empty())
    }

    /// Every shader a level can have, as one list: (globally) Batocera's default, none, Forma's
    /// koko-aio from each of its presets, Batocera's sets.
    fn shader_choices(&self, presets: &[OptionChoice], global: bool) -> Vec<OptionChoice> {
        let default = global.then(|| OptionChoice {
            value: DEFAULT_CHOICE.into(),
            label: match self.batocera_default() {
                Some(name) => format!("Batocera default · {}", name.replace(['_', '-'], " ")),
                None => "Batocera default".into(),
            },
        });
        default
            .into_iter()
            .chain(std::iter::once(OptionChoice {
                value: "none".into(),
                label: "None".into(),
            }))
            .chain(presets.iter().map(|p| OptionChoice {
                value: format!("crt:{}", p.value),
                label: format!("CRT · {}", p.label),
            }))
            .chain(self.sets().into_iter().map(|n| OptionChoice {
                label: format!("Batocera · {}", n.replace(['_', '-'], " ")),
                value: format!("set:{n}"),
            }))
            .collect()
    }

    /// The shader `scope` has, as a choice's value, and the level it comes from.
    fn shader_at(&self, state: &State, scope: &Scope) -> (String, &'static str) {
        let level = |l: Option<&Level>| -> Option<String> {
            let l = l?;
            match l.enabled? {
                true => Some(format!("crt:{}", self.chain(state, scope).0)),
                false => Some(l.set.as_ref().map_or("none".into(), |n| format!("set:{n}"))),
            }
        };
        // Globally, no set is Batocera's default; `none` is said.
        let global = || {
            if state.enabled {
                format!("crt:{}", self.chain(state, scope).0)
            } else {
                match state.global.set.as_deref() {
                    None => DEFAULT_CHOICE.into(),
                    Some("none") => "none".into(),
                    Some(n) => format!("set:{n}"),
                }
            }
        };
        match scope {
            Scope::Global => (global(), "global"),
            Scope::System(s) => level(state.systems.get(s))
                .map(|v| (v, "system"))
                .unwrap_or_else(|| (global(), "global")),
            Scope::Game { system, file } => {
                level(state.games.get(system).and_then(|g| g.get(file)))
                    .map(|v| (v, "game"))
                    .or_else(|| level(state.systems.get(system)).map(|v| (v, "system")))
                    .unwrap_or_else(|| (global(), "global"))
            }
        }
    }

    /// `scope`'s shader as one choice: what it chose and what it would have without.
    fn shader_of(&self, state: &State, scope: &Scope, presets: &[OptionChoice]) -> ShaderChoice {
        let mut without = state.clone();
        if let Some(level) = own_level_mut(&mut without, scope)
            && !matches!(scope, Scope::Global)
        {
            level.enabled = None;
            level.set = None;
            level.preset = None;
        }
        let (inherited, from) = self.shader_at(&without, scope);
        let own = match scope {
            Scope::Global => true,
            _ => own_level(state, scope).is_some_and(|l| l.enabled.is_some()),
        };
        ShaderChoice {
            value: own.then(|| self.shader_at(state, scope).0),
            inherited: ShaderInherited {
                value: inherited,
                from: if matches!(scope, Scope::Global) {
                    "shader"
                } else {
                    from
                },
            },
            choices: self.shader_choices(presets, matches!(scope, Scope::Global)),
        }
    }

    /// Chooses the shader at `scope`: `none`, `crt:<preset>` (Forma's koko-aio, from that
    /// preset), `set:<name>` (one of Batocera's sets), or `None` for what the level above has
    /// (not globally). The level's settings stay, for when it is Forma's again.
    pub fn choose(&self, scope: &Scope, choice: Option<&str>) -> Result<()> {
        self.catalog().ok_or_else(not_installed)?;
        enum Pick {
            Inherit,
            /// Globally: no `global.shaderset`, Batocera's default.
            Default,
            Off(Option<String>),
            Crt(String),
        }
        let pick = match choice {
            None if matches!(scope, Scope::Global) => {
                return Err(Error::Invalid(
                    "every game has a shader, or none: global inherits from nothing".into(),
                ));
            }
            None => Pick::Inherit,
            Some(DEFAULT_CHOICE) if matches!(scope, Scope::Global) => Pick::Default,
            Some("none") => Pick::Off(None),
            Some(c) => match (c.strip_prefix("crt:"), c.strip_prefix("set:")) {
                (Some(p), _) if self.presets().iter().any(|x| x.value == p) => {
                    Pick::Crt(p.to_owned())
                }
                (_, Some(n)) if self.sets().iter().any(|x| x == n) => Pick::Off(Some(n.to_owned())),
                _ => return Err(Error::Invalid(format!("no shader `{c}`"))),
            },
        };
        // The preset this level starts from when it picks none of its own.
        let mut without = self.state();
        if let Some(level) = own_level_mut(&mut without, scope) {
            level.preset = None;
        }
        let start = self.chain(&without, scope).0;
        if let Scope::Global = scope {
            let mut state = self.state();
            match pick {
                Pick::Crt(p) => {
                    state.enabled = true;
                    state.global.preset = (p != start).then_some(p);
                }
                Pick::Off(set) => {
                    state.enabled = false;
                    state.global.set = Some(set.unwrap_or_else(|| "none".into()));
                }
                Pick::Default => {
                    state.enabled = false;
                    state.global.set = None;
                }
                Pick::Inherit => unreachable!("refused above"),
            }
            return self.apply(state);
        }
        self.change(scope, |level| match pick {
            Pick::Default => unreachable!("only globally"),
            Pick::Inherit => {
                level.enabled = None;
                level.set = None;
                level.preset = None;
            }
            Pick::Off(set) => {
                level.enabled = Some(false);
                level.set = set;
            }
            Pick::Crt(p) => {
                level.enabled = Some(true);
                level.set = None;
                // The same as it would start from: the levels above's settings still apply.
                level.preset = (p != start).then_some(p);
            }
        })
    }

    /// The presets a level can start from, as `Presets-ng/Monitor-Balanced`.
    fn presets(&self) -> Vec<OptionChoice> {
        let mut out = Vec::new();
        for dir in PRESET_DIRS {
            let Ok(entries) = std::fs::read_dir(self.koko().join(dir)) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter_map(|n| n.strip_suffix(".slangp").map(str::to_owned))
                .collect();
            names.sort();
            for name in names {
                let handheld = dir.contains("Handheld");
                let words = name.replace(['_', '-'], " ");
                let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
                out.push(OptionChoice {
                    value: format!("{dir}/{name}"),
                    label: if handheld {
                        format!("Handheld · {words}")
                    } else {
                        words
                    },
                });
            }
        }
        out
    }

    /// The preset a scope starts from, and the levels whose values apply over it, top down.
    fn chain<'s>(
        &self,
        state: &'s State,
        scope: &Scope,
    ) -> (String, Vec<(&'s Level, &'static str)>) {
        let mut preset = state
            .global
            .preset
            .clone()
            .unwrap_or_else(|| DEFAULT_PRESET.to_owned());
        let mut levels = vec![(&state.global, "global")];
        let (system, file) = match scope {
            Scope::Global => return (preset, levels),
            Scope::System(s) => (s.as_str(), None),
            Scope::Game { system, file } => (system.as_str(), Some(file.as_str())),
        };
        let level = state.systems.get(system);
        if let Some(own) = level.and_then(|l| l.preset.clone()) {
            preset = own;
            levels.clear();
        } else if let Some((_, handheld)) = HANDHELDS.iter().find(|(s, _)| *s == system) {
            preset = (*handheld).to_owned();
            levels.clear();
        }
        if let Some(level) = level {
            levels.push((level, "system"));
        }
        if let Some(file) = file {
            let level = state.games.get(system).and_then(|g| g.get(file));
            if let Some(own) = level.and_then(|l| l.preset.clone()) {
                preset = own;
                levels.clear();
            }
            if let Some(level) = level {
                levels.push((level, "game"));
            }
        }
        (preset, levels)
    }

    /// A preset's values, following its references (each file's own values over theirs).
    fn preset_values(&self, catalog: &Catalog, preset: &str) -> HashMap<String, f64> {
        let mut values = HashMap::new();
        let path = self.koko().join(format!("{preset}.slangp"));
        read_preset(&path, catalog, &mut values, 0);
        values
    }

    /// The settings at `scope`, with what it sets and what it inherits.
    pub fn page(&self, scope: &Scope) -> Result<ShaderPage> {
        let state = self.state();
        let Some(catalog) = self.catalog() else {
            return Ok(ShaderPage {
                available: false,
                enabled: state.on_at(scope),
                shader: ShaderChoice {
                    value: None,
                    inherited: ShaderInherited {
                        value: "none".into(),
                        from: "shader",
                    },
                    choices: Vec::new(),
                },
                essentials: Vec::new(),
                sections: Vec::new(),
                draws_bezel: false,
            });
        };
        let here = own_level(&state, scope);
        let (preset, levels) = self.chain(&state, scope);
        let preset_values = self.preset_values(catalog, &preset);
        let above: Vec<_> = levels
            .iter()
            .filter(|(l, _)| !here.is_some_and(|h| std::ptr::eq(*l, h)))
            .collect();
        let inherited = |p: &Param| {
            above
                .iter()
                .rev()
                .find_map(|(l, from)| l.values.get(&p.key).map(|v| (*v, *from)))
                .or_else(|| preset_values.get(&p.key).map(|v| (*v, "preset")))
                .map_or(
                    ShaderInherited {
                        value: p.default,
                        from: "shader",
                    },
                    |(value, from)| ShaderInherited { value, from },
                )
        };
        let mut sections: Vec<ShaderSection> = catalog
            .sections
            .iter()
            .map(|(title, switch)| ShaderSection {
                title: title.clone(),
                switch: switch.clone(),
                params: Vec::new(),
            })
            .collect();
        for p in &catalog.params {
            sections[p.section].params.push(ShaderParam {
                key: p.key.clone(),
                label: p.label.clone(),
                min: p.min,
                max: p.max,
                step: p.step,
                value: here.and_then(|l| l.values.get(&p.key).copied()),
                inherited: inherited(p),
            });
        }
        sections.retain(|s| !s.params.is_empty());
        let presets = self.presets();
        Ok(ShaderPage {
            available: true,
            enabled: state.on_at(scope),
            shader: self.shader_of(&state, scope, &presets),
            essentials: ESSENTIALS
                .iter()
                .filter(|k| catalog.param(k).is_some())
                .map(|k| (*k).to_owned())
                .collect(),
            sections,
            draws_bezel: state.on_at(scope) && self.effective(&state, catalog, scope, BEZEL) >= 0.5,
        })
    }

    /// Sets a setting at `scope`, or unsets it with `None` (the level then inherits).
    pub fn set(&self, scope: &Scope, key: &str, value: Option<f64>) -> Result<()> {
        let catalog = self.catalog().ok_or_else(not_installed)?;
        let param = catalog
            .param(key)
            .ok_or_else(|| Error::Invalid(format!("`{key}` is not a koko-aio setting")))?;
        if let Some(v) = value
            && (!v.is_finite() || v < param.min - 1e-9 || v > param.max + 1e-9)
        {
            return Err(Error::Invalid(format!(
                "{} goes from {} to {}",
                param.label, param.min, param.max
            )));
        }
        self.change(scope, |level| match value {
            Some(v) => {
                level.values.insert(key.to_owned(), v);
            }
            None => {
                level.values.remove(key);
            }
        })
    }

    fn change(&self, scope: &Scope, edit: impl FnOnce(&mut Level)) -> Result<()> {
        let mut state = self.state();
        let level = match scope {
            Scope::Global => &mut state.global,
            Scope::System(s) => state.systems.entry(s.clone()).or_default(),
            Scope::Game { system, file } => state
                .games
                .entry(system.clone())
                .or_default()
                .entry(file.clone())
                .or_default(),
        };
        edit(level);
        state.systems.retain(|_, l| !l.is_empty());
        let mut gone = Vec::new();
        for (system, games) in &mut state.games {
            games.retain(|file, l| {
                if l.is_empty() {
                    gone.push((system.clone(), file.clone()));
                }
                !l.is_empty()
            });
        }
        state.games.retain(|_, g| !g.is_empty());
        for (system, file) in gone {
            conf::clear(&self.conf, &renderer_key(&system, &file))?;
        }
        self.apply(state)
    }

    /// Writes Forma's file, a preset per level that has one, its shader set, and the games'
    /// keys in `batocera.conf` (only while on).
    fn apply(&self, mut state: State) -> Result<()> {
        let catalog = self.catalog().ok_or_else(not_installed)?;
        let root = self.user.join(SET);
        mkdir(&root)?;
        self.sync_switches(&mut state)?;
        self.sync_decorations(&mut state, catalog)?;
        write(
            &self.state_file(),
            &serde_json::to_string_pretty(&state).expect("state serialises"),
        )?;
        let state = &state;
        // Written afresh each time: a preset left from an unset level would linger.
        for dir in ["systems", "games"] {
            let path = root.join(dir);
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|e| Error::io(&path, e))?;
            }
        }
        let preset = |scope: &Scope, name: &str| -> Result<()> {
            let (base, levels) = self.chain(state, scope);
            let mut values = BTreeMap::new();
            for (level, _) in &levels {
                values.extend(level.values.iter().map(|(k, v)| (k.clone(), *v)));
            }
            let path = root.join(format!("{name}.slangp"));
            if let Some(dir) = path.parent() {
                mkdir(dir)?;
            }
            let mut text = format!(
                "# Forma's CRT shader: koko-aio, as tuned in Forma's menus. Written by Forma.\n\
                 #reference \"{}\"\n",
                self.koko().join(format!("{base}.slangp")).display()
            );
            for (key, value) in values {
                if catalog.param(&key).is_some() {
                    text.push_str(&format!("{key} = \"{value:.6}\"\n"));
                }
            }
            write(&path, &text)
        };
        preset(&Scope::Global, "global")?;
        let mut defaults = String::from(
            "## Forma: koko-aio, as tuned in Forma's menus (written by Forma)\n\
             default:\n  shader: forma/global\n",
        );
        let mut systems: Vec<&str> = state.systems.keys().map(String::as_str).collect();
        for (handheld, _) in HANDHELDS {
            if !systems.contains(handheld) {
                systems.push(handheld);
            }
        }
        systems.sort_unstable();
        for system in systems {
            preset(
                &Scope::System(system.to_owned()),
                &format!("systems/{system}"),
            )?;
            defaults.push_str(&format!("{system}:\n  shader: forma/systems/{system}\n"));
        }
        let set = self.user.join("configs").join(SET);
        mkdir(&set)?;
        write(&set.join("rendering-defaults.yml"), &defaults)?;
        for (system, games) in &state.games {
            for file in games.keys() {
                let name = format!("games/{system}/{}", file_slug(file));
                let scope = Scope::Game {
                    system: system.clone(),
                    file: file.clone(),
                };
                preset(&scope, &name)?;
                let key = renderer_key(system, file);
                if state.on_at(&scope) {
                    self.shell.run(
                        "batocera-settings-set",
                        &[key.as_str(), &format!("{SET}/{name}")],
                    )?;
                } else {
                    conf::clear(&self.conf, &key)?;
                }
            }
        }
        Ok(())
    }

    /// A setting's value at `scope`: the nearest level's, else its preset's, else koko-aio's.
    fn effective(&self, state: &State, catalog: &Catalog, scope: &Scope, key: &str) -> f64 {
        let (preset, levels) = self.chain(state, scope);
        levels
            .iter()
            .rev()
            .find_map(|(l, _)| l.values.get(key).copied())
            .or_else(|| self.preset_values(catalog, &preset).get(key).copied())
            .or_else(|| catalog.param(key).map(|p| p.default))
            .unwrap_or(0.0)
    }

    /// Batocera's decoration off (`none`) at each level where Forma's shader is on and draws
    /// its own bezel, and as the user had it everywhere else: two frames round a game are one
    /// too many. Levels are walked top down, so a system without koko-aio's bezel under a
    /// global with it gets its decoration back by a key of its own.
    fn sync_decorations(&self, state: &mut State, catalog: &Catalog) -> Result<()> {
        let conf = conf::read(&self.conf);
        let key = |prefix: &str| format!("{prefix}.bezel");
        // The user's own key at a level: what Forma found there, unless changed since.
        let user = |state: &State, prefix: &str| -> Option<String> {
            let now = conf.get(&key(prefix)).cloned();
            match state.decorations.get(prefix) {
                Some(d) if d.wrote == now => d.before.clone(),
                _ => now,
            }
        };
        let mut systems: std::collections::BTreeSet<String> =
            state.systems.keys().cloned().collect();
        systems.extend(HANDHELDS.iter().map(|(s, _)| (*s).to_owned()));
        systems.extend(state.games.keys().cloned());
        for k in conf.keys() {
            if let Some(sys) = k.strip_suffix(".bezel")
                && !sys.contains(['[', '.'])
                && sys != "global"
            {
                systems.insert(sys.to_owned());
            }
        }
        let mut scopes = vec![(Scope::Global, "global".to_owned())];
        for system in &systems {
            scopes.push((Scope::System(system.clone()), system.clone()));
            for file in state.games.get(system).into_iter().flat_map(|g| g.keys()) {
                let name: String = file.chars().filter(|c| *c != '=' && *c != '#').collect();
                scopes.push((
                    Scope::Game {
                        system: system.clone(),
                        file: file.clone(),
                    },
                    conf::game_prefix(system, &name),
                ));
            }
        }
        let none = || "none".to_owned();
        // What applies at each level after Forma's keys, and what the user's own keys give.
        let mut written: HashMap<String, String> = HashMap::new();
        let mut users: HashMap<String, Option<String>> = HashMap::new();
        let mut seen = Vec::new();
        for (scope, prefix) in &scopes {
            let parent = match scope {
                Scope::Global => None,
                Scope::System(_) => Some("global".to_owned()),
                Scope::Game { system, .. } => Some(system.clone()),
            };
            let parent_written = parent
                .as_ref()
                .and_then(|p| written.get(p).cloned())
                .unwrap_or_else(none);
            let own = user(state, prefix);
            let user_eff = own.clone().or_else(|| {
                parent
                    .as_ref()
                    .and_then(|p| users.get(p).cloned().flatten())
            });
            let draws = state.on_at(scope) && self.effective(state, catalog, scope, BEZEL) >= 0.5;
            let target = if draws {
                none()
            } else {
                user_eff.clone().unwrap_or_else(none)
            };
            let desired = if own.clone().unwrap_or_else(|| parent_written.clone()) == target {
                own.clone()
            } else {
                Some(target)
            };
            written.insert(prefix.clone(), desired.clone().unwrap_or(parent_written));
            users.insert(prefix.clone(), user_eff);
            self.set_key(&conf, &key(prefix), desired.as_deref())?;
            if desired == own {
                state.decorations.remove(prefix);
            } else {
                state.decorations.insert(
                    prefix.clone(),
                    Decoration {
                        before: own,
                        wrote: desired,
                    },
                );
            }
            seen.push(prefix.clone());
        }
        // A level Forma had changed that is gone (its game unset): back as it was.
        let gone: Vec<String> = state
            .decorations
            .keys()
            .filter(|p| !seen.contains(p))
            .cloned()
            .collect();
        for prefix in gone {
            let d = state.decorations.remove(&prefix).expect("listed");
            if conf.get(&key(&prefix)) == d.wrote.as_ref() {
                self.set_key(&conf, &key(&prefix), d.before.as_deref())?;
            }
        }
        Ok(())
    }

    /// Batocera's `shaderset` keys, which Forma owns: each level's choice (`forma`, `none` or
    /// one of Batocera's), no key at a level that chooses none. And RetroArch on Vulkan where
    /// Forma's shader is chosen unless a backend that runs slang shaders applies there (the
    /// user's own backend back where it is not).
    fn sync_switches(&self, state: &mut State) -> Result<()> {
        let conf = conf::read(&self.conf);
        state.adopted = true;
        let shaderset = |l: &Level, on: bool| -> String {
            if on {
                SET.to_owned()
            } else {
                l.set.clone().unwrap_or_else(|| "none".to_owned())
            }
        };
        let mut want: BTreeMap<String, Option<String>> = conf
            .keys()
            .filter(|k| k.ends_with(".shaderset"))
            .map(|k| (k.clone(), None))
            .collect();
        // Globally, no set is none written: Batocera's default.
        want.insert(
            "global.shaderset".into(),
            if state.enabled {
                Some(SET.to_owned())
            } else {
                state.global.set.clone()
            },
        );
        let mut scopes = Vec::new();
        for system in state.systems.keys() {
            scopes.push((Scope::System(system.clone()), system.clone()));
        }
        for (system, games) in &state.games {
            for file in games.keys() {
                let name: String = file.chars().filter(|c| *c != '=' && *c != '#').collect();
                scopes.push((
                    Scope::Game {
                        system: system.clone(),
                        file: file.clone(),
                    },
                    conf::game_prefix(system, &name),
                ));
            }
        }
        for (scope, prefix) in &scopes {
            let level = own_level(state, scope).expect("listed");
            want.insert(
                format!("{prefix}.shaderset"),
                level.enabled.map(|on| shaderset(level, on)),
            );
        }
        for (key, value) in &want {
            self.set_key(&conf, key, value.as_deref())?;
        }
        // Tracked before Forma owned them: theirs now.
        state.switches.retain(|k, _| !k.ends_with(".shaderset"));

        let slang = |b: &Option<String>| matches!(b.as_deref(), Some("vulkan" | "glcore"));
        let key = "global.gfxbackend";
        let user_global = own_key(state, &conf, key);
        let want_global = (state.enabled && !slang(&user_global)).then(|| "vulkan".to_owned());
        let global_backend = want_global.clone().or_else(|| user_global.clone());
        self.write_own(state, &conf, key, user_global, want_global)?;
        let mut seen = vec![key.to_owned()];
        for (scope, prefix) in scopes {
            let backend = format!("{prefix}.gfxbackend");
            let user_backend = own_key(state, &conf, &backend);
            let chosen = own_level(state, &scope).and_then(|l| l.enabled) == Some(true);
            let system = scope.system().unwrap_or_default();
            let applies = [
                user_backend.clone(),
                if matches!(scope, Scope::Game { .. }) {
                    conf.get(&format!("{system}.gfxbackend")).cloned()
                } else {
                    None
                },
                global_backend.clone(),
            ]
            .into_iter()
            .flatten()
            .next();
            let want_backend = (chosen && !slang(&applies)).then(|| "vulkan".to_owned());
            self.write_own(state, &conf, &backend, user_backend, want_backend)?;
            seen.push(backend);
        }
        let gone: Vec<String> = state
            .switches
            .keys()
            .filter(|k| !seen.contains(k))
            .cloned()
            .collect();
        for key in gone {
            let d = state.switches.remove(&key).expect("listed");
            if conf.get(&key) == d.wrote.as_ref() {
                self.set_key(&conf, &key, d.before.as_deref())?;
            }
        }
        Ok(())
    }

    /// `key` as Forma wants it (`want`), else as the user had it (`user`), keeping track.
    fn write_own(
        &self,
        state: &mut State,
        conf: &HashMap<String, String>,
        key: &str,
        user: Option<String>,
        want: Option<String>,
    ) -> Result<()> {
        let target = want.or_else(|| user.clone());
        self.set_key(conf, key, target.as_deref())?;
        if target == user {
            state.switches.remove(key);
        } else {
            state.switches.insert(
                key.to_owned(),
                Decoration {
                    before: user,
                    wrote: target,
                },
            );
        }
        Ok(())
    }

    /// A `batocera.conf` key set, or cleared with `None`; left alone if it already is.
    fn set_key(
        &self,
        conf: &HashMap<String, String>,
        key: &str,
        value: Option<&str>,
    ) -> Result<()> {
        if conf.get(key).map(String::as_str) == value {
            return Ok(());
        }
        match value {
            Some(v) => self
                .shell
                .run("batocera-settings-set", &[key, v])
                .map(|_| ()),
            None => conf::clear(&self.conf, key),
        }
    }
}

/// The user's own value of a key Forma may have set: what Forma found there, unless changed
/// since.
fn own_key(state: &State, conf: &HashMap<String, String>, key: &str) -> Option<String> {
    let now = conf.get(key).cloned();
    match state.switches.get(key) {
        Some(d) if d.wrote == now => d.before.clone(),
        _ => now,
    }
}

/// Batocera's `shaderset` keys as the levels' choices, once, before Forma owns them: `forma`
/// and Batocera's stock koko-aio as Forma's shader, `none` as none, any other as that set. And
/// the global backend Forma set before it tracked keys as it does now.
fn adopt(state: &mut State, conf: &HashMap<String, String>) {
    fn take(level: &mut Level, value: &str) -> bool {
        match value {
            SET | "koko-aio" => {
                level.set = None;
                true
            }
            "none" | "" => {
                level.set = None;
                false
            }
            other => {
                level.set = Some(other.to_owned());
                false
            }
        }
    }
    for (key, value) in conf {
        let Some(prefix) = key.strip_suffix(".shaderset") else {
            continue;
        };
        if prefix == "global" {
            let mut global = std::mem::take(&mut state.global);
            state.enabled = take(&mut global, value);
            // Globally `none` is said; no key at all is Batocera's default.
            if matches!(value.as_str(), "none" | "") {
                global.set = Some("none".into());
            }
            state.global = global;
        } else if let Some((system, rest)) = prefix.split_once("[\"") {
            let Some(name) = rest.strip_suffix("\"]") else {
                continue;
            };
            let games = state.games.entry(system.to_owned()).or_default();
            let file = games
                .keys()
                .find(|f| {
                    f.chars()
                        .filter(|c| *c != '=' && *c != '#')
                        .collect::<String>()
                        == name
                })
                .cloned()
                .unwrap_or_else(|| name.to_owned());
            let level = games.entry(file).or_default();
            level.enabled = Some(take(level, value));
        } else if !prefix.contains('.') {
            let level = state.systems.entry(prefix.to_owned()).or_default();
            level.enabled = Some(take(level, value));
        }
    }
    if std::mem::take(&mut state.set_backend) {
        state.switches.insert(
            "global.gfxbackend".into(),
            Decoration {
                before: state.before_backend.take(),
                wrote: Some("vulkan".into()),
            },
        );
    }
    state.before_shaderset = None;
}

fn not_installed() -> Error {
    Error::Unsupported("koko-aio is not installed (Batocera 41 or later ships it)".into())
}

fn own_level<'s>(state: &'s State, scope: &Scope) -> Option<&'s Level> {
    match scope {
        Scope::Global => Some(&state.global),
        Scope::System(s) => state.systems.get(s),
        Scope::Game { system, file } => state.games.get(system)?.get(file),
    }
}

fn own_level_mut<'s>(state: &'s mut State, scope: &Scope) -> Option<&'s mut Level> {
    match scope {
        Scope::Global => Some(&mut state.global),
        Scope::System(s) => state.systems.get_mut(s),
        Scope::Game { system, file } => state.games.get_mut(system)?.get_mut(file),
    }
}

/// A game's shader key, by its file name as configgen names it (no `=` nor `#`).
fn renderer_key(system: &str, file: &str) -> String {
    let name: String = file.chars().filter(|c| *c != '=' && *c != '#').collect();
    format!("{}-renderer.shader", conf::game_prefix(system, &name))
}

/// A file name for a game's preset: its own name's letters and digits, and a hash of it.
fn file_slug(file: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hash);
    let stem: String = file
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    format!("{stem}-{:08x}", hash.finish() as u32)
}

fn mkdir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| Error::io(path, e))
}

/// Written to a temporary file first, then renamed over.
fn write(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("forma-tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| Error::io(path, e))
}

/// A preset's values into `values`: its references first, in order, then its own lines.
fn read_preset(path: &Path, catalog: &Catalog, values: &mut HashMap<String, f64>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut own = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#reference") {
            let target = rest.trim().trim_matches('"');
            read_preset(&dir.join(target), catalog, values, depth + 1);
        } else if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if catalog.param(key).is_some()
                && let Ok(v) = value.trim().trim_matches('"').trim().parse::<f64>()
            {
                own.push((key.to_owned(), v));
            }
        }
    }
    values.extend(own);
}

/// koko-aio's settings from its `config.inc`: each `#pragma parameter NAME "label" default min
/// max step`, in order. A `★ … ==>` setting at the left starts a section (it switches it on); a
/// `★` setting at the left with no arrow belongs to the first, general, section; the rest to
/// the section they follow. Labels (`min == max`) are left out.
fn parse_config(text: &str) -> Catalog {
    let mut catalog = Catalog {
        params: Vec::new(),
        sections: vec![("General".to_owned(), None)],
    };
    let mut current = 0;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("#pragma parameter") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some((key, rest)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let Some((raw, numbers)) = rest.split_once('"') else {
            continue;
        };
        let numbers: Vec<f64> = numbers
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let [default, min, max, step] = numbers[..] else {
            continue;
        };
        if max <= min {
            continue;
        }
        let at_left = !raw.starts_with(' ');
        let starred = raw.trim_start().starts_with('★');
        // A section's switch is named as the section is.
        let mut label = param_label(raw);
        if at_left && starred && raw.contains("==>") {
            label = section_title(raw);
            catalog.sections.push((label.clone(), Some(key.to_owned())));
            current = catalog.sections.len() - 1;
        } else if at_left && starred {
            current = 0;
        }
        catalog.params.push(Param {
            key: key.to_owned(),
            label,
            default,
            min,
            max,
            step: if step > 0.0 {
                step
            } else {
                (max - min) / 100.0
            },
            section: current,
        });
    }
    catalog
}

/// `★ Bloom/Halation enable? (bl) ==>` as `Bloom/Halation`.
fn section_title(raw: &str) -> String {
    let mut t = raw.replace('★', "").replace("==>", "");
    for cut in [" enable?", " enable", "?"] {
        t = t.replace(cut, "");
    }
    let t = strip_short_parens(t.trim());
    let t = t.trim().trim_end_matches(':').trim();
    capitalise(t)
}

/// A setting's label without its stars, arrows and leading dots.
fn param_label(raw: &str) -> String {
    let t = raw.replace('★', "").replace("==>", "");
    let t = strip_short_parens(t.trim().trim_start_matches(['.', ' ']));
    let t = t.trim();
    capitalise(t.strip_suffix('?').unwrap_or(t).trim())
}

/// Takes off koko-aio's short abbreviations in brackets, `(bl)`, `(co)`: its own shorthand.
fn strip_short_parens(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let inside = &rest[open + 1..open + close];
        out.push_str(&rest[..open]);
        if !(inside.len() <= 3 && inside.chars().all(|c| c.is_ascii_lowercase())) {
            out.push_str(&rest[open..=open + close]);
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn capitalise(t: &str) -> String {
    let mut chars = t.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
