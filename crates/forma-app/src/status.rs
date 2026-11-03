//! The status Forma keeps in a corner of the screen: the controllers connected, each by kind,
//! and the connection, with whether the internet answers. Forma's shell draws it; a theme can
//! restyle it, or draw its own (`"draws": ["status"]` in its theme.json).

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use forma_core::input::{ControllerKind, InputHandle, controller_kind};
use forma_core::network::Link;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::state::AppState;

/// The status changed: a controller came or went, or the connection did.
pub const EVENT_STATUS_CHANGED: &str = "status:changed";

/// How often the connection is looked at; controllers are looked at every second.
const NETWORK_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// In player order.
    pub controllers: Vec<ControllerStatus>,
    /// `null` where Forma cannot tell (no connman: not a Batocera box).
    pub network: Option<Link>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerStatus {
    /// 1-based.
    pub player: usize,
    pub name: String,
    pub kind: ControllerKind,
}

/// The connection as last looked at, so `status_get` never waits on connman.
static NETWORK: Mutex<Option<Link>> = Mutex::new(None);

fn controllers(state: &AppState) -> Vec<ControllerStatus> {
    state
        .input
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(InputHandle::controllers)
        .unwrap_or_default()
        .into_iter()
        .map(|c| ControllerStatus {
            player: c.index + 1,
            kind: controller_kind(&c.guid, &c.name),
            name: c.name,
        })
        .collect()
}

fn now(state: &AppState) -> Status {
    Status {
        controllers: controllers(state),
        network: *NETWORK.lock().unwrap_or_else(PoisonError::into_inner),
    }
}

/// What the status shows now. Any theme may ask: it holds no address, and no network's name.
#[tauri::command]
pub fn status_get(state: State<'_, AppState>) -> Status {
    now(&state)
}

/// Watches controllers and the connection, and tells themes when either changes.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        let mut last: Option<Status> = None;
        let mut network_at: Option<Instant> = None;
        loop {
            let state = app.state::<AppState>();
            if network_at.is_none_or(|t| t.elapsed() >= NETWORK_EVERY) {
                let link = state.network.link().ok();
                *NETWORK.lock().unwrap_or_else(PoisonError::into_inner) = link;
                network_at = Some(Instant::now());
            }
            let status = now(&state);
            if last.as_ref() != Some(&status) {
                let _ = app.emit(EVENT_STATUS_CHANGED, &status);
                last = Some(status);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
