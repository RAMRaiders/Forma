//! Magazines (`forma.magazines` in the SDK): scanned magazines in `/userdata/magazines`, read in
//! Forma's own reader. Themes list them and ask Forma to open one; the reader is Forma's.

use std::collections::BTreeMap;

use forma_core::magazines::{Format, scan};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::state::AppState;

/// Asks Forma's shell to open its magazines page, or one magazine: `{ id }` (`null`: the list).
pub const EVENT_MAGAZINES_OPEN: &str = "magazines:open";

/// Forma's setting holding where the user is in each magazine: `{ id: page }`.
const READING_SETTING: &str = "magazines.reading";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagazineInfo {
    pub id: String,
    pub title: String,
    /// The folders it is in, `/`-separated; empty at the top.
    pub folder: String,
    pub format: Format,
    /// Where the user left it (from 0), if they opened it before.
    pub page: Option<usize>,
    /// Its pages' URL, with `{page}` for the page: see `forma.magazines.pageUrl`.
    pub page_url: String,
}

fn reading(state: &AppState) -> BTreeMap<String, usize> {
    state
        .library()
        .setting(READING_SETTING)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn magazines_list<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<MagazineInfo>> {
    off_thread(app, |state| {
        let at = reading(state);
        Ok(scan(&state.paths)
            .into_iter()
            .map(|m| MagazineInfo {
                page: at.get(&m.id).copied(),
                page_url: crate::protocol::magazine_url(&m.id),
                id: m.id,
                title: m.title,
                folder: m.folder,
                format: m.format,
            })
            .collect())
    })
    .await
}

/// How many pages a magazine has. The first time, a big archive takes a moment to read.
#[tauri::command]
pub async fn magazines_pages<R: Runtime>(app: AppHandle<R>, id: String) -> ApiResult<usize> {
    off_thread(app, move |state| Ok(state.magazines.page_count(&id)?)).await
}

/// Remembers the page the user is on, to open there next time.
#[tauri::command]
pub async fn magazines_set_page<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    page: usize,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        let mut at = reading(state);
        at.insert(id, page);
        // Gone magazines drop out, so the setting does not grow forever.
        let known: std::collections::HashSet<String> =
            scan(&state.paths).into_iter().map(|m| m.id).collect();
        at.retain(|id, _| known.contains(id));
        let value = serde_json::to_string(&at).unwrap_or_else(|_| "{}".into());
        state.library().set_setting(READING_SETTING, Some(&value))?;
        Ok(())
    })
    .await
}

/// Opens Forma's magazines, or one magazine, over the theme (Forma's reader, for every theme).
#[tauri::command]
pub fn magazines_open<R: Runtime>(app: AppHandle<R>, id: Option<String>) -> ApiResult<()> {
    let _ = app.emit(EVENT_MAGAZINES_OPEN, serde_json::json!({ "id": id }));
    Ok(())
}
