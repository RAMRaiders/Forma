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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use forma_core::input::{Action, InputEvent};
use forma_core::themes::ThemeSetting;
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::{CommandArg, CommandItem, InvokeError};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{off_thread, theme};
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
/// The user chose a game in Forma's menu (its versions, sibling games); the theme shows
/// it. `{ game, system }`.
pub const EVENT_NAV_GAME: &str = "nav:game";
/// Forma's screensaver came on or went. `{ active }`.
pub const EVENT_SCREENSAVER: &str = "screensaver";
/// The shell's menu opened or closed: `{ open }`.
pub const EVENT_MENU: &str = "menu";
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
    /// Counts page loads, so a watcher can tell its page is gone.
    page: AtomicU64,
    /// This page's theme has called `theme_ready`.
    page_ready: AtomicBool,
    /// The entry cover has gone over a started theme: the UI is up, and music may start.
    pub ui_started: AtomicBool,
}

impl ShellState {
    /// A new page is loading: a new token, unclaimed, and nothing open.
    pub fn new_page(&self) {
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) = (fresh_token(), false);
        self.page.fetch_add(1, Ordering::SeqCst);
        self.page_ready.store(false, Ordering::SeqCst);
        self.open.store(false, Ordering::SeqCst);
        *self.context.lock().unwrap_or_else(PoisonError::into_inner) = Value::Null;
    }

    /// A menu, a page or the screensaver is up: all input is the shell's.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    pub fn page(&self) -> u64 {
        self.page.load(Ordering::SeqCst)
    }

    pub fn page_ready(&self) -> bool {
        self.page_ready.load(Ordering::SeqCst)
    }

    /// The view the theme says it is showing.
    pub fn view(&self) -> Option<String> {
        self.context
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get("view")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub fn set_page_ready(&self) {
        self.page_ready.store(true, Ordering::SeqCst);
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

    pub(crate) fn shell_only(self) -> ApiResult<()> {
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

/// The shell opened or closed its menu: input follows, and the theme hears of it (`menu`) to
/// stop what it plays under the menu (a video under its see-through panel) and pick it up again.
#[tauri::command]
pub fn shell_set_open<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    open: bool,
) -> ApiResult<()> {
    caller.shell_only()?;
    let was = state.shell.open.swap(open, Ordering::SeqCst);
    if was != open {
        let _ = app.emit(EVENT_MENU, serde_json::json!({ "open": open }));
    }
    Ok(())
}

/// Forma's screensaver came on or went: all input is the shell's while it shows, and the
/// theme hears of it (`screensaver`) to pause what it plays and pick it up again.
#[tauri::command]
pub fn shell_screensaver<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    active: bool,
) -> ApiResult<()> {
    caller.shell_only()?;
    state.shell.open.store(active, Ordering::SeqCst);
    let _ = app.emit(EVENT_SCREENSAVER, serde_json::json!({ "active": active }));
    Ok(())
}

/// The shell's entry cover has gone, over a theme that reported ready: Forma is up. Background
/// music waits for this (see `music::start`).
#[tauri::command]
pub fn shell_started(state: State<'_, AppState>, caller: Caller) -> ApiResult<()> {
    caller.shell_only()?;
    state.shell.ui_started.store(true, Ordering::SeqCst);
    // Tells the Batocera service the cover is going over the theme: its boot picture under the
    // see-through window can go now, not before (see scripts/batocera/forma-standalone).
    if let Some(file) = &state.ready_file {
        let _ = std::fs::write(crate::state::up_file(file), "");
    }
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
    // Off the game list: a filter on it would only surprise the user on coming back.
    if context.get("list").and_then(Value::as_str) != Some("games") {
        state
            .list_filter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .filter = Default::default();
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
    /// The only views the theme shows, if it says (`views` in its theme.json).
    pub views: Option<Vec<String>>,
    /// What the theme draws itself (`draws` in its theme.json): the shell leaves those to it.
    pub draws: Vec<String>,
    /// Where to load the theme's logo from, shown in place of its name.
    pub logo: Option<String>,
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
pub async fn shell_info<R: Runtime>(app: AppHandle<R>, caller: Caller) -> ApiResult<ShellInfo> {
    off_thread(app, move |state| {
        caller.shell_only()?;
        let theme = theme(state)?;
        let logo = crate::state::theme_logo_url(&theme);
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
                views: manifest.views,
                draws: manifest.draws,
                logo,
            },
            forma_version: env!("CARGO_PKG_VERSION"),
        })
    })
    .await
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

/// The user chose a game in Forma's menu: the theme shows it, in its system.
#[tauri::command]
pub fn shell_open_game<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    id: i64,
) -> ApiResult<()> {
    caller.shell_only()?;
    let game = state
        .library()
        .game(id)
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game {id}")))?;
    let _ = app.emit(
        EVENT_NAV_GAME,
        serde_json::json!({ "game": id, "system": game.system }),
    );
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

/// What the shell's cover shows while a theme page loads, handed to it on its script tag so it
/// can draw at once, before any call to Forma.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverInfo {
    /// Batocera's version, e.g. `43.1 2026/05/29`.
    pub batocera: String,
    /// The theme and its version, e.g. `Cyber 0.2.0`.
    pub theme: String,
    /// The theme's logo, to show in place of its name; its version then follows it.
    pub theme_logo: Option<String>,
    pub theme_version: String,
    /// The same logo's file, for the boot picture ([`crate::splash`]).
    pub theme_logo_file: Option<std::path::PathBuf>,
    /// Screen margins for a TV's overscan, in percent per side.
    pub margins: u8,
    /// At boot on Batocera: the boot picture is under the see-through window, and the cover
    /// lets it show, drawing only what moves over it.
    pub see_through: bool,
}

/// The script tag that brings the shell into a theme page, first thing in its `<head>`, so it
/// runs (and claims its token) before any of the theme's scripts.
pub fn shell_tag(cover: &CoverInfo) -> String {
    format!(
        "<script src=\"forma://localhost/shell/shell.js\" data-batocera=\"{}\" data-forma=\"{}\" data-theme=\"{}\" data-theme-logo=\"{}\" \
         data-theme-version=\"{}\" data-margins=\"{}\" data-see-through=\"{}\"></script>",
        attribute(&cover.batocera),
        attribute(env!("CARGO_PKG_VERSION")),
        attribute(&cover.theme),
        attribute(cover.theme_logo.as_deref().unwrap_or_default()),
        attribute(&cover.theme_version),
        cover.margins,
        u8::from(cover.see_through)
    )
}

/// Text made safe inside a double-quoted HTML attribute.
fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `html` with the shell's script tag inserted.
pub fn inject(html: &str, cover: &CoverInfo) -> String {
    let tag = shell_tag(cover);
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
    let mut out = String::with_capacity(html.len() + tag.len());
    out.push_str(&html[..at]);
    out.push_str(&tag);
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
        let cover = CoverInfo::default();
        let tag = shell_tag(&cover);
        assert_eq!(
            inject(
                "<!doctype html><html lang=\"en\"><HEAD data-x><title>t</title></HEAD>",
                &cover
            ),
            format!("<!doctype html><html lang=\"en\"><HEAD data-x>{tag}<title>t</title></HEAD>")
        );
        assert_eq!(
            inject("<html><body>x</body></html>", &cover),
            format!("<html>{tag}<body>x</body></html>")
        );
        assert_eq!(inject("<p>x</p>", &cover), format!("{tag}<p>x</p>"));
    }

    #[test]
    fn the_cover_gets_what_it_shows_on_the_tag_escaped() {
        let tag = shell_tag(&CoverInfo {
            batocera: "43.1 2026/05/29".into(),
            theme: "Neon \"<b>\" 2".into(),
            margins: 3,
            see_through: true,
            ..Default::default()
        });
        assert!(tag.contains("data-see-through=\"1\""), "{tag}");
        assert!(tag.contains("data-batocera=\"43.1 2026/05/29\""), "{tag}");
        assert!(
            tag.contains("data-theme=\"Neon &quot;&lt;b&gt;&quot; 2\""),
            "{tag}"
        );
        assert!(tag.contains("data-margins=\"3\""), "{tag}");
    }
}
