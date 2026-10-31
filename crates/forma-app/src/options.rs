//! Emulator, core and game options (`forma.options` in the SDK): Batocera's, at the global,
//! system and game levels, as ES offers them in its game settings menus.

use std::path::Path;

use forma_core::options::{OptionsPage, Scope};
use forma_core::systems::{System, load_systems};
use serde::Deserialize;
use tauri::{AppHandle, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{off_thread, theme};
use crate::permissions;
use crate::state::AppState;

/// Where an option is set, as a theme names it: a game by its id.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "level", rename_all = "camelCase")]
pub enum OptionsScope {
    Global,
    System { system: String },
    Game { game: i64 },
}

fn allowed(state: &AppState) -> ApiResult<()> {
    permissions::require(&theme(state)?, permissions::SETTINGS_EMULATORS)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "emulator options are only available on Batocera",
        ));
    }
    Ok(())
}

/// The core's scope, and the system it needs (its emulators and cores).
fn resolve(state: &AppState, scope: OptionsScope) -> ApiResult<(Scope, Option<System>)> {
    let (scope, name) = match scope {
        OptionsScope::Global => return Ok((Scope::Global, None)),
        OptionsScope::System { system } => (Scope::System(system.clone()), system),
        OptionsScope::Game { game } => {
            let game = state
                .library()
                .game(game)?
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game {game}")))?;
            // ES keys a game by its file name alone.
            let file = Path::new(&game.path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&game.path)
                .to_owned();
            (
                Scope::Game {
                    system: game.system.clone(),
                    file,
                },
                game.system,
            )
        }
    };
    let system = load_systems(&state.paths)?
        .systems
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no system `{name}`")))?;
    Ok((scope, Some(system)))
}

#[tauri::command]
pub async fn options_get<R: Runtime>(
    app: AppHandle<R>,
    scope: OptionsScope,
) -> ApiResult<OptionsPage> {
    off_thread(app, move |state| {
        allowed(state)?;
        let (scope, system) = resolve(state, scope)?;
        Ok(state.options.page(&scope, system.as_ref())?)
    })
    .await
}

/// Sets an option, or unsets it with `null`: the level then inherits.
#[tauri::command]
pub async fn options_set<R: Runtime>(
    app: AppHandle<R>,
    scope: OptionsScope,
    key: String,
    value: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state)?;
        let (scope, system) = resolve(state, scope)?;
        Ok(state
            .options
            .set(&scope, system.as_ref(), &key, value.as_deref())?)
    })
    .await
}
