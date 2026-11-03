//! The commands behind `@ramraiders/forma-sdk`. Thin: each one checks what the theme may do, calls
//! `forma-core`, and hands back JSON. Paths on disk never reach a theme; media goes out as
//! `forma://` URLs.

use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use forma_core::batocera::Setting;
use forma_core::hierarchy::{Browse, DEFAULT_VIEW_SETTING, Listing, Origin, Page};
use forma_core::launch::{self, Controller, GameEvent, LaunchRequest};
use forma_core::net::{Fetched, achievements, netplay};
use forma_core::network::{EthernetConfig, Joined, NetworkStatus, WifiNetwork};
use forma_core::themes::{Theme, ThemeOrigin};
use forma_core::{Game, SDK_API_VERSION, SystemSummary};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::permissions::{self, Scope};
use crate::protocol::{RA_MEDIA_PREFIX, media_url, theme_url};
use crate::shell::Caller;
use crate::state::{AppState, THEME_SETTING};

pub const EVENT_INPUT: &str = "input";
pub const EVENT_LIBRARY_CHANGED: &str = "library:changed";
pub const EVENT_VIEWS_CHANGED: &str = "views:changed";
pub const EVENT_GAME_START: &str = "game:start";
pub const EVENT_GAME_END: &str = "game:end";
/// After a game: the achievements its session earned (`RaSession`), when it earned any.
pub const EVENT_RA_UNLOCKED: &str = "ra:unlocked";

#[derive(Debug, Clone, Serialize)]
pub struct LibraryChanged {
    pub systems: Vec<String>,
}

pub(crate) fn theme(state: &AppState) -> ApiResult<Theme> {
    state
        .current_theme()
        .ok_or_else(|| ApiError::new(ErrorCode::Internal, "no theme is loaded"))
}

/// Media paths become `forma://` URLs the theme can load.
fn expose(mut game: Game) -> Game {
    for (kind, url) in &mut game.media {
        *url = media_url(game.id, kind);
    }
    game
}

#[tauri::command]
pub fn sdk_api_version() -> &'static str {
    SDK_API_VERSION
}

/// Called by the SDK once the theme has started, so a theme that fails to boot can be
/// replaced by the default one instead of leaving a blank screen.
#[tauri::command]
pub fn theme_ready<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) {
    state.shell.set_page_ready();
    let _ = app.emit(crate::shell::EVENT_THEME_READY, ());
    // Only the first report is start-up; later ones are theme switches and reloads.
    if !state.theme_ready.swap(true, Ordering::SeqCst) {
        let id = state.current_theme().map(|t| t.id).unwrap_or_default();
        eprintln!(
            "forma: theme `{id}` ready {} ms after start",
            crate::boot::since_start().as_millis()
        );
        // Tells the Batocera service Forma is usable (see scripts/batocera/forma-standalone).
        if let Some(file) = &state.ready_file {
            let _ = std::fs::write(file, id);
        }
    }
}

#[tauri::command]
pub fn library_systems(state: State<'_, AppState>) -> ApiResult<Vec<SystemSummary>> {
    Ok(state.library().systems()?)
}

#[tauri::command]
pub fn library_game(state: State<'_, AppState>, id: i64) -> ApiResult<Option<Game>> {
    Ok(state.library().game(id)?.map(expose))
}

#[tauri::command]
pub fn library_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> ApiResult<Vec<Game>> {
    let games = state
        .library()
        .search(&query, limit.unwrap_or(50).min(500))?;
    Ok(games.into_iter().map(expose).collect())
}

#[tauri::command]
pub fn library_set_favorite<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> ApiResult<()> {
    let system = {
        let mut lib = state.library();
        let game = lib
            .game(id)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game with id {id}")))?;
        lib.set_favorite(id, favorite)?;
        game.system
    };
    let _ = app.emit(
        EVENT_LIBRARY_CHANGED,
        LibraryChanged {
            systems: vec![system],
        },
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `bundled` or `user`.
    pub origin: &'static str,
}

#[tauri::command]
pub fn nav_views(state: State<'_, AppState>) -> Vec<ViewInfo> {
    state
        .views()
        .views
        .iter()
        .map(|e| ViewInfo {
            id: e.view.id.clone(),
            name: e.view.name.clone(),
            description: e.view.description.clone(),
            origin: match e.origin {
                Origin::Bundled => "bundled",
                Origin::User(_) => "user",
            },
        })
        .collect()
}

#[tauri::command]
pub fn nav_browse(
    state: State<'_, AppState>,
    view: String,
    path: Vec<String>,
    page: Option<Page>,
) -> ApiResult<Browse> {
    let view = state
        .views()
        .get(&view)
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no view `{view}`")))?;
    let mut browse = state
        .library()
        .browse(&view, &path, page.unwrap_or_default())?;
    if let Listing::Games { items, .. } = &mut browse.listing {
        *items = std::mem::take(items).into_iter().map(expose).collect();
    }
    Ok(browse)
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultView {
    pub id: Option<String>,
    /// Why a preferred view was passed over, for the user to see.
    pub notes: Vec<String>,
}

#[tauri::command]
pub fn nav_default_view(state: State<'_, AppState>) -> ApiResult<DefaultView> {
    let setting = state.library().setting_text(DEFAULT_VIEW_SETTING)?;
    let suggested = state.current_theme().and_then(|t| t.manifest?.hierarchy);
    let views = state.views();
    let (view, notes) = views.boot_view(setting.as_deref(), suggested.as_deref());
    Ok(DefaultView {
        id: view.map(|v| v.id.clone()),
        notes,
    })
}

/// Sets the view Forma opens with. Any theme may: it is the user's choice, made in the UI.
#[tauri::command]
pub fn nav_set_default_view(state: State<'_, AppState>, id: Option<String>) -> ApiResult<()> {
    if let Some(id) = &id
        && state.views().get(id).is_none()
    {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("no view `{id}`"),
        ));
    }
    Ok(state
        .library()
        .set_setting(DEFAULT_VIEW_SETTING, id.as_deref())?)
}

#[tauri::command]
pub fn settings_get(
    state: State<'_, AppState>,
    caller: Caller,
    key: String,
    scope: Option<Scope>,
) -> ApiResult<Option<Value>> {
    let key = permissions::setting_key(
        &theme(&state)?,
        scope.unwrap_or(Scope::Theme),
        &key,
        caller.shell,
    )?;
    let raw = state.library().setting(&key)?;
    // Values are stored as JSON; a value written outside the SDK is returned as a string.
    Ok(raw.map(|r| serde_json::from_str(&r).unwrap_or(Value::String(r))))
}

#[tauri::command]
pub fn settings_set<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
    key: String,
    value: Option<Value>,
    scope: Option<Scope>,
) -> ApiResult<()> {
    let scope = scope.unwrap_or(Scope::Theme);
    let stored = permissions::setting_key(&theme(&state)?, scope, &key, caller.shell)?;
    let json = value.as_ref().map(Value::to_string);
    state.library().set_setting(&stored, json.as_deref())?;
    if stored == achievements::API_KEY_SETTING || stored == achievements::USERNAME_SETTING {
        crate::ra_match::account_changed();
    }
    // A change made in Forma's menu: the theme follows it (screen margins, say).
    if caller.shell {
        let scope = match scope {
            Scope::Theme => "theme",
            Scope::System => "system",
        };
        crate::shell::settings_changed(&app, scope, &key, &value.unwrap_or(Value::Null));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    pub id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub origin: ThemeOrigin,
    pub permissions: Vec<String>,
    pub current: bool,
    pub unavailable: Option<String>,
}

/// Re-scans the theme folders first, so a theme copied into `/userdata/themes-forma/` is
/// listed without restarting Forma.
#[tauri::command]
pub fn themes_list(state: State<'_, AppState>) -> Vec<ThemeInfo> {
    let found = forma_core::themes::discover(&state.bundled_themes, &state.paths);
    let mut themes = state
        .themes
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *themes = found;
    drop(themes);

    let current = state.current_theme().map(|t| t.id);
    state
        .themes
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|t| ThemeInfo {
            id: t.id.clone(),
            name: t.manifest.as_ref().map(|m| m.name.clone()),
            version: t.manifest.as_ref().map(|m| m.version.clone()),
            origin: t.origin,
            permissions: t
                .manifest
                .as_ref()
                .map(|m| m.permissions.clone())
                .unwrap_or_default(),
            current: current.as_deref() == Some(t.id.as_str()) && t.is_available(),
            unavailable: t.unavailable.clone(),
        })
        .collect()
}

/// Switches theme and reloads the window into it. Any theme may: it is the user's choice.
#[tauri::command]
pub fn themes_select<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    let theme = state.available_theme(&id).ok_or_else(|| {
        ApiError::new(
            ErrorCode::NotFound,
            format!("theme `{id}` is not available"),
        )
    })?;
    state.library().set_setting(THEME_SETTING, Some(&id))?;
    crate::boot::show_theme(&app, &state, &theme).map_err(|e| ApiError::new(ErrorCode::Internal, e))
}

#[tauri::command]
pub fn input_controllers(state: State<'_, AppState>) -> Vec<Controller> {
    state
        .input
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(forma_core::input::InputHandle::controllers)
        .unwrap_or_default()
}

/// Quits Forma. On Batocera, the service then hands the screen back to EmulationStation.
#[tauri::command]
pub fn system_quit<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    caller: Caller,
) -> ApiResult<()> {
    caller.require(&state, permissions::SYSTEM_POWER)?;
    app.exit(0);
    Ok(())
}

/// `restart` or `shutdown`, with `shutdown -r` / `shutdown -P -h` as EmulationStation's
/// own path ends in. Not through `batocera-es-swissknife`: it powers off only while
/// EmulationStation is running, and does nothing once Forma has replaced it. Anywhere but
/// Batocera it is refused: Forma must not power off a
/// development machine because a theme asked it to.
#[tauri::command]
pub fn system_power(state: State<'_, AppState>, caller: Caller, action: String) -> ApiResult<()> {
    caller.require(&state, permissions::SYSTEM_POWER)?;
    let args: &[&str] = match action.as_str() {
        "restart" => &["-r", "now"],
        "shutdown" => &["-P", "-h", "now"],
        _ => {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("unknown action `{action}`"),
            ));
        }
    };
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            format!("{action} is only available on Batocera"),
        ));
    }
    forma_core::host::command("shutdown")
        .args(args)
        .spawn()
        .map(drop)
        .map_err(|e| ApiError::new(ErrorCode::Internal, format!("could not {action}: {e}")))
}

/// Starts a game and returns at once; `game:start` and `game:end` follow as events. The window
/// hides and gamepad input pauses while it runs, since the emulator owns the screen and pads.
#[tauri::command]
pub fn launch_game<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: i64,
    request: Option<LaunchRequest>,
) -> ApiResult<()> {
    if state.playing.swap(true, Ordering::SeqCst) {
        return Err(ApiError::new(ErrorCode::Busy, "a game is already running"));
    }
    let controllers = input_controllers(state.clone());
    let prepared = launch::prepare(
        &state.library(),
        &state.paths,
        id,
        &request.unwrap_or_default(),
        &controllers,
    );
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            state.playing.store(false, Ordering::SeqCst);
            return Err(e.into());
        }
    };
    std::thread::Builder::new()
        .name("forma-game".into())
        .spawn(move || run_session(&app, prepared))
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
    Ok(())
}

fn run_session<R: Runtime>(app: &AppHandle<R>, prepared: launch::Prepared) {
    let state = app.state::<AppState>();
    let game = &prepared.game;
    let _ = app.emit(
        EVENT_GAME_START,
        GameEvent::Start {
            game_id: game.id,
            system: game.system.clone(),
            name: game.name.clone(),
        },
    );
    set_playing_ui(app, &state, true);
    state.music.game(true);
    let started = SystemTime::now();

    let result = state.launcher.run(&prepared.command);
    let (seconds, exit_code, error) = match &result {
        Ok(exit) => {
            let recorded = launch::finish(&mut state.library(), game.id, exit, SystemTime::now());
            (
                exit.duration.as_secs(),
                exit.code,
                recorded.err().map(|e| e.to_string()),
            )
        }
        Err(e) => (0, None, Some(e.to_string())),
    };

    // Not playing first: a delayed hide (see set_playing_ui) must not land after the show.
    state.playing.store(false, Ordering::SeqCst);
    set_playing_ui(app, &state, false);
    state.music.game(false);
    let _ = app.emit(
        EVENT_GAME_END,
        GameEvent::End {
            game_id: game.id,
            seconds,
            exit_code,
            error,
        },
    );
    let _ = app.emit(
        EVENT_LIBRARY_CHANGED,
        LibraryChanged {
            systems: vec![game.system.clone()],
        },
    );
    if result.is_ok() && seconds >= MIN_SESSION.as_secs() {
        report_unlocks(app, game.id, started);
    }
}

/// Shorter than this, nothing could have been earned: no question to RetroAchievements.
const MIN_SESSION: Duration = Duration::from_secs(10);

/// Asks RetroAchievements what the session earned, shortly after it ends and once more a
/// little later (an unlock sent as the emulator closed can take a moment), and emits
/// `ra:unlocked` with it. Quiet without an RA id for the game, an account, or a connection.
fn report_unlocks<R: Runtime>(app: &AppHandle<R>, game_id: i64, started: SystemTime) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let (ra_id, account) = {
            let lib = state.library();
            let Some(ra_id) = lib.game(game_id).ok().flatten().and_then(|g| g.ra_game_id) else {
                return;
            };
            let Ok(account) = achievements::account(&lib, &state.paths) else {
                return;
            };
            (ra_id, account)
        };
        for wait in [3, 20] {
            std::thread::sleep(Duration::from_secs(wait));
            // Another game started meanwhile: its own end will ask.
            if state.playing.load(Ordering::SeqCst) {
                return;
            }
            match achievements::session(
                &*state.http,
                &state.http_cache(),
                &account,
                game_id,
                ra_id,
                RA_MEDIA_PREFIX,
                started,
            ) {
                Ok(Some(session)) if !session.unlocked.is_empty() => {
                    let _ = app.emit(EVENT_RA_UNLOCKED, &session);
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("forma: achievements after the game: {e}");
                    return;
                }
            }
        }
    });
}

fn set_playing_ui<R: Runtime>(app: &AppHandle<R>, state: &AppState, playing: bool) {
    if let Some(input) = state
        .input
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        input.set_paused(playing);
    }
    if playing {
        // The theme's launch screen stays up until the emulator's window opens over it and takes
        // focus (see `on_window_event`); then Forma's window goes, so it neither draws nor takes
        // input behind the game. An emulator that never takes focus still gets the screen to
        // itself after LAUNCH_SCREEN. A game that ends sooner leaves the window as it is.
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(LAUNCH_SCREEN);
            if app.state::<AppState>().playing.load(Ordering::SeqCst)
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }
        });
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The longest the launch screen shows, for an emulator that never takes focus.
const LAUNCH_SCREEN: std::time::Duration = std::time::Duration::from_secs(20);

/// While a game runs, Forma losing focus means the emulator's window has opened over it: the
/// launch screen has done its job, and the window goes.
pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::Focused(false) = event
        && window
            .app_handle()
            .state::<AppState>()
            .playing
            .load(Ordering::SeqCst)
    {
        let _ = window.hide();
    }
}

/// Runs network work on a blocking thread: a slow service must never stall the UI thread.
pub(crate) async fn off_thread<R: Runtime, T: Send + 'static>(
    app: AppHandle<R>,
    work: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
}

/// The user's RetroAchievements profile. Read-only: unlocking happens in RetroArch.
#[tauri::command]
pub async fn ra_user<R: Runtime>(app: AppHandle<R>) -> ApiResult<Fetched<achievements::RaUser>> {
    off_thread(app, move |state| {
        // Read the account, then let go of the library before going to the network.
        let account = achievements::account(&state.library(), &state.paths)?;
        Ok(achievements::user(
            &*state.http,
            &state.http_cache(),
            &account,
            RA_MEDIA_PREFIX,
        )?)
    })
    .await
}

/// A local game's achievements and the user's progress in it.
#[tauri::command]
pub async fn ra_game<R: Runtime>(
    app: AppHandle<R>,
    id: i64,
) -> ApiResult<Fetched<achievements::RaGame>> {
    off_thread(app, move |state| {
        let (ra_id, account) = {
            let lib = state.library();
            let game = lib.game(id)?.ok_or_else(|| {
                ApiError::new(ErrorCode::NotFound, format!("no game with id {id}"))
            })?;
            let ra_id = game.ra_game_id.ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    format!("no RetroAchievements id is known for {} yet", game.name),
                )
            })?;
            (ra_id, achievements::account(&lib, &state.paths)?)
        };
        Ok(achievements::game(
            &*state.http,
            &state.http_cache(),
            &account,
            ra_id,
            RA_MEDIA_PREFIX,
        )?)
    })
    .await
}

/// Open netplay rooms, joinable first, matched to local games where possible.
#[tauri::command]
pub async fn netplay_lobby<R: Runtime>(
    app: AppHandle<R>,
) -> ApiResult<Fetched<Vec<netplay::Room>>> {
    off_thread(app, move |state| {
        let mut rooms = netplay::lobby(&*state.http, &state.http_cache())?;
        // The library is locked only for the name match, never across the request.
        netplay::match_local(&mut rooms.data, &state.library())?;
        Ok(rooms)
    })
    .await
}

/// Network settings run Batocera's own tools: refused anywhere else, as power is.
fn network_allowed(state: &AppState, caller: Caller) -> ApiResult<()> {
    caller.require(state, permissions::SYSTEM_NETWORK)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "network settings are only available on Batocera",
        ));
    }
    Ok(())
}

/// The wired and Wi-Fi connections, and what is saved for each.
#[tauri::command]
pub async fn network_status<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<NetworkStatus> {
    off_thread(app, move |state| {
        network_allowed(state, caller)?;
        Ok(state.network.status()?)
    })
    .await
}

/// Wi-Fi networks in range. Takes a few seconds: it scans first.
#[tauri::command]
pub async fn network_wifi_scan<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<WifiNetwork>> {
    off_thread(app, move |state| {
        network_allowed(state, caller)?;
        Ok(state.network.wifi_scan()?)
    })
    .await
}

/// Saves a Wi-Fi network as Batocera's and waits, up to 30 seconds, for it to connect.
#[tauri::command]
pub async fn network_wifi_join<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    ssid: String,
    passphrase: Option<String>,
) -> ApiResult<Joined> {
    off_thread(app, move |state| {
        network_allowed(state, caller)?;
        Ok(state.network.wifi_join(&ssid, passphrase.as_deref())?)
    })
    .await
}

#[tauri::command]
pub async fn network_wifi_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    enabled: bool,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        network_allowed(state, caller)?;
        Ok(state.network.wifi_set_enabled(enabled)?)
    })
    .await
}

/// DHCP, or a static address that survives reboots. The connection drops for a few seconds
/// while connman restarts to apply it.
#[tauri::command]
pub async fn network_ethernet_configure<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    config: EthernetConfig,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        network_allowed(state, caller)?;
        Ok(state.network.ethernet_configure(&config)?)
    })
    .await
}

/// Batocera's settings belong to Batocera's tools: refused anywhere else, as power is.
fn batocera_allowed(state: &AppState, caller: Caller) -> ApiResult<()> {
    caller.require(state, permissions::SETTINGS_BATOCERA)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "Batocera's settings are only available on Batocera",
        ));
    }
    Ok(())
}

/// Batocera's RetroAchievements or netplay settings (`prefix`, e.g. `global.netplay`). Secrets
/// only say whether they are set.
#[tauri::command]
pub fn batocera_settings(
    state: State<'_, AppState>,
    caller: Caller,
    prefix: String,
) -> ApiResult<Vec<Setting>> {
    batocera_allowed(&state, caller)?;
    Ok(state.batocera.get(&prefix))
}

#[tauri::command]
pub async fn batocera_set<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    key: String,
    value: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        batocera_allowed(state, caller)?;
        Ok(state.batocera.set(&key, &value)?)
    })
    .await
}

/// Logs in to RetroAchievements and saves the account and its token, as ES does. Returns the
/// user name as RetroAchievements spells it.
#[tauri::command]
pub async fn ra_login<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    username: String,
    password: String,
) -> ApiResult<String> {
    off_thread(app, move |state| {
        batocera_allowed(state, caller)?;
        let name = state
            .batocera
            .ra_login(&*state.http, &username, &password)?;
        crate::ra_match::account_changed();
        Ok(name)
    })
    .await
}

#[tauri::command]
pub async fn ra_logout<R: Runtime>(app: AppHandle<R>, caller: Caller) -> ApiResult<()> {
    off_thread(app, move |state| {
        batocera_allowed(state, caller)?;
        Ok(state.batocera.ra_logout()?)
    })
    .await
}

/// The URL a theme is served from, for the window to load.
pub fn entry_url(theme: &Theme) -> String {
    let entry = theme
        .manifest
        .as_ref()
        .map_or("index.html", |m| m.entry.as_str());
    theme_url(&theme.id, entry)
}
