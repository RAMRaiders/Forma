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
use forma_core::music::{self, Mpv, Player};
use forma_core::network::{Network, SystemShell};
use forma_core::options::Options;
use forma_core::sync::{self, SyncEvent, SyncOptions};
use forma_core::system_settings::SystemSettings;
use forma_core::themes::{Theme, discover};
use forma_core::thumbs::{self, Thumbnailer, Thumbs};
use forma_core::{Library, Paths};
use tauri::{App, AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::commands::{EVENT_LIBRARY_CHANGED, EVENT_VIEWS_CHANGED, LibraryChanged, entry_url};
use crate::state::{AppState, DEFAULT_THEME, FALLBACK_THEME, THEME_SETTING, pick_theme};

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

/// How long a theme page has, from when it starts loading, to call `theme_ready` before Forma
/// falls back to the default theme. Generous: a Raspberry Pi still busy booting is slow.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

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

    let music = music_player(&paths, &library);
    let shell_dir = shell_dir(app.handle());
    if shell_dir.is_none() {
        eprintln!("forma: the shell (shell.js) is missing: no Start menu over themes");
    }
    app.manage(AppState {
        views: RwLock::new(load_views(&paths)),
        library: Mutex::new(library),
        themes: RwLock::new(themes),
        bundled_themes,
        current_theme: RwLock::new(current.clone()),
        launcher: launcher_from_env(),
        http: Box::new(forma_core::net::UreqHttp::default()),
        network: Network::new(Box::new(SystemShell), &paths),
        bluetooth: forma_core::bluetooth::Bluetooth::new(Box::new(SystemShell), &paths),
        batocera: BatoceraSettings::new(Box::new(SystemShell), &paths.batocera_conf()),
        store: forma_core::store::Store::default(),
        scraper: crate::scraper::Scraping::new(
            std::sync::Arc::new(forma_core::net::UreqHttp::default()),
            std::sync::Arc::new(forma_core::host::SystemShell),
        ),
        system_settings: SystemSettings::new(Box::new(SystemShell), &paths.batocera_conf()),
        options: Options::new(
            Box::new(SystemShell),
            &paths.batocera_conf(),
            &paths.systems_cfg.with_file_name("es_features.cfg"),
            &paths.userdata,
        ),
        shaders: forma_core::shaders::Shaders::new(
            Box::new(SystemShell),
            &paths.batocera_conf(),
            &paths.userdata,
        ),
        music,
        magazines: forma_core::magazines::Reader::new(
            &paths,
            Box::new(forma_core::magazines::HostTools),
        ),
        thumbnails: Thumbnailer::spawn(Thumbs::new(
            paths.cache_dir().join("thumbs"),
            thumbs::DEFAULT_MAX_BYTES,
        )),
        video: forma_core::video::VideoPlayer::new(Box::new(forma_core::music::Mpv::video(
            paths.forma_dir().join("run").join("video.sock"),
        ))),
        playing: AtomicBool::new(false),
        list_filter: std::sync::Mutex::default(),
        theme_ready: AtomicBool::new(false),
        ready_file: std::env::var_os("FORMA_READY_FILE").map(PathBuf::from),
        on_batocera: Path::new(BATOCERA_VERSION_FILE).is_file(),
        see_through: Path::new(BATOCERA_VERSION_FILE).is_file() && crate::splash::drawn(),
        input: Mutex::new(None),
        shell: crate::shell::ShellState::default(),
        shell_dir,
        sync: Mutex::new(None),
        bios: std::sync::OnceLock::new(),
        paths: paths.clone(),
    });
    let state = app.state::<AppState>();
    // Batocera's BIOS list read now, off the UI: the first launch need not wait for it.
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        handle.state::<AppState>().bios_table();
    });

    // The sync's first full rescan reads every gamelist off the card: at boot, on a Pi, that
    // held the theme's start back past the 30 s the service allows. The library from the last
    // run is in the database, so it waits for the UI to be up; with nothing in it yet (a
    // first start) it runs at once, as there is nothing else to show.
    let empty = state.library().systems().map_or(true, |s| s.is_empty());
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        let since = std::time::Instant::now();
        while !empty
            && !state.shell.ui_started.load(Ordering::SeqCst)
            && since.elapsed() < SYNC_WAIT
        {
            std::thread::sleep(Duration::from_millis(200));
        }
        let events = handle.clone();
        let sync = sync::spawn(
            state.paths.clone(),
            state.paths.database(),
            SyncOptions::default(),
            move |event| on_sync(&events, event),
        );
        *state.sync.lock().unwrap_or_else(PoisonError::into_inner) = Some(sync);
        // Batocera's shader keys from before Forma owned them, as Forma's choices from now on.
        if let Err(error) = state.shaders.adopt() {
            eprintln!("forma: shader keys: {error}");
        }
    });

    crate::music::start(app.handle().clone());
    crate::status::start(app.handle().clone());
    crate::ra_match::start(app.handle().clone());
    crate::scraper::upgrade_launchbox(app.handle().clone());

    let handle = app.handle().clone();
    // Pads SDL's database does not list, as ES knows them: Batocera's file, then the user's.
    let es_input = input::es_input::mappings_from(
        &state.paths.systems_cfg.with_file_name("es_input.cfg"),
        &state.paths.systems_overlay_dir().join("es_input.cfg"),
    );
    match input::spawn(InputOptions::default(), es_input, move |event| {
        crate::shell::deliver(&handle, &event);
    }) {
        Ok(input) => *state.input.lock().unwrap_or_else(PoisonError::into_inner) = Some(input),
        Err(e) => eprintln!("forma: no gamepad input: {e}"),
    }

    let url = match state.available_theme(&current) {
        // Straight to the theme: Forma's cover in its page is the entry screen.
        Some(theme) => WebviewUrl::CustomProtocol(perf_url(entry_url(&theme)).parse()?),
        // No theme can load: the bundled boot page explains instead of showing nothing.
        None => WebviewUrl::App("index.html".into()),
    };
    // On Batocera the boot picture is on the screen under the window (forma-standalone puts it
    // there): the window starts see-through, so it stays in view until the page draws its
    // cover, the same picture, instead of a blank window. Elsewhere, dark from the first frame:
    // no white flash before the cover is drawn.
    let see_through = state.see_through;
    let window = WebviewWindowBuilder::new(app, "main", url)
        .title("Forma")
        .inner_size(1280.0, 720.0)
        .fullscreen(fullscreen())
        // On Batocera, games' videos play under the window too (see `video_play`).
        .transparent(see_through || state.on_batocera)
        .background_color(tauri::window::Color(
            4,
            6,
            12,
            if see_through { 0 } else { 255 },
        ))
        .build()?;
    reload_if_the_page_dies(&window);

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

/// Batocera's splash files: its boot image (`boot-logo.png`), whose joystick emblem marks
/// Batocera's version on Forma's cover.
pub const BATOCERA_SPLASH: &str = "/usr/share/batocera/splash";
/// One of Batocera's own splash files.
pub fn batocera_splash_file(name: &str) -> PathBuf {
    Path::new(BATOCERA_SPLASH).join(name)
}

/// Batocera's logo as EmulationStation shows it, for the corner of Forma's cover.
pub const BATOCERA_LOGO: &str = "/usr/share/emulationstation/resources/splash_batocera.svg";

/// What Forma's cover shows over a theme page while it loads: Forma's logo and loader, with
/// Batocera's, Forma's and the theme's versions in a corner. Batocera shows the same picture at
/// the next boot ([`crate::splash`]).
pub fn cover_info(state: &AppState) -> crate::shell::CoverInfo {
    // As Batocera's own splash subtitles it: "43.1 2026/05/29".
    let batocera = std::fs::read_to_string(BATOCERA_VERSION_FILE)
        .ok()
        .and_then(|v| {
            let mut words = v.split_whitespace();
            Some(format!(
                "{} {}",
                words.next()?,
                words.next().unwrap_or_default()
            ))
        })
        .unwrap_or_default();
    let current = state.current_theme();
    let theme = current.as_ref().map_or_else(String::new, |t| {
        t.manifest
            .as_ref()
            .map_or(t.id.clone(), |m| format!("{} {}", m.name, m.version))
    });
    let info = crate::shell::CoverInfo {
        batocera: batocera.trim().to_owned(),
        theme: theme.trim().to_owned(),
        theme_logo: current.as_ref().and_then(crate::state::theme_logo_url),
        theme_logo_file: current.as_ref().and_then(|t| {
            let logo = t.manifest.as_ref()?.logo.as_ref()?;
            Some(t.dir.join(logo))
        }),
        theme_version: current
            .and_then(|t| t.manifest.map(|m| m.version))
            .unwrap_or_default(),
        margins: screen_margins(state),
        see_through: state.see_through && !state.shell.ui_started.load(Ordering::SeqCst),
    };
    // The next boot shows the same, as Batocera's boot picture.
    crate::splash::refresh(&info);
    info
}

/// The Forma setting every theme shares for a TV that cuts the picture's edges (overscan): how
/// much, in percent of each side, to keep clear. 0 to 10.
/// The longest the library sync waits for the UI to be up before it starts anyway.
const SYNC_WAIT: Duration = Duration::from_secs(60);

pub const MARGINS_SETTING: &str = "display.margins";

fn screen_margins(state: &AppState) -> u8 {
    state
        .library()
        .setting_text(MARGINS_SETTING)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(0, |m| m.clamp(0.0, 10.0).round() as u8)
}

/// Batocera's own music, played when the music folder has none.
const BATOCERA_MUSIC: &str = "/usr/share/batocera/music";

/// The music player, at the volume the user left it.
fn music_player(paths: &Paths, library: &Library) -> Player {
    let volume = library
        .setting_text(crate::music::VOLUME_SETTING)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::music::DEFAULT_VOLUME);
    let run = paths.forma_dir().join("run");
    Player::new(
        music::Library {
            music: paths.music_dir(),
            fallback: BATOCERA_MUSIC.into(),
            addon_radios: Default::default(),
        },
        Box::new(Mpv::new(run.join("mpv.sock"))),
        run,
        volume,
    )
}

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

/// A theme page started loading: falls back to the default theme if it never reports ready.
/// Counted from the page, not from Forma's start, and dropped once another page loads.
pub fn watch_ready<R: Runtime>(app: AppHandle<R>) {
    let page = app.state::<AppState>().shell.page();
    std::thread::spawn(move || {
        std::thread::sleep(READY_TIMEOUT);
        let state = app.state::<AppState>();
        if state.shell.page() != page || state.shell.page_ready() {
            return;
        }
        let Some(current) = state.current_theme() else {
            return;
        };
        // A newer copy of a bundled theme gives way to the bundled one; any other theme to the
        // default, and the default to the plain fallback.
        let reverted = current.overrides
            && forma_core::themes::revert_override(
                &mut state.themes.write().unwrap_or_else(PoisonError::into_inner),
                &current.id,
            );
        let next = if reverted {
            current.id.as_str()
        } else if current.id != DEFAULT_THEME {
            DEFAULT_THEME
        } else if current.id != FALLBACK_THEME {
            FALLBACK_THEME
        } else {
            return;
        };
        eprintln!(
            "forma: theme `{}` did not start within {READY_TIMEOUT:?}; using `{next}`{}",
            current.id,
            if reverted { " as built in" } else { "" }
        );
        if let Some(theme) = state.available_theme(next) {
            let _ = show_theme(&app, &state, &theme);
        }
    });
}

fn on_sync(app: &AppHandle, event: SyncEvent) {
    match event {
        SyncEvent::Refreshed(report) => {
            if !report.changed_systems.is_empty() {
                crate::ra_match::systems_changed(&report.changed_systems);
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

/// Forma's shell (`shell.js`): `<resources>/shell` in a packaged build; in development,
/// `packages/shell/dist` built from this repository.
fn shell_dir(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join("shell"));
    }
    #[cfg(debug_assertions)]
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/shell/dist"));
    candidates
        .into_iter()
        .find(|d| d.join("shell.js").is_file())
}

/// Bundled themes: `<resources>/themes/*` in a packaged build; in development, the pinned
/// themes unpacked in `build/themes` and every `packages/theme-*/dist` built here.
fn bundled_theme_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        dirs.extend(subdirs(&resources.join("themes")));
    }
    #[cfg(debug_assertions)]
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        dirs.extend(subdirs(&root.join("build/themes")));
        let packages = root.join("packages");
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
