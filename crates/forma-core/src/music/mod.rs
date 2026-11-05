//! Forma's music player: what is in the music folder and the web radios, played by Batocera's
//! own `mpv` running in the background and driven over its JSON IPC socket. Forma owns what plays
//! and when (it pauses for games); themes only show it and send commands.

mod mpv;
mod sources;

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};

pub use mpv::Mpv;
pub use sources::{Library, Playable, RADIOS_FILE, Radio, Source, SourceKind};

/// Talks to the player: one JSON IPC command in, its `data` out.
pub trait Backend: Send + Sync {
    fn send(&self, command: Value) -> Result<Value>;
    /// Whether the player is running; status never starts it.
    fn running(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: PlayState,
    /// What was chosen: a folder, a playlist or a radio.
    pub source: Option<Source>,
    /// The track, or for a radio the song it announces.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Seconds; `None` for a radio's endless stream.
    pub position: Option<f64>,
    pub duration: Option<f64>,
    /// 1-based, of `tracks`.
    pub track: Option<u64>,
    pub tracks: Option<u64>,
    pub volume: u8,
    pub shuffle: bool,
}

impl Status {
    /// What changes for a listener: not the position, which moves every second.
    pub fn same_moment(&self, other: &Status) -> bool {
        self.state == other.state
            && self.source == other.source
            && self.title == other.title
            && self.track == other.track
            && self.volume == other.volume
            && self.shuffle == other.shuffle
    }
}

/// A song's artist and title from its file's name, as music folders name them:
/// `Megadeth - Angry Again.mp3` is Megadeth's "Angry Again". The extension goes; a name without
/// " - " is all title.
pub fn from_file_name(name: &str) -> (Option<String>, String) {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    // An extension is a few letters or digits after the last dot: "Mr. Blue Sky" keeps its dot.
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    };
    let stem = stem.trim();
    // " - " is the rule; "Artist- Title" and "Artist -Title" are common slips. A dash inside a
    // word ("AC-DC") never splits.
    let split = stem
        .split_once(" - ")
        .or_else(|| stem.split_once("- "))
        .or_else(|| stem.split_once(" -"));
    match split {
        Some((artist, title)) if !artist.trim().is_empty() && !title.trim().is_empty() => {
            (Some(artist.trim().to_owned()), title.trim().to_owned())
        }
        _ => (None, stem.to_owned()),
    }
}

#[derive(Default)]
struct State {
    source: Option<Source>,
    shuffle: bool,
    volume: u8,
    /// Paused by Forma for a game, so resumed after it; a pause the user chose stays.
    paused_for_game: bool,
}

pub struct Player {
    library: Library,
    backend: Box<dyn Backend>,
    /// Where the track lists for folders are written.
    run_dir: PathBuf,
    state: Mutex<State>,
}

impl Player {
    pub fn new(library: Library, backend: Box<dyn Backend>, run_dir: PathBuf, volume: u8) -> Self {
        Self {
            library,
            backend,
            run_dir,
            state: Mutex::new(State {
                volume: volume.min(100),
                ..State::default()
            }),
        }
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn play(&self, source_id: &str, shuffle: bool) -> Result<Status> {
        let source = self
            .library
            .sources()
            .into_iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| Error::Invalid(format!("nothing to play called `{source_id}`")))?;
        let playable = self.library.resolve(source_id)?;
        let volume = self.state().volume;
        self.backend
            .send(json!(["set_property", "volume", volume]))?;
        self.backend
            .send(json!(["set_property", "shuffle", shuffle]))?;
        match playable {
            Playable::Tracks(tracks) => {
                let list = self.run_dir.join("music.m3u");
                std::fs::create_dir_all(&self.run_dir).map_err(|e| Error::io(&self.run_dir, e))?;
                let text: String = tracks
                    .iter()
                    .map(|t| format!("{}\n", t.display()))
                    .collect();
                std::fs::write(&list, text).map_err(|e| Error::io(&list, e))?;
                self.backend.send(json!(["loadlist", list, "replace"]))?;
            }
            Playable::List(file) => {
                self.backend.send(json!(["loadlist", file, "replace"]))?;
            }
            Playable::Url(url) => {
                self.backend.send(json!(["loadfile", url, "replace"]))?;
            }
        }
        self.backend.send(json!(["set_property", "pause", false]))?;
        {
            let mut state = self.state();
            state.source = Some(source);
            state.shuffle = shuffle;
            state.paused_for_game = false;
        }
        self.status()
    }

    /// Pauses, or resumes. Nothing loaded: nothing happens.
    pub fn toggle(&self) -> Result<Status> {
        if self.loaded() {
            self.backend.send(json!(["cycle", "pause"]))?;
            self.state().paused_for_game = false;
        }
        self.status()
    }

    pub fn next(&self) -> Result<Status> {
        if self.loaded() {
            self.backend.send(json!(["playlist-next", "weak"]))?;
        }
        self.status()
    }

    pub fn previous(&self) -> Result<Status> {
        if self.loaded() {
            self.backend.send(json!(["playlist-prev", "weak"]))?;
        }
        self.status()
    }

    pub fn stop(&self) -> Result<Status> {
        if self.backend.running() {
            self.backend.send(json!(["stop"]))?;
        }
        let mut state = self.state();
        state.source = None;
        state.paused_for_game = false;
        drop(state);
        self.status()
    }

    pub fn set_volume(&self, volume: u8) -> Result<Status> {
        let volume = volume.min(100);
        self.state().volume = volume;
        if self.backend.running() {
            self.backend
                .send(json!(["set_property", "volume", volume]))?;
        }
        self.status()
    }

    /// A game starts (`true`) or ends: music playing pauses for it, and comes back after.
    pub fn game(&self, playing: bool) {
        if !self.backend.running() {
            return;
        }
        let mut state = self.state();
        if playing {
            let was_playing =
                self.property("pause").ok() == Some(Value::Bool(false)) && state.source.is_some();
            if was_playing
                && self
                    .backend
                    .send(json!(["set_property", "pause", true]))
                    .is_ok()
            {
                state.paused_for_game = true;
            }
        } else if state.paused_for_game {
            state.paused_for_game = false;
            let _ = self.backend.send(json!(["set_property", "pause", false]));
        }
    }

    pub fn status(&self) -> Result<Status> {
        let state = self.state();
        let stopped = Status {
            state: PlayState::Stopped,
            source: None,
            title: None,
            artist: None,
            album: None,
            position: None,
            duration: None,
            track: None,
            tracks: None,
            volume: state.volume,
            shuffle: state.shuffle,
        };
        if !self.backend.running() || state.source.is_none() {
            return Ok(stopped);
        }
        let source = state.source.clone();
        let (volume, shuffle) = (state.volume, state.shuffle);
        drop(state);
        if self.property("idle-active").ok() == Some(Value::Bool(true)) {
            return Ok(stopped);
        }
        let paused = self.property("pause").ok() == Some(Value::Bool(true));
        let meta = self.property("metadata").unwrap_or(Value::Null);
        let tag = |names: &[&str]| -> Option<String> {
            let map = meta.as_object()?;
            names.iter().find_map(|n| {
                map.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(n))
                    .and_then(|(_, v)| v.as_str())
                    .map(str::to_owned)
                    .filter(|v| !v.trim().is_empty())
            })
        };
        let radio = source.as_ref().is_some_and(|s| s.kind == SourceKind::Radio);
        // A radio announces the song as "Artist - Title" in its ICY metadata.
        let (artist, title) = match tag(&["icy-title"]) {
            Some(icy) => match icy.split_once(" - ") {
                Some((a, t)) => (Some(a.trim().to_owned()), Some(t.trim().to_owned())),
                None => (None, Some(icy)),
            },
            None => match tag(&["title"]) {
                Some(title) => (tag(&["artist"]), Some(title)),
                // No tags: mpv's title is the file's name, "Artist - Title.mp3" more often than
                // not.
                None => {
                    let file = self
                        .property("media-title")
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned));
                    match file.as_deref().map(from_file_name) {
                        Some((artist, title)) => (tag(&["artist"]).or(artist), Some(title)),
                        None => (tag(&["artist"]), None),
                    }
                }
            },
        };
        let number = |name: &str| self.property(name).ok().and_then(|v| v.as_f64());
        Ok(Status {
            state: if paused {
                PlayState::Paused
            } else {
                PlayState::Playing
            },
            source,
            title,
            artist,
            album: tag(&["album"]),
            position: if radio { None } else { number("time-pos") },
            duration: if radio { None } else { number("duration") },
            track: if radio {
                None
            } else {
                number("playlist-pos").map(|p| p as u64 + 1)
            },
            tracks: if radio {
                None
            } else {
                number("playlist-count").map(|c| c as u64)
            },
            volume,
            shuffle,
        })
    }

    fn loaded(&self) -> bool {
        self.backend.running() && self.state().source.is_some()
    }

    fn property(&self, name: &str) -> Result<Value> {
        self.backend.send(json!(["get_property", name]))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Records commands; answers `get_property` from a map.
    #[derive(Default)]
    struct Fake {
        sent: Mutex<Vec<Value>>,
        props: Mutex<HashMap<String, Value>>,
        running: std::sync::atomic::AtomicBool,
    }

    impl Backend for &'static Fake {
        fn send(&self, command: Value) -> Result<Value> {
            self.running
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let args = command.as_array().unwrap().clone();
            if args[0] == "get_property" {
                return self
                    .props
                    .lock()
                    .unwrap()
                    .get(args[1].as_str().unwrap())
                    .cloned()
                    .ok_or_else(|| Error::Network("property unavailable".into()));
            }
            if args[0] == "set_property" {
                self.props
                    .lock()
                    .unwrap()
                    .insert(args[1].as_str().unwrap().to_owned(), args[2].clone());
            }
            self.sent.lock().unwrap().push(command);
            Ok(Value::Null)
        }
        fn running(&self) -> bool {
            self.running.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn player() -> (tempfile::TempDir, &'static Fake, Player) {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir_all(music.join("Chiptune")).unwrap();
        std::fs::write(music.join("Chiptune/a.ogg"), "").unwrap();
        std::fs::write(music.join("Chiptune/b.ogg"), "").unwrap();
        std::fs::write(
            music.join(RADIOS_FILE),
            "#EXTINF:-1,Groove\nhttps://radio.example/live\n",
        )
        .unwrap();
        let fake: &'static Fake = Box::leak(Box::default());
        let lib = Library {
            music,
            fallback: dir.path().join("none"),
            addon_radios: Default::default(),
        };
        let player = Player::new(lib, Box::new(fake), dir.path().join("run"), 60);
        (dir, fake, player)
    }

    fn commands(fake: &Fake) -> Vec<String> {
        fake.sent
            .lock()
            .unwrap()
            .iter()
            .map(|c| c[0].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_folder_plays_as_a_written_track_list() {
        let (dir, fake, player) = player();
        let status = player.play("folder:Chiptune", true).unwrap();
        assert_eq!(status.source.unwrap().name, "Chiptune");
        assert!(status.shuffle);
        let list = std::fs::read_to_string(dir.path().join("run/music.m3u")).unwrap();
        let names: Vec<_> = list
            .lines()
            .map(|l| l.rsplit(['/', '\\']).next().unwrap())
            .collect();
        assert_eq!(names, ["a.ogg", "b.ogg"]);
        assert_eq!(
            commands(fake),
            ["set_property", "set_property", "loadlist", "set_property"]
        );
        assert_eq!(fake.props.lock().unwrap()["volume"], json!(60));
    }

    #[test]
    fn a_file_name_gives_artist_and_title_without_its_extension() {
        let pick = |n: &str| {
            let (a, t) = from_file_name(n);
            (a, t)
        };
        assert_eq!(
            pick("Megadeth - Angry Again.mp3"),
            (Some("Megadeth".into()), "Angry Again".into())
        );
        assert_eq!(
            pick("/music/System Of A Down - Toxicity.flac"),
            (Some("System Of A Down".into()), "Toxicity".into())
        );
        assert_eq!(
            pick("Scorpions- Blackout.mp3"),
            (Some("Scorpions".into()), "Blackout".into())
        );
        assert_eq!(pick("Mr. Blue Sky"), (None, "Mr. Blue Sky".into()));
        assert_eq!(pick("01 Intro.ogg"), (None, "01 Intro".into()));
        assert_eq!(
            pick("AC-DC - T.N.T..mp3"),
            (Some("AC-DC".into()), "T.N.T.".into())
        );
    }

    #[test]
    fn status_reads_tags_and_a_radio_announces_artist_and_title() {
        let (_dir, fake, player) = player();
        player
            .play("radio:https://radio.example/live", false)
            .unwrap();
        fake.props.lock().unwrap().extend([
            ("idle-active".to_owned(), json!(false)),
            (
                "metadata".to_owned(),
                json!({ "icy-title": "Freshmoods - Seafly", "icy-name": "Groove" }),
            ),
            ("time-pos".to_owned(), json!(12.5)),
        ]);
        let status = player.status().unwrap();
        assert_eq!(status.state, PlayState::Playing);
        assert_eq!(
            (status.artist.as_deref(), status.title.as_deref()),
            (Some("Freshmoods"), Some("Seafly"))
        );
        assert_eq!(status.position, None, "a stream has no position");

        player.play("folder:Chiptune", false).unwrap();
        fake.props.lock().unwrap().extend([
            (
                "metadata".to_owned(),
                json!({ "ARTIST": "Yuzo Koshiro", "Title": "Go Straight", "album": "SoR2" }),
            ),
            ("playlist-pos".to_owned(), json!(1)),
            ("playlist-count".to_owned(), json!(2)),
            ("duration".to_owned(), json!(180.0)),
        ]);
        let status = player.status().unwrap();
        assert_eq!(status.title.as_deref(), Some("Go Straight"));
        assert_eq!(status.artist.as_deref(), Some("Yuzo Koshiro"));
        assert_eq!(
            (status.track, status.tracks, status.duration),
            (Some(2), Some(2), Some(180.0))
        );
    }

    #[test]
    fn a_game_pauses_the_music_and_brings_it_back_unless_the_user_had_paused() {
        let (_dir, fake, player) = player();
        player.play("folder:Chiptune", false).unwrap();
        player.game(true);
        assert_eq!(fake.props.lock().unwrap()["pause"], json!(true));
        player.game(false);
        assert_eq!(fake.props.lock().unwrap()["pause"], json!(false));

        // Paused by the user: a game neither pauses nor resumes it.
        fake.props
            .lock()
            .unwrap()
            .insert("pause".into(), json!(true));
        player.game(true);
        player.game(false);
        assert_eq!(fake.props.lock().unwrap()["pause"], json!(true));
    }

    #[test]
    fn nothing_loaded_means_stopped_and_no_player_is_started() {
        let (_dir, fake, player) = player();
        assert_eq!(player.status().unwrap().state, PlayState::Stopped);
        player.toggle().unwrap();
        player.next().unwrap();
        player.set_volume(150).unwrap();
        assert!(!fake.running(), "status and controls never start mpv");
        assert_eq!(player.status().unwrap().volume, 100);
        assert!(player.play("folder:Nope", false).is_err());
        assert!(commands(fake).is_empty());
    }
}
