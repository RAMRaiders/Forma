//! The commands behind `@ramraiders/forma-sdk`. Thin: each one checks what the theme may do, calls
//! `forma-core`, and hands back JSON. Paths on disk never reach a theme; media goes out as
//! `forma://` URLs.

use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use forma_core::batocera::Setting;
use forma_core::hierarchy::{
    Browse, DEFAULT_VIEW_SETTING, FilterOptions, ListFilter, Listing, Origin, Page,
};
use forma_core::launch::{self, Controller, GameEvent, LaunchRequest};
use forma_core::net::{Fetched, achievements, netplay};
use forma_core::network::{EthernetConfig, Joined, NetworkStatus, WifiNetwork};
use forma_core::themes::{Theme, ThemeOrigin};
use forma_core::{Game, SDK_API_VERSION, SystemSummary};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::permissions::{self, Scope};
use crate::protocol::{RA_MEDIA_PREFIX, media_url, theme_url};
use crate::shell::Caller;
use crate::state::{AppState, ListOnScreen, THEME_SETTING};

pub const EVENT_INPUT: &str = "input";
pub const EVENT_LIBRARY_CHANGED: &str = "library:changed";
pub const EVENT_VIEWS_CHANGED: &str = "views:changed";
/// A game's pictures were replaced, often under the same file names (`MediaChanged`).
pub const EVENT_MEDIA_CHANGED: &str = "media:changed";
pub const EVENT_GAME_START: &str = "game:start";
pub const EVENT_GAME_END: &str = "game:end";
/// After a game: the achievements its session earned (`RaSession`), when it earned any.
pub const EVENT_RA_UNLOCKED: &str = "ra:unlocked";

#[derive(Debug, Clone, Serialize)]
pub struct LibraryChanged {
    pub systems: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaChanged {
    pub game: i64,
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
pub async fn library_systems<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<SystemSummary>> {
    off_thread(app, move |state| Ok(state.library().systems_in_order()?)).await
}

/// The titles of a system with more than one version.
#[tauri::command]
pub async fn library_versions<R: Runtime>(
    app: AppHandle<R>,
    system: String,
) -> ApiResult<Vec<forma_core::library::VersionGroup>> {
    off_thread(app, move |state| Ok(state.library().versions(&system)?)).await
}

#[tauri::command]
pub async fn library_game<R: Runtime>(app: AppHandle<R>, id: i64) -> ApiResult<Option<Game>> {
    off_thread(app, move |state| Ok(state.library().game(id)?.map(expose))).await
}

/// What a game has saved (RetroArch's battery save and save states); none for a game Forma
/// does not have.
#[tauri::command]
pub async fn library_saves<R: Runtime>(
    app: AppHandle<R>,
    id: i64,
) -> ApiResult<forma_core::saves::Saves> {
    off_thread(app, move |state| {
        let Some(game) = state.library().game(id)? else {
            return Ok(forma_core::saves::Saves::default());
        };
        Ok(forma_core::saves::of(
            &state.paths.userdata.join("saves"),
            &game.system,
            &game.path,
        ))
    })
    .await
}

/// A picture a theme will soon show: a game's media `kind` at about `width` px.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAhead {
    game_id: i64,
    kind: String,
    width: u32,
}

/// At most this many pictures in one ask.
const MAX_PREPARE: usize = 64;

/// Makes resized pictures ahead of time, the nearest (first) first, when nothing on screen
/// waits: the games next to the one shown, so they show at once when reached.
#[tauri::command]
pub async fn library_prepare_media<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<MediaAhead>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        // The latest asked is made first: ask in reverse, the nearest last.
        for item in items.iter().take(MAX_PREPARE).rev() {
            if matches!(item.kind.as_str(), "video" | "manual") || item.width == 0 {
                continue;
            }
            let Some(game) = state.library().game(item.game_id)? else {
                continue;
            };
            if let Some(source) = game.media.get(&item.kind) {
                state
                    .thumbnails
                    .prepare(std::path::Path::new(source), item.width.min(4096));
            }
        }
        Ok(())
    })
    .await
}

/// A picture of every game in a list: `kind` at about `width` px.
#[derive(Debug, Deserialize)]
pub struct Picture {
    kind: String,
    width: u32,
}

/// How many games of a list, around the selection, have their pictures made ahead.
const LIST_AROUND: u32 = 300;
/// The widest picture a list shows in a row: after the nearest games, made first for the rest.
const LIST_PICTURE_MAX: u32 = 320;
/// Games either side of the selection whose pictures are all made first.
const LIST_NEAR: usize = 15;

/// A theme opened a game list: the pictures of up to [`LIST_AROUND`] of its games, nearest
/// `around` (the selection) first, are made in the background when nothing else waits, and not
/// while a game runs. The next list replaces this one's.
#[tauri::command]
pub async fn library_prepare_list<R: Runtime>(
    app: AppHandle<R>,
    view: String,
    path: Vec<String>,
    around: u32,
    pictures: Vec<Picture>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        let view = state
            .views()
            .get(&view)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no view `{view}`")))?;
        let offset = around.saturating_sub(LIST_AROUND / 2);
        let page = Page {
            offset,
            limit: LIST_AROUND,
        };
        let Listing::Games { items, .. } = state.library().browse(&view, &path, page)?.listing
        else {
            return Ok(());
        };
        if items.is_empty() {
            return Ok(());
        }
        // Outward from the selection: it, then one either side, two either side…
        let at = ((around - offset) as usize).min(items.len() - 1);
        let mut order: Vec<usize> = Vec::with_capacity(items.len());
        for d in 0..items.len() {
            if at + d < items.len() {
                order.push(at + d);
            }
            if d > 0 && d <= at {
                order.push(at - d);
            }
        }
        // Everything for the games nearest the selection (its page, its backdrop, where the
        // list goes next); then the small pictures (a list's rows, a shelf's spines) for every
        // other game round it; then their large ones. Each outward.
        let near = order.len().min(LIST_NEAR * 2 + 1);
        let wanted = |games: &[usize], small: Option<bool>| -> Vec<(std::path::PathBuf, u32)> {
            games
                .iter()
                .flat_map(|&i| {
                    let game = &items[i];
                    pictures.iter().filter_map(move |p| {
                        if matches!(p.kind.as_str(), "video" | "manual")
                            || p.width == 0
                            || small.is_some_and(|small| (p.width <= LIST_PICTURE_MAX) != small)
                        {
                            return None;
                        }
                        game.media
                            .get(&p.kind)
                            .map(|src| (std::path::PathBuf::from(src), p.width.min(4096)))
                    })
                })
                .collect()
        };
        let (close, rest) = order.split_at(near);
        let mut jobs = wanted(close, None);
        jobs.extend(wanted(rest, Some(true)));
        jobs.extend(wanted(rest, Some(false)));
        state.thumbnails.prepare_list(jobs);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn library_search<R: Runtime>(
    app: AppHandle<R>,
    query: String,
    limit: Option<usize>,
) -> ApiResult<Vec<Game>> {
    off_thread(app, move |state| {
        let games = state
            .library()
            .search(&query, limit.unwrap_or(50).min(500))?;
        Ok(games.into_iter().map(expose).collect())
    })
    .await
}

/// Games in a random order, for a screensaver: those with any of `media` (any game when
/// empty), only favourites or one system's when asked. At most 200.
#[tauri::command]
pub async fn library_random<R: Runtime>(
    app: AppHandle<R>,
    media: Option<Vec<String>>,
    limit: Option<usize>,
    favorites: Option<bool>,
    system: Option<String>,
) -> ApiResult<Vec<Game>> {
    off_thread(app, move |state| {
        let media = media.unwrap_or_default();
        let kinds: Vec<&str> = media.iter().map(String::as_str).collect();
        let games = state.library().random_games(
            &kinds,
            limit.unwrap_or(50).min(200),
            favorites.unwrap_or(false),
            system.as_deref(),
        )?;
        Ok(games.into_iter().map(expose).collect())
    })
    .await
}

/// Deletes a game for good: only Forma's own menu can, and it asks first.
#[tauri::command]
pub async fn library_delete_game<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
) -> ApiResult<()> {
    let events = app.clone();
    let beside = off_thread(app, move |state| {
        caller.shell_only()?;
        let (game, system) = state
            .library()
            .launch_target(id)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game with id {id}")))?;
        let beside = alone_in_filter(state, id)?;
        forma_core::delete::delete_game(&state.paths, &game, &system)
            .map_err(|e| ApiError::new(ErrorCode::Internal, e))?;
        if beside.is_some() {
            list_on_screen(state).filter = ListFilter::default();
        }
        Ok(beside)
    })
    .await?;
    // The list on screen was filtered down to this game alone: every game again, the theme on
    // the one that was beside it, as near as can be to where it was.
    if let Some((game, system)) = beside {
        let _ = events.emit(
            EVENT_LIBRARY_CHANGED,
            LibraryChanged {
                systems: Vec::new(),
            },
        );
        let _ = events.emit(
            crate::shell::EVENT_NAV_GAME,
            serde_json::json!({ "game": game, "system": system }),
        );
    }
    Ok(())
}

/// When the filtered list on screen holds game `id` alone: the game beside it in the list
/// unfiltered (the next, or the one before at the end), and its system.
fn alone_in_filter(state: &AppState, id: i64) -> ApiResult<Option<(i64, String)>> {
    let (list, filter) = {
        let on_screen = list_on_screen(state);
        (on_screen.list.clone(), on_screen.filter.clone())
    };
    let Some((view, path)) = list.filter(|_| !filter.is_empty()) else {
        return Ok(None);
    };
    let Some(view) = state.views().get(&view).cloned() else {
        return Ok(None);
    };
    let library = state.library();
    let one = Page {
        offset: 0,
        limit: 2,
    };
    let kept = library.browse_filtered(&view, &path, one, &filter)?;
    let Listing::Games { items, .. } = &kept.listing else {
        return Ok(None);
    };
    if items.len() != 1 || items[0].id != id {
        return Ok(None);
    }
    let ids = library.game_ids(&view, &path)?;
    let Some(at) = ids.iter().position(|&g| g == id) else {
        return Ok(None);
    };
    let beside = ids
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| ids.get(i)));
    let Some(&beside) = beside else {
        return Ok(None);
    };
    Ok(library.game(beside)?.map(|g| (g.id, g.system)))
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
        write_back(&lib, &state, id);
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

/// The game's favourite and play stats to its `gamelist.xml`, if the user asked for that. A
/// failure (a gamelist that does not parse, a read-only share) only goes to the log: the
/// change is already safe in the library.
fn write_back(lib: &forma_core::library::Library, state: &AppState, id: i64) {
    if let Err(e) = lib.write_back_to_gamelist(&state.paths, id) {
        eprintln!("forma: gamelist write-back: {e}");
    }
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
pub async fn nav_browse<R: Runtime>(
    app: AppHandle<R>,
    view: String,
    path: Vec<String>,
    page: Option<Page>,
) -> ApiResult<Browse> {
    off_thread(app, move |state| {
        let view = state
            .views()
            .get(&view)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no view `{view}`")))?;
        let page = page.unwrap_or_default();
        let key = (view.id.clone(), path.clone());
        let filter = {
            let on_screen = list_on_screen(state);
            if on_screen.list.as_ref() == Some(&key) {
                on_screen.filter.clone()
            } else {
                ListFilter::default()
            }
        };
        let mut browse = state
            .library()
            .browse_filtered(&view, &path, page, &filter)?;
        // Its filter leaves nothing now (the last game it kept was deleted): every game again,
        // as a filter that would leave none is never set.
        if browse.filtered && matches!(browse.listing, Listing::Games { total: 0, .. }) {
            list_on_screen(state).filter = ListFilter::default();
            browse = state.library().browse(&view, &path, page)?;
        }
        if let Listing::Games { items, .. } = &mut browse.listing {
            *items = std::mem::take(items).into_iter().map(expose).collect();
            // A list being opened. A theme counting another list's games asks for one.
            let mut on_screen = list_on_screen(state);
            if page.offset == 0 && page.limit > 1 && on_screen.list.as_ref() != Some(&key) {
                *on_screen = ListOnScreen {
                    list: Some(key),
                    filter: ListFilter::default(),
                };
            }
        }
        Ok(browse)
    })
    .await
}

fn list_on_screen(state: &AppState) -> std::sync::MutexGuard<'_, ListOnScreen> {
    state
        .list_filter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The filter's result for `filter` on the list on screen, if there is one.
fn filter_result(state: &AppState, filter: Option<ListFilter>) -> ApiResult<Option<FilterOptions>> {
    let (list, current) = {
        let on_screen = list_on_screen(state);
        (on_screen.list.clone(), on_screen.filter.clone())
    };
    let Some((view, path)) = list else {
        return Ok(None);
    };
    let Some(view) = state.views().get(&view).cloned() else {
        return Ok(None);
    };
    let filter = filter.unwrap_or(current);
    Ok(Some(state.library().filter_options(&view, &path, &filter)?))
}

/// What the game list on screen can be filtered by, and its filter now. `null` with no list.
#[tauri::command]
pub async fn nav_filter_options<R: Runtime>(app: AppHandle<R>) -> ApiResult<Option<FilterOptions>> {
    off_thread(app, move |state| filter_result(state, None)).await
}

/// Filters the game list on screen; the theme reads it again on `library:changed`. Returns how
/// many games are left: at `0` the filter is not applied, so a list never ends up empty.
#[tauri::command]
pub fn nav_set_filter<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    filter: Option<ListFilter>,
) -> ApiResult<i64> {
    let filter = filter.unwrap_or_default();
    let Some(result) = filter_result(&state, Some(filter.clone()))? else {
        return Ok(0);
    };
    if result.total > 0 || filter.is_empty() {
        list_on_screen(&state).filter = filter;
        let _ = app.emit(
            EVENT_LIBRARY_CHANGED,
            LibraryChanged {
                systems: Vec::new(),
            },
        );
    }
    Ok(result.total)
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
pub async fn settings_get<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    key: String,
    scope: Option<Scope>,
) -> ApiResult<Option<Value>> {
    off_thread(app, move |state| {
        let key = permissions::setting_key(
            &theme(state)?,
            scope.unwrap_or(Scope::Theme),
            &key,
            caller.shell,
        )?;
        let raw = state.library().setting(&key)?;
        // Values are stored as JSON; a value written outside the SDK is returned as a string.
        Ok(raw.map(|r| serde_json::from_str(&r).unwrap_or(Value::String(r))))
    })
    .await
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
    if stored == crate::music::VIDEO_SOUND_SETTING {
        let sound = value.as_ref() != Some(&Value::Bool(false));
        state.video.set_sound(sound);
        state.music.duck(state.video.playing() && sound);
    }
    if stored == achievements::API_KEY_SETTING || stored == achievements::USERNAME_SETTING {
        crate::ra_match::account_changed();
    }
    // Turned on: check what is hashed (off, the results stay until it is on again).
    if stored == forma_core::dumps::SETTING && value.as_ref() != Some(&Value::Bool(false)) {
        crate::ra_match::dumps_changed();
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
    pub author: Option<String>,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    /// What each of `permissions` lets the theme do, in words, in the same order.
    pub permission_descriptions: Vec<String>,
    /// A theme the user installed and is not using: `themes.remove` takes it away.
    pub removable: bool,
    /// A newer version in the catalogue (as fetched last), for `themes.install` to fetch.
    pub update: Option<String>,
    pub current: bool,
    pub unavailable: Option<String>,
    /// Where to load the theme's logo from, if it has one.
    pub logo: Option<String>,
}

/// Re-scans the theme folders first, so a theme copied into `/userdata/themes-forma/` is
/// listed without restarting Forma. The plain fallback theme and copies another stands in for
/// are left out ([`crate::state::listed_themes`]).
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
    let catalogue = crate::catalogue::cached(&state);
    let themes = state
        .themes
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    crate::state::listed_themes(&themes, current.as_deref())
        .into_iter()
        .map(|t| {
            let manifest = t.manifest.as_ref();
            let permissions = manifest.map(|m| m.permissions.clone()).unwrap_or_default();
            let in_use = current.as_deref() == Some(t.id.as_str()) && t.is_available();
            ThemeInfo {
                id: t.id.clone(),
                name: manifest.map(|m| m.name.clone()),
                version: manifest.map(|m| m.version.clone()),
                origin: t.origin,
                author: manifest.and_then(|m| m.author.clone()),
                description: manifest.and_then(|m| m.description.clone()),
                permission_descriptions: permissions
                    .iter()
                    .map(|p| {
                        forma_core::themes::permission_description(p)
                            .map_or_else(|| p.clone(), str::to_owned)
                    })
                    .collect(),
                permissions,
                removable: t.origin == ThemeOrigin::User && !in_use,
                update: catalogue
                    .as_ref()
                    .and_then(|c| c.update_for(t))
                    .map(|c| c.version.clone()),
                current: in_use,
                unavailable: t.unavailable.clone(),
                logo: crate::state::theme_logo_url(t),
            }
        })
        .collect()
}

/// Deletes a theme the user installed, from `/userdata/themes-forma/`. Not one that comes with
/// Forma, and not the one in use: switch away first. A newer copy of a bundled theme going
/// brings the bundled one back.
#[tauri::command]
pub fn themes_remove(state: State<'_, AppState>, caller: Caller, id: String) -> ApiResult<()> {
    caller.require(&state, permissions::THEMES_MANAGE)?;
    let current = state.current_theme().map(|t| t.id);
    let theme = {
        let themes = state
            .themes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let in_use = |t: &&Theme| current.as_deref() == Some(t.id.as_str()) && t.is_available();
        let user = themes
            .iter()
            .filter(|t| t.id == id && t.origin == ThemeOrigin::User)
            .collect::<Vec<_>>();
        if user.iter().any(in_use) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("theme `{id}` is in use: switch to another one first"),
            ));
        }
        user.first().map(|t| (*t).clone()).ok_or_else(|| {
            let bundled = themes.iter().any(|t| t.id == id);
            ApiError::new(
                if bundled {
                    ErrorCode::Invalid
                } else {
                    ErrorCode::NotFound
                },
                if bundled {
                    format!("theme `{id}` comes with Forma and cannot be removed")
                } else {
                    format!("no installed theme `{id}`")
                },
            )
        })?
    };
    forma_core::themes::remove_user_theme(&state.paths, &theme)?;
    let found = forma_core::themes::discover(&state.bundled_themes, &state.paths);
    *state
        .themes
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = found;
    Ok(())
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
    let request = request.unwrap_or_default();
    let prepared = launch::prepare(&state.library(), &state.paths, id, &request, &controllers);
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            state.playing.store(false, Ordering::SeqCst);
            return Err(e.into());
        }
    };
    let fullname = state
        .library()
        .systems()
        .ok()
        .and_then(|s| s.into_iter().find(|s| s.name == prepared.game.system))
        .map_or_else(|| prepared.game.system.clone(), |s| s.fullname);
    if let Err(e) = crate::bios::before_launch(&app, &state, &prepared.game, &fullname, &request) {
        state.playing.store(false, Ordering::SeqCst);
        return Err(e);
    }
    // Never in the way of the game: at worst the emulator asks RetroAchievements as before.
    if let Err(e) = forma_core::ra_match::before_launch(
        &state.library(),
        &state.paths,
        &state.http_cache(),
        &prepared.game,
    ) {
        eprintln!("forma: achievements setting for the launch: {e}");
    }
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
    let _ = state.video.stop();
    state.thumbnails.pause(true);
    state.music.game(true);
    let started = SystemTime::now();

    let result = state.launcher.run(&prepared.command);
    if let Err(e) = forma_core::ra_match::after_launch(&state.paths) {
        eprintln!("forma: achievements setting after the launch: {e}");
    }
    let (seconds, exit_code, error) = match &result {
        Ok(exit) => {
            let recorded = {
                let mut lib = state.library();
                let recorded = launch::finish(&mut lib, game.id, exit, SystemTime::now());
                write_back(&lib, &state, game.id);
                recorded
            };
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
    state.thumbnails.pause(false);
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
    filter: Option<netplay::RoomFilter>,
) -> ApiResult<Fetched<Vec<netplay::Room>>> {
    off_thread(app, move |state| {
        let mut rooms = netplay::lobby(&*state.http, &state.http_cache())?;
        rooms.data = rooms_for_here(state, rooms.data, &filter.unwrap_or_default())?;
        Ok(rooms)
    })
    .await
}

/// How long hosts on the local network get to answer.
const LAN_WAIT: Duration = Duration::from_millis(1500);

/// RetroArch hosts on the local network, as its own "Refresh netplay host list" finds them.
#[tauri::command]
pub async fn netplay_lan<R: Runtime>(
    app: AppHandle<R>,
    filter: Option<netplay::RoomFilter>,
) -> ApiResult<Vec<netplay::Room>> {
    off_thread(app, move |state| {
        let rooms = netplay::discover_lan(LAN_WAIT);
        Ok(rooms_for_here(state, rooms, &filter.unwrap_or_default())?)
    })
    .await
}

/// Matches rooms to this box's games and cores, then filters them.
fn rooms_for_here(
    state: &AppState,
    mut rooms: Vec<netplay::Room>,
    filter: &netplay::RoomFilter,
) -> forma_core::Result<Vec<netplay::Room>> {
    static CORES: std::sync::OnceLock<netplay::Cores> = std::sync::OnceLock::new();
    // The library is locked only for the name match, never across a request.
    netplay::match_local(&mut rooms, &state.library())?;
    netplay::match_cores(&mut rooms, CORES.get_or_init(netplay::Cores::batocera));
    filter.apply(&mut rooms, &netplay::tcp_ping);
    Ok(rooms)
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
