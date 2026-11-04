//! Batocera's own updates, as EmulationStation's Updates menu has them: the box's version
//! against the one on Batocera's update server for its board and channel (`updates.type`,
//! `updates.url`), and the update itself through `batocera-upgrade`, which downloads the new
//! system to `/userdata` and unpacks it onto `/boot`. It takes effect after a reboot. Nothing
//! here ever starts an update by itself.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Stdio;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host::command;
use crate::net::{Http, batocera_conf};

const SERVER: &str = "https://updates.batocera.org";
const VERSION_FILE: &str = "/usr/share/batocera/batocera.version";
const BOARD_FILE: &str = "/boot/boot/batocera.board";
const CHANNELS: &[&str] = &["stable", "unstable", "butterfly"];

/// What the box runs and what Batocera offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// The version the box runs, e.g. `43.1`.
    pub current: String,
    /// The newest on the server for this board and channel.
    pub latest: String,
    /// `stable`, `unstable` or `butterfly`.
    pub channel: String,
    /// `latest` is not what the box runs.
    pub available: bool,
}

/// While updating: Batocera's word on what it is doing, and how far, when it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub message: String,
    pub percent: Option<u8>,
}

pub struct Updates {
    conf: PathBuf,
    version_file: PathBuf,
    board_file: PathBuf,
}

impl Updates {
    pub fn new(conf: PathBuf) -> Self {
        Self {
            conf,
            version_file: VERSION_FILE.into(),
            board_file: BOARD_FILE.into(),
        }
    }

    /// For tests: where the version and the board are read from.
    pub fn with_files(mut self, version_file: PathBuf, board_file: PathBuf) -> Self {
        self.version_file = version_file;
        self.board_file = board_file;
        self
    }

    /// Asks Batocera's update server, as `batocera-upgrade` would download from it.
    pub fn check(&self, http: &dyn Http) -> Result<UpdateCheck> {
        let read = |path: &PathBuf| {
            std::fs::read_to_string(path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })
        };
        let current = version(&read(&self.version_file)?);
        let conf = batocera_conf(&self.conf);
        let channel = conf
            .get("updates.type")
            .map(String::as_str)
            .filter(|c| CHANNELS.contains(c))
            .unwrap_or("stable")
            .to_owned();
        let server = conf
            .get("updates.url")
            .map(|u| u.trim_end_matches('/'))
            .filter(|u| !u.is_empty())
            .unwrap_or(SERVER);
        let board = match read(&self.board_file)?.trim() {
            // As batocera-upgrade: the Pi 4 downloads the 64-bit build.
            "rpi4" => "rpi464".to_owned(),
            b => b.to_owned(),
        };
        let url = format!("{server}/{board}/{channel}/last/batocera.version");
        let body = http
            .get(&url)
            .map_err(|e| Error::Network(format!("Batocera's update server: {}", e.message)))?;
        let latest = version(&String::from_utf8_lossy(&body));
        if latest.is_empty() {
            return Err(Error::Network(
                "Batocera's update server sent no version".into(),
            ));
        }
        Ok(UpdateCheck {
            available: latest != current,
            current,
            latest,
            channel,
        })
    }

    /// Downloads and unpacks the update with `batocera-upgrade`, reporting as it goes. Takes
    /// a long while (gigabytes); the box needs a reboot after it.
    pub fn install(&self, mut on_progress: impl FnMut(UpdateProgress)) -> Result<()> {
        let mut child = command("batocera-upgrade")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Network(format!("could not run `batocera-upgrade`: {e}")))?;
        let mut last = String::new();
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(std::io::Result::ok) {
                if let Some(progress) = progress(&line) {
                    last.clone_from(&progress.message);
                    on_progress(progress);
                }
            }
        }
        let status = child
            .wait()
            .map_err(|e| Error::Network(format!("`batocera-upgrade`: {e}")))?;
        if status.success() {
            Ok(())
        } else if last.is_empty() {
            Err(Error::Network("the update failed".into()))
        } else {
            Err(Error::Network(format!("the update failed: {last}")))
        }
    }
}

/// `43.1 2026/05/29 12:46`: the version is the first word.
fn version(text: &str) -> String {
    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// A line of `batocera-upgrade`: `300 of 2229 MB downloaded ... >>> 13%`, `Extracting files
/// ...`; the noise in between (`url: …`, tar's file list) is left out.
fn progress(line: &str) -> Option<UpdateProgress> {
    let line = line.trim();
    if let Some((message, percent)) = line.split_once(">>>") {
        let percent = percent.trim().trim_end_matches('%').trim().parse().ok();
        let message = message.trim().trim_end_matches('.').trim();
        return Some(UpdateProgress {
            message: message.to_owned(),
            percent: percent.filter(|p| *p <= 100),
        });
    }
    let said = [
        "Starting",
        "need to",
        "Not enough",
        "Check",
        "Checksum",
        "Backing",
        "Extracting",
        "Generating",
        "Finally",
        "Error",
        "remounting",
    ];
    said.iter()
        .any(|s| line.starts_with(s))
        .then(|| UpdateProgress {
            message: line.trim_end_matches('.').trim().to_owned(),
            percent: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpError;
    use std::sync::Mutex;

    struct Server {
        asked: Mutex<Vec<String>>,
        answer: &'static str,
    }

    impl Http for Server {
        fn get(&self, url: &str) -> std::result::Result<Vec<u8>, HttpError> {
            self.asked.lock().unwrap().push(url.to_owned());
            Ok(self.answer.as_bytes().to_vec())
        }
    }

    fn updates(conf: &str, board: &str) -> (tempfile::TempDir, Updates) {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path
        };
        let u = Updates::new(file("batocera.conf", conf)).with_files(
            file("batocera.version", "43.1 2026/05/29 12:46\n"),
            file("batocera.board", board),
        );
        (dir, u)
    }

    #[test]
    fn asks_the_server_for_this_board_and_channel() {
        let (_dir, u) = updates("updates.type=unstable\n", "rpi4\n");
        let server = Server {
            asked: Mutex::default(),
            answer: "44 2026/10/02 09:00\n",
        };
        let check = u.check(&server).unwrap();
        assert_eq!(
            check,
            UpdateCheck {
                current: "43.1".into(),
                latest: "44".into(),
                channel: "unstable".into(),
                available: true,
            }
        );
        assert_eq!(
            *server.asked.lock().unwrap(),
            ["https://updates.batocera.org/rpi464/unstable/last/batocera.version"]
        );
    }

    #[test]
    fn the_same_version_is_up_to_date_and_an_odd_channel_is_stable() {
        let (_dir, u) = updates(
            "updates.type=nightly\nupdates.url=https://mirror.example/\n",
            "bcm2712",
        );
        let server = Server {
            asked: Mutex::default(),
            answer: "43.1 2026/05/29 12:46",
        };
        let check = u.check(&server).unwrap();
        assert!(!check.available);
        assert_eq!(check.channel, "stable");
        assert_eq!(
            *server.asked.lock().unwrap(),
            ["https://mirror.example/bcm2712/stable/last/batocera.version"]
        );
    }

    #[test]
    fn reads_batocera_upgrades_progress() {
        assert_eq!(
            progress("300 of 2229 MB downloaded ... >>> 13%"),
            Some(UpdateProgress {
                message: "300 of 2229 MB downloaded".into(),
                percent: Some(13),
            })
        );
        assert_eq!(
            progress("Extracting files ...").unwrap().message,
            "Extracting files"
        );
        assert_eq!(progress("./boot/linux"), None);
        assert_eq!(progress("url: https://updates.batocera.org/x"), None);
    }
}
