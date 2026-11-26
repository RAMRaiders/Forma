//! `es_features.cfg`: the options Batocera defines for its emulators and cores, which
//! EmulationStation draws as its game settings menus.
//!
//! The file declares shared features once (`<sharedFeatures>`) and refers to them by key from
//! the global list, emulators, cores and systems. Here every reference is resolved, so each
//! level holds whole features.

use std::collections::HashMap;

use crate::xml::{self, Element};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Feature {
    /// The `batocera.conf` key after the level prefix, e.g. `ratio` in `snes.ratio`.
    pub key: String,
    pub label: String,
    pub group: Option<String>,
    pub submenu: Option<String>,
    pub description: Option<String>,
    pub order: Option<i64>,
    pub choices: Vec<(String, String)>,
    pub preset: Option<Preset>,
}

/// Choices Batocera builds at run time instead of listing them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Preset {
    /// The display's modes, from `batocera-resolution listModes`.
    VideoModes,
    /// Shader sets: folders of `shaders/configs`.
    ShaderSet,
    /// Decorations (bezels): folders of `decorations`.
    Bezel,
    /// A number from `min` to `max`; `default` is what applies when unset.
    Slider {
        min: f64,
        max: f64,
        step: f64,
        default: Option<f64>,
    },
    /// On or off; unset leaves it to the emulator.
    Switch,
    /// Free text (a URL, a file path).
    Text,
}

#[derive(Debug, Default)]
pub(crate) struct Level {
    pub flags: Vec<String>,
    pub features: Vec<Feature>,
    /// System-specific additions, by system name.
    pub systems: HashMap<String, Level>,
}

#[derive(Debug, Default)]
pub(crate) struct EmulatorFeatures {
    pub level: Level,
    pub cores: HashMap<String, Level>,
}

#[derive(Debug, Default)]
pub(crate) struct Features {
    /// What ES shows in its global game settings.
    pub global: Vec<Feature>,
    pub emulators: HashMap<String, EmulatorFeatures>,
    pub error: Option<String>,
}

impl Features {
    pub fn parse(xml: &str) -> Self {
        let parsed = xml::parse(xml);
        let Some(root) = parsed.root else {
            return Self {
                error: parsed.error,
                ..Self::default()
            };
        };
        let shared: HashMap<String, Feature> = root
            .child("sharedFeatures")
            .map(|s| {
                s.children_named("feature")
                    .filter_map(feature)
                    .map(|f| (f.key.clone(), f))
                    .collect()
            })
            .unwrap_or_default();
        let mut global = root
            .child("globalFeatures")
            .map(|g| level(g, &shared).features)
            .unwrap_or_default();
        // ES draws it in its global menu without the file listing it: after rewind there.
        if !global.iter().any(|f| f.key == INCREMENTAL) {
            let at = global
                .iter()
                .position(|f| f.key == "rewind")
                .map_or(global.len(), |i| i + 1);
            global.insert(at, incremental());
        }
        let emulators = root
            .children_named("emulator")
            .filter_map(|em| {
                let cores = em
                    .child("cores")
                    .map(|c| {
                        c.children_named("core")
                            .filter_map(|core| {
                                Some((core.attr("name")?.to_owned(), level(core, &shared)))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some((
                    em.attr("name")?.to_owned(),
                    EmulatorFeatures {
                        level: level(em, &shared),
                        cores,
                    },
                ))
            })
            .collect();
        Self {
            global,
            emulators,
            error: parsed.error,
        }
    }

    /// The options for a system run with `emulator` and `core`, as ES lists them: the
    /// emulator's, then its system-specific ones, then the core's and the core's for the system.
    /// A key appears once, where it first does.
    pub fn for_system(&self, system: &str, emulator: &str, core: Option<&str>) -> Applicable<'_> {
        let mut out = Applicable::default();
        let Some(em) = self.emulators.get(emulator) else {
            return out;
        };
        out.add(&em.level, system);
        if let Some(core) = core.and_then(|c| em.cores.get(c)) {
            out.add(core, system);
        }
        out
    }
}

#[derive(Debug, Default)]
pub(crate) struct Applicable<'a> {
    pub features: Vec<&'a Feature>,
    /// What the emulator supports: `netplay`, `cheevos`, `padtokeyboard`.
    pub flags: Vec<String>,
}

impl<'a> Applicable<'a> {
    fn add(&mut self, level: &'a Level, system: &str) {
        self.take(level);
        if let Some(s) = level.systems.get(system) {
            self.take(s);
        }
    }

    fn take(&mut self, level: &'a Level) {
        for flag in &level.flags {
            if !self.flags.contains(flag) {
                self.flags.push(flag.clone());
            }
        }
        for f in &level.features {
            if !self.features.iter().any(|k| k.key == f.key) {
                self.features.push(f);
            }
        }
    }
}

/// Batocera's `incrementalsavestates`: each save state in a new slot (its default) or over the
/// last. ES adds it to its menus itself; `es_features.cfg` does not list it.
const INCREMENTAL: &str = "incrementalsavestates";

fn incremental() -> Feature {
    Feature {
        key: INCREMENTAL.to_owned(),
        label: "INCREMENTAL SAVE STATES".to_owned(),
        group: None,
        submenu: None,
        description: Some(
            "On: each save state goes in a new slot. Off: saving replaces the one in the slot."
                .to_owned(),
        ),
        order: None,
        choices: vec![
            ("1".to_owned(), "On".to_owned()),
            ("0".to_owned(), "Off".to_owned()),
        ],
        preset: None,
    }
}

fn level(el: &Element, shared: &HashMap<String, Feature>) -> Level {
    let mut features = Vec::new();
    // In document order: shared references and inline features interleave.
    for child in &el.children {
        match child.name.as_str() {
            "sharedFeature" => {
                if let Some(f) = child.attr("value").and_then(|v| shared.get(v)) {
                    features.push(f.clone());
                }
            }
            "feature" => features.extend(feature(child)),
            _ => {}
        }
    }
    // Beside auto save, as ES draws it, wherever an emulator saves states.
    if let Some(at) = features.iter().position(|f| f.key == "autosave")
        && !features.iter().any(|f| f.key == INCREMENTAL)
    {
        features.insert(at + 1, incremental());
    }
    let systems = el
        .child("systems")
        .map(|s| {
            s.children_named("system")
                .filter_map(|sys| Some((sys.attr("name")?.to_owned(), level(sys, shared))))
                .collect()
        })
        .unwrap_or_default();
    Level {
        flags: el
            .attr("features")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
            .collect(),
        features,
        systems,
    }
}

fn feature(el: &Element) -> Option<Feature> {
    let key = el.attr("value").filter(|v| !v.is_empty())?.to_owned();
    let text = |name: &str| {
        el.attr(name)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let numbers: Vec<f64> = el
        .attr("preset-parameters")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let preset = match el.attr("preset") {
        Some("videomodes") => Some(Preset::VideoModes),
        Some("shaderset") => Some(Preset::ShaderSet),
        Some("bezel") => Some(Preset::Bezel),
        Some("slider" | "sliderauto") => match numbers[..] {
            [min, max, step, ref rest @ ..] if min < max && step > 0.0 => Some(Preset::Slider {
                min,
                max,
                step,
                default: rest.first().copied(),
            }),
            _ => return None,
        },
        Some("switch" | "switchon" | "switchoff" | "switchauto") => Some(Preset::Switch),
        Some("input" | "image") => Some(Preset::Text),
        // Anything newer than this reader: left out rather than offered wrongly.
        Some(_) => return None,
        None => None,
    };
    let choices: Vec<(String, String)> = el
        .children_named("choice")
        .filter_map(|c| Some((c.attr("value")?.to_owned(), c.attr("name")?.to_owned())))
        .collect();
    if preset.is_none() && choices.is_empty() {
        return None;
    }
    Some(Feature {
        label: text("name").unwrap_or_else(|| key.clone()),
        group: text("group"),
        submenu: text("submenu"),
        description: text("description"),
        order: el.attr("order").and_then(|o| o.parse().ok()),
        choices,
        preset,
        key,
    })
}
