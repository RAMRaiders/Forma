//! The theme catalogue (`forma.themes.catalogue()` and `install()` in the SDK): themes to
//! install and update from Forma's menu.

use forma_core::net::Fetched;
use forma_core::themes::catalogue::{
    self, CATALOGUE_URL_SETTING, Catalogue, DEFAULT_CATALOGUE_URL,
};
use forma_core::themes::{Theme, permission_description};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

/// The catalogue to use: the user's setting, else Forma's own.
pub fn catalogue_url(state: &AppState) -> String {
    state
        .library()
        .setting_text(CATALOGUE_URL_SETTING)
        .ok()
        .flatten()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CATALOGUE_URL.to_owned())
}

/// The catalogue fetched last, without the network: for marking updates in `themes.list()`.
pub fn cached(state: &AppState) -> Option<Catalogue> {
    catalogue::cached_catalogue(&state.http_cache(), &catalogue_url(state))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    /// What each of `permissions` would let the theme do, in words, in the same order.
    pub permission_descriptions: Vec<String>,
    pub previews: Vec<String>,
    /// The version installed now, if any.
    pub installed: Option<String>,
    /// Installed, and the catalogue has a newer version this Forma can run.
    pub update: bool,
    /// This Forma's SDK is one the theme can use.
    pub runs_here: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueInfo {
    pub themes: Vec<CatalogueEntry>,
    /// Entries left out of the catalogue, and why.
    pub skipped: Vec<String>,
}

fn describe(c: &Catalogue, installed: &[Theme]) -> CatalogueInfo {
    let themes = c
        .themes
        .iter()
        .map(|t| {
            // The copy Forma would load: a user copy standing in, else the bundled one.
            let mine = installed
                .iter()
                .filter(|i| i.id == t.id && i.manifest.is_some())
                .find(|i| i.is_available())
                .or_else(|| {
                    installed
                        .iter()
                        .find(|i| i.id == t.id && i.manifest.is_some())
                });
            CatalogueEntry {
                id: t.id.clone(),
                name: t.name.clone(),
                version: t.version.clone(),
                author: t.author.clone(),
                description: t.description.clone(),
                permission_descriptions: t
                    .permissions
                    .iter()
                    .map(|p| permission_description(p).map_or_else(|| p.clone(), str::to_owned))
                    .collect(),
                permissions: t.permissions.clone(),
                previews: t.previews.clone(),
                installed: mine
                    .and_then(|m| m.manifest.as_ref())
                    .map(|m| m.version.clone()),
                update: mine.is_some_and(|m| c.update_for(m).is_some()),
                runs_here: t.runs_here(),
            }
        })
        .collect();
    CatalogueInfo {
        themes,
        skipped: c.skipped.clone(),
    }
}

/// The catalogue: fetched at most every few hours, the last one offline (`stale`).
#[tauri::command]
pub async fn themes_catalogue<R: Runtime>(app: AppHandle<R>) -> ApiResult<Fetched<CatalogueInfo>> {
    off_thread(app, move |state| {
        let url = catalogue_url(state);
        let fetched = catalogue::fetch_catalogue(&*state.http, &state.http_cache(), &url)?;
        let installed = rediscover(state);
        Ok(Fetched {
            data: describe(&fetched.data, &installed),
            stale: fetched.stale,
            fetched_at: fetched.fetched_at,
        })
    })
    .await
}

/// Installs a theme from the catalogue, or updates it. Resolves once it is in place; the
/// theme in use, updated, is reloaded into its new version.
#[tauri::command]
pub async fn themes_install<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
) -> ApiResult<()> {
    let window = app.clone();
    off_thread(app, move |state| {
        caller.require(state, permissions::THEMES_MANAGE)?;
        let url = catalogue_url(state);
        let fetched = catalogue::fetch_catalogue(&*state.http, &state.http_cache(), &url)?;
        let entry = fetched.data.find(&id).cloned().ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no theme `{id}` in the catalogue"),
            )
        })?;
        catalogue::install(&*state.http, &state.paths, &entry)?;
        rediscover(state);
        if let Some(current) = state.current_theme()
            && current.id == id
        {
            crate::boot::show_theme(&window, state, &current)
                .map_err(|e| ApiError::new(ErrorCode::Internal, e))?;
        }
        Ok(())
    })
    .await
}

/// Scans the theme folders again and keeps the result, as `themes.list()` does.
fn rediscover(state: &AppState) -> Vec<Theme> {
    let found = forma_core::themes::discover(&state.bundled_themes, &state.paths);
    *state
        .themes
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = found.clone();
    found
}
