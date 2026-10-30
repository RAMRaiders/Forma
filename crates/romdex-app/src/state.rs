//! Everything the app holds while running.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};

use romdex_core::hierarchy::ViewCatalog;
use romdex_core::input::InputHandle;
use romdex_core::launch::Launcher;
use romdex_core::net::{Cache, Http, achievements};
use romdex_core::sync::SyncHandle;
use romdex_core::themes::Theme;
use romdex_core::{Library, Paths};

use crate::protocol::Resolver;

/// The setting that names the selected theme.
pub const THEME_SETTING: &str = "theme.current";
pub const DEFAULT_THEME: &str = "default";

pub struct AppState {
    pub paths: Paths,
    /// The UI's connection. The sync thread has its own; WAL lets them work side by side.
    pub library: Mutex<Library>,
    pub views: RwLock<ViewCatalog>,
    pub themes: RwLock<Vec<Theme>>,
    pub bundled_themes: Vec<PathBuf>,
    pub current_theme: RwLock<String>,
    pub launcher: Box<dyn Launcher>,
    /// How RomDex reaches RetroAchievements and the netplay lobby.
    pub http: Box<dyn Http>,
    pub playing: AtomicBool,
    /// Set by the SDK once the current theme has started.
    pub theme_ready: AtomicBool,
    pub input: Mutex<Option<InputHandle>>,
    pub sync: Mutex<Option<SyncHandle>>,
}

impl AppState {
    pub fn library(&self) -> MutexGuard<'_, Library> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn http_cache(&self) -> Cache {
        Cache::new(self.paths.cache_dir().join("http"))
    }

    pub fn views(&self) -> RwLockReadGuard<'_, ViewCatalog> {
        self.views.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The theme the window is showing.
    pub fn current_theme(&self) -> Option<Theme> {
        let id = self
            .current_theme
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.available_theme(&id)
    }

    pub fn available_theme(&self, id: &str) -> Option<Theme> {
        self.themes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|t| t.id == id && t.is_available())
            .cloned()
    }
}

/// The selected theme if it can load, else the default, else any that can.
pub fn pick_theme(themes: &[Theme], selected: Option<&str>) -> Option<String> {
    let available = |id: &str| themes.iter().any(|t| t.id == id && t.is_available());
    selected
        .filter(|id| available(id))
        .or(Some(DEFAULT_THEME).filter(|id| available(id)))
        .map(str::to_owned)
        .or_else(|| {
            themes
                .iter()
                .find(|t| t.is_available())
                .map(|t| t.id.clone())
        })
}

impl Resolver for AppState {
    fn theme(&self, id: &str) -> Option<(PathBuf, Vec<String>)> {
        let theme = self.available_theme(id)?;
        let remote = theme.manifest.map(|m| m.remote).unwrap_or_default();
        Some((theme.dir, remote))
    }

    fn media(&self, game_id: i64, kind: &str) -> Option<PathBuf> {
        let game = self.library().game(game_id).ok()??;
        game.media.get(kind).map(PathBuf::from)
    }

    fn ra_media(&self, path: &str) -> Option<PathBuf> {
        let dir = self.paths.cache_dir().join("ra-media");
        achievements::media_file(&*self.http, &dir, path).ok()
    }
}
