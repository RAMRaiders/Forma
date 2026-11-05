//! Games' videos, played by Forma rather than by the theme's page.
//!
//! WebKitGTK's own player damages these videos on a Raspberry Pi (the bytes reach the decoder
//! garbled: squares, then a frozen picture), whatever the file. So Forma plays them in a second
//! `mpv`, full screen under its window; the theme leaves its page see-through where the video
//! shows. One video at a time: a new one replaces the last, and the window goes when it stops.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::Result;
use crate::music::Backend;

/// How long a video may take to show its first picture before it counts as not playing.
const FIRST_PICTURE: Duration = Duration::from_millis(2500);
const POLL: Duration = Duration::from_millis(40);
/// After mpv has its picture: the compositor puts its window up the frame after.
const WINDOW_SETTLE: Duration = Duration::from_millis(80);

#[derive(Default)]
struct State {
    /// Counts plays and stops: a play that is no longer the latest gives up.
    generation: u64,
    playing: Option<PathBuf>,
    sound: bool,
}

pub struct VideoPlayer {
    backend: Box<dyn Backend>,
    state: Mutex<State>,
}

impl VideoPlayer {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Plays `file` on a loop, heard or muted, and returns once its first picture is up: `true`.
    /// `false` if it could not, or another play or a stop came meanwhile.
    pub fn play(&self, file: &Path, sound: bool) -> Result<bool> {
        let generation = {
            let mut state = self.state();
            state.generation += 1;
            state.playing = Some(file.to_path_buf());
            state.sound = sound;
            state.generation
        };
        self.backend.send(json!(["set_property", "mute", !sound]))?;
        self.backend.send(json!(["loadfile", file, "replace"]))?;
        self.backend.send(json!(["set_property", "pause", false]))?;
        let deadline = Instant::now() + FIRST_PICTURE;
        loop {
            if self.state().generation != generation {
                return Ok(false);
            }
            let shown = self.property("vo-configured") == Some(Value::Bool(true))
                && self.property("idle-active") != Some(Value::Bool(true));
            if shown {
                std::thread::sleep(WINDOW_SETTLE);
                return Ok(self.state().generation == generation);
            }
            if Instant::now() >= deadline {
                let _ = self.stop();
                return Ok(false);
            }
            std::thread::sleep(POLL);
        }
    }

    /// Stops what plays; its window goes. Nothing plays: nothing is started.
    pub fn stop(&self) -> Result<()> {
        let was = {
            let mut state = self.state();
            state.generation += 1;
            state.playing.take()
        };
        if was.is_some() && self.backend.running() {
            self.backend.send(json!(["stop"]))?;
        }
        Ok(())
    }

    /// Video sound was turned on or off: what plays follows.
    pub fn set_sound(&self, sound: bool) {
        let playing = {
            let mut state = self.state();
            state.sound = sound;
            state.playing.is_some()
        };
        if playing && self.backend.running() {
            let _ = self.backend.send(json!(["set_property", "mute", !sound]));
        }
    }

    /// A video plays, or is about to.
    pub fn playing(&self) -> bool {
        self.state().playing.is_some()
    }

    fn property(&self, name: &str) -> Option<Value> {
        self.backend.send(json!(["get_property", name])).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// mpv as far as the player sees it: shows a picture once a file is loaded, unless `blind`.
    #[derive(Default)]
    struct Fake {
        sent: Mutex<Vec<Value>>,
        props: Mutex<HashMap<String, Value>>,
        running: AtomicBool,
        blind: bool,
    }

    impl Backend for &'static Fake {
        fn send(&self, command: Value) -> Result<Value> {
            self.running.store(true, Ordering::SeqCst);
            let args = command.as_array().unwrap().clone();
            let mut props = self.props.lock().unwrap();
            match args[0].as_str().unwrap() {
                "get_property" => {
                    return Ok(props
                        .get(args[1].as_str().unwrap())
                        .cloned()
                        .unwrap_or(Value::Null));
                }
                "set_property" => {
                    props.insert(args[1].as_str().unwrap().to_owned(), args[2].clone());
                }
                "loadfile" => {
                    props.insert("idle-active".into(), json!(false));
                    props.insert("vo-configured".into(), json!(!self.blind));
                }
                "stop" => {
                    props.insert("idle-active".into(), json!(true));
                    props.insert("vo-configured".into(), json!(false));
                }
                _ => {}
            }
            drop(props);
            self.sent.lock().unwrap().push(command);
            Ok(Value::Null)
        }
        fn running(&self) -> bool {
            self.running.load(Ordering::SeqCst)
        }
    }

    fn player(blind: bool) -> (&'static Fake, VideoPlayer) {
        let fake: &'static Fake = Box::leak(Box::new(Fake {
            blind,
            ..Fake::default()
        }));
        (fake, VideoPlayer::new(Box::new(fake)))
    }

    fn sent(fake: &Fake) -> Vec<String> {
        fake.sent
            .lock()
            .unwrap()
            .iter()
            .map(|c| c[0].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn plays_heard_or_muted_once_its_picture_is_up_and_stops() {
        let (fake, player) = player(false);
        assert!(player.play(Path::new("/roms/a.mp4"), false).unwrap());
        assert!(player.playing());
        assert_eq!(fake.props.lock().unwrap()["mute"], json!(true));
        player.set_sound(true);
        assert_eq!(fake.props.lock().unwrap()["mute"], json!(false));
        player.stop().unwrap();
        assert!(!player.playing());
        assert_eq!(sent(fake).last().unwrap(), "stop");
    }

    #[test]
    fn a_video_that_never_shows_counts_as_not_playing() {
        let (fake, player) = player(true);
        assert!(!player.play(Path::new("/roms/broken.mp4"), true).unwrap());
        assert!(!player.playing());
        assert_eq!(sent(fake).last().unwrap(), "stop");
    }

    #[test]
    fn stopping_with_nothing_playing_starts_nothing() {
        let (fake, player) = player(false);
        player.stop().unwrap();
        player.set_sound(false);
        assert!(sent(fake).is_empty());
        assert!(!fake.running());
    }
}
