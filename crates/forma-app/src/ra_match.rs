//! Matching the library to RetroAchievements in the background (#41): romdex hashes one system
//! at a time, never while a game runs, and the hashes are matched through RetroAchievements'
//! Web API once an account and key are set. It runs at start, for systems whose files change,
//! and again when the RetroAchievements key or login changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use forma_core::library::RaMatch;
use forma_core::ra_match;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::state::AppState;

enum Work {
    /// Hash these systems (all, when empty), then match.
    Hash(Vec<String>),
    /// Only match: the account or key changed.
    Match,
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
        run(&state, &rx);
    });
}

const UI_WAIT: Duration = Duration::from_secs(60);

fn run(state: &AppState, rx: &Receiver<Work>) {
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
                    Ok((hashes, report)) => {
                        let stored = state.library().set_ra_hashes(&target.system, &hashes);
                        if report.hashed > 0 {
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

fn match_now(state: &AppState) {
    // The library stays free while RetroAchievements answers.
    let result = (|| {
        let pending = ra_match::unmatched(&state.library())?;
        if pending.is_empty() {
            return Ok(0);
        }
        let matched = ra_match::look_up(&*state.http, &state.http_cache(), pending)?;
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
            .collect();
        let systems = state.library().ra_match_report(&names)?;
        Ok(RaMatches {
            running: RUNNING.load(Ordering::SeqCst),
            systems: systems.into_iter().filter(|s| s.games > 0).collect(),
        })
    })
    .await
}
