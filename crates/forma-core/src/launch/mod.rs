//! Launching games the way Batocera's EmulationStation does.
//!
//! The command comes from the system's `<command>` template in `es_systems.cfg`, so whatever
//! Batocera runs (normally `emulatorlauncher`) keeps working: emulator choice, per-game
//! settings and RetroArch achievements are configgen's business, not ours.
//!
//! Unlike EmulationStation, the command is never handed to a shell. The template is split
//! into arguments first and each placeholder is filled into its own argument, so a ROM called
//! `$(rm -rf ~).nes` is a file name and nothing else.

mod runner;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::library::{Game, Library};
use crate::paths::Paths;
use crate::systems::System;

pub use runner::{ExitInfo, Launcher, MockLauncher, ProcessLauncher, launcher_from_env};

/// A connected gamepad, in player order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Controller {
    pub index: usize,
    /// SDL GUID, lower-case hex.
    pub guid: String,
    pub name: String,
    /// The device node and SDL's counts, for the launcher. Never sent to a theme: paths on disk
    /// stay inside Forma.
    #[serde(skip)]
    pub device: Option<crate::input::Device>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    /// Overrides Batocera's configured emulator/core for this launch only.
    pub emulator: Option<String>,
    pub core: Option<String>,
    pub netplay: Option<Netplay>,
    /// Start it though its system's BIOS is missing (the user said so).
    #[serde(default)]
    pub ignore_bios: bool,
}

/// Netplay through Batocera's launcher (`-netplaymode`, `-netplayip`, `-netplayport`,
/// `-netplaypass`, `-netplaysession`), which sets RetroArch up as EmulationStation does. The
/// rest (nickname, relay, the host's own port) comes from Batocera's `global.netplay.*`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum Netplay {
    #[serde(rename_all = "camelCase")]
    Host {
        port: Option<u16>,
        /// Players must give it to join.
        #[serde(default)]
        password: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Client {
        host: String,
        port: u16,
        #[serde(default)]
        password: Option<String>,
        /// A relay's session for the room, when joining through a relay.
        #[serde(default)]
        session: Option<String>,
        /// Watch instead of play.
        #[serde(default)]
        spectate: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    /// Where the launcher writes the emulator's output, for diagnosing a failed launch.
    pub log: PathBuf,
}

impl LaunchCommand {
    /// The command as a shell would need it typed, for logs and bug reports only.
    pub fn display(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A game ready to run: everything taken from the library, so the library is not held while
/// the game plays (which can be hours).
#[derive(Debug, Clone)]
pub struct Prepared {
    pub game: Game,
    pub command: LaunchCommand,
}

/// What themes hear about a game session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GameEvent {
    #[serde(rename = "game:start", rename_all = "camelCase")]
    Start {
        game_id: i64,
        system: String,
        name: String,
    },
    #[serde(rename = "game:end", rename_all = "camelCase")]
    End {
        game_id: i64,
        seconds: u64,
        exit_code: Option<i32>,
        /// Why the launch failed, if it did.
        error: Option<String>,
    },
}

/// A launch that exits non-zero this quickly crashed or was refused; it is not a play.
const MIN_PLAY: Duration = Duration::from_secs(10);

/// Looks the game up, writes the game-info file and builds the command.
pub fn prepare(
    lib: &Library,
    paths: &Paths,
    game_id: i64,
    request: &LaunchRequest,
    controllers: &[Controller],
) -> Result<Prepared> {
    let (game, system) = lib
        .launch_target(game_id)?
        .ok_or_else(|| Error::Launch(format!("no game with id {game_id}")))?;
    let rom = system.rom_path.join(&game.path);
    let run_dir = paths.forma_dir().join("run");
    std::fs::create_dir_all(&run_dir).map_err(|e| Error::io(&run_dir, e))?;
    let game_info = run_dir.join("game.xml");
    std::fs::write(&game_info, game_info_xml(&game, &rom)).map_err(|e| Error::io(&game_info, e))?;

    let command = build_command(
        &system,
        &rom,
        &Placeholders {
            game_info: &game_info,
            home: &paths.userdata.join("system"),
            controllers,
        },
        request,
        run_dir.join("launch.log"),
    )?;
    Ok(Prepared { game, command })
}

/// Records a finished session: play count, last played and time played, as the user's own
/// data so a gamelist re-import never undoes them. A quick failure is not counted.
pub fn finish(lib: &mut Library, game_id: i64, exit: &ExitInfo, ended: SystemTime) -> Result<()> {
    let crashed = exit.code != Some(0) && exit.duration < MIN_PLAY;
    if crashed {
        return Ok(());
    }
    lib.record_play(game_id, ended, exit.duration)
}

struct Placeholders<'a> {
    game_info: &'a Path,
    home: &'a Path,
    controllers: &'a [Controller],
}

fn build_command(
    system: &System,
    rom: &Path,
    p: &Placeholders<'_>,
    request: &LaunchRequest,
    log: PathBuf,
) -> Result<LaunchCommand> {
    let template = system.command.as_deref().ok_or_else(|| {
        Error::Launch(format!(
            "system `{}` has no <command> in es_systems.cfg",
            system.name
        ))
    })?;
    let rom_str = rom.to_string_lossy();
    let basename = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut argv: Vec<String> = Vec::new();
    for token in split_template(template) {
        if token == "%CONTROLLERSCONFIG%" {
            argv.extend(controllers_args(p.controllers));
            continue;
        }
        argv.push(
            token
                .replace("%ROM%", &rom_str)
                .replace("%ROM_RAW%", &rom_str)
                .replace("%BASENAME%", &basename)
                .replace("%SYSTEM%", &system.name)
                .replace("%SYSTEMNAME%", &system.fullname)
                .replace("%GAMEINFOXML%", &p.game_info.to_string_lossy())
                .replace("%HOME%", &p.home.to_string_lossy()),
        );
    }

    if let Some(emulator) = &request.emulator {
        argv.extend(["-emulator".to_owned(), emulator.clone()]);
    }
    if let Some(core) = &request.core {
        argv.extend(["-core".to_owned(), core.clone()]);
    }
    match &request.netplay {
        None => {}
        Some(Netplay::Host { port, password }) => {
            argv.extend(["-netplaymode".to_owned(), "host".to_owned()]);
            if let Some(port) = port {
                argv.extend(["-netplayport".to_owned(), port.to_string()]);
            }
            if let Some(password) = password.as_deref().filter(|p| !p.is_empty()) {
                argv.extend([
                    "-netplaypass".to_owned(),
                    checked_word(password, "password")?,
                ]);
            }
        }
        Some(Netplay::Client {
            host,
            port,
            password,
            session,
            spectate,
        }) => {
            argv.extend([
                "-netplaymode".to_owned(),
                if *spectate { "spectator" } else { "client" }.to_owned(),
                "-netplayip".to_owned(),
                checked_host(host)?,
                "-netplayport".to_owned(),
                port.to_string(),
            ]);
            if let Some(password) = password.as_deref().filter(|p| !p.is_empty()) {
                argv.extend([
                    "-netplaypass".to_owned(),
                    checked_word(password, "password")?,
                ]);
            }
            if let Some(session) = session.as_deref().filter(|s| !s.is_empty()) {
                argv.extend([
                    "-netplaysession".to_owned(),
                    checked_word(session, "session")?,
                ]);
            }
        }
    }

    if argv.is_empty() {
        return Err(Error::Launch(format!(
            "system `{}` has an empty <command>",
            system.name
        )));
    }
    let program = argv.remove(0);
    Ok(LaunchCommand {
        program,
        args: argv,
        log,
    })
}

/// A host name or address: letters, digits, dots, dashes and colons (IPv6), nothing a launcher
/// could read as an option.
fn checked_host(host: &str) -> Result<String> {
    let host = host.trim();
    let ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    if ok {
        Ok(host.to_owned())
    } else {
        Err(Error::Invalid(format!(
            "`{host}` is not a host name or address"
        )))
    }
}

/// One word for Batocera's launcher: no spaces or newlines (it writes it into RetroArch's
/// config line by line) and not an option.
fn checked_word(word: &str, what: &str) -> Result<String> {
    let ok = word.len() <= 128
        && !word.starts_with('-')
        && word.chars().all(|c| c.is_ascii_graphic() && c != '"');
    if ok {
        Ok(word.to_owned())
    } else {
        Err(Error::Invalid(format!(
            "the netplay {what} must be one word without quotes"
        )))
    }
}

/// Splits a command template on whitespace, keeping "quoted parts" together (quotes removed).
fn split_template(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in template.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(current);
    }
    out
}

/// `-p1index 0 -p1guid … -p1name … -p1devicepath …` per connected pad, as EmulationStation
/// passes them: configgen maps controllers by GUID and name, and Batocera 43's fails without
/// the device node.
fn controllers_args(controllers: &[Controller]) -> Vec<String> {
    let mut args = Vec::new();
    for (player, c) in controllers.iter().enumerate().take(8) {
        let p = player + 1;
        args.extend([
            format!("-p{p}index"),
            c.index.to_string(),
            format!("-p{p}guid"),
            c.guid.clone(),
            format!("-p{p}name"),
            c.name.clone(),
        ]);
        if let Some(d) = &c.device {
            args.extend([
                format!("-p{p}devicepath"),
                d.path.to_string_lossy().into_owned(),
                format!("-p{p}nbbuttons"),
                d.buttons.to_string(),
                format!("-p{p}nbhats"),
                d.hats.to_string(),
                format!("-p{p}nbaxes"),
                d.axes.to_string(),
            ]);
        }
    }
    args
}

/// The `-gameinfoxml` file: the game's metadata in gamelist form, which configgen reads for
/// RetroAchievements, bezels and the like.
fn game_info_xml(game: &Game, rom: &Path) -> String {
    let mut fields: Vec<(&str, String)> = vec![
        ("path", rom.to_string_lossy().into_owned()),
        ("name", game.name.clone()),
    ];
    let optional = [
        ("desc", &game.description),
        ("developer", &game.developer),
        ("publisher", &game.publisher),
        ("genre", &game.genre),
        ("players", &game.players),
    ];
    fields.extend(
        optional
            .into_iter()
            .filter_map(|(k, v)| Some((k, v.clone()?))),
    );
    if let Some(id) = game.ra_game_id {
        fields.push(("cheevosId", id.to_string()));
    }
    for (kind, path) in &game.media {
        if matches!(kind.as_str(), "image" | "thumbnail" | "marquee" | "video") {
            fields.push((kind.as_str(), path.clone()));
        }
    }
    let body: String = fields
        .iter()
        .map(|(k, v)| format!("\t\t<{k}>{}</{k}>\n", xml_escape(v)))
        .collect();
    format!("<?xml version=\"1.0\"?>\n<gameList>\n\t<game>\n{body}\t</game>\n</gameList>\n")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:%".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests;
