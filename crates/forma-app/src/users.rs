//! Who is playing (#301): the users, entering as one, and what only the admin may do.
//!
//! Entering as a user swaps what is theirs alone outside the library: their RetroAchievements
//! and netplay accounts in `batocera.conf` (and Forma's RetroAchievements settings), and their
//! saves folder over `saves/` (forma_core::saves::UserSaves).

use forma_core::batocera::ACCOUNT_KEYS;
use forma_core::library::{ADMIN, User};
use forma_core::net::achievements;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{EVENT_LIBRARY_CHANGED, LibraryChanged, off_thread};
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

/// Sent with the user in, once another enters (or the one in was removed).
pub const EVENT_USERS_CHANGED: &str = "users:changed";

/// What a user who is not the admin may still do: shut down and restart, and set up netplay
/// and RetroAchievements (Batocera's settings Forma reaches are only those). Finding netplay
/// games asks for no permission.
const USER_PERMISSIONS: [&str; 2] = [permissions::SYSTEM_POWER, permissions::SETTINGS_BATOCERA];

/// Forma's own settings that are each user's: their RetroAchievements account, for the
/// profile and the games' achievements Forma shows.
pub(crate) const OWN_SETTINGS: [&str; 2] = [
    achievements::USERNAME_SETTING,
    achievements::API_KEY_SETTING,
];

/// How a user's copy of an account or setting is kept among their settings.
const OWN_PREFIX: &str = "own.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Users {
    /// The admin first, then by name.
    pub users: Vec<User>,
    /// The one in.
    pub current: User,
}

/// Fails unless the user in is the admin.
pub(crate) fn require_admin(state: &AppState) -> ApiResult<()> {
    if state.library().current_user()?.admin {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "only the admin can do this",
        ))
    }
}

/// Fails when `permission` is the admin's alone and the user in is not the admin.
pub(crate) fn require_for_user(state: &AppState, permission: &str) -> ApiResult<()> {
    if USER_PERMISSIONS.contains(&permission) {
        Ok(())
    } else {
        require_admin(state)
    }
}

fn users(state: &AppState) -> ApiResult<Users> {
    let library = state.library();
    Ok(Users {
        users: library.users()?,
        current: library.current_user()?,
    })
}

/// `id` in from now on, if `pin` lets them: what is theirs swapped in.
fn enter(state: &AppState, id: i64, pin: Option<&str>) -> ApiResult<User> {
    if state.playing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(ApiError::new(
            ErrorCode::Busy,
            "a game is running: quit it first",
        ));
    }
    let (from, known, allowed) = {
        let library = state.library();
        let known = library.users()?.iter().any(|u| u.id == id);
        (
            library.current_user()?,
            known,
            library.pin_matches(id, pin)?,
        )
    };
    if !known {
        return Err(ApiError::new(ErrorCode::NotFound, format!("no user {id}")));
    }
    if !allowed {
        return Err(ApiError::new(ErrorCode::PermissionDenied, "wrong PIN"));
    }
    if from.id != id {
        // Batocera's accounts: elsewhere there are none to swap.
        if state.on_batocera {
            keep_accounts(state, from.id)?;
            put_accounts(state, id)?;
        }
        if let Some(saves) = &state.saves
            && let Err(e) = saves.switch(id)
        {
            // Theirs could not be put in place: the one in stays, with their own accounts.
            if state.on_batocera {
                let _ = put_accounts(state, from.id);
            }
            return Err(e.into());
        }
        state.library().enter(id)?;
        crate::ra_match::account_changed();
    }
    Ok(state.library().current_user()?)
}

/// The accounts of the user leaving, kept as theirs.
fn keep_accounts(state: &AppState, user: i64) -> ApiResult<()> {
    let accounts = state.batocera.accounts();
    let mut library = state.library();
    for (key, value) in accounts {
        library.set_user_setting(
            user,
            &format!("{OWN_PREFIX}{key}"),
            Some(value.as_str()).filter(|v| !v.is_empty()),
        )?;
    }
    for key in OWN_SETTINGS {
        let value = library.setting(key)?;
        library.set_user_setting(user, &format!("{OWN_PREFIX}{key}"), value.as_deref())?;
    }
    Ok(())
}

/// The accounts of the user entering, in place (none for a user who has not set them).
fn put_accounts(state: &AppState, user: i64) -> ApiResult<()> {
    let mut library = state.library();
    let mut accounts = Vec::new();
    for key in ACCOUNT_KEYS {
        if let Some(value) = library.user_setting(user, &format!("{OWN_PREFIX}{key}"))? {
            accounts.push((key, value));
        }
    }
    for key in OWN_SETTINGS {
        let value = library.user_setting(user, &format!("{OWN_PREFIX}{key}"))?;
        library.set_setting(key, value.as_deref())?;
    }
    drop(library);
    let pairs: Vec<(&str, &str)> = accounts.iter().map(|(k, v)| (*k, v.as_str())).collect();
    state.batocera.set_accounts(&pairs)?;
    Ok(())
}

/// At start: the saves folder of the user who was in last, in place (a restart left it so,
/// a power cut did not).
pub(crate) fn restore(state: &AppState) {
    let Some(saves) = &state.saves else { return };
    let user = state.library().current_user().map_or(ADMIN, |u| u.id);
    if let Err(e) = saves.switch(user) {
        eprintln!("forma: user {user}'s saves could not be put in place: {e}");
    }
}

/// Every list shows the user in's games anew.
fn changed<R: Runtime>(app: &AppHandle<R>, current: &User) {
    let _ = app.emit(EVENT_USERS_CHANGED, current);
    let state = app.state::<AppState>();
    if let Ok(systems) = state.library().systems() {
        let _ = app.emit(
            EVENT_LIBRARY_CHANGED,
            LibraryChanged {
                systems: systems.into_iter().map(|s| s.name).collect(),
            },
        );
    }
}

/// The users, and the one in. Any theme may know who is playing.
#[tauri::command]
pub async fn users_list<R: Runtime>(app: AppHandle<R>) -> ApiResult<Users> {
    off_thread(app, users).await
}

/// Enters as user `id`; `pin` when they have one. Forma's own menu only.
#[tauri::command]
pub async fn users_enter<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
    pin: Option<String>,
) -> ApiResult<User> {
    caller.shell_only()?;
    let events = app.clone();
    let user = off_thread(app, move |state| enter(state, id, pin.as_deref())).await?;
    changed(&events, &user);
    Ok(user)
}

/// A new user (the admin only), their list the whole pool.
#[tauri::command]
pub async fn users_add<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    name: String,
) -> ApiResult<User> {
    caller.shell_only()?;
    off_thread(app, move |state| {
        require_admin(state)?;
        Ok(state.library().add_user(&name)?)
    })
    .await
}

/// Removes a user and what is theirs in the library (the admin only, never themselves). Their
/// saves folder stays on disk.
#[tauri::command]
pub async fn users_remove<R: Runtime>(app: AppHandle<R>, caller: Caller, id: i64) -> ApiResult<()> {
    caller.shell_only()?;
    off_thread(app, move |state| {
        require_admin(state)?;
        Ok(state.library().remove_user(id)?)
    })
    .await
}

/// Sets the admin's PIN (4 to 8 digits), or with none, takes it off. The admin only.
#[tauri::command]
pub async fn users_set_pin<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    pin: Option<String>,
) -> ApiResult<()> {
    caller.shell_only()?;
    off_thread(app, move |state| {
        require_admin(state)?;
        Ok(state.library().set_pin(pin.as_deref())?)
    })
    .await
}

/// Puts a game of the pool back on the user's list, or takes it off.
#[tauri::command]
pub async fn library_set_listed<R: Runtime>(
    app: AppHandle<R>,
    id: i64,
    listed: bool,
) -> ApiResult<()> {
    let events = app.clone();
    let system = off_thread(app, move |state| {
        let mut library = state.library();
        let game = library
            .game(id)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game {id}")))?;
        library.set_listed(id, listed)?;
        Ok(game.system)
    })
    .await?;
    let _ = events.emit(
        EVENT_LIBRARY_CHANGED,
        LibraryChanged {
            systems: vec![system],
        },
    );
    Ok(())
}

/// The pool's games of a system the user took off their list, to put back.
#[tauri::command]
pub async fn library_unlisted<R: Runtime>(
    app: AppHandle<R>,
    system: String,
) -> ApiResult<Vec<forma_core::library::Game>> {
    off_thread(app, move |state| {
        Ok(state.library().unlisted_games(&system)?)
    })
    .await
}
