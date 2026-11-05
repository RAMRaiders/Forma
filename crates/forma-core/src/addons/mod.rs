//! Forma add-ons (see `docs/addons.md`): folders in `/userdata/system/forma/addons/<id>/`, each
//! with an `addon.json` and one JavaScript file that Forma runs in QuickJS ([`runtime`]). An
//! add-on reaches only what Forma hands it: HTTP to the hosts it declares, its own settings
//! and a log. What it `provides` decides what Forma calls:
//!
//! - `scraper`: a source beside the built-in ones (`lookup(query)`);
//! - `music`: web radios in Forma's Music page (`stations()`);
//! - `menu`: pages in Forma's own menu (`page(path)` and `action(id, input)`), drawn by Forma.
//!
//! An add-on from Forma's own catalogue may also have a **system part**: scripts Forma runs with
//! full rights when it installs and removes it, and commands its pages can run. Anywhere else,
//! an add-on with a system part is refused: only reviewed, checksummed add-ons get that trust.

pub mod runtime;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SDK_API_VERSION;
use crate::error::{Error, Result};
use crate::net::Http;
use crate::paths::Paths;
use crate::scraper::MediaKind;
use crate::themes::catalogue::{CatalogueTheme, MAX_PACKAGE, hex, swap_in, unpack};
use crate::themes::satisfies;

/// The ids of the add-ons the user turned off (JSON list).
pub const DISABLED_SETTING: &str = "addons.disabled";

/// What add-ons may provide.
pub const PROVIDES: [&str; 3] = ["scraper", "music", "menu"];

/// What an add-on may ask for, and what it means, for the user to read before installing.
pub const PERMISSIONS: [(&str, &str); 2] = [
    (
        "network",
        "reach the internet sites it lists, and only those",
    ),
    (
        "system",
        "install and run its own programs with full control of this machine",
    ),
];

/// Written by [`install`] beside an add-on from Forma's own catalogue: the package's hash.
/// Without it, an add-on's system part never runs.
const VERIFIED_FILE: &str = ".forma-verified";

/// The largest entry file.
const MAX_CODE: u64 = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingDecl {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
}

/// For a `scraper` add-on: what it gives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScraperDecl {
    pub gives: Vec<MediaKind>,
    #[serde(default = "yes")]
    pub metadata: bool,
}

fn yes() -> bool {
    true
}

fn main_js() -> String {
    "main.js".to_owned()
}

/// For a `menu` add-on: its tile in Forma's menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuDecl {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// The system part: shell scripts in the add-on's folder, run with Forma's rights (root on
/// Batocera). `commands` are what its pages may run, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDecl {
    #[serde(default)]
    pub install: Option<String>,
    #[serde(default)]
    pub remove: Option<String>,
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
}

impl SystemDecl {
    fn scripts(&self) -> impl Iterator<Item = &String> {
        self.install
            .iter()
            .chain(&self.remove)
            .chain(self.commands.values())
    }
}

/// `addon.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Caret range of the Forma API it needs, as a theme's `sdk`.
    pub sdk: String,
    /// The JavaScript module Forma runs: one file, bundled.
    #[serde(default = "main_js")]
    pub entry: String,
    pub provides: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// `https://host[:port]` origins it may reach, with `network`.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// What the user enters for it (API keys, logins), shown in Forma's menu.
    #[serde(default)]
    pub settings: Vec<SettingDecl>,
    #[serde(default)]
    pub scraper: Option<ScraperDecl>,
    #[serde(default)]
    pub menu: Option<MenuDecl>,
    #[serde(default)]
    pub system: Option<SystemDecl>,
}

impl Manifest {
    pub fn provides(&self, what: &str) -> bool {
        self.provides.iter().any(|p| p == what)
    }
}

/// An add-on folder, and whether it can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Addon {
    pub id: String,
    #[serde(skip)]
    pub dir: PathBuf,
    pub manifest: Option<Manifest>,
    /// Why it cannot run, if it cannot.
    pub problem: Option<String>,
    /// Installed from Forma's own catalogue, as the catalogue listed it: its system part may
    /// run.
    pub verified: bool,
}

impl Addon {
    pub fn usable(&self) -> bool {
        self.manifest.is_some() && self.problem.is_none()
    }
}

pub fn addons_dir(paths: &Paths) -> PathBuf {
    paths.userdata.join("system/forma/addons")
}

/// Every add-on folder, by id. Hidden folders (installs in progress) are skipped.
pub fn discover(paths: &Paths) -> Vec<Addon> {
    let Ok(entries) = std::fs::read_dir(addons_dir(paths)) else {
        return Vec::new();
    };
    let mut found: Vec<Addon> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.path().is_dir())
        .map(|e| load(&e.path()))
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// Reads and checks one add-on folder, which must be named after its id.
pub fn load(dir: &Path) -> Addon {
    let mut addon = read(dir);
    let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if addon.problem.is_none() && addon.id != folder {
        addon.problem = Some(format!("it is in a folder not named `{}`", addon.id));
    }
    addon
}

/// Reads and checks an add-on folder, whatever its name.
fn read(dir: &Path) -> Addon {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let fail = |id: String, why: String| Addon {
        id,
        dir: dir.to_path_buf(),
        manifest: None,
        problem: Some(why),
        verified: false,
    };
    let file = dir.join("addon.json");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => return fail(folder, format!("no readable addon.json: {e}")),
    };
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => return fail(folder, format!("addon.json: {e}")),
    };
    let verified = dir.join(VERIFIED_FILE).is_file();
    let mut problem = check(&manifest, dir).err();
    if problem.is_none() && manifest.system.is_some() && !verified {
        problem = Some(
            "it has a system part, which runs only when installed from Forma's catalogue".into(),
        );
    }
    Addon {
        id: manifest.id.clone(),
        dir: dir.to_path_buf(),
        manifest: Some(manifest),
        problem,
        verified,
    }
}

fn kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// An `https://host[:port]` origin: no path, no query, no user.
fn origin(host: &str) -> bool {
    host.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
    })
}

fn check(m: &Manifest, dir: &Path) -> std::result::Result<(), String> {
    if !kebab(&m.id) {
        return Err("the id must be kebab-case".into());
    }
    if !satisfies(&m.sdk, SDK_API_VERSION) {
        return Err(format!(
            "it needs Forma API {}; this Forma has {SDK_API_VERSION}",
            m.sdk
        ));
    }
    if let Some(p) = m.provides.iter().find(|p| !PROVIDES.contains(&p.as_str())) {
        return Err(format!("this Forma has nothing for it to provide as `{p}`"));
    }
    if m.provides.is_empty() {
        return Err("it provides nothing".into());
    }
    if m.provides.iter().any(|p| p == "scraper") && m.scraper.is_none() {
        return Err("a scraper add-on needs a `scraper` section".into());
    }
    if m.provides("menu") && m.menu.is_none() {
        return Err("a menu add-on needs a `menu` section".into());
    }
    let wants_system = m.permissions.iter().any(|p| p == "system");
    if m.system.is_some() != wants_system {
        return Err("a system part and the `system` permission go together".into());
    }
    if let Some(system) = &m.system {
        for script in system.scripts() {
            inside(dir, script)?;
        }
    }
    if let Some(p) = m
        .permissions
        .iter()
        .find(|p| !PERMISSIONS.iter().any(|(k, _)| k == p))
    {
        return Err(format!("unknown permission `{p}`"));
    }
    if let Some(h) = m.hosts.iter().find(|h| !origin(h)) {
        return Err(format!("host `{h}` is not an https:// origin"));
    }
    if !m.hosts.is_empty() && !m.permissions.iter().any(|p| p == "network") {
        return Err("it lists hosts without the `network` permission".into());
    }
    let mut keys = HashSet::new();
    if let Some(s) = m
        .settings
        .iter()
        .find(|s| !keys.insert(&s.key) || s.key.is_empty())
    {
        return Err(format!("setting `{}` is empty or listed twice", s.key));
    }
    let size = inside(dir, &m.entry)?;
    if size > MAX_CODE {
        return Err(format!("`{}` is over {} MB", m.entry, MAX_CODE >> 20));
    }
    Ok(())
}

/// Checks `file` is a file inside `dir`; returns its size.
fn inside(dir: &Path, file: &str) -> std::result::Result<u64, String> {
    let path = Path::new(file);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(format!("`{file}` must be a file inside the add-on"));
    }
    match std::fs::symlink_metadata(dir.join(path)) {
        Ok(meta) if meta.is_file() => Ok(meta.len()),
        _ => Err(format!("no `{file}`")),
    }
}

/// Runs one of the system part's scripts with `args`, from the add-on's folder. Only for a
/// verified add-on.
pub fn run_script(
    shell: &dyn crate::host::Shell,
    addon: &Addon,
    script: &str,
    args: &[&str],
) -> Result<String> {
    if !addon.verified {
        return Err(Error::Invalid(format!(
            "{} is not from Forma's catalogue: its system part does not run",
            addon.id
        )));
    }
    inside(&addon.dir, script).map_err(Error::Invalid)?;
    let path = addon.dir.join(script);
    let path = path.to_string_lossy();
    let mut all = vec![path.as_ref()];
    all.extend_from_slice(args);
    shell.run("sh", &all)
}
/// What each of an add-on's permissions lets it do, in words.
pub fn permission_descriptions(m: &Manifest) -> Vec<String> {
    m.permissions
        .iter()
        .map(|p| {
            PERMISSIONS
                .iter()
                .find(|(k, _)| k == p)
                .map_or_else(|| p.clone(), |(_, d)| (*d).to_owned())
        })
        .collect()
}

/// The disabled ids, from [`DISABLED_SETTING`].
pub fn disabled(stored: Option<&str>) -> HashSet<String> {
    stored
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Deletes an installed add-on.
pub fn remove(shell: &dyn crate::host::Shell, paths: &Paths, id: &str) -> Result<()> {
    if !kebab(id) {
        return Err(Error::Invalid(format!("`{id}` is not an add-on id")));
    }
    let dir = addons_dir(paths).join(id);
    let addon = load(&dir);
    if let Some(script) = addon
        .manifest
        .as_ref()
        .and_then(|m| m.system.as_ref())
        .and_then(|s| s.remove.as_ref())
        .filter(|_| addon.verified)
    {
        // It undoes what it installed; a failure is reported, and the folder stays so it can
        // be tried again.
        run_script(shell, &addon, script, &[])?;
    }
    let meta = std::fs::symlink_metadata(&dir)
        .map_err(|_| Error::Invalid(format!("no add-on `{id}` is installed")))?;
    if meta.file_type().is_symlink() {
        std::fs::remove_file(&dir).map_err(|e| Error::io(&dir, e))
    } else {
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))
    }
}

/// Downloads, checks and installs an add-on from a catalogue, replacing an installed copy.
/// `official`: the catalogue is Forma's own, so an add-on with a system part may install and
/// its install script runs; from any other catalogue such an add-on is refused.
pub fn install(
    http: &dyn Http,
    shell: &dyn crate::host::Shell,
    paths: &Paths,
    entry: &CatalogueTheme,
    official: bool,
) -> Result<Addon> {
    if !satisfies(&entry.sdk, SDK_API_VERSION) {
        return Err(Error::Invalid(format!(
            "{} {} needs Forma API {}; this Forma has {SDK_API_VERSION}",
            entry.name, entry.version, entry.sdk
        )));
    }
    let package = http
        .download(&entry.download, MAX_PACKAGE)
        .map_err(|e| Error::Network(format!("could not download {}: {}", entry.name, e.message)))?;
    let hash = hex(&Sha256::digest(&package));
    if !hash.eq_ignore_ascii_case(&entry.sha256) {
        return Err(Error::Invalid(format!(
            "the download of {} does not match the catalogue (sha256 {hash}); not installed",
            entry.name
        )));
    }
    let root = addons_dir(paths);
    std::fs::create_dir_all(&root).map_err(|e| Error::io(&root, e))?;
    let work = root.join(format!(".install-{}", entry.id));
    let _ = std::fs::remove_dir_all(&work);
    let bad = |why: String| Error::Invalid(format!("{} is not a usable add-on: {why}", entry.name));
    let result = (|| {
        unpack(&package, &work).map_err(bad)?;
        // `addon.json` at the root or in its one folder.
        let inner = if work.join("addon.json").is_file() {
            work.clone()
        } else {
            let only: Vec<PathBuf> = std::fs::read_dir(&work)
                .map_err(|e| Error::io(&work, e))?
                .flatten()
                .map(|e| e.path())
                .collect();
            match only.as_slice() {
                [one] if one.join("addon.json").is_file() => one.clone(),
                _ => return Err(bad("no addon.json at its root or in one folder".into())),
            }
        };
        let addon = read(&inner);
        let manifest = addon
            .manifest
            .as_ref()
            .ok_or_else(|| bad(addon.problem.clone().unwrap_or_default()))?;
        if manifest.id != entry.id || manifest.version != entry.version {
            return Err(bad(format!(
                "it holds {} {}, not {} {}",
                manifest.id, manifest.version, entry.id, entry.version
            )));
        }
        let system = manifest.system.clone();
        if system.is_some() && !official {
            return Err(bad(
                "it has a system part, and only add-ons from Forma's own catalogue may".into(),
            ));
        }
        if let Some(why) = addon
            .problem
            .as_ref()
            .filter(|p| !p.starts_with("it has a system part"))
        {
            return Err(bad(why.clone()));
        }
        // Only Forma marks an add-on verified: a package's own mark goes.
        let _ = std::fs::remove_file(inner.join(VERIFIED_FILE));
        if system.is_some() {
            std::fs::write(inner.join(VERIFIED_FILE), &entry.sha256)
                .map_err(|e| Error::io(&inner, e))?;
        }
        let target = root.join(&entry.id);
        swap_in(&inner, &target)?;
        let installed = load(&target);
        if let Some(script) = system.as_ref().and_then(|s| s.install.as_ref())
            && let Err(e) = run_script(shell, &installed, script, &[])
        {
            // Half installed is worse than not: take it out again.
            let _ = std::fs::remove_dir_all(&target);
            return Err(bad(format!("its install script failed: {e}")));
        }
        Ok(installed)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests;
