//! Themes: discovering them, and deciding whether each one may load.
//!
//! A theme is a folder with a `theme.json` manifest and a web app. Bundled themes ship with
//! Forma; users install their own by copying a folder into `/userdata/themes-forma/`. A theme
//! that cannot load (bad manifest, incompatible SDK, a permission Forma does not know) is still
//! listed, with the reason, so the user sees why it is missing rather than nothing at all.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::SDK_API_VERSION;
use crate::paths::Paths;

/// Privileged capabilities a theme must declare in `permissions` before using them.
pub const PERMISSIONS: &[(&str, &str)] = &[
    (
        "system.power",
        "quit Forma, restart or shut down the machine",
    ),
    (
        "settings.system",
        "change Forma settings beyond the theme's own",
    ),
    (
        "music.manage",
        "add and remove web radios in the music folder",
    ),
    (
        "settings.batocera",
        "change Batocera's RetroAchievements and netplay settings, and log in to RetroAchievements",
    ),
    (
        "settings.emulators",
        "change Batocera's emulator, core and game options",
    ),
    (
        "system.settings",
        "see and change Batocera's system settings: display, audio, language, region, name, security and storage",
    ),
    (
        "store.manage",
        "install and remove games, decorations and music from Batocera's content downloader",
    ),
    (
        "system.network",
        "see and change the machine's Wi-Fi and Ethernet settings",
    ),
    (
        "system.controllers",
        "see, pair, connect and forget Bluetooth controllers",
    ),
    ("system.update", "check for and install Batocera updates"),
    (
        "network",
        "load content from the internet (the origins listed in `remote`)",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThemeManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default = "default_entry")]
    pub entry: String,
    /// Caret range of the SDK API the theme needs, e.g. `^1`.
    pub sdk: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The view the theme suggests booting into; the user's own choice still wins.
    pub hierarchy: Option<String>,
    #[serde(default)]
    pub resolutions: Vec<String>,
    /// `https://` origins the theme may load from. Requires the `network` permission.
    #[serde(default)]
    pub remote: Vec<String>,
    /// What the user can set from Forma's menu (Start → Interface), for the theme to follow.
    #[serde(default)]
    pub settings: Vec<ThemeSetting>,
    /// The only views (by id) the theme shows the library in: Forma's views list and its
    /// "Start on" setting keep to them. Absent, every view.
    pub views: Option<Vec<String>>,
    /// What the theme draws itself, in its own style, instead of Forma's shell drawing it:
    /// `nowPlaying` (the song that just started, `music:song`), `status` (controllers and the
    /// connection, `status:changed`) and `achievements` (what a game just earned,
    /// `ra:unlocked`). What happens stays Forma's.
    #[serde(default)]
    pub draws: Vec<String>,
    /// The theme's logo (an SVG, PNG or WebP inside its folder), which Forma's menus and cover
    /// show in place of its name.
    pub logo: Option<String>,
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema: Option<String>,
}

impl ThemeManifest {
    /// A manifest for a theme that has none, so Forma's menu still has a name to show.
    pub fn unnamed(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: id.to_owned(),
            version: String::new(),
            entry: default_entry(),
            sdk: String::new(),
            permissions: Vec::new(),
            hierarchy: None,
            resolutions: Vec::new(),
            remote: Vec::new(),
            settings: Vec::new(),
            views: None,
            draws: Vec::new(),
            logo: None,
            schema: None,
        }
    }
}

/// One of a theme's own settings, which Forma's menu shows and sets. The theme reads it with
/// `forma.settings.get(key)` and hears of changes through the `settings:changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThemeSetting {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A heading the menu groups settings under, e.g. `Look`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(rename = "type")]
    pub kind: SettingKind,
    /// For a `choice`: the values it can take, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<SettingChoice>,
    pub default: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingKind {
    /// On or off: `true` or `false`.
    Toggle,
    /// One of `choices`, by its value (a string).
    Choice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingChoice {
    pub value: String,
    pub label: String,
}

impl ThemeSetting {
    /// Whether `value` is one this setting can take.
    pub fn accepts(&self, value: &serde_json::Value) -> bool {
        match self.kind {
            SettingKind::Toggle => value.is_boolean(),
            SettingKind::Choice => value
                .as_str()
                .is_some_and(|v| self.choices.iter().any(|c| c.value == v)),
        }
    }
}

fn default_entry() -> String {
    "index.html".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeOrigin {
    Bundled,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    /// The folder name when the manifest cannot be read.
    pub id: String,
    pub dir: PathBuf,
    pub origin: ThemeOrigin,
    pub manifest: Option<ThemeManifest>,
    /// Why the theme cannot load; `None` means it can.
    pub unavailable: Option<String>,
    /// A user theme standing in for the bundled theme with its id (it is newer).
    pub overrides: bool,
}

impl Theme {
    pub fn is_available(&self) -> bool {
        self.unavailable.is_none()
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.manifest
            .as_ref()
            .is_some_and(|m| m.permissions.iter().any(|p| p == permission))
    }
}

/// Where user themes are installed.
pub fn user_themes_dir(paths: &Paths) -> PathBuf {
    paths.userdata.join("themes-forma")
}

/// Every theme in `bundled` folders, then in the user's themes folder. A user theme with a
/// bundled theme's id stands in for it only when its version is newer (an update installed
/// without a new Forma); the bundled one is then listed as replaced, and comes back if the
/// newer one fails ([`revert_override`]). An older or equal one is listed as unavailable: an
/// old copy must not hide the theme Forma ships.
pub fn discover(bundled: &[PathBuf], paths: &Paths) -> Vec<Theme> {
    let mut themes: Vec<Theme> = bundled
        .iter()
        .map(|dir| load(dir, ThemeOrigin::Bundled))
        .collect();

    let mut user_dirs: Vec<PathBuf> = std::fs::read_dir(user_themes_dir(paths))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    user_dirs.sort();
    for dir in user_dirs {
        let mut theme = load(&dir, ThemeOrigin::User);
        if theme.is_available()
            && let Some(other) = themes
                .iter_mut()
                .find(|t| t.id == theme.id && t.is_available())
        {
            let version = |t: &Theme| t.manifest.as_ref().map(|m| m.version.clone());
            match (other.origin, version(&theme), version(other)) {
                (ThemeOrigin::Bundled, Some(mine), Some(theirs)) if newer(&mine, &theirs) => {
                    other.unavailable = Some(format!("replaced by version {mine}, installed"));
                    theme.overrides = true;
                }
                (ThemeOrigin::Bundled, Some(mine), Some(theirs)) => {
                    theme.unavailable = Some(format!(
                        "version {mine} is not newer than the built-in {theirs}"
                    ));
                }
                _ => {
                    theme.unavailable = Some(format!(
                        "id `{}` is already used by another theme",
                        theme.id
                    ));
                }
            }
        }
        themes.push(theme);
    }
    themes
}

/// A user theme that stood in for a bundled one failed: it is set aside and the bundled one
/// is back. `false` when `id` was not an override.
pub fn revert_override(themes: &mut [Theme], id: &str) -> bool {
    let Some(user) = themes.iter_mut().find(|t| t.id == id && t.overrides) else {
        return false;
    };
    user.overrides = false;
    user.unavailable = Some("did not start; using the built-in version".into());
    if let Some(bundled) = themes
        .iter_mut()
        .find(|t| t.id == id && t.origin == ThemeOrigin::Bundled)
    {
        bundled.unavailable = None;
    }
    true
}

/// `a` is a later version than `b`: `major.minor.patch`, a prerelease (`0.3.1-dev.2`) coming
/// before its release. Anything unreadable is never newer.
pub fn newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> Option<([u64; 3], Option<&str>)> {
        let (core, pre) = match v.trim().split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v.trim(), None),
        };
        let mut n = core.split('.').map(|p| p.parse::<u64>().ok());
        let out = [n.next()??, n.next()??, n.next()??];
        n.next().is_none().then_some((out, pre))
    }
    let (Some((a, a_pre)), Some((b, b_pre))) = (parse(a), parse(b)) else {
        return false;
    };
    if a != b {
        return a > b;
    }
    match (a_pre, b_pre) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

fn load(dir: &Path, origin: ThemeOrigin) -> Theme {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut theme = Theme {
        id: folder,
        dir: dir.to_path_buf(),
        origin,
        manifest: None,
        unavailable: None,
        overrides: false,
    };
    let manifest = std::fs::read_to_string(dir.join("theme.json"))
        .map_err(|e| format!("theme.json: {e}"))
        .and_then(|json| {
            serde_json::from_str::<ThemeManifest>(&json).map_err(|e| format!("theme.json: {e}"))
        });
    match manifest {
        Ok(manifest) => {
            theme.id.clone_from(&manifest.id);
            theme.unavailable = check(&manifest, dir).err();
            theme.manifest = Some(manifest);
        }
        Err(e) => theme.unavailable = Some(e),
    }
    theme
}

fn check(m: &ThemeManifest, dir: &Path) -> Result<(), String> {
    if m.id.is_empty()
        || !m
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "id `{}` must be lower-case letters, digits and dashes",
            m.id
        ));
    }
    if !satisfies(&m.sdk, SDK_API_VERSION) {
        return Err(format!(
            "needs SDK {}, but this Forma provides {SDK_API_VERSION}",
            m.sdk
        ));
    }
    if let Some(unknown) = m
        .permissions
        .iter()
        .find(|p| !PERMISSIONS.iter().any(|(known, _)| known == p))
    {
        return Err(format!("asks for unknown permission `{unknown}`"));
    }
    if !m.remote.is_empty() && !m.permissions.iter().any(|p| p == "network") {
        return Err("lists `remote` origins without the `network` permission".to_owned());
    }
    if let Some(bad) = m.remote.iter().find(|o| !is_https_origin(o)) {
        return Err(format!(
            "remote origin `{bad}` must look like https://example.com"
        ));
    }
    check_settings(&m.settings)?;
    let entry = Path::new(&m.entry);
    if entry.is_absolute()
        || entry
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "entry `{}` must be inside the theme folder",
            m.entry
        ));
    }
    if !dir.join(entry).is_file() {
        return Err(format!("entry `{}` does not exist", m.entry));
    }
    if let Some(logo) = &m.logo {
        let path = Path::new(logo);
        let image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "svg" | "png" | "webp"));
        if !image {
            return Err(format!("logo `{logo}` must be an .svg, .png or .webp"));
        }
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || !dir.join(path).is_file()
        {
            return Err(format!(
                "logo `{logo}` must be a file inside the theme folder"
            ));
        }
    }
    Ok(())
}

fn check_settings(settings: &[ThemeSetting]) -> Result<(), String> {
    for (i, s) in settings.iter().enumerate() {
        if s.key.is_empty() || s.key.len() > 100 {
            return Err("a setting's key must be 1–100 characters".to_owned());
        }
        if settings[..i].iter().any(|o| o.key == s.key) {
            return Err(format!("setting `{}` is listed twice", s.key));
        }
        match s.kind {
            SettingKind::Choice if s.choices.is_empty() => {
                return Err(format!("setting `{}` has no choices", s.key));
            }
            SettingKind::Toggle if !s.choices.is_empty() => {
                return Err(format!("setting `{}` is a toggle: no choices", s.key));
            }
            _ => {}
        }
        if !s.accepts(&s.default) {
            return Err(format!("setting `{}` has a default it cannot take", s.key));
        }
    }
    Ok(())
}

fn is_https_origin(origin: &str) -> bool {
    origin.strip_prefix("https://").is_some_and(|host| {
        !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
    })
}

/// Whether `version` satisfies a caret `range` (`^1`, `^1.2`, `^1.2.3`). The same rule as
/// `satisfies` in `@ramraiders/forma-sdk`, so host and theme agree on what compatible means.
pub fn satisfies(range: &str, version: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> { s.split('.').map(|p| p.parse().ok()).collect() };
    let (Some(want), Some(have)) = (
        range.trim().strip_prefix('^').and_then(parse),
        parse(version.trim()),
    ) else {
        return false;
    };
    if want.is_empty() || want.len() > 3 || have.len() != 3 {
        return false;
    }
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    if at(&want, 0) != have[0] {
        return false;
    }
    (at(&want, 1), at(&want, 2)) <= (have[1], have[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_copy;

    fn write_theme(dir: &Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("theme.json"), manifest).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>").unwrap();
    }

    fn manifest(extra: &str) -> String {
        format!(r#"{{ "id": "neon", "name": "Neon", "version": "1.0.0", "sdk": "^1"{extra} }}"#)
    }

    fn reason(extra: &str) -> Option<String> {
        let dir = tempfile::tempdir().unwrap();
        write_theme(dir.path(), &manifest(extra));
        load(dir.path(), ThemeOrigin::User).unavailable
    }

    #[test]
    fn a_valid_theme_is_available() {
        assert_eq!(reason(r#", "permissions": ["system.power"]"#), None);
        assert_eq!(
            reason(r#", "permissions": ["network"], "remote": ["https://cdn.example.com"]"#),
            None
        );
    }

    #[test]
    fn a_logo_must_be_an_image_in_the_theme_folder() {
        let with_logo = |file: Option<&str>, logo: &str| {
            let dir = tempfile::tempdir().unwrap();
            write_theme(dir.path(), &manifest(&format!(r#", "logo": "{logo}""#)));
            if let Some(file) = file {
                std::fs::write(dir.path().join(file), "<svg/>").unwrap();
            }
            load(dir.path(), ThemeOrigin::User).unavailable
        };
        assert_eq!(with_logo(Some("logo.svg"), "logo.svg"), None);
        assert!(
            with_logo(None, "logo.svg")
                .unwrap()
                .contains("inside the theme folder")
        );
        assert!(
            with_logo(None, "../logo.svg")
                .unwrap()
                .contains("inside the theme folder")
        );
        assert!(
            with_logo(Some("logo.gif"), "logo.gif")
                .unwrap()
                .contains(".svg, .png or .webp")
        );
    }

    #[test]
    fn says_why_a_theme_cannot_load() {
        let r = |extra: &str| reason(extra).unwrap();
        assert!(r(r#", "entry": "missing.html""#).contains("does not exist"));
        assert!(r(r#", "entry": "../escape.html""#).contains("inside the theme folder"));
        assert!(r(r#", "permissions": ["root"]"#).contains("unknown permission `root`"));
        assert!(r(r#", "remote": ["https://x.com"]"#).contains("without the `network`"));
        assert!(
            r(r#", "permissions": ["network"], "remote": ["http://x.com"]"#).contains("https://")
        );
        assert!(r(r#", "colour": "red""#).contains("colour"));

        let dir = tempfile::tempdir().unwrap();
        write_theme(
            dir.path(),
            r#"{ "id": "old", "name": "Old", "version": "1", "sdk": "^2" }"#,
        );
        let theme = load(dir.path(), ThemeOrigin::User);
        assert!(theme.unavailable.unwrap().contains("needs SDK ^2"));
        assert!(theme.manifest.is_some(), "the manifest is still shown");
    }

    #[test]
    fn a_theme_declares_settings_for_formas_menu() {
        let settings = r#", "settings": [
            { "key": "board", "label": "Board", "group": "Look", "type": "choice",
              "choices": [{ "value": "on", "label": "On" }, { "value": "off", "label": "Off" }],
              "default": "on" },
            { "key": "motion", "label": "Animations", "type": "toggle", "default": true }
        ]"#;
        assert_eq!(reason(settings), None);
        let dir = tempfile::tempdir().unwrap();
        write_theme(dir.path(), &manifest(settings));
        let m = load(dir.path(), ThemeOrigin::User).manifest.unwrap();
        let [board, motion] = &m.settings[..] else {
            panic!("two settings")
        };
        assert!(board.accepts(&serde_json::json!("off")));
        assert!(!board.accepts(&serde_json::json!("dim")));
        assert!(motion.accepts(&serde_json::json!(false)));
        assert!(!motion.accepts(&serde_json::json!("false")));

        let r = |extra: &str| reason(extra).unwrap();
        assert!(
            r(r#", "settings": [{ "key": "a", "label": "A", "type": "choice", "choices": [], "default": "x" }]"#)
                .contains("no choices")
        );
        assert!(
            r(r#", "settings": [{ "key": "a", "label": "A", "type": "toggle", "default": "on" }]"#)
                .contains("default it cannot take")
        );
        assert!(
            r(
                r#", "settings": [{ "key": "a", "label": "A", "type": "toggle", "default": true },
                                { "key": "a", "label": "B", "type": "toggle", "default": true }]"#
            )
            .contains("listed twice")
        );
        assert!(
            r(r#", "settings": [{ "key": "a", "label": "A", "type": "slider", "default": 1 }]"#)
                .contains("theme.json")
        );
    }

    #[test]
    fn a_folder_without_a_manifest_is_listed_under_its_folder_name() {
        let dir = tempfile::tempdir().unwrap();
        let theme_dir = dir.path().join("half-copied");
        std::fs::create_dir(&theme_dir).unwrap();
        let theme = load(&theme_dir, ThemeOrigin::User);
        assert_eq!(theme.id, "half-copied");
        assert!(theme.unavailable.unwrap().starts_with("theme.json"));
    }

    #[test]
    fn discovers_bundled_then_user_themes_and_refuses_a_clashing_id() {
        let (_dir, paths) = fixture_copy();
        let bundled = paths.userdata.join("bundled/default");
        write_theme(
            &bundled,
            r#"{ "id": "default", "name": "Default", "version": "1", "sdk": "^1" }"#,
        );
        let users = user_themes_dir(&paths);
        write_theme(&users.join("neon"), &manifest(""));
        write_theme(
            &users.join("fake-default"),
            r#"{ "id": "default", "name": "Mine", "version": "1", "sdk": "^1" }"#,
        );

        let themes = discover(&[bundled], &paths);
        let summary: Vec<(&str, ThemeOrigin, bool)> = themes
            .iter()
            .map(|t| (t.id.as_str(), t.origin, t.is_available()))
            .collect();
        assert_eq!(
            summary,
            [
                ("default", ThemeOrigin::Bundled, true),
                ("default", ThemeOrigin::User, false),
                ("neon", ThemeOrigin::User, true),
            ]
        );
    }

    #[test]
    fn a_newer_user_copy_of_a_bundled_theme_stands_in_for_it_and_can_be_set_aside() {
        let (_dir, paths) = fixture_copy();
        let bundled = paths.userdata.join("bundled/cyber");
        write_theme(
            &bundled,
            r#"{ "id": "cyber", "name": "Cyber", "version": "0.3.0", "sdk": "^1" }"#,
        );
        let users = user_themes_dir(&paths);
        write_theme(
            &users.join("cyber"),
            r#"{ "id": "cyber", "name": "Cyber", "version": "0.3.1-dev", "sdk": "^1" }"#,
        );
        let mut themes = discover(std::slice::from_ref(&bundled), &paths);
        let summary = |themes: &[Theme]| -> Vec<(ThemeOrigin, bool)> {
            themes
                .iter()
                .map(|t| (t.origin, t.is_available()))
                .collect()
        };
        assert_eq!(
            summary(&themes),
            [(ThemeOrigin::Bundled, false), (ThemeOrigin::User, true)]
        );
        assert!(themes[1].overrides);

        assert!(revert_override(&mut themes, "cyber"));
        assert_eq!(
            summary(&themes),
            [(ThemeOrigin::Bundled, true), (ThemeOrigin::User, false)]
        );
        assert!(!revert_override(&mut themes, "cyber"));

        // The same version, or an older one, does not replace it.
        write_theme(
            &users.join("cyber"),
            r#"{ "id": "cyber", "name": "Cyber", "version": "0.3.0", "sdk": "^1" }"#,
        );
        let themes = discover(&[bundled], &paths);
        assert_eq!(
            summary(&themes),
            [(ThemeOrigin::Bundled, true), (ThemeOrigin::User, false)]
        );
    }

    #[test]
    fn newer_compares_versions_with_prereleases() {
        assert!(newer("0.3.1", "0.3.0"));
        assert!(newer("0.3.1-dev", "0.3.0"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(newer("0.3.0", "0.3.0-rc.1"));
        assert!(newer("0.3.0-rc.2", "0.3.0-rc.1"));
        assert!(!newer("0.3.0", "0.3.0"));
        assert!(!newer("0.3.0-dev", "0.3.0"));
        assert!(!newer("0.2.9", "0.3.0"));
        assert!(!newer("latest", "0.3.0"));
        assert!(!newer("1.0", "0.3.0"));
    }

    #[test]
    fn satisfies_matches_the_sdk_rule() {
        assert!(satisfies("^1", "1.0.0"));
        assert!(satisfies("^1.2", "1.3.0"));
        assert!(satisfies("^1.2.3", "1.2.4"));
        assert!(!satisfies("^1.2", "1.1.9"));
        assert!(!satisfies("^2", "1.9.9"));
        assert!(!satisfies("1", "1.0.0"));
        assert!(!satisfies("^1", "1.0"));
        assert!(!satisfies("^1.2.3.4", "1.2.3"));
    }
}
