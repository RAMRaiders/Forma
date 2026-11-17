//! BIOS files: each system's, for Forma's BIOS page, and a check before a game starts.

use forma_core::bios::{BiosFile, BiosState, SystemBios, Table};
use forma_core::launch::LaunchRequest;
use forma_core::library::Game;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::state::AppState;

/// A game was asked to start but its system's BIOS is missing: Forma's shell asks whether to
/// start it anyway. The launch failed with `biosMissing`.
pub const EVENT_BIOS_MISSING: &str = "bios:missing";

/// A system with games, and its BIOS files.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemBiosStatus {
    pub fullname: String,
    #[serde(flatten)]
    pub bios: SystemBios,
}

/// `bios:missing`: the game, what its system lacks, and the launch to make if the user goes on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosMissing {
    pub game_id: i64,
    pub name: String,
    pub system: String,
    pub fullname: String,
    /// Its files that are not there and right.
    pub files: Vec<BiosFile>,
    /// What the launch asked for, with `ignoreBios` added by whoever starts it anyway.
    pub request: LaunchRequest,
}

/// The BIOS files of every system that has games and lists some; empty off Batocera.
#[tauri::command]
pub fn bios_status(state: State<'_, AppState>) -> ApiResult<Vec<SystemBiosStatus>> {
    let Some(table) = state.bios_table() else {
        return Ok(Vec::new());
    };
    let systems = state
        .library()
        .systems_in_order()
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
    Ok(systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .filter_map(|s| {
            table
                .check(&s.name, &state.paths.userdata)
                .map(|bios| SystemBiosStatus {
                    fullname: s.fullname,
                    bios,
                })
        })
        .collect())
}

/// Before `game` starts: an error, and `bios:missing` for the shell to ask, when its system
/// does not start without a BIOS it lacks; fine when asked to start anyway.
pub fn before_launch<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    game: &Game,
    fullname: &str,
    request: &LaunchRequest,
) -> ApiResult<()> {
    if request.ignore_bios {
        return Ok(());
    }
    let Some(bios) = state
        .bios_table()
        .and_then(|t: &Table| t.check(&game.system, &state.paths.userdata))
    else {
        return Ok(());
    };
    if !bios.missing {
        return Ok(());
    }
    let files: Vec<BiosFile> = bios
        .files
        .into_iter()
        .filter(|f| f.state != BiosState::Ok)
        .collect();
    let _ = app.emit(
        EVENT_BIOS_MISSING,
        BiosMissing {
            game_id: game.id,
            name: game.name.clone(),
            system: game.system.clone(),
            fullname: fullname.to_owned(),
            files,
            request: request.clone(),
        },
    );
    Err(ApiError::new(
        ErrorCode::BiosMissing,
        format!("{fullname} needs a BIOS that is missing"),
    ))
}
