//! The scraper (`forma.scraper` in the SDK): its sources, the user's choices and accounts, and
//! a run over a game, a system or the whole library, in the background, with progress events.
//! The engine is `forma_core::scraper`.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use forma_core::addons::runtime::{ScraperAddon, Script};
use forma_core::host::Shell;
use forma_core::net::Http;

use forma_core::scraper::{
    self, CONFIG_SETTING, CREDENTIALS_SETTING, Config, Credential, MediaKind, Source, SourceChoice,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
use crate::state::AppState;

/// After each game of a run, and when it ends: a [`RunStatus`].
pub const EVENT_SCRAPER_PROGRESS: &str = "scraper:progress";

/// How many problems a run keeps to show.
const PROBLEMS_KEPT: usize = 20;

/// The sources (built in, then add-ons), and the run going on (or the last one).
pub struct Scraping {
    sources: RwLock<Arc<Vec<Box<dyn Source>>>>,
    /// The add-ons' sources must be read again (at start, and after an add-on changed).
    stale: AtomicBool,
    /// How add-ons reach the network: theirs, so a script never holds Forma's state.
    addon_http: Arc<dyn Http>,
    /// What runs a verified add-on's system part.
    addon_shell: Arc<dyn Shell>,
    /// The music add-ons' stations must be fetched again.
    pub music_stale: AtomicBool,
    status: Mutex<RunStatus>,
    stop: Arc<AtomicBool>,
}

impl Scraping {
    pub fn new(addon_http: Arc<dyn Http>, addon_shell: Arc<dyn Shell>) -> Self {
        Self {
            sources: RwLock::new(Arc::new(scraper::builtin())),
            stale: AtomicBool::new(true),
            addon_http,
            addon_shell,
            music_stale: AtomicBool::new(true),
            status: Mutex::new(RunStatus::default()),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The add-ons changed: read them again before the sources are next used.
    pub fn addons_changed(&self) {
        self.stale.store(true, Ordering::SeqCst);
        self.music_stale.store(true, Ordering::SeqCst);
    }

    /// An add-on's script, ready to call; `None` for one that cannot run.
    pub fn script(&self, addon: &forma_core::addons::Addon) -> Option<Script> {
        Script::new(addon, self.addon_http.clone(), self.addon_shell.clone())
    }

    pub fn shell(&self) -> &dyn Shell {
        &*self.addon_shell
    }

    /// Every source: built in, then each usable scraper add-on that is turned on. A run holds
    /// its own copy, so changing add-ons never waits for it.
    pub fn sources(&self, state: &AppState) -> Arc<Vec<Box<dyn Source>>> {
        if self.stale.swap(false, Ordering::SeqCst) {
            let disabled = forma_core::addons::disabled(
                state
                    .library()
                    .setting(forma_core::addons::DISABLED_SETTING)
                    .ok()
                    .flatten()
                    .as_deref(),
            );
            let mut all = scraper::builtin();
            for addon in forma_core::addons::discover(&state.paths) {
                if disabled.contains(&addon.id) {
                    continue;
                }
                if let Some(source) = self
                    .script(&addon)
                    .and_then(|script| ScraperAddon::new(&addon, script))
                {
                    all.push(Box::new(source));
                }
            }
            *self
                .sources
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(all);
        }
        self.sources
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn status(&self) -> std::sync::MutexGuard<'_, RunStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What a run is doing, or did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub running: bool,
    /// What it is over: a game's name, a system's, or "Everything".
    pub scope: Option<String>,
    pub total: usize,
    pub done: usize,
    /// Games something was found for.
    pub found: usize,
    /// Games no source knew.
    pub not_found: usize,
    /// Games that already had everything asked for.
    pub skipped: usize,
    /// The game being scraped now.
    pub current: Option<String>,
    /// The latest problems: a source refusing, a file not written.
    pub problems: Vec<String>,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    pub key: String,
    pub label: String,
    pub secret: bool,
    pub required: bool,
    /// Whether the user entered it.
    pub set: bool,
    /// What was entered, for one that is not secret.
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub about: String,
    pub enabled: bool,
    /// Every required credential is entered.
    pub ready: bool,
    pub needs: Vec<CredentialInfo>,
    pub gives: Vec<MediaKind>,
    pub metadata: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaKindInfo {
    pub kind: MediaKind,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScraperSettings {
    /// In the order they are asked.
    pub sources: Vec<SourceInfo>,
    /// The kinds to fetch.
    pub media: Vec<MediaKind>,
    /// Every kind there is, with its name.
    pub media_kinds: Vec<MediaKindInfo>,
    pub metadata: bool,
    pub replace: bool,
}

type Loaded = (
    Arc<Vec<Box<dyn Source>>>,
    Config,
    BTreeMap<String, scraper::Credentials>,
);

fn load(state: &AppState) -> ApiResult<Loaded> {
    let sources = state.scraper.sources(state);
    let library = state.library();
    let config = Config::load(library.setting(CONFIG_SETTING)?.as_deref(), &sources);
    let credentials = scraper::load_credentials(library.setting(CREDENTIALS_SETTING)?.as_deref());
    Ok((sources, config, credentials))
}

fn settings(state: &AppState) -> ApiResult<ScraperSettings> {
    let (all, config, credentials) = load(state)?;
    let empty = scraper::Credentials::new();
    let sources = config
        .sources
        .iter()
        .filter_map(|choice| {
            let source = all.iter().find(|s| s.id() == choice.id)?;
            let mine = credentials.get(source.id()).unwrap_or(&empty);
            Some(SourceInfo {
                id: source.id().to_owned(),
                name: source.name().to_owned(),
                about: source.about().to_owned(),
                enabled: choice.enabled,
                ready: source.ready(mine),
                needs: source
                    .needs()
                    .iter()
                    .map(|c: &Credential| {
                        let value = mine.get(c.key.as_ref()).filter(|v| !v.trim().is_empty());
                        CredentialInfo {
                            key: c.key.to_string(),
                            label: c.label.to_string(),
                            secret: c.secret,
                            required: c.required,
                            set: value.is_some(),
                            value: value.filter(|_| !c.secret).cloned(),
                        }
                    })
                    .collect(),
                gives: source.gives().to_vec(),
                metadata: source.has_metadata(),
            })
        })
        .collect();
    Ok(ScraperSettings {
        sources,
        media: config.media,
        media_kinds: MediaKind::ALL
            .iter()
            .map(|k| MediaKindInfo {
                kind: *k,
                label: k.label(),
            })
            .collect(),
        metadata: config.metadata,
        replace: config.replace,
    })
}

#[tauri::command]
pub async fn scraper_settings<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
) -> ApiResult<ScraperSettings> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        settings(state)
    })
    .await
}

/// Changes what is given; the rest stays. `sources` is the whole list, in order.
#[tauri::command]
pub async fn scraper_configure<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    sources: Option<Vec<SourceChoice>>,
    media: Option<Vec<MediaKind>>,
    metadata: Option<bool>,
    replace: Option<bool>,
) -> ApiResult<ScraperSettings> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (all, mut config, _) = load(state)?;
        if let Some(sources) = sources {
            config.sources = sources;
        }
        if let Some(media) = media {
            config.media = media;
        }
        if let Some(metadata) = metadata {
            config.metadata = metadata;
        }
        if let Some(replace) = replace {
            config.replace = replace;
        }
        // Through `load` again: unknown sources dropped, missing ones added.
        let config = Config::load(
            Some(&serde_json::to_string(&config).map_err(internal)?),
            &all,
        );
        state.library().set_setting(
            CONFIG_SETTING,
            Some(&serde_json::to_string(&config).map_err(internal)?),
        )?;
        settings(state)
    })
    .await
}

/// Sets (or, with no value, clears) one of a source's credentials.
#[tauri::command]
pub async fn scraper_set_credential<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    source: String,
    key: String,
    value: Option<String>,
) -> ApiResult<ScraperSettings> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (all, _, mut credentials) = load(state)?;
        let known = all
            .iter()
            .find(|s| s.id() == source)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no source `{source}`")))?;
        if !known.needs().iter().any(|c| c.key == key.as_str()) {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("{} has no `{key}`", known.name()),
            ));
        }
        let mine = credentials.entry(source.clone()).or_default();
        match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            Some(v) => {
                mine.insert(key, v);
            }
            None => {
                mine.remove(&key);
            }
        }
        credentials.retain(|_, c| !c.is_empty());
        state.library().set_setting(
            CREDENTIALS_SETTING,
            Some(&serde_json::to_string(&credentials).map_err(internal)?),
        )?;
        settings(state)
    })
    .await
}

/// What to scrape.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    Game { id: i64 },
    System { name: String },
    All,
}

/// Starts a run in the background and returns at once; `scraper:progress` follows it. One
/// run at a time.
#[tauri::command]
pub async fn scraper_start<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    scope: Scope,
) -> ApiResult<RunStatus> {
    let runner = app.clone();
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (label, ids) = games_in(state, &scope)?;
        {
            let mut status = state.scraper.status();
            if status.running {
                return Err(ApiError::new(
                    ErrorCode::Busy,
                    "the scraper is already running",
                ));
            }
            *status = RunStatus {
                running: true,
                scope: Some(label),
                total: ids.len(),
                ..RunStatus::default()
            };
        }
        state.scraper.stop.store(false, Ordering::SeqCst);
        let started = state.scraper.status().clone();
        std::thread::Builder::new()
            .name("forma-scraper".into())
            .spawn(move || run(&runner, &ids))
            .map_err(internal)?;
        Ok(started)
    })
    .await
}

#[tauri::command]
pub async fn scraper_status<R: Runtime>(app: AppHandle<R>) -> ApiResult<RunStatus> {
    off_thread(app, move |state| Ok(state.scraper.status().clone())).await
}

/// Stops the run after the game it is on.
#[tauri::command]
pub async fn scraper_stop<R: Runtime>(app: AppHandle<R>, caller: Caller) -> ApiResult<()> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        state.scraper.stop.store(true, Ordering::SeqCst);
        Ok(())
    })
    .await
}

/// The run's name and its games, in library order.
fn games_in(state: &AppState, scope: &Scope) -> ApiResult<(String, Vec<i64>)> {
    let library = state.library();
    Ok(match scope {
        Scope::Game { id } => {
            let game = library
                .game(*id)?
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game {id}")))?;
            (game.name, vec![*id])
        }
        Scope::System { name } => {
            let system = library
                .systems()?
                .into_iter()
                .find(|s| s.name == *name)
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no system `{name}`")))?;
            let ids = library.games(name)?.iter().map(|g| g.id).collect();
            (system.fullname, ids)
        }
        Scope::All => {
            let mut ids = Vec::new();
            for system in library.systems_in_order()? {
                ids.extend(library.games(&system.name)?.iter().map(|g| g.id));
            }
            ("Everything".to_owned(), ids)
        }
    })
}

/// The run itself, on its own thread: one game at a time, the library held only to read.
fn run<R: Runtime>(app: &AppHandle<R>, ids: &[i64]) {
    let state = app.state::<AppState>();
    let emit = |status: &RunStatus| {
        let _ = app.emit(EVENT_SCRAPER_PROGRESS, status);
    };
    let (sources, config, credentials) = match load(&state) {
        Ok(loaded) => loaded,
        Err(e) => {
            let mut status = state.scraper.status();
            status.running = false;
            status.problems.push(e.message);
            emit(&status);
            return;
        }
    };
    let mut out_of_service = HashSet::new();
    for id in ids {
        if state.scraper.stop.load(Ordering::SeqCst) {
            state.scraper.status().stopped = true;
            break;
        }
        let target = state.library().launch_target(*id);
        let (game, system) = match target {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(e) => {
                note(&mut state.scraper.status(), e.to_string());
                continue;
            }
        };
        {
            let mut status = state.scraper.status();
            status.current = Some(game.name.clone());
            emit(&status);
        }
        let outcome = scraper::scrape_game(
            &*state.http,
            &state.paths,
            &sources,
            &config,
            &credentials,
            &game,
            &system,
            &mut out_of_service,
        );
        let mut status = state.scraper.status();
        status.done += 1;
        match outcome {
            Ok(o) if o.skipped => status.skipped += 1,
            Ok(o) => {
                if o.found() {
                    status.found += 1;
                } else {
                    status.not_found += 1;
                }
                for p in o.problems {
                    note(&mut status, p);
                }
            }
            Err(e) => {
                status.not_found += 1;
                note(&mut status, format!("{}: {e}", game.name));
            }
        }
        // Every source is out of service: nothing more can be found.
        let usable = config.sources.iter().filter(|c| c.enabled).any(|c| {
            !out_of_service.contains(&c.id)
                && sources.iter().any(|s| {
                    s.id() == c.id && s.ready(credentials.get(&c.id).unwrap_or(&BTreeMap::new()))
                })
        });
        if !usable {
            note(
                &mut status,
                "No source can be asked: check their settings".to_owned(),
            );
            break;
        }
    }
    let mut status = state.scraper.status();
    status.running = false;
    status.current = None;
    emit(&status);
}

/// Keeps a problem once, the latest ones only.
fn note(status: &mut RunStatus, problem: String) {
    if !status.problems.contains(&problem) {
        status.problems.push(problem);
        let extra = status.problems.len().saturating_sub(PROBLEMS_KEPT);
        status.problems.drain(..extra);
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(ErrorCode::Internal, e.to_string())
}
