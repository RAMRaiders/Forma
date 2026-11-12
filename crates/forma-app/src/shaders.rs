//! Forma's CRT shader for games (`forma.shaders` in the SDK): koko-aio, switched on and tuned at
//! the global, system and game levels. Same permission and levels as the emulator options.

use forma_core::shaders::ShaderPage;
use tauri::{AppHandle, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::options::{OptionsScope, allowed, resolve};
use crate::shell::Caller;

#[tauri::command]
pub async fn shaders_get<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    scope: OptionsScope,
) -> ApiResult<ShaderPage> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        let (scope, _) = resolve(state, scope)?;
        Ok(state.shaders.page(&scope)?)
    })
    .await
}

/// Sets one setting, or unsets it with `null`: the level then inherits.
#[tauri::command]
pub async fn shaders_set<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    scope: OptionsScope,
    key: String,
    value: Option<f64>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        let (scope, _) = resolve(state, scope)?;
        Ok(state.shaders.set(&scope, &key, value)?)
    })
    .await
}

/// The preset a level starts from, or `null` for the level above's.
#[tauri::command]
pub async fn shaders_set_preset<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    scope: OptionsScope,
    preset: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        let (scope, _) = resolve(state, scope)?;
        Ok(state.shaders.set_preset(&scope, preset.as_deref())?)
    })
    .await
}

/// Chooses the shader at a level: `none`, `crt:<preset>` (Forma's koko-aio from that preset),
/// `set:<name>` (one of Batocera's sets), or `null` for the level above's (not globally).
#[tauri::command]
pub async fn shaders_choose<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    scope: OptionsScope,
    choice: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        let scope = resolve(state, scope)?.0;
        Ok(state.shaders.choose(&scope, choice.as_deref())?)
    })
    .await
}

/// Switches Forma's shader on for every game, or off (Batocera's shader set back); with a
/// system's or a game's `scope`, on or off there over the level above, or `null` to inherit.
#[tauri::command]
pub async fn shaders_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    enabled: Option<bool>,
    scope: Option<OptionsScope>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        let scope = match scope {
            Some(scope) => resolve(state, scope)?.0,
            None => forma_core::options::Scope::Global,
        };
        Ok(state.shaders.set_enabled_at(&scope, enabled)?)
    })
    .await
}
