//! What a theme may do, from the `permissions` in its `theme.json`.
//!
//! Checked in Rust before a privileged command runs, so a theme cannot get around it from
//! JavaScript. Tauri's capability files are a second, coarser layer.

use forma_core::themes::Theme;

use crate::api::{ApiError, ApiResult, ErrorCode};

pub const SYSTEM_POWER: &str = "system.power";
pub const SETTINGS_SYSTEM: &str = "settings.system";
pub const SYSTEM_NETWORK: &str = "system.network";
pub const SETTINGS_BATOCERA: &str = "settings.batocera";
pub const MUSIC_MANAGE: &str = "music.manage";
pub const SETTINGS_EMULATORS: &str = "settings.emulators";
pub const SYSTEM_SETTINGS: &str = "system.settings";
pub const STORE_MANAGE: &str = "store.manage";
pub const SYSTEM_CONTROLLERS: &str = "system.controllers";
pub const SYSTEM_UPDATE: &str = "system.update";
pub const THEMES_MANAGE: &str = "themes.manage";
pub const SCRAPER_MANAGE: &str = "scraper.manage";

pub fn require(theme: &Theme, permission: &str) -> ApiResult<()> {
    if theme.has_permission(permission) {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::PermissionDenied,
            format!(
                "theme `{}` needs the `{permission}` permission in its theme.json",
                theme.id
            ),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// The theme's own settings: free for any theme, invisible to other themes.
    Theme,
    /// Forma's settings: needs `settings.system`.
    System,
}

/// Where a setting is stored, after checking the caller may touch it. Forma's own shell
/// (`by_shell`) may reach Forma's settings whatever the theme declares.
pub fn setting_key(theme: &Theme, scope: Scope, key: &str, by_shell: bool) -> ApiResult<String> {
    if key.is_empty() || key.len() > 200 {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            "setting keys are 1–200 characters",
        ));
    }
    match scope {
        Scope::Theme => Ok(format!("theme.{}.{key}", theme.id)),
        Scope::System => {
            if !by_shell {
                require(theme, SETTINGS_SYSTEM)?;
            }
            if key.starts_with("theme.") {
                // Another theme's private settings are not system settings.
                return Err(ApiError::new(
                    ErrorCode::PermissionDenied,
                    "theme settings are only reachable by their own theme",
                ));
            }
            Ok(key.to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forma_core::themes::{ThemeManifest, ThemeOrigin};

    fn theme(permissions: &[&str]) -> Theme {
        Theme {
            overrides: false,
            id: "neon".into(),
            dir: "/t".into(),
            origin: ThemeOrigin::User,
            manifest: Some(ThemeManifest {
                id: "neon".into(),
                name: "Neon".into(),
                version: "1".into(),
                author: None,
                description: None,
                entry: "index.html".into(),
                sdk: "^1".into(),
                permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
                hierarchy: None,
                resolutions: vec![],
                remote: vec![],
                settings: vec![],
                views: None,
                draws: vec![],
                logo: None,
                schema: None,
            }),
            unavailable: None,
        }
    }

    #[test]
    fn a_privileged_call_needs_the_declared_permission() {
        assert!(require(&theme(&[SYSTEM_POWER]), SYSTEM_POWER).is_ok());
        let err = require(&theme(&[]), SYSTEM_POWER).unwrap_err();
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert!(err.message.contains("`system.power`"));
    }

    #[test]
    fn theme_settings_are_namespaced_and_free() {
        assert_eq!(
            setting_key(&theme(&[]), Scope::Theme, "layout", false).unwrap(),
            "theme.neon.layout"
        );
    }

    #[test]
    fn system_settings_need_permission_and_cannot_reach_other_themes() {
        let err = setting_key(&theme(&[]), Scope::System, "views.default", false).unwrap_err();
        assert!(setting_key(&theme(&[]), Scope::System, "views.default", true).is_ok());
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let admin = theme(&[SETTINGS_SYSTEM]);
        assert_eq!(
            setting_key(&admin, Scope::System, "views.default", false).unwrap(),
            "views.default"
        );
        assert!(setting_key(&admin, Scope::System, "theme.other.secret", true).is_err());
        assert!(setting_key(&admin, Scope::Theme, "", false).is_err());
    }
}
