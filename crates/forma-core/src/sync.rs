//! Keeping the library current in the background.
//!
//! The UI reads the database straight away at boot; this thread reconciles it with the disk
//! afterwards and then follows changes. A file watcher says *where* something changed, so only
//! that system is rescanned; bursts (a scraper writing thousands of images) are merged into one
//! refresh. Watching is not available everywhere — a network share rarely reports changes made
//! by another machine — so a periodic full rescan backs it up.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::Warning;
use crate::library::{Library, RefreshReport};
use crate::paths::Paths;
use crate::systems::{System, load_systems};

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Quiet time after the last change before refreshing.
    pub debounce: Duration,
    /// Refresh anyway after this long, even if changes keep arriving.
    pub max_delay: Duration,
    /// Full rescan interval: the safety net for changes no watcher reported.
    pub rescan_every: Option<Duration>,
    /// Use the OS file watcher. Off means polling only, via `rescan_every`.
    pub watch: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            rescan_every: Some(Duration::from_secs(300)),
            watch: true,
        }
    }
}

#[derive(Debug)]
pub enum SyncEvent {
    /// After every refresh, including the first. `changed_systems` is empty when nothing a UI
    /// shows has changed.
    Refreshed(RefreshReport),
    /// A refresh failed; the next change or rescan retries it.
    Failed(String),
    /// A path could not be watched; changes there are found by the periodic rescan.
    WatchUnavailable(Warning),
    /// Something in the user's views folder changed: reload the view catalog.
    ViewsChanged,
}

/// Runs until dropped. Dropping stops the thread and waits for it.
pub struct SyncHandle {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Msg {
    Fs(notify::Result<notify::Event>),
    Stop,
}

/// Starts the background sync on its own thread, with its own database connection: WAL lets
/// the UI keep reading while it writes.
pub fn spawn(
    paths: Paths,
    database: PathBuf,
    options: SyncOptions,
    on_event: impl FnMut(SyncEvent) + Send + 'static,
) -> SyncHandle {
    let (tx, rx) = mpsc::channel();
    let watcher_tx = tx.clone();
    let thread = std::thread::Builder::new()
        .name("forma-sync".into())
        .spawn(move || run(&paths, &database, &options, &rx, &watcher_tx, on_event))
        .ok();
    SyncHandle { tx, thread }
}

/// What a batch of changes needs: a full refresh, some systems rescanned, the views reloaded.
#[derive(Debug, Default, PartialEq)]
struct Scope {
    full: bool,
    systems: BTreeSet<String>,
    views: bool,
}

impl Scope {
    fn full() -> Self {
        Self {
            full: true,
            ..Self::default()
        }
    }

    fn merge(&mut self, other: Scope) {
        self.full |= other.full;
        self.views |= other.views;
        self.systems.extend(other.systems);
    }
}

fn run(
    paths: &Paths,
    database: &Path,
    options: &SyncOptions,
    rx: &Receiver<Msg>,
    watcher_tx: &Sender<Msg>,
    mut emit: impl FnMut(SyncEvent),
) {
    let mut lib = match Library::open(database) {
        Ok(lib) => lib,
        Err(e) => return emit(SyncEvent::Failed(e.to_string())),
    };
    // Created up front so they can be watched, and so users find where views and imports go.
    let _ = std::fs::create_dir_all(paths.views_dir());
    let _ = std::fs::create_dir_all(paths.import_dir());
    refresh_all(&mut lib, paths, &mut emit);
    let mut targets = Targets::of(paths);
    // Held only to keep watching; replacing it unwatches everything the old one held.
    let mut _watcher = rewatch(&targets, options, watcher_tx, &mut emit);

    let mut next_rescan = options.rescan_every.map(|every| Instant::now() + every);
    loop {
        let first = match next_rescan {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut scope = match first {
            Ok(Msg::Fs(event)) => targets.classify(&event),
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => Scope::full(),
        };

        // Merge everything that arrives until it goes quiet, or max_delay passes.
        let started = Instant::now();
        while let Some(left) = options.max_delay.checked_sub(started.elapsed()) {
            match rx.recv_timeout(options.debounce.min(left)) {
                Ok(Msg::Fs(event)) => scope.merge(targets.classify(&event)),
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }

        if scope.views {
            emit(SyncEvent::ViewsChanged);
        }
        if scope.full {
            refresh_all(&mut lib, paths, &mut emit);
            // System definitions may have changed, and with them the folders to watch.
            targets = Targets::of(paths);
            _watcher = None;
            _watcher = rewatch(&targets, options, watcher_tx, &mut emit);
            next_rescan = options.rescan_every.map(|every| Instant::now() + every);
        } else if !scope.systems.is_empty() {
            let names: Vec<String> = scope.systems.into_iter().collect();
            emit(match lib.refresh_systems(paths, &names) {
                Ok(report) => SyncEvent::Refreshed(report),
                Err(e) => SyncEvent::Failed(e.to_string()),
            });
        }
    }
}

fn refresh_all(lib: &mut Library, paths: &Paths, emit: &mut impl FnMut(SyncEvent)) {
    emit(match lib.refresh(paths) {
        Ok(report) => SyncEvent::Refreshed(report),
        Err(e) => SyncEvent::Failed(e.to_string()),
    });
}

fn rewatch(
    targets: &Targets,
    options: &SyncOptions,
    tx: &Sender<Msg>,
    emit: &mut impl FnMut(SyncEvent),
) -> Option<RecommendedWatcher> {
    if options.watch {
        watch(targets, tx.clone(), emit)
    } else {
        None
    }
}

/// What is watched, and how a changed path maps back to a refresh scope.
#[derive(Default)]
struct Targets {
    systems_cfg: PathBuf,
    overlay_dir: PathBuf,
    views_dir: PathBuf,
    import_dir: PathBuf,
    roms: PathBuf,
    /// System name and ROM folder; a symlinked folder is listed under both of its paths,
    /// since watchers report the resolved one.
    systems: Vec<(String, PathBuf)>,
    /// Folders to watch recursively.
    folders: Vec<PathBuf>,
}

impl Targets {
    fn of(paths: &Paths) -> Self {
        let systems: Vec<System> = load_systems(paths).map(|l| l.systems).unwrap_or_default();
        let mut targets = Self {
            systems_cfg: paths.systems_cfg.clone(),
            overlay_dir: paths.systems_overlay_dir(),
            views_dir: paths.views_dir(),
            import_dir: paths.import_dir(),
            roms: paths.roms(),
            ..Self::default()
        };
        for system in systems {
            if !system.rom_path.is_dir() {
                continue;
            }
            if let Ok(real) = system.rom_path.canonicalize()
                && real != system.rom_path
            {
                targets.systems.push((system.name.clone(), real));
            }
            targets.folders.push(system.rom_path.clone());
            targets.systems.push((system.name, system.rom_path));
        }
        targets
    }

    fn classify(&self, event: &notify::Result<notify::Event>) -> Scope {
        let event = match event {
            Ok(event) => event,
            // The watcher lost track (e.g. its queue overflowed): only a full rescan is safe.
            Err(_) => return Scope::full(),
        };
        if event.need_rescan() {
            return Scope::full();
        }
        let mut scope = Scope::default();
        if matches!(event.kind, EventKind::Access(_)) {
            return scope;
        }
        for path in &event.paths {
            // An import file can touch any system; imports are rare, so refresh them all.
            if path == &self.systems_cfg
                || path.starts_with(&self.overlay_dir)
                || path.starts_with(&self.import_dir)
            {
                scope.full = true;
            } else if path.starts_with(&self.views_dir) {
                scope.views = true;
            } else if let Some(name) = self.system_of(path) {
                scope.systems.insert(name.to_owned());
            } else if path.parent() == Some(self.roms.as_path()) {
                // A folder appeared in `roms/` that no watched system owns yet.
                scope.full = true;
            }
        }
        scope
    }

    /// The system owning `path`, preferring the deepest folder if they nest.
    fn system_of(&self, path: &Path) -> Option<&str> {
        self.systems
            .iter()
            .filter(|(_, dir)| path.starts_with(dir))
            .max_by_key(|(_, dir)| dir.components().count())
            .map(|(name, _)| name.as_str())
    }
}

fn watch(
    targets: &Targets,
    tx: Sender<Msg>,
    emit: &mut dyn FnMut(SyncEvent),
) -> Option<RecommendedWatcher> {
    let mut watcher = match notify::recommended_watcher(move |event| {
        let _ = tx.send(Msg::Fs(event));
    }) {
        Ok(watcher) => watcher,
        Err(e) => {
            emit(SyncEvent::WatchUnavailable(Warning::new(
                &targets.roms,
                format!("no file watcher: {e}"),
            )));
            return None;
        }
    };

    // Files are watched through their folder: editors and scrapers replace them by renaming.
    let cfg_dir = targets.systems_cfg.parent().map(Path::to_path_buf);
    let shallow = [
        cfg_dir,
        Some(targets.overlay_dir.clone()),
        Some(targets.views_dir.clone()),
        Some(targets.import_dir.clone()),
        Some(targets.roms.clone()),
    ];
    let shallow = shallow
        .into_iter()
        .flatten()
        .map(|p| (p, RecursiveMode::NonRecursive));
    let deep = targets
        .folders
        .iter()
        .map(|p| (p.clone(), RecursiveMode::Recursive));
    for (path, mode) in shallow.chain(deep) {
        if !path.is_dir() {
            continue;
        }
        if let Err(e) = watcher.watch(&path, mode) {
            emit(SyncEvent::WatchUnavailable(Warning::new(
                &path,
                format!("cannot watch: {e}; relying on the periodic rescan"),
            )));
        }
    }
    Some(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_copy;
    use notify::event::{AccessKind, CreateKind};

    const WAIT: Duration = Duration::from_secs(15);

    struct Running {
        events: Receiver<SyncEvent>,
        handle: SyncHandle,
    }

    impl Running {
        fn start(paths: &Paths, options: SyncOptions) -> Self {
            let (tx, events) = mpsc::channel();
            let handle = spawn(paths.clone(), paths.database(), options, move |event| {
                let _ = tx.send(event);
            });
            Self { events, handle }
        }

        /// Delivers a file event the way the OS watcher would, so the debounce and scoping
        /// logic is tested without depending on FSEvents/inotify being available.
        fn touch(&self, path: PathBuf) {
            let event = notify::Event::new(EventKind::Create(CreateKind::File)).add_path(path);
            self.handle.tx.send(Msg::Fs(Ok(event))).unwrap();
        }

        /// The next refresh, skipping watch warnings.
        fn next_refresh(&self) -> RefreshReport {
            let deadline = Instant::now() + WAIT;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.events.recv_timeout(left) {
                    Ok(SyncEvent::Refreshed(report)) => return report,
                    Ok(SyncEvent::Failed(e)) => panic!("refresh failed: {e}"),
                    Ok(SyncEvent::WatchUnavailable(_) | SyncEvent::ViewsChanged) => {}
                    Err(e) => panic!("no refresh within {WAIT:?}: {e}"),
                }
            }
        }

        /// Refreshes until one reports a change, returning it.
        fn next_change(&self) -> RefreshReport {
            loop {
                let report = self.next_refresh();
                if !report.changed_systems.is_empty() {
                    return report;
                }
            }
        }

        fn next_views_changed(&self) {
            loop {
                match self.events.recv_timeout(WAIT) {
                    Ok(SyncEvent::ViewsChanged) => return,
                    Ok(SyncEvent::Refreshed(r)) => panic!("refreshed instead: {r:?}"),
                    Ok(_) => {}
                    Err(e) => panic!("no views change within {WAIT:?}: {e}"),
                }
            }
        }

        fn quiet_for(&self, d: Duration) -> bool {
            !matches!(self.events.recv_timeout(d), Ok(SyncEvent::Refreshed(_)))
        }
    }

    /// Injected events only: no OS watcher, no periodic rescan.
    fn injected() -> SyncOptions {
        SyncOptions {
            debounce: Duration::from_millis(150),
            max_delay: Duration::from_secs(2),
            rescan_every: None,
            watch: false,
        }
    }

    #[test]
    fn refreshes_everything_at_start() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        let report = sync.next_refresh();
        assert_eq!(report.systems, 5);
        assert_eq!(report.changed_systems.len(), 5);
    }

    #[test]
    fn a_change_refreshes_only_its_system() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        sync.next_refresh();

        let rom = paths.roms().join("megadrive/Ecco the Dolphin.md");
        std::fs::write(&rom, b"").unwrap();
        sync.touch(rom);
        let report = sync.next_change();
        assert_eq!(report.changed_systems, ["megadrive"]);
        assert_eq!(report.systems, 1);
    }

    #[test]
    fn a_burst_across_systems_is_one_refresh() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        sync.next_refresh();

        for i in 0..30 {
            let rom = paths.roms().join(format!("nes/Burst {i}.nes"));
            std::fs::write(&rom, b"").unwrap();
            sync.touch(rom);
        }
        let md = paths.roms().join("megadrive/Burst.md");
        std::fs::write(&md, b"").unwrap();
        sync.touch(md);

        let report = sync.next_refresh();
        assert_eq!(report.added, 31);
        assert_eq!(report.systems, 2);
        assert!(
            sync.quiet_for(Duration::from_millis(500)),
            "a second refresh followed"
        );
    }

    #[test]
    fn a_new_overlay_triggers_a_full_refresh() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        sync.next_refresh();

        std::fs::create_dir_all(paths.roms().join("gb")).unwrap();
        std::fs::write(paths.roms().join("gb/Tetris.gb"), b"").unwrap();
        let overlay = paths.systems_overlay_dir().join("es_systems_gb.cfg");
        std::fs::write(
            &overlay,
            "<systemList><system><name>gb</name><path>/userdata/roms/gb</path>\
             <extension>.gb</extension></system></systemList>",
        )
        .unwrap();
        sync.touch(overlay);
        let report = sync.next_change();
        assert!(
            report.changed_systems.contains(&"gb".to_owned()),
            "{report:?}"
        );
        assert_eq!(report.systems, 6);
    }

    #[test]
    fn reads_and_unrelated_paths_do_not_refresh() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        sync.next_refresh();
        let read = notify::Event::new(EventKind::Access(AccessKind::Any))
            .add_path(paths.roms().join("nes/Homebrew Demo.nes"));
        sync.handle.tx.send(Msg::Fs(Ok(read))).unwrap();
        sync.touch(paths.database());
        assert!(sync.quiet_for(Duration::from_millis(500)));
    }

    #[test]
    fn a_view_file_change_reloads_views_without_a_rescan() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(&paths, injected());
        sync.next_refresh();
        assert!(paths.views_dir().is_dir(), "created at start");
        sync.touch(paths.views_dir().join("mine.json"));
        sync.next_views_changed();
        assert!(sync.quiet_for(Duration::from_millis(400)));
    }

    #[test]
    fn polling_finds_changes_without_a_watcher() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(
            &paths,
            SyncOptions {
                rescan_every: Some(Duration::from_millis(200)),
                ..injected()
            },
        );
        sync.next_refresh();
        std::fs::write(paths.roms().join("snes/Polled.sfc"), b"").unwrap();
        assert_eq!(sync.next_change().changed_systems, ["snes"]);
    }

    /// End to end through the OS watcher. Ignored by default because some sandboxes (including
    /// the one this was developed in) block FSEvents/inotify; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs OS file events, which some sandboxes block"]
    fn the_os_watcher_reports_a_new_rom() {
        let (_dir, paths) = fixture_copy();
        let sync = Running::start(
            &paths,
            SyncOptions {
                watch: true,
                ..injected()
            },
        );
        sync.next_refresh();
        std::fs::write(paths.roms().join("megadrive/Ecco the Dolphin.md"), b"").unwrap();
        assert_eq!(sync.next_change().changed_systems, ["megadrive"]);
    }

    #[test]
    fn classifies_paths_into_scopes() {
        let (_dir, paths) = fixture_copy();
        let targets = Targets::of(&paths);
        let event = |path: PathBuf| {
            Ok(notify::Event::new(EventKind::Create(CreateKind::File)).add_path(path))
        };
        let systems = |names: &[&str]| Scope {
            systems: names.iter().map(|n| (*n).to_owned()).collect(),
            ..Scope::default()
        };
        assert_eq!(
            targets.classify(&event(paths.roms().join("nes/images/x.png"))),
            systems(&["nes"])
        );
        assert_eq!(
            targets.classify(&event(paths.systems_overlay_dir().join("es_systems_x.cfg"))),
            Scope::full()
        );
        assert_eq!(
            targets.classify(&event(paths.roms().join("newsystem"))),
            Scope::full()
        );
        assert!(
            targets
                .classify(&event(paths.views_dir().join("v.json")))
                .views
        );
        assert_eq!(
            targets.classify(&event(paths.import_dir().join("retroworker.jsonl"))),
            Scope::full()
        );
        assert_eq!(
            targets.classify(&event(paths.userdata.join("system/forma/library.db"))),
            Scope::default()
        );
        assert_eq!(
            targets.classify(&Err(notify::Error::generic("overflow"))),
            Scope::full()
        );
        let mut merged = systems(&["nes"]);
        merged.merge(systems(&["snes"]));
        assert_eq!(merged, systems(&["nes", "snes"]));
    }
}
