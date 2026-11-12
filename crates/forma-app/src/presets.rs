//! Forma's presets (`forma.presets` in the SDK): ready-made setups for a kind of machine and
//! screen, Batocera's options and Forma's CRT shader under the user's own. Same permission as
//! the emulator options.

use forma_core::presets::PresetSummary;
use tauri::{AppHandle, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::options::allowed;
use crate::shell::Caller;

#[tauri::command]
pub async fn presets_list<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<PresetSummary>> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.presets.list())
    })
    .await
}

/// Makes a preset the active one, or none with `null`; Forma's CRT shader follows at once, the
/// options at the next launch.
#[tauri::command]
pub async fn presets_set_active<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: Option<String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        state.presets.set_active(id.as_deref())?;
        Ok(state.shaders.refresh()?)
    })
    .await
}
