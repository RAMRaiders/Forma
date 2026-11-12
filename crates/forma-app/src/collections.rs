//! The user's collections (`forma.collections` in the SDK): EmulationStation's custom
//! collections, read and written by the core. Like favourites they are the user's own lists,
//! so any theme may change them.

use forma_core::hierarchy::load_views;
use forma_core::library::Collection;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::ApiResult;
use crate::commands::{EVENT_LIBRARY_CHANGED, EVENT_VIEWS_CHANGED, LibraryChanged, off_thread};
use crate::state::AppState;

#[tauri::command]
pub async fn collections_list<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<Collection>> {
    off_thread(app, move |state| Ok(state.library().collections()?)).await
}

/// The collections a game is in.
#[tauri::command]
pub async fn collections_of<R: Runtime>(app: AppHandle<R>, game_id: i64) -> ApiResult<Vec<String>> {
    off_thread(app, move |state| {
        Ok(state.library().collections_of(game_id)?)
    })
    .await
}

#[tauri::command]
pub fn collections_create<R: Runtime>(app: AppHandle<R>, name: String) -> ApiResult<String> {
    let state = app.state::<AppState>();
    let name = state.library().create_collection(&state.paths, &name)?;
    changed(&app, None);
    Ok(name)
}

#[tauri::command]
pub fn collections_delete<R: Runtime>(app: AppHandle<R>, name: String) -> ApiResult<()> {
    let state = app.state::<AppState>();
    state.library().delete_collection(&state.paths, &name)?;
    changed(&app, None);
    Ok(())
}

/// Puts a game in a collection (`member`), or takes it out.
#[tauri::command]
pub fn collections_set<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    game_id: i64,
    member: bool,
) -> ApiResult<()> {
    let state = app.state::<AppState>();
    let system = {
        let mut lib = state.library();
        lib.set_in_collection(&state.paths, &name, game_id, member)?;
        lib.game(game_id)?.map(|g| g.system)
    };
    changed(&app, system);
    Ok(())
}

/// The collections view follows the collections; a game's system shows its new membership.
fn changed<R: Runtime>(app: &AppHandle<R>, system: Option<String>) {
    let state = app.state::<AppState>();
    *state
        .views
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = load_views(&state.paths);
    let _ = app.emit(EVENT_VIEWS_CHANGED, ());
    if let Some(system) = system {
        let _ = app.emit(
            EVENT_LIBRARY_CHANGED,
            LibraryChanged {
                systems: vec![system],
            },
        );
    }
}
