//! Everything the app holds while running.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};

use forma_core::batocera::BatoceraSettings;
use forma_core::hierarchy::ViewCatalog;
use forma_core::input::InputHandle;
use forma_core::launch::Launcher;
use forma_core::net::{Cache, Http, achievements};
use forma_core::network::Network;
use forma_core::sync::SyncHandle;
use forma_core::themes::Theme;
use forma_core::thumbs::Thumbnailer;
use forma_core::{Library, Paths};

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
    /// How Forma reaches RetroAchievements and the netplay lobby.
    pub http: Box<dyn Http>,
    /// This box's Ethernet and Wi-Fi, through Batocera's tools.
    pub network: Network,
    /// Batocera's RetroAchievements and netplay settings.
    pub batocera: BatoceraSettings,
    /// Resized artwork, made in the background.
    pub thumbnails: Thumbnailer,
    pub playing: AtomicBool,
    /// Set by the SDK once the current theme has started.
    pub theme_ready: AtomicBool,
    /// Written once the first theme is ready, for the Batocera service (`FORMA_READY_FILE`).
    pub ready_file: Option<PathBuf>,
    /// Running on Batocera: power and network settings go to its own tools.
    pub on_batocera: bool,
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

    fn media(&self, game_id: i64, kind: &str, width: Option<u32>) -> Option<PathBuf> {
        let game = self.library().game(game_id).ok()??;
        let source = PathBuf::from(game.media.get(kind)?);
        // Video and manuals (PDF) are served as they are.
        let image = !matches!(kind, "video" | "manual");
        Some(match width {
            Some(w) if image => self.thumbnails.file_for(&source, w),
            _ => source,
        })
    }

    fn ra_media(&self, path: &str) -> Option<PathBuf> {
        let dir = self.paths.cache_dir().join("ra-media");
        achievements::media_file(&*self.http, &dir, path).ok()
    }
}
