//! Forma's shell: the part of the interface that belongs to Forma, not the theme.
//!
//! Every theme page gets Forma's shell script (`forma://localhost/shell/shell.js`, inserted by
//! the protocol before the theme's own scripts). It draws, over any theme:
//!
//! - the **Start menu** (theme, music, network, settings, power…) and the **Select** views list,
//!   so switching theme never depends on the theme having a menu of its own;
//! - the **entry cover**: Forma's logo while a theme loads, gone once it reports ready.
//!
//! Start and Select are Forma's: their presses go to the shell (`shell:input`), never to the
//! theme. While the shell has something open, every other press goes to it as well.
//!
//! The shell runs in the theme's page but acts for Forma, so its calls are not held to the
//! theme's permissions. It proves who it is with a token it claims once per page load, before
//! any theme script runs, and sends as the [`SHELL_HEADER`] header. A theme loaded after it
//! cannot claim it again.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use forma_core::input::{Action, InputEvent};
use forma_core::themes::ThemeSetting;
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::{CommandArg, CommandItem, InvokeError};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::theme;
use crate::permissions;
use crate::state::AppState;

/// The header the shell's calls carry its token in.
pub const SHELL_HEADER: &str = "x-forma-shell";

/// Input for the shell: Start and Select always, everything while it has something open.
pub const EVENT_SHELL_INPUT: &str = "shell:input";
/// The theme has called `theme_ready`: the shell's entry cover can go.
pub const EVENT_THEME_READY: &str = "shell:theme-ready";
/// The user picked a view in Forma's views list; the theme shows it. `{ view }`.
pub const EVENT_NAV_OPEN: &str = "nav:open";
/// A setting changed from Forma's menu. `{ scope, key, value }`.
pub const EVENT_SETTINGS_CHANGED: &str = "settings:changed";

#[derive(Default)]
pub struct ShellState {
    /// This page load's token, and whether the shell has claimed it.
    token: Mutex<(String, bool)>,
    /// The shell has a menu or page open: all input goes to it.
    open: AtomicBool,
    /// What the theme says is on screen (`forma.shell.setContext`).
    context: Mutex<Value>,
}

impl ShellState {
    /// A new page is loading: a new token, unclaimed, and nothing open.
    pub fn new_page(&self) {
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) = (fresh_token(), false);
        self.open.store(false, Ordering::SeqCst);
        *self.context.lock().unwrap_or_else(PoisonError::into_inner) = Value::Null;
    }

    /// The token, the first time it is asked for on this page.
    fn claim(&self) -> Option<String> {
        let mut token = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        if token.1 || token.0.is_empty() {
            return None;
        }
        token.1 = true;
        Some(token.0.clone())
    }

    pub fn is_token(&self, candidate: &str) -> bool {
        let token = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        token.1 && !token.0.is_empty() && token.0 == candidate
    }

    /// Where an input event goes: the shell's event or the theme's.
    pub fn route(&self, event: &InputEvent) -> Route {
        if matches!(event.action, Action::Start | Action::Select)
            || self.open.load(Ordering::SeqCst)
        {
            Route::Shell
        } else {
            Route::Theme
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Shell,
    Theme,
}

/// 128 random bits as hex, from the standard library's randomly seeded hasher.
fn fresh_token() -> String {
    (0..2)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos()),
            );
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Who made a call: Forma's shell, or the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub shell: bool,
}

impl Caller {
    /// The shell may do anything Forma offers; a theme what its `theme.json` declares.
    pub fn require(self, state: &AppState, permission: &str) -> ApiResult<()> {
        if self.shell {
            Ok(())
        } else {
            permissions::require(&theme(state)?, permission)
        }
    }

    fn shell_only(self) -> ApiResult<()> {
        if self.shell {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::PermissionDenied,
                "only Forma's own menu can do this",
            ))
        }
    }
}

impl<'de, R: Runtime> CommandArg<'de, R> for Caller {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let message = command.message;
        let shell = message
            .headers()
            .get(SHELL_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|token| {
                message
                    .webview()
                    .try_state::<AppState>()
                    .is_some_and(|state| state.shell.is_token(token))
            });
        Ok(Self { shell })
    }
}

/// The shell's token for this page. Only the first call on a page gets it.
#[tauri::command]
pub fn shell_claim(state: State<'_, AppState>) -> ApiResult<String> {
    state.shell.claim().ok_or_else(|| {
        ApiError::new(
            ErrorCode::PermissionDenied,
            "the shell has already started on this page",
        )
    })
}

/// The shell opened or closed its menu: input follows.
#[tauri::command]
pub fn shell_set_open(state: State<'_, AppState>, caller: Caller, open: bool) -> ApiResult<()> {
    caller.shell_only()?;
    state.shell.open.store(open, Ordering::SeqCst);
    Ok(())
}

/// What the theme shows now (the view, the selected system and game), for the menu's entries
/// that act on it. Any theme may set it; it is only ever read by Forma's menu.
#[tauri::command]
pub fn shell_set_context(state: State<'_, AppState>, context: Value) -> ApiResult<()> {
    if !context.is_object() && !context.is_null() {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            "the context is an object",
        ));
    }
    *state
        .shell
        .context
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = context;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    /// The theme's context, as it last set it.
    pub context: Value,
    pub theme: ShellTheme,
    pub forma_version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellTheme {
    pub id: String,
    pub name: String,
    pub version: String,
    /// What the theme lets the user set from Forma's menu, with the values now.
    pub settings: Vec<ThemeSettingValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSettingValue {
    #[serde(flatten)]
    pub setting: ThemeSetting,
    pub value: Value,
}

/// What the menu needs when it opens: the theme's context and its settings.
#[tauri::command]
pub fn shell_info(state: State<'_, AppState>, caller: Caller) -> ApiResult<ShellInfo> {
    caller.shell_only()?;
    let theme = theme(&state)?;
    let manifest = theme.manifest.clone().unwrap_or_else(|| {
        // An available theme always has one; this keeps the menu working regardless.
        forma_core::themes::ThemeManifest::unnamed(&theme.id)
    });
    let library = state.library();
    let settings = manifest
        .settings
        .iter()
        .map(|s| {
            let stored = library
                .setting(&format!("theme.{}.{}", theme.id, s.key))
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str(&raw).ok());
            ThemeSettingValue {
                value: stored
                    .filter(|v| s.accepts(v))
                    .unwrap_or_else(|| s.default.clone()),
                setting: s.clone(),
            }
        })
        .collect();
    drop(library);
    Ok(ShellInfo {
        context: state
            .shell
            .context
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        theme: ShellTheme {
            id: theme.id,
            name: manifest.name,
            version: manifest.version,
            settings,
        },
        forma_version: env!("CARGO_PKG_VERSION"),
    })
}

/// Sets one of the theme's declared settings and tells the theme.
#[tauri::command]
pub fn shell_set_theme_setting<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    key: String,
    value: Value,
) -> ApiResult<()> {
    caller.shell_only()?;
    let theme = theme(&state)?;
    let setting = theme
        .manifest
        .as_ref()
        .and_then(|m| m.settings.iter().find(|s| s.key == key))
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("theme `{}` has no setting `{key}`", theme.id),
            )
        })?;
    if !setting.accepts(&value) {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            format!("`{value}` is not a value for `{key}`"),
        ));
    }
    state.library().set_setting(
        &format!("theme.{}.{key}", theme.id),
        Some(&value.to_string()),
    )?;
    settings_changed(&app, "theme", &key, &value);
    Ok(())
}

/// Tells the theme a setting changed from Forma's menu.
pub fn settings_changed<R: Runtime>(app: &AppHandle<R>, scope: &str, key: &str, value: &Value) {
    let _ = app.emit(
        EVENT_SETTINGS_CHANGED,
        serde_json::json!({ "scope": scope, "key": key, "value": value }),
    );
}

/// The user picked a view in Forma's views list: the theme shows it.
#[tauri::command]
pub fn shell_open_view<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    view: String,
) -> ApiResult<()> {
    caller.shell_only()?;
    if state.views().get(&view).is_none() {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no view `{view}`"),
        ));
    }
    let _ = app.emit(EVENT_NAV_OPEN, serde_json::json!({ "view": view }));
    Ok(())
}

/// Sends an input event where it belongs.
pub fn deliver<R: Runtime>(app: &AppHandle<R>, event: &InputEvent) {
    let route = app
        .try_state::<AppState>()
        .map_or(Route::Theme, |s| s.shell.route(event));
    let name = match route {
        Route::Shell => EVENT_SHELL_INPUT,
        Route::Theme => crate::commands::EVENT_INPUT,
    };
    let _ = app.emit(name, event);
}

/// The script tag that brings the shell into a theme page, first thing in its `<head>`, so it
/// runs (and claims its token) before any of the theme's scripts.
pub const SHELL_TAG: &str = "<script src=\"forma://localhost/shell/shell.js\"></script>";

/// `html` with the shell's script tag inserted.
pub fn inject(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let at = lower
        .find("<head")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .or_else(|| {
            lower
                .find("<html")
                .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        })
        .unwrap_or(0);
    let mut out = String::with_capacity(html.len() + SHELL_TAG.len());
    out.push_str(&html[..at]);
    out.push_str(SHELL_TAG);
    out.push_str(&html[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use forma_core::input::Button;

    fn press(action: Action) -> InputEvent {
        InputEvent {
            action,
            button: Some(Button::South),
            gamepad: 0,
            pressed: true,
            repeat: false,
        }
    }

    #[test]
    fn the_token_is_claimed_once_per_page() {
        let shell = ShellState::default();
        assert_eq!(shell.claim(), None, "no page yet");
        shell.new_page();
        let token = shell.claim().unwrap();
        assert_eq!(token.len(), 32);
        assert!(shell.is_token(&token));
        assert_eq!(shell.claim(), None);
        assert!(!shell.is_token("guess"));

        shell.new_page();
        assert!(!shell.is_token(&token), "a new page, a new token");
        assert_ne!(shell.claim().unwrap(), token);
    }

    #[test]
    fn an_unclaimed_token_is_worth_nothing() {
        let shell = ShellState::default();
        shell.new_page();
        let token = shell.token.lock().unwrap().0.clone();
        assert!(!shell.is_token(&token));
    }

    #[test]
    fn start_and_select_go_to_the_shell_and_everything_while_it_is_open() {
        let shell = ShellState::default();
        assert_eq!(shell.route(&press(Action::Start)), Route::Shell);
        assert_eq!(shell.route(&press(Action::Select)), Route::Shell);
        assert_eq!(shell.route(&press(Action::Confirm)), Route::Theme);
        shell.open.store(true, Ordering::SeqCst);
        assert_eq!(shell.route(&press(Action::Confirm)), Route::Shell);
        shell.new_page();
        assert_eq!(shell.route(&press(Action::Confirm)), Route::Theme);
    }

    #[test]
    fn the_shell_goes_first_in_the_head() {
        assert_eq!(
            inject("<!doctype html><html lang=\"en\"><HEAD data-x><title>t</title></HEAD>"),
            format!(
                "<!doctype html><html lang=\"en\"><HEAD data-x>{SHELL_TAG}<title>t</title></HEAD>"
            )
        );
        assert_eq!(
            inject("<html><body>x</body></html>"),
            format!("<html>{SHELL_TAG}<body>x</body></html>")
        );
        assert_eq!(inject("<p>x</p>"), format!("{SHELL_TAG}<p>x</p>"));
    }
}
