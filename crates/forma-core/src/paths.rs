//! Where Batocera keeps things, and how to point Forma somewhere else for development.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Overrides the `/userdata` root, e.g. `fixtures/userdata`.
pub const ENV_USERDATA: &str = "FORMA_USERDATA";
/// Overrides the base system definitions, normally shipped read-only in the Batocera image.
pub const ENV_SYSTEMS_CFG: &str = "FORMA_SYSTEMS_CFG";

const BATOCERA_USERDATA: &str = "/userdata";
const BATOCERA_SYSTEMS_CFG: &str = "/usr/share/emulationstation/es_systems.cfg";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub userdata: PathBuf,
    pub systems_cfg: PathBuf,
}

impl Paths {
    /// Stock Batocera locations, unless overridden by [`ENV_USERDATA`] / [`ENV_SYSTEMS_CFG`].
    pub fn from_env() -> Self {
        Self::resolve(|key| std::env::var_os(key))
    }

    /// [`Paths::from_env`] with the environment injected, so it can be tested. An empty value
    /// counts as unset: `FORMA_USERDATA= forma` should not resolve everything to the cwd.
    pub fn resolve(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let var = |key: &str, default: &str| {
            get(key)
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from(default), PathBuf::from)
        };
        Self {
            userdata: var(ENV_USERDATA, BATOCERA_USERDATA),
            systems_cfg: var(ENV_SYSTEMS_CFG, BATOCERA_SYSTEMS_CFG),
        }
    }

    pub fn roms(&self) -> PathBuf {
        self.userdata.join("roms")
    }

    /// Where users drop `es_systems_*.cfg` files that add or replace systems.
    pub fn systems_overlay_dir(&self) -> PathBuf {
        self.userdata.join("system/configs/emulationstation")
    }

    pub fn batocera_conf(&self) -> PathBuf {
        self.userdata.join("system/batocera.conf")
    }

    /// Batocera's music folder: tracks, sub-folders, playlists and `radios.m3u`.
    pub fn music_dir(&self) -> PathBuf {
        self.userdata.join("music")
    }

    /// connman's provisioning file, which Batocera links into `/var/lib/connman` at boot.
    pub fn network_config(&self) -> PathBuf {
        self.userdata.join("system/network-connman.config")
    }

    /// Forma's own state: the library database, user views, imports, caches.
    pub fn forma_dir(&self) -> PathBuf {
        self.userdata.join("system/forma")
    }

    pub fn database(&self) -> PathBuf {
        self.forma_dir().join("library.db")
    }

    /// Answers from online services and media fetched for themes. Safe to delete.
    pub fn cache_dir(&self) -> PathBuf {
        self.forma_dir().join("cache")
    }

    /// Where other tools drop import files (`*.jsonl`, see `docs/import-format.md`).
    pub fn import_dir(&self) -> PathBuf {
        self.forma_dir().join("import")
    }

    /// The user's own hierarchy views, one JSON file each.
    pub fn views_dir(&self) -> PathBuf {
        self.forma_dir().join("views")
    }

    /// Batocera's files spell paths as `/userdata/...`. Moves such a path under the configured
    /// root, so a fixture tree behaves like a real box. Any other path is returned unchanged.
    pub fn rebase(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(BATOCERA_USERDATA) {
            Ok(rest) => self.userdata.join(rest),
            Err(_) => path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn with(vars: &[(&str, &str)]) -> Paths {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        Paths::resolve(|key| map.get(key).cloned())
    }

    #[test]
    fn defaults_match_a_stock_batocera_install() {
        let paths = with(&[]);
        assert_eq!(paths.userdata, Path::new("/userdata"));
        assert_eq!(
            paths.systems_cfg,
            Path::new("/usr/share/emulationstation/es_systems.cfg")
        );
        assert_eq!(paths.roms(), Path::new("/userdata/roms"));
        assert_eq!(
            paths.systems_overlay_dir(),
            Path::new("/userdata/system/configs/emulationstation")
        );
        assert_eq!(
            paths.batocera_conf(),
            Path::new("/userdata/system/batocera.conf")
        );
        assert_eq!(
            paths.database(),
            Path::new("/userdata/system/forma/library.db")
        );
    }

    #[test]
    fn env_overrides_both_roots() {
        let paths = with(&[(ENV_USERDATA, "/tmp/ud"), (ENV_SYSTEMS_CFG, "/tmp/es.cfg")]);
        assert_eq!(paths.roms(), Path::new("/tmp/ud/roms"));
        assert_eq!(paths.systems_cfg, Path::new("/tmp/es.cfg"));
    }

    #[test]
    fn an_empty_override_counts_as_unset() {
        assert_eq!(with(&[(ENV_USERDATA, "")]).userdata, Path::new("/userdata"));
    }

    #[test]
    fn rebase_moves_only_userdata_paths() {
        let paths = with(&[(ENV_USERDATA, "/tmp/ud")]);
        assert_eq!(
            paths.rebase(Path::new("/userdata/roms/nes")),
            Path::new("/tmp/ud/roms/nes")
        );
        assert_eq!(
            paths.rebase(Path::new("/media/usb/nes")),
            Path::new("/media/usb/nes")
        );
        // A prefix match on a path component, not a string: `/userdatabase` is not `/userdata`.
        assert_eq!(
            paths.rebase(Path::new("/userdatabase/x")),
            Path::new("/userdatabase/x")
        );
    }
}
