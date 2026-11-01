//! Batocera's content downloader (`forma.store` in the SDK): free games, ports, decorations and
//! music from Batocera's package store.

use forma_core::store::Package;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{off_thread, theme};
use crate::permissions;
use crate::state::AppState;

/// While a package installs: `{ name, stage, percent }`.
pub const EVENT_STORE_PROGRESS: &str = "store:progress";

#[derive(Debug, Clone, Serialize)]
struct StoreProgress<'a> {
    name: &'a str,
    stage: &'static str,
    percent: u8,
}

fn allowed(state: &AppState) -> ApiResult<()> {
    permissions::require(&theme(state)?, permissions::STORE_MANAGE)?;
    if !state.on_batocera {
        return Err(ApiError::new(
            ErrorCode::Unsupported,
            "the content downloader is only available on Batocera",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn store_list<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<Package>> {
    off_thread(app, |state| {
        allowed(state)?;
        Ok(state.store.list()?)
    })
    .await
}

/// Reads the catalogue again from Batocera's server.
#[tauri::command]
pub async fn store_refresh<R: Runtime>(app: AppHandle<R>) -> ApiResult<()> {
    off_thread(app, |state| {
        allowed(state)?;
        Ok(state.store.refresh()?)
    })
    .await
}

/// Resolves once installed; `store:progress` reports on the way.
#[tauri::command]
pub async fn store_install<R: Runtime>(app: AppHandle<R>, name: String) -> ApiResult<()> {
    let events = app.clone();
    off_thread(app, move |state| {
        allowed(state)?;
        Ok(state.store.install(&name, |p| {
            let _ = events.emit(
                EVENT_STORE_PROGRESS,
                StoreProgress {
                    name: &name,
                    stage: p.stage,
                    percent: p.percent,
                },
            );
        })?)
    })
    .await
}

#[tauri::command]
pub async fn store_remove<R: Runtime>(app: AppHandle<R>, name: String) -> ApiResult<()> {
    off_thread(app, move |state| {
        allowed(state)?;
        Ok(state.store.remove(&name)?)
    })
    .await
}

/// Updates every installed package that has a newer version.
#[tauri::command]
pub async fn store_update<R: Runtime>(app: AppHandle<R>) -> ApiResult<()> {
    off_thread(app, |state| {
        allowed(state)?;
        Ok(state.store.update()?)
    })
    .await
}
