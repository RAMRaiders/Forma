//! Starting Forma: open the library, find themes, start the background threads, open the
//! window on the right theme.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::Duration;

use forma_core::batocera::BatoceraSettings;
use forma_core::hierarchy::load_views;
use forma_core::input::{self, InputOptions};
use forma_core::launch::launcher_from_env;
use forma_core::network::{Network, SystemShell};
use forma_core::sync::{self, SyncEvent, SyncOptions};
use forma_core::themes::{Theme, discover};
use forma_core::thumbs::{self, Thumbnailer, Thumbs};
use forma_core::{Library, Paths};
use tauri::{App, AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::commands::{
    EVENT_INPUT, EVENT_LIBRARY_CHANGED, EVENT_VIEWS_CHANGED, LibraryChanged, entry_url,
};
use crate::state::{AppState, DEFAULT_THEME, THEME_SETTING, pick_theme};

static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Called first thing in `main`, so start-up time includes Tauri and WebView creation.
pub fn mark_process_start() {
    PROCESS_START.get_or_init(std::time::Instant::now);
}

/// Time since the process started, for the start-up measurement the Batocera spike (#39) needs.
pub fn since_start() -> Duration {
    PROCESS_START
        .get()
        .map_or(Duration::ZERO, std::time::Instant::elapsed)
}

/// Present on every Batocera install, and nowhere else.
const BATOCERA_VERSION_FILE: &str = "/usr/share/batocera/batocera.version";

/// How long a theme has to call `theme_ready` before Forma falls back to the default theme.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

pub fn start(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let paths = Paths::from_env();
    let library = Library::open(&paths.database())?;
    let bundled_themes = bundled_theme_dirs(app.handle());
    let themes = discover(&bundled_themes, &paths);
    for t in themes.iter().filter(|t| !t.is_available()) {
        eprintln!(
            "forma: theme `{}` unavailable: {}",
            t.id,
            t.unavailable.as_deref().unwrap_or_default()
        );
    }
    let selected = library.setting_text(THEME_SETTING)?;
    let current = pick_theme(&themes, selected.as_deref()).unwrap_or_default();

    app.manage(AppState {
        views: RwLock::new(load_views(&paths)),
        library: Mutex::new(library),
        themes: RwLock::new(themes),
        bundled_themes,
        current_theme: RwLock::new(current.clone()),
        launcher: launcher_from_env(),
        http: Box::new(forma_core::net::UreqHttp::default()),
        network: Network::new(Box::new(SystemShell), &paths),
        batocera: BatoceraSettings::new(Box::new(SystemShell), &paths.batocera_conf()),
        thumbnails: Thumbnailer::spawn(Thumbs::new(
            paths.cache_dir().join("thumbs"),
            thumbs::DEFAULT_MAX_BYTES,
        )),
        playing: AtomicBool::new(false),
        theme_ready: AtomicBool::new(false),
        ready_file: std::env::var_os("FORMA_READY_FILE").map(PathBuf::from),
        on_batocera: Path::new(BATOCERA_VERSION_FILE).is_file(),
        input: Mutex::new(None),
        sync: Mutex::new(None),
        paths: paths.clone(),
    });
    let state = app.state::<AppState>();

    let handle = app.handle().clone();
    let sync = sync::spawn(
        paths.clone(),
        paths.database(),
        SyncOptions::default(),
        move |event| on_sync(&handle, event),
    );
    *state.sync.lock().unwrap_or_else(PoisonError::into_inner) = Some(sync);

    let handle = app.handle().clone();
    match input::spawn(InputOptions::default(), move |event| {
        let _ = handle.emit(EVENT_INPUT, event);
    }) {
        Ok(input) => *state.input.lock().unwrap_or_else(PoisonError::into_inner) = Some(input),
        Err(e) => eprintln!("forma: no gamepad input: {e}"),
    }

    let url = match state.available_theme(&current) {
        Some(theme) => WebviewUrl::CustomProtocol(perf_url(entry_url(&theme)).parse()?),
        // No theme can load: the bundled boot page explains instead of showing nothing.
        None => WebviewUrl::App("index.html".into()),
    };
    let window = WebviewWindowBuilder::new(app, "main", url)
        .title("Forma")
        .inner_size(1280.0, 720.0)
        .fullscreen(fullscreen())
        .build()?;
    reload_if_the_page_dies(&window);

    watch_ready(app.handle().clone());
    #[cfg(debug_assertions)]
    reload_on_theme_change(app.handle().clone());
    Ok(())
}

/// WebKitGTK draws the page in a process of its own. If that process dies, the window keeps its
/// last frame and stops answering input while Forma itself runs on, so the Batocera service sees
/// nothing wrong. Reload the page instead.
#[cfg(target_os = "linux")]
fn reload_if_the_page_dies<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use webkit2gtk::WebViewExt;
    let attached = window.with_webview(|webview| {
        webview
            .inner()
            .connect_web_process_terminated(|view, reason| {
                eprintln!("forma: the page's process ended ({reason:?}); reloading");
                view.reload();
            });
    });
    if let Err(e) = attached {
        eprintln!("forma: cannot watch the page's process: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
fn reload_if_the_page_dies<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

/// Loads `theme` in the window and makes it current.
pub fn show_theme<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    theme: &Theme,
) -> Result<(), String> {
    *state
        .current_theme
        .write()
        .unwrap_or_else(PoisonError::into_inner) = theme.id.clone();
    state.theme_ready.store(false, Ordering::SeqCst);
    let window = app
        .get_webview_window("main")
        .ok_or("the main window is gone")?;
    let url = perf_url(entry_url(theme))
        .parse()
        .map_err(|e| format!("{e}"))?;
    window.navigate(url).map_err(|e| e.to_string())?;
    watch_ready(app.clone());
    Ok(())
}

/// `FORMA_PERF=1` asks the theme for its frame-time overlay (`#perf`), for measuring scrolling.
fn perf_url(url: String) -> String {
    if std::env::var("FORMA_PERF").as_deref() == Ok("1") {
        url + "#perf"
    } else {
        url
    }
}

/// Release builds are a kiosk; development is windowed. `FORMA_FULLSCREEN=1|0` overrides.
fn fullscreen() -> bool {
    match std::env::var("FORMA_FULLSCREEN").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => !cfg!(debug_assertions),
    }
}

/// Falls back to the default theme if the current one never reports ready.
fn watch_ready<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        std::thread::sleep(READY_TIMEOUT);
        let state = app.state::<AppState>();
        if state.theme_ready.load(Ordering::SeqCst) {
            return;
        }
        let Some(current) = state.current_theme() else {
            return;
        };
        if current.id == DEFAULT_THEME {
            return;
        }
        eprintln!(
            "forma: theme `{}` did not start within {READY_TIMEOUT:?}; using the default theme",
            current.id
        );
        if let Some(default) = state.available_theme(DEFAULT_THEME) {
            let _ = show_theme(&app, &state, &default);
        }
    });
}

fn on_sync(app: &AppHandle, event: SyncEvent) {
    match event {
        SyncEvent::Refreshed(report) => {
            if !report.changed_systems.is_empty() {
                let _ = app.emit(
                    EVENT_LIBRARY_CHANGED,
                    LibraryChanged {
                        systems: report.changed_systems,
                    },
                );
            }
        }
        SyncEvent::ViewsChanged => {
            let state = app.state::<AppState>();
            *state.views.write().unwrap_or_else(PoisonError::into_inner) = load_views(&state.paths);
            let _ = app.emit(EVENT_VIEWS_CHANGED, ());
        }
        SyncEvent::Failed(e) => eprintln!("forma: library refresh failed: {e}"),
        SyncEvent::WatchUnavailable(w) => {
            eprintln!("forma: {}: {}", w.file.display(), w.message);
        }
    }
}

/// Bundled themes: `<resources>/themes/*` in a packaged build; in development, every
/// `packages/theme-*/dist` built from this repository.
fn bundled_theme_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        dirs.extend(subdirs(&resources.join("themes")));
    }
    #[cfg(debug_assertions)]
    {
        let packages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages");
        dirs.extend(
            subdirs(&packages)
                .into_iter()
                .filter(|d| {
                    d.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("theme-"))
                })
                .map(|d| d.join("dist"))
                .filter(|d| d.is_dir()),
        );
    }
    dirs
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Development only: reload the window when the current theme's files change, so
/// `vite build --watch` gives a live edit loop.
#[cfg(debug_assertions)]
fn reload_on_theme_change(app: AppHandle) {
    use notify::Watcher;
    let Some(theme) = app.state::<AppState>().current_theme() else {
        return;
    };
    let handle = app.clone();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let relevant = event.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_)));
        if relevant && let Some(window) = handle.get_webview_window("main") {
            let _ = window.eval("location.reload()");
        }
    });
    match watcher {
        Ok(mut watcher) => {
            if watcher
                .watch(&theme.dir, notify::RecursiveMode::Recursive)
                .is_ok()
            {
                // Kept for the life of the app.
                std::mem::forget(watcher);
            }
        }
        Err(e) => eprintln!("forma: theme live reload unavailable: {e}"),
    }
}
