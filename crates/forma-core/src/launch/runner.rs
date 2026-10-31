//! Running a launch command: for real, or pretending to for development.

use std::fs::File;
use std::io::Write as _;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::LaunchCommand;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitInfo {
    /// `None` when the process was killed by a signal.
    pub code: Option<i32>,
    pub duration: Duration,
}

/// Runs a game to completion. Blocking: call it off the UI thread.
pub trait Launcher: Send + Sync {
    fn run(&self, command: &LaunchCommand) -> Result<ExitInfo>;
}

/// Starts the real process, with its output in `command.log`.
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn run(&self, command: &LaunchCommand) -> Result<ExitInfo> {
        let log = File::create(&command.log).map_err(|e| Error::io(&command.log, e))?;
        let err_log = log.try_clone().map_err(|e| Error::io(&command.log, e))?;
        let started = Instant::now();
        let status = crate::host::command(&command.program)
            .args(&command.args)
            .stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(err_log)
            .status()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::Launch(format!(
                    "`{}` was not found. Outside Batocera, set FORMA_LAUNCHER=mock",
                    command.program
                )),
                _ => Error::Launch(format!("could not start `{}`: {e}", command.program)),
            })?;
        Ok(ExitInfo {
            code: status.code(),
            duration: started.elapsed(),
        })
    }
}

/// Logs the command it would have run and "plays" for a set time. For development and tests.
pub struct MockLauncher {
    pub play_for: Duration,
    pub exit_code: i32,
    ran: Mutex<Vec<LaunchCommand>>,
}

impl MockLauncher {
    pub fn new(play_for: Duration) -> Self {
        Self {
            play_for,
            exit_code: 0,
            ran: Mutex::new(Vec::new()),
        }
    }

    /// Every command run so far.
    pub fn ran(&self) -> Vec<LaunchCommand> {
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Launcher for MockLauncher {
    fn run(&self, command: &LaunchCommand) -> Result<ExitInfo> {
        let line = format!("[mock launcher] {}\n", command.display());
        File::create(&command.log)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(|e| Error::io(&command.log, e))?;
        eprint!("{line}");
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command.clone());
        std::thread::sleep(self.play_for);
        Ok(ExitInfo {
            code: Some(self.exit_code),
            duration: self.play_for,
        })
    }
}

/// `FORMA_LAUNCHER=mock` selects the [`MockLauncher`] (a two-second "game"); anything else
/// runs the real command.
pub fn launcher_from_env() -> Box<dyn Launcher> {
    match std::env::var("FORMA_LAUNCHER").as_deref() {
        Ok("mock") => Box::new(MockLauncher::new(Duration::from_secs(2))),
        _ => Box::new(ProcessLauncher),
    }
}
