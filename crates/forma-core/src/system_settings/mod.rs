//! Batocera's system settings, as EmulationStation's System settings menu offers them: what
//! the box is, its display, audio, language and region, name, security and storage.
//!
//! Values live in `batocera.conf` and are written through `batocera-settings-set`, as ES does.
//! Some also need Batocera's own tool to take effect now (the audio output, the time zone, the
//! keyboard layout). Display and name settings apply at the next boot: Batocera copies them into
//! its boot configuration on a clean shutdown, and its start script applies them before Forma
//! takes over the screen. Each setting says whether it waits for a reboot.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host::Shell;
use crate::options::{OptionChoice, conf, video_mode_label};

const VERSION_FILE: &str = "/usr/share/batocera/batocera.version";
const ZONEINFO: &str = "/usr/share/zoneinfo";
/// The regions of the IANA database; the rest of `zoneinfo` is aliases and legacy names.
const ZONE_REGIONS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Arctic",
    "Asia",
    "Atlantic",
    "Australia",
    "Europe",
    "Indian",
    "Pacific",
];

/// The languages ES offers (it has translations for these), by Batocera's `system.language`.
const LANGUAGES: &[(&str, &str)] = &[
    ("en_US", "English (US)"),
    ("en_GB", "English (UK)"),
    ("ar_YE", "العربية"),
    ("ca_ES", "Català"),
    ("cs_CZ", "Čeština"),
    ("cy_GB", "Cymraeg"),
    ("de_DE", "Deutsch"),
    ("el_GR", "Ελληνικά"),
    ("es_ES", "Español"),
    ("es_MX", "Español (México)"),
    ("eu_ES", "Euskara"),
    ("fi_FI", "Suomi"),
    ("fr_FR", "Français"),
    ("gl_ES", "Galego"),
    ("he_IL", "עברית"),
    ("hu_HU", "Magyar"),
    ("id_ID", "Bahasa Indonesia"),
    ("it_IT", "Italiano"),
    ("ja_JP", "日本語"),
    ("ko_KR", "한국어"),
    ("nb_NO", "Norsk bokmål"),
    ("nl_NL", "Nederlands"),
    ("nn_NO", "Norsk nynorsk"),
    ("oc_FR", "Occitan"),
    ("pl_PL", "Polski"),
    ("pt_BR", "Português (Brasil)"),
    ("pt_PT", "Português (Portugal)"),
    ("ro_RO", "Română"),
    ("ru_RU", "Русский"),
    ("sk_SK", "Slovenčina"),
    ("sv_SE", "Svenska"),
    ("tr_TR", "Türkçe"),
    ("uk_UA", "Українська"),
    ("vi_VN", "Tiếng Việt"),
    ("zh_CN", "简体中文"),
    ("zh_TW", "正體中文"),
];

const ROTATIONS: &[(&str, &str)] = &[
    ("0", "Normal"),
    ("1", "90° clockwise"),
    ("2", "Upside down"),
    ("3", "90° anticlockwise"),
];

const ON_OFF: &[(&str, &str)] = &[("1", "On"), ("0", "Off")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Choices from a fixed list.
    Fixed(&'static [(&'static str, &'static str)]),
    VideoModes,
    VideoOutputs,
    AudioOutputs,
    Timezones,
    KeyboardLayouts,
    Hostname,
    /// Not a `batocera.conf` key: Batocera's boot configuration, through `batocera-config`.
    Storage,
}

struct Def {
    key: &'static str,
    label: &'static str,
    section: &'static str,
    description: &'static str,
    kind: Kind,
    /// Waits for a reboot (`restart` for Forma's own restart).
    applies: &'static str,
}

const DEFS: &[Def] = &[
    Def {
        key: "es.resolution",
        label: "Video mode",
        section: "Display",
        description: "The display's resolution and refresh rate. Unset: the highest it supports.",
        kind: Kind::VideoModes,
        applies: "reboot",
    },
    Def {
        key: "global.videooutput",
        label: "Video output",
        section: "Display",
        description: "The screen to use when several are connected.",
        kind: Kind::VideoOutputs,
        applies: "reboot",
    },
    Def {
        key: "display.rotate",
        label: "Screen rotation",
        section: "Display",
        description: "For a screen mounted on its side or upside down.",
        kind: Kind::Fixed(ROTATIONS),
        applies: "reboot",
    },
    Def {
        key: "audio.device",
        label: "Audio output",
        section: "Audio",
        description: "Where the sound goes.",
        kind: Kind::AudioOutputs,
        applies: "now",
    },
    Def {
        key: "system.language",
        label: "Language",
        section: "Language & region",
        description: "For EmulationStation and the emulators that follow it.",
        kind: Kind::Fixed(LANGUAGES),
        applies: "restart",
    },
    Def {
        key: "system.timezone",
        label: "Time zone",
        section: "Language & region",
        description: "For the clock and dates.",
        kind: Kind::Timezones,
        applies: "now",
    },
    Def {
        key: "system.kblayout",
        label: "Keyboard layout",
        section: "Language & region",
        description: "For a keyboard plugged into the box.",
        kind: Kind::KeyboardLayouts,
        applies: "now",
    },
    Def {
        key: "system.hostname",
        label: "Name on the network",
        section: "System",
        description: "Letters, digits and hyphens, up to 63.",
        kind: Kind::Hostname,
        applies: "reboot",
    },
    Def {
        key: "system.security.enabled",
        label: "Security",
        section: "System",
        description: "Protects the network shares and SSH with the root password.",
        kind: Kind::Fixed(ON_OFF),
        applies: "reboot",
    },
    Def {
        key: "updates.enabled",
        label: "Check for updates",
        section: "System",
        description: "Look for new Batocera versions.",
        kind: Kind::Fixed(ON_OFF),
        applies: "now",
    },
    Def {
        key: "storage",
        label: "Storage device",
        section: "Storage",
        description: "Where games, saves and settings are kept.",
        kind: Kind::Storage,
        applies: "reboot",
    },
];

/// One setting as a theme sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSetting {
    pub key: &'static str,
    pub label: &'static str,
    pub section: &'static str,
    pub description: &'static str,
    /// `choice` (one of `choices`) or `text`.
    pub kind: &'static str,
    pub choices: Vec<OptionChoice>,
    /// `None` when unset: Batocera's default applies.
    pub value: Option<String>,
    /// `now`, `restart` (Forma's) or `reboot`.
    pub applies: &'static str,
}

/// A line of the box's information, e.g. `Model` and `Raspberry Pi 5 Model B`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InfoItem {
    pub label: String,
    pub value: String,
}

pub struct SystemSettings {
    shell: Box<dyn Shell>,
    conf: PathBuf,
    zoneinfo: PathBuf,
    version_file: PathBuf,
}

impl SystemSettings {
    pub fn new(shell: Box<dyn Shell>, conf: &Path) -> Self {
        Self {
            shell,
            conf: conf.to_path_buf(),
            zoneinfo: ZONEINFO.into(),
            version_file: VERSION_FILE.into(),
        }
    }

    /// For tests and the mock: where the time zones and the version file are.
    pub fn with_files(mut self, zoneinfo: &Path, version_file: &Path) -> Self {
        self.zoneinfo = zoneinfo.to_path_buf();
        self.version_file = version_file.to_path_buf();
        self
    }

    /// What the box is: Batocera's version, then `batocera-info`'s lines that have a value.
    pub fn info(&self) -> Vec<InfoItem> {
        let version = std::fs::read_to_string(&self.version_file).unwrap_or_default();
        let version = version.trim();
        let mut items = Vec::new();
        if !version.is_empty() {
            items.push(InfoItem {
                label: "Batocera".into(),
                value: version.to_owned(),
            });
        }
        let info = self.shell.run("batocera-info", &[]).unwrap_or_default();
        items.extend(info.lines().filter_map(|line| {
            let (label, value) = line.split_once(':')?;
            let (label, value) = (label.trim(), value.trim().replace('_', " "));
            // Empty, or a template with nothing filled in (`.Hz`): a value starts with a letter
            // or a digit.
            let filled = value.chars().next().is_some_and(char::is_alphanumeric);
            (!label.is_empty() && filled).then(|| InfoItem {
                label: label.to_owned(),
                value,
            })
        }));
        items
    }

    pub fn get(&self) -> Vec<SystemSetting> {
        let conf = conf::read(&self.conf);
        DEFS.iter()
            .map(|def| SystemSetting {
                key: def.key,
                label: def.label,
                section: def.section,
                description: def.description,
                kind: if def.kind == Kind::Hostname {
                    "text"
                } else {
                    "choice"
                },
                choices: self.choices(def.kind),
                value: if def.kind == Kind::Storage {
                    self.current_storage()
                } else {
                    conf.get(def.key).cloned()
                },
                applies: def.applies,
            })
            .collect()
    }

    /// Sets one, checked against its choices; `None` unsets it (Batocera's default applies).
    pub fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
        let def = DEFS
            .iter()
            .find(|d| d.key == key)
            .ok_or_else(|| Error::Invalid(format!("`{key}` is not a system setting")))?;
        let value = value.map(str::trim).filter(|v| !v.is_empty());
        let Some(value) = value else {
            if def.kind == Kind::Storage {
                return Err(Error::Invalid("choose a storage device".into()));
            }
            return conf::clear(&self.conf, def.key);
        };
        match def.kind {
            Kind::Hostname => {
                let ok = value.len() <= 63
                    && !value.starts_with('-')
                    && !value.ends_with('-')
                    && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                if !ok {
                    return Err(Error::Invalid(
                        "a name on the network is letters, digits and hyphens, up to 63".into(),
                    ));
                }
            }
            kind if !self.choices(kind).iter().any(|c| c.value == value) => {
                return Err(Error::Invalid(format!(
                    "`{value}` is not one of the choices for `{key}`"
                )));
            }
            _ => {}
        }
        match def.kind {
            // `DEV <uuid>` is two arguments.
            Kind::Storage => {
                let args: Vec<&str> = std::iter::once("storage")
                    .chain(value.split_whitespace())
                    .collect();
                return self.shell.run("batocera-config", &args).map(drop);
            }
            Kind::AudioOutputs => {
                self.shell.run("batocera-audio", &["set", value])?;
            }
            Kind::Timezones => {
                self.shell.run("batocera-timezone", &["set", value])?;
            }
            Kind::KeyboardLayouts => {
                let variant = conf::read(&self.conf)
                    .get("system.kbvariant")
                    .cloned()
                    .unwrap_or_default();
                self.shell
                    .run("batocera-keyboard", &["set", value, &variant])?;
            }
            _ => {}
        }
        self.shell
            .run("batocera-settings-set", &[def.key, value])
            .map(drop)
    }

    fn choices(&self, kind: Kind) -> Vec<OptionChoice> {
        let pairs = |out: String, sep: &dyn Fn(&str) -> Option<(String, String)>| {
            out.lines()
                .filter_map(sep)
                .filter(|(v, _)| !v.is_empty())
                .map(|(value, label)| OptionChoice {
                    label: if label.is_empty() {
                        value.clone()
                    } else {
                        label
                    },
                    value,
                })
                .collect::<Vec<_>>()
        };
        let run = |program: &str, args: &[&str]| self.shell.run(program, args).unwrap_or_default();
        match kind {
            Kind::Fixed(list) => list
                .iter()
                .map(|(value, label)| OptionChoice {
                    value: (*value).to_owned(),
                    label: (*label).to_owned(),
                })
                .collect(),
            // `1920x1080.60000:1920x1080 60 Hz`
            Kind::VideoModes => pairs(run("batocera-resolution", &["listModes"]), &|l| {
                let (v, label) = l.split_once(':')?;
                Some((v.trim().to_owned(), video_mode_label(label)))
            }),
            Kind::VideoOutputs => pairs(run("batocera-resolution", &["listOutputs"]), &|l| {
                Some((l.trim().to_owned(), String::new()))
            }),
            // `<device>\t<description>`, starting with `auto`.
            Kind::AudioOutputs => pairs(run("batocera-audio", &["list"]), &|l| {
                let (v, label) = l.split_once('\t').unwrap_or((l, ""));
                let label = if v.trim() == "auto" {
                    "Automatic"
                } else {
                    label
                };
                Some((v.trim().to_owned(), label.trim().to_owned()))
            }),
            // `pt Portuguese`
            Kind::KeyboardLayouts => pairs(run("batocera-keyboard", &["list-layouts"]), &|l| {
                let (v, label) = l.split_once(' ').unwrap_or((l, ""));
                Some((v.trim().to_owned(), label.trim().to_owned()))
            }),
            // `INTERNAL INTERNAL`, `DEV <uuid> SHARE - 10.9T`: the value is what `current` says.
            Kind::Storage => pairs(run("batocera-config", &["storage", "list"]), &|l| {
                let mut words = l.split_whitespace();
                let first = words.next()?;
                let value = if first == "DEV" {
                    format!("DEV {}", words.next()?)
                } else {
                    first.to_owned()
                };
                let rest: Vec<&str> = words.collect();
                Some((value, rest.join(" ")))
            }),
            Kind::Timezones => self.timezones(),
            Kind::Hostname => Vec::new(),
        }
    }

    fn current_storage(&self) -> Option<String> {
        let out = self
            .shell
            .run("batocera-config", &["storage", "current"])
            .ok()?;
        let words: Vec<&str> = out.split_whitespace().collect();
        match words[..] {
            ["DEV", uuid, ..] => Some(format!("DEV {uuid}")),
            [first, ..] => Some(first.to_owned()),
            [] => None,
        }
    }

    /// `Region/City` names from the time zone database, and `UTC`.
    fn timezones(&self) -> Vec<OptionChoice> {
        fn walk(dir: &Path, name: &str, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let Ok(file) = e.file_name().into_string() else {
                    continue;
                };
                let full = format!("{name}/{file}");
                if e.path().is_dir() {
                    walk(&e.path(), &full, out);
                } else {
                    out.push(full);
                }
            }
        }
        let mut zones = Vec::new();
        for region in ZONE_REGIONS {
            walk(&self.zoneinfo.join(region), region, &mut zones);
        }
        zones.sort();
        std::iter::once("UTC".to_owned())
            .chain(zones)
            .map(|z| OptionChoice {
                label: z.replace('_', " "),
                value: z,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
