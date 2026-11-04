//! Bluetooth controllers (`forma.bluetooth` in the SDK), through Batocera's own tools.

use forma_core::bluetooth::{BluetoothDevice, BluetoothStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

/// While pairing: `{ message }`, Batocera's word on what it is doing.
pub const EVENT_BLUETOOTH_PAIRING: &str = "bluetooth:pairing";

#[derive(Debug, Clone, Serialize)]
struct Pairing<'a> {
    message: &'a str,
}

fn allowed(state: &AppState, caller: Caller) -> ApiResult<()> {
    caller.require(state, permissions::SYSTEM_CONTROLLERS)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "Bluetooth settings are only available on Batocera",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn bluetooth_status<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<BluetoothStatus> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.status()?)
    })
    .await
}

#[tauri::command]
pub async fn bluetooth_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    enabled: bool,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.set_enabled(enabled)?)
    })
    .await
}

/// Looks for pads in pairing mode for up to a minute; resolves with those it paired.
/// `bluetooth:pairing` reports on the way.
#[tauri::command]
pub async fn bluetooth_pair<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<BluetoothDevice>> {
    let events = app.clone();
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.pair(|message| {
            let _ = events.emit(EVENT_BLUETOOTH_PAIRING, Pairing { message });
        })?)
    })
    .await
}

#[tauri::command]
pub async fn bluetooth_connect<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    address: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.connect(&address)?)
    })
    .await
}

#[tauri::command]
pub async fn bluetooth_disconnect<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    address: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.disconnect(&address)?)
    })
    .await
}

#[tauri::command]
pub async fn bluetooth_forget<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    address: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(state.bluetooth.forget(&address)?)
    })
    .await
}
