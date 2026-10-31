//! The music player's commands (`@ramraiders/forma-sdk`'s `music`), and what Forma does with it on
//! its own: start the last playlist at boot when Batocera's background music is on, and tell
//! themes when what plays changes, mpv moving on to the next track included.

use std::time::Duration;

use forma_core::music::{Radio, Source, Status};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::permissions;
use crate::state::AppState;

pub const EVENT_MUSIC_CHANGED: &str = "music:changed";

const SOURCE_SETTING: &str = "music.source";
const SHUFFLE_SETTING: &str = "music.shuffle";
pub const VOLUME_SETTING: &str = "music.volume";
pub const DEFAULT_VOLUME: u8 = 60;

/// Runs a player command off the UI thread, then tells every theme what plays now.
async fn act<R: Runtime>(
    app: AppHandle<R>,
    work: impl FnOnce(&AppState) -> ApiResult<Status> + Send + 'static,
) -> ApiResult<Status> {
    let emitter = app.clone();
    let status = off_thread(app, work).await?;
    let _ = emitter.emit(EVENT_MUSIC_CHANGED, &status);
    Ok(status)
}

/// Folders, playlists and radios there are to play.
#[tauri::command]
pub async fn music_sources<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<Source>> {
    off_thread(app, |state| Ok(state.music.library().sources())).await
}

#[tauri::command]
pub async fn music_status<R: Runtime>(app: AppHandle<R>) -> ApiResult<Status> {
    off_thread(app, |state| Ok(state.music.status()?)).await
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayOptions {
    #[serde(default)]
    shuffle: bool,
}

/// Plays a source, and remembers it (and the shuffle) for the next boot.
#[tauri::command]
pub async fn music_play<R: Runtime>(
    app: AppHandle<R>,
    source: String,
    options: Option<PlayOptions>,
) -> ApiResult<Status> {
    let shuffle = options.unwrap_or_default().shuffle;
    act(app, move |state| {
        let status = state.music.play(&source, shuffle)?;
        let mut lib = state.library();
        lib.set_setting(
            SOURCE_SETTING,
            Some(&serde_json::to_string(&source).unwrap_or_default()),
        )?;
        lib.set_setting(
            SHUFFLE_SETTING,
            Some(if shuffle { "true" } else { "false" }),
        )?;
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn music_toggle<R: Runtime>(app: AppHandle<R>) -> ApiResult<Status> {
    act(app, |state| Ok(state.music.toggle()?)).await
}

#[tauri::command]
pub async fn music_next<R: Runtime>(app: AppHandle<R>) -> ApiResult<Status> {
    act(app, |state| Ok(state.music.next()?)).await
}

#[tauri::command]
pub async fn music_previous<R: Runtime>(app: AppHandle<R>) -> ApiResult<Status> {
    act(app, |state| Ok(state.music.previous()?)).await
}

/// Stops, and forgets the source: nothing plays at the next boot either.
#[tauri::command]
pub async fn music_stop<R: Runtime>(app: AppHandle<R>) -> ApiResult<Status> {
    act(app, |state| {
        let status = state.music.stop()?;
        state.library().set_setting(SOURCE_SETTING, None)?;
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn music_set_volume<R: Runtime>(app: AppHandle<R>, volume: u8) -> ApiResult<Status> {
    act(app, move |state| {
        let status = state.music.set_volume(volume)?;
        state
            .library()
            .set_setting(VOLUME_SETTING, Some(&status.volume.to_string()))?;
        Ok(status)
    })
    .await
}

/// The web radios (`radios.m3u` in the music folder). Changing them needs `music.manage`.
#[tauri::command]
pub async fn music_radios<R: Runtime>(app: AppHandle<R>) -> ApiResult<Vec<Radio>> {
    off_thread(app, |state| Ok(state.music.library().radios())).await
}

#[tauri::command]
pub async fn music_add_radio<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    url: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        permissions::require(&crate::commands::theme(state)?, permissions::MUSIC_MANAGE)?;
        Ok(state.music.library().add_radio(&name, &url)?)
    })
    .await
}

#[tauri::command]
pub async fn music_remove_radio<R: Runtime>(app: AppHandle<R>, url: String) -> ApiResult<()> {
    off_thread(app, move |state| {
        permissions::require(&crate::commands::theme(state)?, permissions::MUSIC_MANAGE)?;
        Ok(state.music.library().remove_radio(&url)?)
    })
    .await
}

/// At boot: with Batocera's background music on, the last source (else the first there is)
/// starts, then a watcher tells themes whenever what plays changes.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let conf = forma_core::net::batocera_conf(&state.paths.batocera_conf());
        if conf.get("audio.bgmusic").is_some_and(|v| v == "1") && state.on_batocera {
            let (saved, shuffle) = {
                let lib = state.library();
                (
                    lib.setting_text(SOURCE_SETTING).ok().flatten(),
                    lib.setting_text(SHUFFLE_SETTING)
                        .ok()
                        .flatten()
                        .is_none_or(|s| s == "true"),
                )
            };
            let sources = state.music.library().sources();
            let pick = saved
                .filter(|id| sources.iter().any(|s| &s.id == id))
                .or_else(|| sources.iter().find(|s| s.tracks > 0).map(|s| s.id.clone()));
            if let Some(id) = pick {
                match state.music.play(&id, shuffle) {
                    Ok(status) => {
                        let _ = app.emit(EVENT_MUSIC_CHANGED, &status);
                    }
                    Err(e) => eprintln!("forma: background music: {e}"),
                }
            }
        }
        watch(&app);
    });
}

/// mpv moves on by itself (the next track, a radio's next song): every second, compare.
fn watch<R: Runtime>(app: &AppHandle<R>) {
    let mut last: Option<Status> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();
        let Ok(now) = state.music.status() else {
            continue;
        };
        if last.as_ref().is_none_or(|l| !l.same_moment(&now)) {
            let _ = app.emit(EVENT_MUSIC_CHANGED, &now);
            last = Some(now);
        }
    }
}
