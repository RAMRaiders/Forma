//! Batocera's system settings (`forma.system` in the SDK): what the box is, its display, audio,
//! language and region, name, security and storage, as ES's System settings menu offers them.

use forma_core::system_settings::{InfoItem, SystemSetting};
use tauri::{AppHandle, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

fn allowed(state: &AppState, caller: Caller) -> ApiResult<()> {
    caller.require(state, permissions::SYSTEM_SETTINGS)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "system settings are only available on Batocera",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn system_info<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<InfoItem>> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.system_settings.info())
    })
    .await
}

#[tauri::command]
pub async fn system_settings<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<SystemSetting>> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.system_settings.get())
    })
    .await
}

/// Sets one, or unsets it with `null` (Batocera's default applies).
#[tauri::command]
pub async fn system_settings_set<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    key: String,
    value: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.system_settings.set(&key, value.as_deref())?)
    })
    .await
}
