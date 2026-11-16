//! The scraper (`forma.scraper` in the SDK): its sources, the user's choices and accounts, and
//! a run over a game, a system or the whole library, in the background, with progress events.
//! The engine is `forma_core::scraper`.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use forma_core::addons::runtime::{ScraperAddon, Script};
use forma_core::host::Shell;
use forma_core::net::Http;
use forma_core::themes::ThemeManifest;

use forma_core::scraper::{
    self, CONFIG_SETTING, CREDENTIALS_SETTING, Config, Credential, MediaKind, Source, SourceChoice,
    pick,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::commands::{EVENT_MEDIA_CHANGED, MediaChanged, off_thread};
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
    /// The offers last asked for, for the user to pick from.
    picking: Mutex<Option<pick::Picking>>,
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
            picking: Mutex::new(None),
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

    fn status(&self) -> MutexGuard<'_, RunStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn picking(&self) -> MutexGuard<'_, Option<pick::Picking>> {
        self.picking.lock().unwrap_or_else(PoisonError::into_inner)
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
    /// Whether what the theme asks for is fetched too.
    pub theme_needs: bool,
    /// What the theme in use asks for, if anything.
    pub theme: Option<ThemeNeedsInfo>,
}

/// What the theme in use asks the scraper for.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeNeedsInfo {
    /// The theme's name.
    pub name: String,
    /// The view these needs are for, when the theme asks for more in some views.
    pub view: Option<String>,
    pub media: Vec<MediaKind>,
    /// Details beyond the usual ones, e.g. `ageRating`.
    pub fields: Vec<String>,
    /// Every kind the theme draws; empty when it does not say.
    pub shows: Vec<MediaKind>,
    /// Kinds drawn only when another is missing: the kind, and the one it replaces.
    pub backups: BTreeMap<String, MediaKind>,
}

type Loaded = (
    Arc<Vec<Box<dyn Source>>>,
    Config,
    BTreeMap<String, scraper::Credentials>,
);

fn load(state: &AppState) -> ApiResult<Loaded> {
    let sources = state.scraper.sources(state);
    let library = state.library();
    let mut config = Config::load(library.setting(CONFIG_SETTING)?.as_deref(), &sources);
    drop(library);
    if let Some(manifest) = state.current_theme().and_then(|t| t.manifest) {
        let view = view_in_use(state, &manifest);
        config.needs = scraper::Needs::from_theme(&manifest.scraper, view.as_deref());
    }
    let library = state.library();
    let credentials = scraper::load_credentials(library.setting(CREDENTIALS_SETTING)?.as_deref());
    Ok((sources, config, credentials))
}

/// The view whose needs apply: the value of the theme's `viewSetting` when it names one (the
/// user's, else its default), otherwise the view the theme says it is showing.
fn view_in_use(state: &AppState, manifest: &ThemeManifest) -> Option<String> {
    if let Some(key) = &manifest.scraper.view_setting {
        let setting = manifest.settings.iter().find(|s| &s.key == key)?;
        let stored = state
            .library()
            .setting(&format!("theme.{}.{key}", manifest.id))
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .filter(|v| setting.accepts(v));
        return stored
            .unwrap_or_else(|| setting.default.clone())
            .as_str()
            .map(str::to_owned);
    }
    state.shell.view()
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
        theme_needs: config.theme_needs,
        theme: state
            .current_theme()
            .and_then(|t| t.manifest)
            .filter(|m| m.scraper != forma_core::themes::ThemeScraper::default())
            .map(|m| {
                let view = view_in_use(state, &m);
                let needs = scraper::Needs::from_theme(&m.scraper, view.as_deref());
                ThemeNeedsInfo {
                    view: view.filter(|v| m.scraper.views.contains_key(v)),
                    name: m.name,
                    media: needs.media,
                    fields: needs
                        .age_rating
                        .then(|| "ageRating".to_owned())
                        .into_iter()
                        .collect(),
                    shows: scraper::known_kinds(&m.scraper.shows),
                    backups: m
                        .scraper
                        .backups
                        .iter()
                        .filter_map(|(kind, main)| {
                            let main = scraper::known_kinds([main]).pop()?;
                            scraper::known_kinds([kind]).pop()?;
                            Some((kind.clone(), main))
                        })
                        .collect(),
                }
            }),
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
    theme_needs: Option<bool>,
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
        if let Some(theme_needs) = theme_needs {
            config.theme_needs = theme_needs;
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

/// A game and its system, or why not.
fn target(
    state: &AppState,
    id: i64,
) -> ApiResult<(forma_core::library::Game, forma_core::systems::System)> {
    state
        .library()
        .launch_target(id)?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no game {id}")))
}

/// Asks every source that is on and set up about a game, for the user to pick from.
#[tauri::command]
pub async fn scraper_offers<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
) -> ApiResult<pick::Offers> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (sources, config, credentials) = load(state)?;
        let (game, system) = target(state, id)?;
        let picking = pick::gather(
            &*state.http,
            &sources,
            &config,
            &credentials,
            &game,
            &system,
        );
        let offers = picking.offers.clone();
        *state.scraper.picking() = Some(picking);
        Ok(offers)
    })
    .await
}

/// A small picture of one of the offers; `null` when it is not a picture or cannot be fetched.
#[tauri::command]
pub async fn scraper_preview<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    offer: usize,
) -> ApiResult<Option<String>> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let picking = state.scraper.picking().clone();
        Ok(picking.and_then(|p| p.preview(&*state.http, offer)))
    })
    .await
}

/// Writes what the user picked from the offers; returns what could not be saved.
#[tauri::command]
pub async fn scraper_apply<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
    text: BTreeMap<String, String>,
    media: Vec<usize>,
) -> ApiResult<Vec<String>> {
    let events = app.clone();
    let replaced = !media.is_empty();
    let problems = off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let picking = state
            .scraper
            .picking()
            .clone()
            .filter(|p| p.game == id)
            .ok_or_else(|| ApiError::new(ErrorCode::Invalid, "these offers are out of date"))?;
        let (game, system) = target(state, id)?;
        pick::apply(
            &*state.http,
            &state.paths,
            &picking,
            &game,
            &system,
            &text,
            &media,
        )
        .map_err(internal)
    })
    .await?;
    if replaced {
        let _ = events.emit(EVENT_MEDIA_CHANGED, MediaChanged { game: id });
    }
    Ok(problems)
}

/// Takes pictures, videos or manuals off a game and deletes their files; returns what could
/// not be deleted.
#[tauri::command]
pub async fn scraper_remove_media<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
    kinds: Vec<MediaKind>,
) -> ApiResult<Vec<String>> {
    let events = app.clone();
    let removed = !kinds.is_empty();
    let problems = off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (game, system) = target(state, id)?;
        // A file another game shows too stays.
        let shared = state
            .library()
            .games(&system.name)?
            .into_iter()
            .filter(|g| g.id != id)
            .flat_map(|g| g.media.into_values())
            .collect();
        pick::remove_media(&state.paths, &game, &system, &kinds, &shared).map_err(internal)
    })
    .await?;
    if removed {
        let _ = events.emit(EVENT_MEDIA_CHANGED, MediaChanged { game: id });
    }
    Ok(problems)
}

/// Changes a game's details by hand; an empty value clears one.
#[tauri::command]
pub async fn scraper_edit<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    id: i64,
    fields: BTreeMap<String, String>,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        caller.require(state, permissions::SCRAPER_MANAGE)?;
        let (game, system) = target(state, id)?;
        pick::edit(&state.paths, &game, &system, &fields)
            .map_err(|e| ApiError::new(ErrorCode::Invalid, e))
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
            let ids = library.game_files(name)?.iter().map(|g| g.id).collect();
            (system.fullname, ids)
        }
        Scope::All => {
            let mut ids = Vec::new();
            for system in library.systems_in_order()? {
                ids.extend(library.game_files(&system.name)?.iter().map(|g| g.id));
            }
            ("Everything".to_owned(), ids)
        }
    })
}

/// A system's games that are one game under different names, by LaunchBox's data on this
/// machine; `null` without it.
#[tauri::command]
pub async fn library_duplicates<R: Runtime>(
    app: AppHandle<R>,
    system: String,
) -> ApiResult<Option<Vec<scraper::duplicates::DuplicateGroup>>> {
    off_thread(app, move |state| {
        let games = state.library().game_files(&system)?;
        let Some(first) = games.first() else {
            return Ok(Some(Vec::new()));
        };
        let (_, definition) = target(state, first.id)?;
        scraper::duplicates::duplicates(&state.paths, &definition, &games).map_err(internal)
    })
    .await
}

/// Before rebuilding LaunchBox's data from an older Forma: the first screens go first.
const LAUNCHBOX_AFTER: Duration = Duration::from_secs(90);
/// The longest it waits for the first screen before counting LAUNCHBOX_AFTER.
const LAUNCHBOX_UI_WAIT: Duration = Duration::from_secs(60);

/// Builds LaunchBox's data again in the background when an older Forma built it, so the
/// Scrape page has its alternate names. Waits for games.
pub fn upgrade_launchbox<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let since = Instant::now();
        while !state.shell.ui_started.load(Ordering::SeqCst) && since.elapsed() < LAUNCHBOX_UI_WAIT
        {
            std::thread::sleep(Duration::from_millis(200));
        }
        std::thread::sleep(LAUNCHBOX_AFTER);
        while state.playing.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(5));
        }
        match scraper::upgrade_launchbox(&*state.http, &state.paths) {
            Ok(true) => eprintln!("forma: LaunchBox's data built again, with alternate names"),
            Ok(false) => {}
            Err(e) => eprintln!("forma: LaunchBox's data not built again: {e}"),
        }
    });
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
        // Its pictures were replaced, often under the same names: as after a pick, so the
        // theme reads them again (their URLs change) rather than showing what it has.
        if outcome.as_ref().is_ok_and(|o| !o.media.is_empty()) {
            let _ = app.emit(EVENT_MEDIA_CHANGED, MediaChanged { game: *id });
        }
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
