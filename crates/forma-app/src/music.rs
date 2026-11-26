//! The music player's commands (`@ramraiders/forma-sdk`'s `music`), and what Forma does with it on
//! its own: start the last playlist at boot when Batocera's background music is on, and tell
//! themes when what plays changes, mpv moving on to the next track included.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use forma_core::music::{PlayState, Radio, Source, Status};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::api::ApiResult;
use crate::commands::off_thread;
use crate::permissions;
use crate::shell::Caller;
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
    off_thread(app, |state| {
        crate::addons::refresh_radios(state);
        Ok(state.music.library().sources())
    })
    .await
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
    caller: Caller,
    name: String,
    url: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        caller.require(state, permissions::MUSIC_MANAGE)?;
        Ok(state.music.library().add_radio(&name, &url)?)
    })
    .await
}

#[tauri::command]
pub async fn music_remove_radio<R: Runtime>(
    app: AppHandle<R>,
    caller: Caller,
    url: String,
) -> ApiResult<()> {
    off_thread(app, move |state| {
        caller.require(state, permissions::MUSIC_MANAGE)?;
        Ok(state.music.library().remove_radio(&url)?)
    })
    .await
}

/// The longest music waits at boot for the UI to be up, should the shell never say so.
const UI_WAIT: Duration = Duration::from_secs(60);

/// At boot: with Batocera's background music on, the last source (else the first there is)
/// starts once the UI is up (the entry screen gone, over a started theme), then a watcher tells
/// themes whenever what plays changes.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let since = Instant::now();
        while !state.shell.ui_started.load(Ordering::SeqCst) && since.elapsed() < UI_WAIT {
            std::thread::sleep(Duration::from_millis(100));
        }
        let conf = forma_core::net::batocera_conf(&state.paths.batocera_conf());
        if conf.get("audio.bgmusic").is_some_and(|v| v == "1") && state.on_batocera {
            menu_music(&app, true);
        }
        watch(&app);
    });
}

/// Batocera's menu music (`audio.bgmusic`) turned on or off: plays what was last chosen (else
/// the first folder with songs), or stops.
pub fn menu_music<R: Runtime>(app: &AppHandle<R>, on: bool) {
    let state = app.state::<AppState>();
    if !on {
        match state.music.stop() {
            Ok(status) => {
                let _ = app.emit(EVENT_MUSIC_CHANGED, &status);
            }
            Err(e) => eprintln!("forma: background music: {e}"),
        }
        return;
    }
    if state.music.status().is_ok_and(|s| s.source.is_some()) {
        return;
    }
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
    if saved.as_deref().is_some_and(|id| id.starts_with("addon:")) {
        crate::addons::refresh_radios(&state);
    }
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

/// A new song started: for the theme (or Forma's shell) to show, for `seconds`.
pub const EVENT_MUSIC_SONG: &str = "music:song";
/// Forma's setting for how long a new song is shown, in seconds; 0 shows none.
pub const SONG_SECONDS_SETTING: &str = "music.songSeconds";
const SONG_SECONDS_DEFAULT: u32 = 8;
const SONG_SECONDS_MAX: u32 = 60;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSong {
    pub title: String,
    pub artist: Option<String>,
    /// How long to show it: Forma's setting (Start → Music).
    pub seconds: u32,
}

/// The song `now` starts, if it is a new one since `last`: playing, with a title, and not the
/// same song as before (a resume after a pause is not new).
fn new_song(last: Option<&Status>, now: &Status, seconds: u32) -> Option<MusicSong> {
    if seconds == 0 || now.state != PlayState::Playing {
        return None;
    }
    let title = now.title.clone()?;
    let same = last.is_some_and(|l| {
        l.title == now.title
            && l.artist == now.artist
            && l.track == now.track
            && l.source == now.source
    });
    (!same).then(|| MusicSong {
        title,
        artist: now.artist.clone(),
        seconds,
    })
}

fn song_seconds(state: &AppState) -> u32 {
    state
        .library()
        .setting_text(SONG_SECONDS_SETTING)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(SONG_SECONDS_DEFAULT, |s| {
            s.clamp(0.0, f64::from(SONG_SECONDS_MAX)).round() as u32
        })
}

/// mpv moves on by itself (the next track, a radio's next song): every second, compare. A new
/// song is announced too, for as long as Forma's setting says.
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
            if let Some(song) = new_song(last.as_ref(), &now, song_seconds(&state)) {
                let _ = app.emit(EVENT_MUSIC_SONG, &song);
            }
            last = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(title: Option<&str>, state: PlayState) -> Status {
        Status {
            state,
            source: None,
            title: title.map(str::to_owned),
            artist: Some("Megadeth".into()),
            album: None,
            position: None,
            duration: None,
            track: Some(1),
            tracks: Some(9),
            volume: 60,
            shuffle: false,
        }
    }

    #[test]
    fn a_new_song_is_announced_once_and_not_on_resume() {
        let first = status(Some("Angry Again"), PlayState::Playing);
        assert_eq!(
            new_song(None, &first, 8),
            Some(MusicSong {
                title: "Angry Again".into(),
                artist: Some("Megadeth".into()),
                seconds: 8
            })
        );
        let paused = status(Some("Angry Again"), PlayState::Paused);
        assert_eq!(new_song(Some(&first), &paused, 8), None);
        assert_eq!(new_song(Some(&paused), &first, 8), None, "a resume");
        let next = status(Some("Symphony of Destruction"), PlayState::Playing);
        assert!(new_song(Some(&first), &next, 8).is_some());
        assert_eq!(new_song(Some(&first), &next, 0), None, "shown for 0 s: off");
        assert_eq!(new_song(None, &status(None, PlayState::Playing), 8), None);
    }
}

/// Forma's setting for whether games' videos play with their sound (`true`, the default).
pub const VIDEO_SOUND_SETTING: &str = "video.sound";

/// Whether games' videos are heard: Start → Audio → Video sound.
pub fn video_sound(state: &AppState) -> bool {
    state
        .library()
        .setting_text(VIDEO_SOUND_SETTING)
        .ok()
        .flatten()
        .is_none_or(|v| v.trim() != "false")
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlay {
    /// The video is up, full screen under the page: the theme leaves its page see-through
    /// where it should show. `false`: the game has none, or it could not play.
    pub playing: bool,
    /// Heard (Start → Audio → Video sound), the music lowered meanwhile.
    pub sound: bool,
}

/// Plays a game's video under the theme's page, full screen or in `area` (fractions of the
/// screen), silent if `muted`, and answers once it shows. With
/// video sound on, the music goes down while it plays. A new page, or a game starting, stops it.
#[tauri::command]
pub async fn video_play<R: Runtime>(
    app: AppHandle<R>,
    game_id: i64,
    area: Option<forma_core::video::Area>,
    muted: Option<bool>,
) -> ApiResult<VideoPlay> {
    let window = app.get_webview_window("main");
    off_thread(app, move |state| {
        // `muted`: silent this once (the screensaver's choice), whatever the setting says.
        let sound = video_sound(state) && muted != Some(true);
        // A game runs: the page under it asks no video of its own (it would play on, heard).
        if state.playing.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(VideoPlay {
                playing: false,
                sound,
            });
        }
        let file = state
            .library()
            .game(game_id)?
            .and_then(|g| g.media.get("video").cloned())
            .map(std::path::PathBuf::from)
            .filter(|f| f.is_file());
        let Some(file) = file else {
            return Ok(VideoPlay {
                playing: false,
                sound,
            });
        };
        // Under the window: the window must let it through where the page does.
        if let Some(window) = window {
            let _ = window.set_background_color(Some(tauri::window::Color(4, 6, 12, 0)));
        }
        state.music.duck(sound);
        let playing = state.video.play(&file, sound, area).unwrap_or_else(|e| {
            eprintln!("forma: video: {e}");
            false
        });
        if !playing && !state.video.playing() {
            state.music.duck(false);
        }
        Ok(VideoPlay { playing, sound })
    })
    .await
}

/// Stops the game's video; the music comes back.
#[tauri::command]
pub async fn video_stop<R: Runtime>(app: AppHandle<R>) -> ApiResult<()> {
    off_thread(app, |state| {
        state.video.stop()?;
        state.music.duck(false);
        Ok(())
    })
    .await
}
