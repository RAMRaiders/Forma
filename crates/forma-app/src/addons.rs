//! Forma add-ons (`forma.addons` in the SDK): the ones installed, turning them on and off,
//! removing them, and installing or updating them from the catalogue (its `addons` list). The
//! add-ons themselves are `forma_core::addons`; a scraper add-on's settings are its
//! credentials in the scraper.

use forma_core::SDK_API_VERSION;
use forma_core::addons::{self, Addon, DISABLED_SETTING, permission_descriptions};
use forma_core::net::Fetched;
use forma_core::themes::catalogue::{self, CatalogueTheme};
use forma_core::themes::{newer, satisfies};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::catalogue::{cached, catalogue_url};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub provides: Vec<String>,
    pub permissions: Vec<String>,
    pub permission_descriptions: Vec<String>,
    /// The sites it may reach.
    pub hosts: Vec<String>,
    pub enabled: bool,
    /// Why it cannot run, if it cannot.
    pub problem: Option<String>,
    /// Its id among the scraper's sources, for a scraper add-on (its settings live there).
    pub scraper_source: Option<String>,
    /// A newer version in the catalogue this Forma can run.
    pub update: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueAddon {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub permission_descriptions: Vec<String>,
    pub installed: Option<String>,
    pub update: bool,
    pub runs_here: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonCatalogue {
    pub addons: Vec<CatalogueAddon>,
}

fn disabled(state: &AppState) -> ApiResult<std::collections::HashSet<String>> {
    Ok(addons::disabled(
        state.library().setting(DISABLED_SETTING)?.as_deref(),
    ))
}

fn describe(
    a: &Addon,
    off: &std::collections::HashSet<String>,
    newest: Option<&CatalogueTheme>,
) -> AddonInfo {
    let m = a.manifest.as_ref();
    let version = m.map(|m| m.version.clone());
    AddonInfo {
        id: a.id.clone(),
        name: m.map_or_else(|| a.id.clone(), |m| m.name.clone()),
        author: m.and_then(|m| m.author.clone()),
        description: m.and_then(|m| m.description.clone()),
        provides: m.map(|m| m.provides.clone()).unwrap_or_default(),
        permissions: m.map(|m| m.permissions.clone()).unwrap_or_default(),
        permission_descriptions: m.map(permission_descriptions).unwrap_or_default(),
        hosts: m.map(|m| m.hosts.clone()).unwrap_or_default(),
        enabled: !off.contains(&a.id),
        problem: a.problem.clone(),
        scraper_source: m
            .filter(|m| a.usable() && m.provides.iter().any(|p| p == "scraper"))
            .map(|m| format!("addon:{}", m.id)),
        update: newest
            .filter(|c| satisfies(&c.sdk, SDK_API_VERSION))
            .filter(|c| version.as_deref().is_none_or(|v| newer(&c.version, v)))
            .map(|c| c.version.clone()),
        version,
    }
}

fn list(state: &AppState) -> ApiResult<Vec<AddonInfo>> {
    let off = disabled(state)?;
    let catalogue = cached(state);
    Ok(addons::discover(&state.paths)
        .iter()
        .map(|a| {
            describe(
                a,
                &off,
                catalogue.as_ref().and_then(|c| c.find_addon(&a.id)),
            )
        })
        .collect())
}

#[tauri::command]
pub async fn addons_list<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Vec<AddonInfo>> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        list(state)
    })
    .await
}

#[tauri::command]
pub async fn addons_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
    enabled: bool,
) -> ApiResult<Vec<AddonInfo>> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        if !addons::discover(&state.paths).iter().any(|a| a.id == id) {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no add-on `{id}`"),
            ));
        }
        let mut off: Vec<String> = disabled(state)?.into_iter().filter(|d| *d != id).collect();
        if !enabled {
            off.push(id);
        }
        off.sort();
        let text = serde_json::to_string(&off)
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
        state.library().set_setting(DISABLED_SETTING, Some(&text))?;
        state.scraper.addons_changed();
        list(state)
    })
    .await
}

#[tauri::command]
pub async fn addons_remove<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        addons::remove(&state.paths, &id)?;
        state.scraper.addons_changed();
        Ok(())
    })
    .await
}

/// The catalogue's add-ons, with what is installed: fetched at most every few hours.
#[tauri::command]
pub async fn addons_catalogue<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<Fetched<AddonCatalogue>> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        let fetched =
            catalogue::fetch_catalogue(&*state.http, &state.http_cache(), &catalogue_url(state))?;
        let installed = addons::discover(&state.paths);
        let addons = fetched
            .data
            .addons
            .iter()
            .map(|c| {
                let mine = installed
                    .iter()
                    .find(|a| a.id == c.id)
                    .and_then(|a| a.manifest.as_ref())
                    .map(|m| m.version.clone());
                let runs_here = satisfies(&c.sdk, SDK_API_VERSION);
                CatalogueAddon {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    version: c.version.clone(),
                    author: c.author.clone(),
                    description: c.description.clone(),
                    permission_descriptions: c
                        .permissions
                        .iter()
                        .map(|p| {
                            addons::PERMISSIONS
                                .iter()
                                .find(|(k, _)| k == p)
                                .map_or_else(|| p.clone(), |(_, d)| (*d).to_owned())
                        })
                        .collect(),
                    permissions: c.permissions.clone(),
                    update: runs_here && mine.as_deref().is_some_and(|v| newer(&c.version, v)),
                    installed: mine,
                    runs_here,
                }
            })
            .collect();
        Ok(Fetched {
            data: AddonCatalogue { addons },
            stale: fetched.stale,
            fetched_at: fetched.fetched_at,
        })
    })
    .await
}

/// Installs or updates an add-on from the catalogue. Resolves once it is in place.
#[tauri::command]
pub async fn addons_install<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
) -> ApiResult<AddonInfo> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        let fetched =
            catalogue::fetch_catalogue(&*state.http, &state.http_cache(), &catalogue_url(state))?;
        let entry = fetched.data.find_addon(&id).cloned().ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no add-on `{id}` in the catalogue"),
            )
        })?;
        let addon = addons::install(&*state.http, &state.paths, &entry)?;
        state.scraper.addons_changed();
        Ok(describe(&addon, &disabled(state)?, None))
    })
    .await
}
