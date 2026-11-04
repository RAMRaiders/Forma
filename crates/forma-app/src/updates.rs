//! Batocera's own updates (`forma.updates` in the SDK): check, and install when asked.

use std::sync::atomic::{AtomicBool, Ordering};

use forma_core::updates::{UpdateCheck, Updates};
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

/// While updating: `{ message, percent }`.
pub const EVENT_UPDATES_PROGRESS: &str = "updates:progress";

/// One update at a time: a second would unpack over the first.
static UPDATING: AtomicBool = AtomicBool::new(false);

fn allowed(state: &AppState, caller: Caller) -> ApiResult<()> {
    caller.require(state, permissions::SYSTEM_UPDATE)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "Batocera updates are only available on Batocera",
        ));
    }
    Ok(())
}

fn updates(state: &AppState) -> Updates {
    Updates::new(state.paths.batocera_conf())
}

/// The box's version and the newest Batocera offers for it.
#[tauri::command]
pub async fn updates_check<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<UpdateCheck> {
    off_thread(app, move |state| {
        allowed(state, caller)?;
        Ok(updates(state).check(state.http.as_ref())?)
    })
    .await
}

/// Downloads and unpacks the update; resolves when done, the box then needing a reboot.
/// `updates:progress` reports on the way.
#[tauri::command]
pub async fn updates_install<R: Runtime>(app: AppHandle<R>, caller: Caller) -> ApiResult<()> {
    let events = app.clone();
    off_thread(app, move |state| {
        allowed(state, caller)?;
        if UPDATING.swap(true, Ordering::SeqCst) {
            return Err(ApiError::new(
                ErrorCode::Unsupported,
                "an update is already running",
            ));
        }
        let done = updates(state).install(|p| {
            let _ = events.emit(EVENT_UPDATES_PROGRESS, &p);
        });
        UPDATING.store(false, Ordering::SeqCst);
        Ok(done?)
    })
    .await
}
