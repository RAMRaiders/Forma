//! Batocera's system settings (`forma.system` in the SDK): what the box is, its display, audio,
//! language and region, name, security and storage, as ES's System settings menu offers them.

use forma_core::system_settings::{InfoItem, SystemSetting};
use tauri::{AppHandle, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{off_thread, theme};
use crate::permissions;
use crate::state::AppState;

fn allowed(state: &AppState) -> ApiResult<()> {
    permissions::require(&theme(state)?, permissions::SYSTEM_SETTINGS)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "system settings are only available on Batocera",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn system_info<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<InfoItem>> {
    off_thread(app, |state| {
        allowed(state)?;
        Ok(state.system_settings.info())
    })
    .await
}

#[tauri::command]
pub async fn system_settings<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<SystemSetting>> {
    off_thread(app, |state| {
        allowed(state)?;
        Ok(state.system_settings.get())
    })
    .await
}

/// Sets one, or unsets it with `null` (Batocera's default applies).
#[tauri::command]
pub async fn system_settings_set<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    value: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state)?;
        Ok(state.system_settings.set(&key, value.as_deref())?)
    })
    .await
}
