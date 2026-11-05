//! Forma add-ons (`forma.addons` in the SDK): the ones installed, turning them on and off,
//! removing them, and installing or updating them from the catalogue (its `addons` list). The
//! add-ons themselves are `forma_core::addons`; a scraper add-on's settings are its
//! credentials in the scraper.

use forma_core::SDK_API_VERSION;
use forma_core::addons::{self, Addon, DISABLED_SETTING, MenuDecl, permission_descriptions};
use forma_core::net::Fetched;
use forma_core::scraper::{CREDENTIALS_SETTING, Credentials, load_credentials};
use forma_core::themes::catalogue::DEFAULT_CATALOGUE_URL;
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
    /// Its id among the scraper's sources, for a scraper add-on.
    pub scraper_source: Option<String>,
    /// What the user enters for it; secret values are never sent back.
    pub settings: Vec<crate::scraper::CredentialInfo>,
    /// Its tile in Forma's menu, for a `menu` add-on.
    pub menu: Option<MenuDecl>,
    /// It has a system part (scripts run with full rights).
    pub system: bool,
    /// Installed from Forma's own catalogue: a system part may run.
    pub verified: bool,
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

/// The key an add-on's settings are kept under, with the scraper's credentials: the same as
/// its scraper source id, so a scraper add-on's settings are its account.
pub fn settings_key(id: &str) -> String {
    format!("addon:{id}")
}

/// What the user entered for an add-on.
pub fn settings_of(state: &AppState, id: &str) -> Credentials {
    let stored = state.library().setting(CREDENTIALS_SETTING).ok().flatten();
    load_credentials(stored.as_deref())
        .remove(&settings_key(id))
        .unwrap_or_default()
}

fn describe(
    a: &Addon,
    off: &std::collections::HashSet<String>,
    newest: Option<&CatalogueTheme>,
    entered: &Credentials,
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
        settings: m
            .map(|m| {
                m.settings
                    .iter()
                    .map(|d| {
                        let value = entered.get(&d.key).filter(|v| !v.trim().is_empty());
                        crate::scraper::CredentialInfo {
                            key: d.key.clone(),
                            label: d.label.clone(),
                            secret: d.secret,
                            required: d.required,
                            set: value.is_some(),
                            value: value.filter(|_| !d.secret).cloned(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        menu: m
            .filter(|m| m.provides("menu"))
            .and_then(|m| m.menu.clone()),
        system: m.is_some_and(|m| m.system.is_some()),
        verified: a.verified,
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
                &settings_of(state, &a.id),
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
        addons::remove(state.scraper.shell(), &state.paths, &id)?;
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
        let official = catalogue_url(state) == DEFAULT_CATALOGUE_URL;
        let addon = addons::install(
            &*state.http,
            state.scraper.shell(),
            &state.paths,
            &entry,
            official,
        )?;
        state.scraper.addons_changed();
        Ok(describe(
            &addon,
            &disabled(state)?,
            None,
            &settings_of(state, &id),
        ))
    })
    .await
}

/// An installed add-on that is turned on and can run, or why not.
fn running(state: &AppState, id: &str) -> ApiResult<(Addon, forma_core::addons::runtime::Script)> {
    let addon = addons::discover(&state.paths)
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no add-on `{id}`")))?;
    if disabled(state)?.contains(id) {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            format!("{id} is turned off"),
        ));
    }
    let script = state.scraper.script(&addon).ok_or_else(|| {
        ApiError::new(
            ErrorCode::Invalid,
            addon
                .problem
                .clone()
                .unwrap_or_else(|| format!("{id} cannot run")),
        )
    })?;
    Ok((addon, script))
}

fn failed(id: &str, e: String) -> ApiError {
    ApiError::new(ErrorCode::Internal, format!("{id}: {e}"))
}

/// Sets (or, with no value, clears) one of an add-on's settings.
#[tauri::command]
pub async fn addons_set_setting<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
    key: String,
    value: Option<String>,
) -> ApiResult<Vec<AddonInfo>> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        let addon = addons::discover(&state.paths)
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no add-on `{id}`")))?;
        if !addon
            .manifest
            .as_ref()
            .is_some_and(|m| m.settings.iter().any(|s| s.key == key))
        {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("{id} has no setting `{key}`"),
            ));
        }
        let stored = state.library().setting(CREDENTIALS_SETTING)?;
        let mut all = load_credentials(stored.as_deref());
        let mine = all.entry(settings_key(&id)).or_default();
        match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            Some(v) => {
                mine.insert(key, v);
            }
            None => {
                mine.remove(&key);
            }
        }
        all.retain(|_, c| !c.is_empty());
        let text = serde_json::to_string(&all)
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
        state
            .library()
            .set_setting(CREDENTIALS_SETTING, Some(&text))?;
        // Stations may depend on a setting (an account, a country).
        state
            .scraper
            .music_stale
            .store(true, std::sync::atomic::Ordering::SeqCst);
        list(state)
    })
    .await
}

/// One of a `menu` add-on's pages: `page({ path })` → `{ title?, rows }`, drawn by Forma.
#[tauri::command]
pub async fn addons_page<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
    path: Vec<String>,
) -> ApiResult<serde_json::Value> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        let (_, script) = running(state, &id)?;
        let page = script
            .call(
                &settings_of(state, &id),
                "page",
                &serde_json::json!({ "path": path }),
            )
            .map_err(|e| failed(&id, e))?;
        Ok(clean_page(&page))
    })
    .await
}

/// What a row of a page asked for: `action(id, input)` → `{ notify?, open?, back? }`.
#[tauri::command]
pub async fn addons_action<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: String,
    action: String,
    input: Option<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    off_thread(app, move |state| {
        caller.require(state, permissions::ADDONS_MANAGE)?;
        let (_, script) = running(state, &id)?;
        let out = script
            .call(
                &settings_of(state, &id),
                "action",
                &serde_json::json!({ "action": action, "input": input.unwrap_or(serde_json::Value::Null) }),
            )
            .map_err(|e| failed(&id, e))?;
        // A change it made (a station added) shows in Music.
        state
            .scraper
            .music_stale
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let text = |k: &str| out[k].as_str().map(|s| s.chars().take(300).collect::<String>());
        Ok(serde_json::json!({
            "notify": text("notify"),
            "open": strings(&out["open"]),
            "back": out["back"].as_bool().unwrap_or(false),
        }))
    })
    .await
}

fn strings(v: &serde_json::Value) -> Option<Vec<String>> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect()
    })
}

/// A page as the shell draws it: only the fields it knows, as text, 200 rows at most.
fn clean_page(page: &serde_json::Value) -> serde_json::Value {
    let text = |v: &serde_json::Value| v.as_str().map(|s| s.chars().take(300).collect::<String>());
    let rows: Vec<serde_json::Value> = page["rows"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            let label = text(&r["label"])?;
            Some(serde_json::json!({
                "label": label,
                "detail": text(&r["detail"]),
                "heading": text(&r["heading"]),
                "action": text(&r["action"]),
                "adjust": text(&r["adjust"]),
                "open": strings(&r["open"]),
                "ask": r["ask"].is_object().then(|| serde_json::json!({
                    "title": text(&r["ask"]["title"]).unwrap_or_else(|| label.clone()),
                    "secret": r["ask"]["secret"].as_bool().unwrap_or(false),
                    "value": text(&r["ask"]["value"]),
                })),
                "alt": r["alt"].is_object().then(|| serde_json::json!({
                    "label": text(&r["alt"]["label"]).unwrap_or_default(),
                    "action": text(&r["alt"]["action"]).unwrap_or_default(),
                })),
            }))
        })
        .take(200)
        .collect();
    serde_json::json!({ "title": text(&page["title"]), "rows": rows })
}

/// Fetches the music add-ons' stations again, if they changed, into the music library. An
/// add-on that fails is left out; the others still show.
pub fn refresh_radios(state: &AppState) {
    if !state
        .scraper
        .music_stale
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    let off = disabled(state).unwrap_or_default();
    let mut radios = Vec::new();
    for addon in addons::discover(&state.paths) {
        let music = addon.manifest.as_ref().is_some_and(|m| m.provides("music"));
        if !music || off.contains(&addon.id) {
            continue;
        }
        let Some(script) = state.scraper.script(&addon) else {
            continue;
        };
        match forma_core::addons::runtime::stations(&script, &settings_of(state, &addon.id)) {
            Ok(found) => radios.extend(found.into_iter().map(|r| (addon.id.clone(), r))),
            Err(e) => eprintln!("[add-on {}] stations: {e}", addon.id),
        }
    }
    state.music.library().set_addon_radios(radios);
}
