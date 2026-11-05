//! `mpv` in the background, idle until told to play, driven over its JSON IPC socket.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Backend;
use crate::error::{Error, Result};

pub struct Mpv {
    socket: PathBuf,
    /// What it is started with, besides its socket.
    args: Vec<String>,
    /// What it is, in errors: "the music player".
    name: &'static str,
    child: Mutex<Option<std::process::Child>>,
}

/// The music player: sound only.
const MUSIC_ARGS: &[&str] = &[
    "--no-video",
    "--idle=yes",
    "--no-terminal",
    "--audio-display=no",
    "--really-quiet",
];

/// Games' videos: full screen, under Forma's window (a labwc rule keeps `forma-video` at the
/// bottom; Forma's window is see-through where the video shows). No window while nothing plays,
/// no keys or on-screen display of its own, filling the screen as a theme's `cover` would.
const VIDEO_ARGS: &[&str] = &[
    "--idle=yes",
    "--force-window=no",
    "--fs",
    "--wayland-app-id=forma-video",
    "--no-input-default-bindings",
    "--input-vo-keyboard=no",
    "--input-cursor=no",
    "--cursor-autohide=always",
    "--osc=no",
    "--osd-level=0",
    "--loop-file=inf",
    "--panscan=1.0",
    "--hwdec=no",
    "--no-terminal",
    "--really-quiet",
];

impl Mpv {
    /// `socket`: where mpv listens. Its path also names the process, so Batocera's scripts can
    /// stop it when the screen goes back to EmulationStation.
    pub fn new(socket: PathBuf) -> Self {
        Self::with(socket, MUSIC_ARGS, "the music player")
    }

    /// The player of games' videos (see [`crate::video`]).
    pub fn video(socket: PathBuf) -> Self {
        Self::with(socket, VIDEO_ARGS, "the video player")
    }

    fn with(socket: PathBuf, args: &[&str], name: &'static str) -> Self {
        Self {
            socket,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            name,
            child: Mutex::new(None),
        }
    }

    fn start(&self) -> Result<()> {
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(c) = child.as_mut()
            && c.try_wait().ok().flatten().is_none()
        {
            return Ok(());
        }
        // An mpv from an earlier run of Forma, still listening: use it.
        if self.connect().is_ok() {
            return Ok(());
        }
        let _ = std::fs::remove_file(&self.socket);
        if let Some(dir) = self.socket.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let started = crate::host::command("mpv")
            .args(&self.args)
            .arg(format!("--input-ipc-server={}", self.socket.display()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Error::Unsupported(format!("{} (mpv) could not start: {e}", self.name)))?;
        *child = Some(started);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.connect().is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(Error::Unsupported(format!(
            "{} (mpv) did not answer",
            self.name
        )))
    }

    #[cfg(unix)]
    fn connect(&self) -> std::io::Result<std::os::unix::net::UnixStream> {
        let stream = std::os::unix::net::UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        Ok(stream)
    }

    #[cfg(not(unix))]
    fn connect(&self) -> std::io::Result<std::fs::File> {
        Err(std::io::Error::other("the music player needs Unix sockets"))
    }
}

impl Backend for Mpv {
    fn send(&self, command: Value) -> Result<Value> {
        use std::io::{BufRead, BufReader, Write};
        self.start()?;
        let stream = self
            .connect()
            .map_err(|e| Error::Network(format!("{} went away: {e}", self.name)))?;
        let mut writer = &stream;
        let request = json!({ "command": command, "request_id": 1 });
        writeln!(writer, "{request}")
            .map_err(|e| Error::Network(format!("{} went away: {e}", self.name)))?;
        // Events can arrive before the answer; the answer carries our request id.
        for line in BufReader::new(&stream).lines() {
            let line =
                line.map_err(|e| Error::Network(format!("{} did not answer: {e}", self.name)))?;
            let Ok(answer) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if answer.get("request_id") != Some(&json!(1)) {
                continue;
            }
            return match answer.get("error").and_then(Value::as_str) {
                Some("success") => Ok(answer.get("data").cloned().unwrap_or(Value::Null)),
                Some(error) => Err(Error::Network(format!("{}: {error}", self.name))),
                None => Ok(Value::Null),
            };
        }
        Err(Error::Network(format!("{} did not answer", self.name)))
    }

    fn running(&self) -> bool {
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        match child.as_mut() {
            Some(c) => c.try_wait().ok().flatten().is_none(),
            // Not started by this Forma, but one may be listening from an earlier run.
            None => self.connect().is_ok(),
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
