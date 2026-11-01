//! Batocera's content downloader: free games, ports, decorations and music from Batocera's
//! package store, as EmulationStation's "Content downloader" offers them.
//!
//! Everything goes through `batocera-store` (pacman underneath), as ES does, so a package
//! installed from Forma is installed for ES and Batocera too. Its games land in the ROM folders,
//! where the library's watcher picks them up. EmulationStation themes are left out: Forma's
//! themes are its own.
//!
//! pacman holds a lock while it works: one operation at a time, the others refused as busy.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host;
use crate::xml;

const PROGRAM: &str = "batocera-store";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub name: String,
    pub description: String,
    /// Batocera's group: `sys-<system>` for a system's games, `bezel`, `music`, `ports`, `misc`.
    pub group: String,
    /// The system a `sys-…` package is for, e.g. `nes`.
    pub system: Option<String>,
    pub version: String,
    /// Where it comes from: a project or forum page.
    pub url: Option<String>,
    pub packager: Option<String>,
    /// In bytes.
    pub download_size: u64,
    pub installed_size: u64,
    pub preview_url: Option<String>,
    pub installed: bool,
    /// Installed, and the store has a newer version.
    pub update: bool,
}

/// Where an install is, as `batocera-store` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    /// `download`, then `install`.
    pub stage: &'static str,
    /// 0–100; while installing it only shows that something is happening.
    pub percent: u8,
}

pub struct Store {
    program: PathBuf,
    busy: Mutex<()>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new(Path::new(PROGRAM))
    }
}

impl Store {
    /// `program` is `batocera-store`; tests give a stand-in.
    pub fn new(program: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            busy: Mutex::new(()),
        }
    }

    /// Every package, with what is installed. Reads the catalogue first if it was never read.
    pub fn list(&self) -> Result<Vec<Package>> {
        let xml = self.run(&["list"], &mut |_| {})?;
        Ok(parse(&xml))
    }

    /// Reads the catalogue again from Batocera's server.
    pub fn refresh(&self) -> Result<()> {
        self.run(&["refresh"], &mut |_| {}).map(drop)
    }

    /// Downloads and installs a package, reporting progress as it goes.
    pub fn install(&self, name: &str, mut on_progress: impl FnMut(Progress)) -> Result<()> {
        check_name(name)?;
        self.run(&["install", name], &mut |line| {
            if let Some(p) = progress(line) {
                on_progress(p);
            }
        })
        .map(drop)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        check_name(name)?;
        self.run(&["remove", name], &mut |_| {}).map(drop)
    }

    /// Updates every installed package that has a newer version.
    pub fn update(&self) -> Result<()> {
        self.run(&["update"], &mut |_| {}).map(drop)
    }

    /// Runs `batocera-store`, handing each line of its output over as it comes, and returns
    /// it all.
    fn run(&self, args: &[&str], on_line: &mut dyn FnMut(&str)) -> Result<String> {
        let _held = self.busy.try_lock().map_err(|_| {
            Error::Unsupported("the content downloader is busy with another package".into())
        })?;
        let mut child = host::command(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::io(&self.program, e))?;
        let mut out = String::new();
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                let line = line.map_err(|e| Error::io(&self.program, e))?;
                on_line(&line);
                out.push_str(&line);
                out.push('\n');
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|e| Error::io(&self.program, e))?;
        if output.status.success() && !out.contains("Download error!") {
            return Ok(out);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let why = stderr
            .lines()
            .chain(out.lines())
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.contains(">>>"))
            .unwrap_or("it failed");
        Err(Error::Network(format!(
            "the content downloader could not {}: {why}",
            args[0]
        )))
    }
}

/// pacman takes anything starting with `-` as an option: package names are plain words.
fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-@".contains(c));
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!("`{name}` is not a package name")))
    }
}

/// `[478MB] - 'Night Slashers X' >>>42` while downloading; `579MB to extract ...>>>15` after.
fn progress(line: &str) -> Option<Progress> {
    let (before, percent) = line.rsplit_once(">>>")?;
    let percent: u8 = percent.trim().parse().ok()?;
    Some(Progress {
        stage: if before.contains("to extract") {
            "install"
        } else {
            "download"
        },
        percent: percent.min(100),
    })
}

/// `pacman -Ss --xml`, Batocera's flavour.
fn parse(text: &str) -> Vec<Package> {
    let Some(root) = xml::parse(text).root else {
        return Vec::new();
    };
    root.children_named("package")
        .filter_map(|p| {
            let text = |name: &str| p.child_text(name).map(str::to_owned);
            let group = text("group").unwrap_or_default();
            if group.contains("theme") {
                return None;
            }
            let status = p.child("status");
            // Sizes are in KiB.
            let size =
                |name: &str| text(name).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 1024;
            Some(Package {
                name: text("name")?,
                description: text("description").unwrap_or_default(),
                system: group.strip_prefix("sys-").map(str::to_owned),
                version: text("available_version").unwrap_or_default(),
                url: text("url"),
                packager: text("packager"),
                download_size: size("download_size"),
                installed_size: size("installed_size"),
                preview_url: text("preview_url"),
                installed: status.is_some_and(|s| s.text.trim() == "installed"),
                update: status.is_some_and(|s| s.attr("need_update") == Some("true")),
                group,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/batocera")
    }

    fn store() -> Store {
        Store::new(&fixtures().join("fake-batocera-store"))
    }

    #[test]
    fn lists_packages_without_es_themes() {
        let packages = store().list().unwrap();
        assert_eq!(packages.len(), 10);
        assert!(packages.iter().all(|p| !p.group.contains("theme")));
        let night = &packages[0];
        assert_eq!(night.name, "NightSlashersXWideAsh_24A");
        assert_eq!(night.system.as_deref(), Some("windows"));
        assert_eq!(night.download_size, 490_102 * 1024);
        assert!(night.installed && !night.update);
        let amoeba = packages
            .iter()
            .find(|p| p.name == "atari2600-amoeba-jump")
            .unwrap();
        assert!(!amoeba.installed);
        assert_eq!(
            amoeba.description,
            "Amoeba Jump - Infinite Jumper with RetroAchievements"
        );
        assert_eq!(packages.iter().filter(|p| p.update).count(), 1);
        let bezel = packages.iter().find(|p| p.group == "bezel").unwrap();
        assert_eq!(bezel.system, None);
    }

    #[test]
    fn install_reports_download_then_install_progress() {
        let mut seen = Vec::new();
        store()
            .install("atari2600-amoeba-jump", |p| seen.push((p.stage, p.percent)))
            .unwrap();
        assert_eq!(
            seen,
            [
                ("download", 0),
                ("download", 50),
                ("download", 100),
                ("install", 5),
                ("install", 10)
            ]
        );
    }

    #[test]
    fn a_failed_download_is_an_error_with_the_reason() {
        let err = store().install("broken-package", |_| {}).unwrap_err();
        assert!(err.to_string().contains("Download error!"), "{err}");
        let err = store().remove("not-installed").unwrap_err();
        assert!(err.to_string().contains("target not found"), "{err}");
    }

    #[test]
    fn names_that_pacman_would_read_as_options_are_refused() {
        assert!(store().install("--overwrite=*", |_| {}).is_err());
        assert!(store().remove("a b").is_err());
        assert!(store().remove("").is_err());
    }

    #[test]
    fn progress_lines_are_read_as_batocera_writes_them() {
        assert_eq!(
            progress("[478MB] - 'Night Slashers X' >>>42"),
            Some(Progress {
                stage: "download",
                percent: 42
            })
        );
        assert_eq!(
            progress("579MB to extract ...>>>15"),
            Some(Progress {
                stage: "install",
                percent: 15
            })
        );
        assert_eq!(progress("Download error!"), None);
    }
}
