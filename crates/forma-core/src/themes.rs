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
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema: Option<String>,
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
/// bundled theme's id is listed as unavailable: replacing the default theme by accident would
/// leave a user without a working UI.
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
        if theme.is_available() && themes.iter().any(|t| t.id == theme.id) {
            theme.unavailable = Some(format!(
                "id `{}` is already used by another theme",
                theme.id
            ));
        }
        themes.push(theme);
    }
    themes
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
