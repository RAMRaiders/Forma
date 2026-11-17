//! Everything the app holds while running.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, RwLock, RwLockReadGuard};

use forma_core::batocera::BatoceraSettings;
use forma_core::bios::{BATOCERA_SYSTEMS, Table};
use forma_core::bluetooth::Bluetooth;
use forma_core::hierarchy::ViewCatalog;
use forma_core::input::InputHandle;
use forma_core::launch::Launcher;
use forma_core::music::Player;
use forma_core::net::{Cache, Http, achievements};
use forma_core::network::Network;
use forma_core::options::Options;
use forma_core::store::Store;
use forma_core::sync::SyncHandle;
use forma_core::system_settings::SystemSettings;
use forma_core::themes::{Theme, ThemeOrigin};
use forma_core::thumbs::Thumbnailer;
use forma_core::{Library, Paths};

use crate::protocol::Resolver;

/// The setting that names the selected theme.
pub const THEME_SETTING: &str = "theme.current";
/// The theme Forma opens on until the user picks another: Cyber, bundled at a pinned version.
pub const DEFAULT_THEME: &str = "cyber";
/// The plain theme Forma also ships, for when even the default cannot start.
pub const FALLBACK_THEME: &str = "default";

/// The list the theme opened last. Its filter goes when another list opens.
#[derive(Debug, Default)]
pub struct ListOnScreen {
    pub list: Option<(String, Vec<String>)>,
    pub filter: forma_core::hierarchy::ListFilter,
}

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
    /// Bluetooth controllers, through Batocera's tools.
    pub bluetooth: Bluetooth,
    /// Batocera's RetroAchievements and netplay settings.
    pub batocera: BatoceraSettings,
    /// Batocera's emulator, core and game options.
    pub options: Options,
    /// Forma's CRT shader for games (koko-aio), tuned per level.
    pub shaders: forma_core::shaders::Shaders,
    /// Batocera's system settings.
    pub system_settings: SystemSettings,
    /// Batocera's content downloader.
    pub store: Store,
    /// The scraper's sources and its run.
    pub scraper: crate::scraper::Scraping,
    /// The music player (mpv in the background).
    pub music: Player,
    /// Resized artwork, made in the background.
    pub thumbnails: Thumbnailer,
    /// Games' videos (a second mpv, under the window).
    pub video: forma_core::video::VideoPlayer,
    /// Opens magazines and makes their pages.
    pub magazines: forma_core::magazines::Reader,
    pub playing: AtomicBool,
    /// The game list on screen and the user's filter on it.
    pub list_filter: Mutex<ListOnScreen>,
    /// Set by the SDK once the current theme has started.
    pub theme_ready: AtomicBool,
    /// Written once the first theme is ready, for the Batocera service (`FORMA_READY_FILE`).
    pub ready_file: Option<PathBuf>,
    /// Running on Batocera: power and network settings go to its own tools.
    pub on_batocera: bool,
    /// The window starts see-through over Forma's boot picture (on Batocera, once it is drawn).
    pub see_through: bool,
    pub input: Mutex<Option<InputHandle>>,
    /// Forma's shell in the theme's page: its token, whether it has a menu open, the context.
    pub shell: crate::shell::ShellState,
    /// Where the shell's files are (`shell.js`).
    pub shell_dir: Option<PathBuf>,
    pub sync: Mutex<Option<SyncHandle>>,
    /// Batocera's list of BIOS files, read once when first needed (`None` off Batocera).
    pub bios: OnceLock<Option<Table>>,
}

impl AppState {
    pub fn library(&self) -> MutexGuard<'_, Library> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Batocera's list of BIOS files, read from its script the first time.
    pub fn bios_table(&self) -> Option<&Table> {
        self.bios
            .get_or_init(|| Table::load(std::path::Path::new(BATOCERA_SYSTEMS)))
            .as_ref()
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

/// The selected theme if it can load, else the default, else the fallback, else any that can.
pub fn pick_theme(themes: &[Theme], selected: Option<&str>) -> Option<String> {
    let available = |id: &str| themes.iter().any(|t| t.id == id && t.is_available());
    selected
        .filter(|id| available(id))
        .or(Some(DEFAULT_THEME).filter(|id| available(id)))
        .or(Some(FALLBACK_THEME).filter(|id| available(id)))
        .map(str::to_owned)
        .or_else(|| {
            themes
                .iter()
                .find(|t| t.is_available())
                .map(|t| t.id.clone())
        })
}

/// The themes the user is offered: not the plain fallback (Forma's safety net, listed only while
/// it is in use), and not a copy that cannot load because another copy of the same theme
/// stands in for it (an older bundled Cyber under a newer installed one).
pub fn listed_themes<'a>(themes: &'a [Theme], current: Option<&str>) -> Vec<&'a Theme> {
    themes
        .iter()
        .filter(|t| {
            t.id != FALLBACK_THEME
                || t.origin != ThemeOrigin::Bundled
                || current == Some(FALLBACK_THEME)
        })
        .filter(|t| {
            t.is_available()
                || !themes
                    .iter()
                    .any(|other| other.id == t.id && other.is_available())
        })
        .collect()
}

/// The URL Forma's menus load a theme's logo from, if it declares one.
pub fn theme_logo_url(theme: &Theme) -> Option<String> {
    let logo = theme.manifest.as_ref()?.logo.as_ref()?;
    theme
        .is_available()
        .then(|| format!("forma://localhost/theme/{}/{}", theme.id, logo))
}

impl Resolver for AppState {
    fn theme(&self, id: &str) -> Option<(PathBuf, Vec<String>)> {
        let theme = self.available_theme(id)?;
        let remote = theme.manifest.map(|m| m.remote).unwrap_or_default();
        Some((theme.dir, remote))
    }

    fn media(&self, game_id: i64, kind: &str, width: Option<u32>) -> Option<PathBuf> {
        let source = PathBuf::from(self.library().media_file(game_id, kind).ok()??);
        // Video and manuals (PDF) are served as they are.
        let image = !matches!(kind, "video" | "manual");
        Some(match width {
            Some(w) if image => self.thumbnails.file_for(&source, w),
            _ => source,
        })
    }

    fn magazine_page(&self, id: &str, page: usize, width: u32) -> Option<PathBuf> {
        match self.magazines.page(id, page, width) {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("forma: magazine page: {e}");
                None
            }
        }
    }

    fn manual_page(&self, game_id: i64, page: usize, width: u32) -> Option<PathBuf> {
        let file = crate::magazines::manual_file(self, game_id)?;
        match self.magazines.file_page(&file, page, width) {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("forma: manual page: {e}");
                None
            }
        }
    }

    fn shell_file(&self, name: &str) -> Option<PathBuf> {
        let dir = self.shell_dir.as_ref()?;
        crate::protocol::inside_dir(dir, name)
    }

    fn splash_file(&self, name: &str) -> Option<PathBuf> {
        let file = match name {
            "batocera-logo.svg" => PathBuf::from(crate::boot::BATOCERA_LOGO),
            _ => crate::boot::batocera_splash_file(name),
        };
        Some(file).filter(|f| f.is_file())
    }

    fn cover(&self) -> crate::shell::CoverInfo {
        crate::boot::cover_info(self)
    }

    fn ra_media(&self, path: &str) -> Option<PathBuf> {
        let dir = self.paths.cache_dir().join("ra-media");
        achievements::media_file(&*self.http, &dir, path).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(id: &str, available: bool) -> Theme {
        Theme {
            id: id.into(),
            dir: PathBuf::new(),
            origin: ThemeOrigin::Bundled,
            manifest: None,
            unavailable: (!available).then(|| "broken".into()),
            overrides: false,
        }
    }

    #[test]
    fn offers_neither_the_fallback_nor_a_copy_another_stands_in_for() {
        let mut old_cyber = theme("cyber", false);
        old_cyber.unavailable = Some("replaced by version 0.8.0-dev, installed".into());
        let mut new_cyber = theme("cyber", true);
        new_cyber.origin = ThemeOrigin::User;
        let all = [
            theme("default", true),
            old_cyber,
            new_cyber,
            theme("broken", false),
        ];
        let ids = |current| {
            listed_themes(&all, current)
                .iter()
                .map(|t| (t.id.as_str(), t.is_available()))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Some("cyber")), [("cyber", true), ("broken", false)]);
        assert_eq!(
            ids(Some("default")),
            [("default", true), ("cyber", true), ("broken", false)],
            "the fallback is listed while it is in use"
        );
    }

    #[test]
    fn picks_the_selected_then_cyber_then_the_plain_default() {
        let all = [
            theme("default", true),
            theme("cyber", true),
            theme("neon", true),
        ];
        assert_eq!(pick_theme(&all, Some("neon")).as_deref(), Some("neon"));
        assert_eq!(pick_theme(&all, None).as_deref(), Some("cyber"));
        let no_cyber = [theme("default", true), theme("cyber", false)];
        assert_eq!(
            pick_theme(&no_cyber, Some("gone")).as_deref(),
            Some("default")
        );
        assert_eq!(
            pick_theme(&[theme("neon", true)], None).as_deref(),
            Some("neon")
        );
    }
}

/// Written beside the service's ready file once Forma's cover has gone over the theme: the
/// boot picture under the see-through window can go then (see scripts/batocera/forma-standalone).
pub fn up_file(ready: &std::path::Path) -> std::path::PathBuf {
    let mut name = ready.as_os_str().to_owned();
    name.push(".up");
    std::path::PathBuf::from(name)
}

#[cfg(test)]
mod up_file_tests {
    #[test]
    fn sits_beside_the_ready_file() {
        assert_eq!(
            super::up_file(std::path::Path::new("/var/run/forma.ready")),
            std::path::PathBuf::from("/var/run/forma.ready.up")
        );
    }
}
