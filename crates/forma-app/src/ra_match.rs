//! Matching the library to RetroAchievements in the background (#41): romdex hashes one system
//! at a time, never while a game runs, and the hashes are matched through RetroAchievements'
//! Web API once an account and key are set. It runs at start, for systems whose files change,
//! and again when the RetroAchievements key or login changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use forma_core::library::RaMatch;
use forma_core::net::achievements;
use forma_core::{dumps, ra_match};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::ApiResult;
use crate::commands::{EVENT_LIBRARY_CHANGED, LibraryChanged, off_thread};
use crate::state::AppState;

enum Work {
    /// Hash these systems (all, when empty), then match.
    Hash(Vec<String>),
    /// Only match: the account or key changed.
    Match,
    /// Check every hashed system against the DATs again: the setting was turned on.
    Dumps,
}

static WORK: OnceLock<Mutex<Sender<Work>>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);

/// After a library refresh: these systems' files changed.
pub fn systems_changed(systems: &[String]) {
    send(Work::Hash(systems.to_vec()));
}

/// The RetroAchievements key or login changed: match what is hashed.
pub fn account_changed() {
    send(Work::Match);
}

/// The dump check was turned on: check every system (hashes come from the cache).
pub fn dumps_changed() {
    send(Work::Dumps);
}

fn send(work: Work) {
    if let Some(tx) = WORK.get() {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(work);
    }
}

/// Starts the worker once the UI is up, so the first screen never waits for the disk.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    let (tx, rx) = mpsc::channel();
    let _ = WORK.set(Mutex::new(tx));
    send(Work::Hash(Vec::new()));
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let since = Instant::now();
        while !state.shell.ui_started.load(Ordering::SeqCst) && since.elapsed() < UI_WAIT {
            std::thread::sleep(Duration::from_millis(200));
        }
        run(&app, &state, &rx);
    });
}

const UI_WAIT: Duration = Duration::from_secs(60);

fn run<R: Runtime>(app: &AppHandle<R>, state: &AppState, rx: &Receiver<Work>) {
    let mut cache = match ra_match::open_cache(&state.paths) {
        Ok(cache) => cache,
        Err(e) => {
            eprintln!("forma: RetroAchievements matching is off: {e}");
            return;
        }
    };
    while let Ok(first) = rx.recv() {
        // Merge what queued up meanwhile.
        let mut all = false;
        let mut systems: Vec<String> = Vec::new();
        let mut hash = false;
        for work in std::iter::once(first).chain(rx.try_iter()) {
            match work {
                Work::Hash(s) if s.is_empty() => (all, hash) = (true, true),
                Work::Hash(s) => {
                    hash = true;
                    systems.extend(s);
                }
                Work::Match => {}
                Work::Dumps => (all, hash) = (true, true),
            }
        }
        RUNNING.store(true, Ordering::SeqCst);
        if hash {
            for target in ra_match::targets(&state.paths) {
                if !all && !systems.contains(&target.system) {
                    continue;
                }
                // A game is the box's whole job while it runs.
                while state.playing.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_secs(2));
                }
                match ra_match::hash_system(&target, &mut cache) {
                    Ok((found, report)) => {
                        let stored = if ra_match::has_console(&target.system) {
                            state
                                .library()
                                .set_ra_hashes(&target.system, &ra_match::ra_hashes(&found))
                                .map(drop)
                        } else {
                            Ok(())
                        };
                        check_dumps(app, state, &target.system, &found);
                        if report.hashed > 0 && ra_match::has_console(&target.system) {
                            eprintln!(
                                "forma: RetroAchievements hashes for {}: {} of {} files read, {} hashed, {} unknown",
                                target.system,
                                report.hashed,
                                report.files,
                                report.ra_hashed,
                                report.ra_unknown
                            );
                        }
                        if let Err(e) = stored {
                            eprintln!("forma: {}: {e}", target.system);
                        }
                    }
                    Err(e) => eprintln!("forma: hashing {}: {e}", target.system),
                }
            }
        }
        match_now(state);
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// The dump check of one hashed system (#115), when it is on and a DAT covers the system:
/// its DATs fetched if missing or old, each file looked up, themes told what changed.
fn check_dumps<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    system: &str,
    found: &[dumps::Identified],
) {
    if !dumps_on(state) || !dumps::has_dats(system) {
        return;
    }
    if let Err(e) = dumps::fetch(&*state.http, &state.paths, system) {
        eprintln!("forma: dump check: {e}");
    }
    let Some(index) = dumps::index(&state.paths, system) else {
        return;
    };
    let checked = dumps::check(&index, found);
    match state.library().set_dumps(system, &checked) {
        Ok(0) => {}
        Ok(changed) => {
            let verified = checked
                .iter()
                .filter(|(_, _, d)| d.status == "verified")
                .count();
            eprintln!(
                "forma: dump check for {system}: {verified} of {} verified ({changed} changed)",
                checked.len()
            );
            let _ = app.emit(
                EVENT_LIBRARY_CHANGED,
                LibraryChanged {
                    systems: vec![system.to_owned()],
                },
            );
        }
        Err(e) => eprintln!("forma: dump check for {system}: {e}"),
    }
}

/// Forma's setting, on unless turned off.
pub fn dumps_on(state: &AppState) -> bool {
    state
        .library()
        .setting_text(dumps::SETTING)
        .ok()
        .flatten()
        .is_none_or(|v| v.trim() != "false")
}

fn match_now(state: &AppState) {
    let account = {
        let lib = state.library();
        achievements::account(&lib, &state.paths)
    };
    // Not signed in yet: hashes wait until there is an account.
    let Ok(account) = account else { return };
    // The library stays free while RetroAchievements answers.
    let result = (|| {
        let pending = ra_match::unmatched(&state.library())?;
        if pending.is_empty() {
            return Ok(0);
        }
        let matched = ra_match::look_up(&*state.http, &state.http_cache(), &account, pending)?;
        state.library().set_ra_game_ids(&matched)?;
        Ok::<_, forma_core::Error>(matched.len())
    })();
    match result {
        Ok(0) => {}
        Ok(n) => eprintln!("forma: matched {n} games to RetroAchievements"),
        Err(e) => eprintln!("forma: RetroAchievements matching: {e}"),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaMatches {
    /// Hashing or matching now.
    pub running: bool,
    /// Per system RetroAchievements supports: games, hashed, matched.
    pub systems: Vec<RaMatch>,
}

/// How much of the library is matched to RetroAchievements, per system.
#[tauri::command]
pub async fn ra_matches<R: Runtime>(app: AppHandle<R>) -> ApiResult<RaMatches> {
    off_thread(app, move |state| {
        let names: Vec<String> = ra_match::targets(&state.paths)
            .into_iter()
            .map(|t| t.system)
            .filter(|s| ra_match::has_console(s))
            .collect();
        let systems = state.library().ra_match_report(&names)?;
        Ok(RaMatches {
            running: RUNNING.load(Ordering::SeqCst),
            systems: systems.into_iter().filter(|s| s.games > 0).collect(),
        })
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpChecks {
    /// Whether the check is on (Forma's setting `library.verifyDumps`).
    pub on: bool,
    /// Hashing or checking now.
    pub running: bool,
    /// Per system a No-Intro or Redump DAT covers: games, checked, and how they came out.
    pub systems: Vec<forma_core::library::DumpReport>,
}

/// How much of the library is checked against the No-Intro and Redump DATs, per system.
#[tauri::command]
pub async fn library_dump_checks<R: Runtime>(app: AppHandle<R>) -> ApiResult<DumpChecks> {
    off_thread(app, move |state| {
        let names: Vec<String> = ra_match::targets(&state.paths)
            .into_iter()
            .map(|t| t.system)
            .filter(|s| dumps::has_dats(s))
            .collect();
        let systems = state.library().dump_report(&names)?;
        Ok(DumpChecks {
            on: dumps_on(state),
            running: RUNNING.load(Ordering::SeqCst),
            systems,
        })
    })
    .await
}
