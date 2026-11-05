//! Writes the data `@ramraiders/forma-sdk`'s mock backend serves: the real engine's answers for the
//! fixture library, so a theme developed in a browser sees exactly what Forma would send.
//!
//! cargo run -p forma-core --example mock_snapshot > packages/sdk/src/mock/library.json
//!
//! The test below fails when the committed file is stale, so the mock cannot drift from the
//! engine it stands in for.

use std::path::Path;

use forma_core::batocera::BatoceraSettings;
use forma_core::hierarchy::{Browse, Listing, Origin, Page, View, load_views};
use forma_core::music;
use forma_core::net::{Cache, Http, HttpError, achievements, netplay};
use forma_core::network::{Network, Shell};
use forma_core::options::{OptionsPage, Scope};
use forma_core::systems::load_systems;
use forma_core::{Library, Paths, SDK_API_VERSION};
use serde_json::{Map, Value, json};

fn main() {
    print!("{}", snapshot());
}

fn fixtures() -> Paths {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    Paths {
        userdata: root.join("userdata"),
        systems_cfg: root.join("usr-share-emulationstation/es_systems.cfg"),
    }
}

fn snapshot() -> String {
    let paths = fixtures();
    let mut lib = Library::open_in_memory().expect("database");
    lib.refresh(&paths).expect("refresh");
    let catalog = load_views(&paths);

    let mut browse = Map::new();
    for entry in &catalog.views {
        walk(&lib, &entry.view, &mut Vec::new(), &mut browse);
    }
    let views: Vec<Value> = catalog
        .views
        .iter()
        .map(|e| {
            json!({
                "id": e.view.id,
                "name": e.view.name,
                "description": e.view.description,
                "origin": match e.origin { Origin::Bundled => "bundled", Origin::User(_) => "user" },
            })
        })
        .collect();
    let mut games: Vec<Value> = Vec::new();
    for system in lib.systems().expect("systems") {
        for game in lib.games(&system.name).expect("games") {
            games.push(scrub(serde_json::to_value(game).expect("json")));
        }
    }

    // Achievements and netplay: the same fixture answers the core tests parse, through the
    // same parsers. Media paths keep a `ra-media/` prefix the mock turns into placeholders.
    lib.set_setting(achievements::API_KEY_SETTING, Some("mock"))
        .expect("setting");
    let cache_dir = tempfile::tempdir().expect("temp dir");
    let cache = Cache::new(cache_dir.path().to_path_buf());
    let account = achievements::account(&lib, &paths).expect("account");
    let ra_user = achievements::user(&FixtureHttp, &cache, &account, "ra-media/").expect("ra user");
    let mut ra_games = Map::new();
    for game in lib.search("super mario", 50).expect("search") {
        if let Some(ra_id) = game.ra_game_id {
            let progress = achievements::game(&FixtureHttp, &cache, &account, ra_id, "ra-media/")
                .expect("ra game");
            ra_games.insert(
                game.id.to_string(),
                serde_json::to_value(progress.data).expect("json"),
            );
        }
    }
    let mut rooms = netplay::lobby(&FixtureHttp, &cache).expect("lobby").data;
    netplay::match_local(&mut rooms, &lib).expect("match");
    // As on a box with FCEUmm installed and Snes9x not.
    netplay::match_cores(
        &mut rooms,
        &netplay::Cores::from_pairs([("FCEUmm", "fceumm")]),
    );

    // The box's network: recorded `connmanctl` answers, through the same parsers.
    let network = Network::new(Box::new(FixtureShell), &paths);
    let network = json!({
        "status": network.status().expect("network status"),
        "wifi": network.wifi_scan().expect("wifi scan"),
    });

    // Batocera's settings from the fixture batocera.conf; secrets only say whether they are set.
    let batocera = BatoceraSettings::new(Box::new(FixtureShell), &paths.batocera_conf())
        .with_sounds_dir(Path::new("/nonexistent"))
        .get("");

    // The fixture's music folder: what a theme's player can offer.
    let music = music::Library {
        music: paths.music_dir(),
        fallback: Path::new("/nonexistent").to_path_buf(),
        addon_radios: Default::default(),
    };
    let music = json!({ "sources": music.sources(), "radios": music.radios() });

    let options = options(&paths);

    // System settings: the fixture batocera.conf, with Batocera's tools answering from fixtures.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let system = forma_core::system_settings::SystemSettings::new(
        Box::new(FixtureShell),
        &paths.batocera_conf(),
    )
    .with_files(
        &root.join("usr-share-zoneinfo"),
        &root.join("usr-share-batocera/batocera.version"),
    );
    let system = json!({ "info": system.info(), "settings": system.get() });

    // The content downloader: Batocera's store answering from a stand-in with real packages.
    let store = forma_core::store::Store::new(&root.join("batocera/fake-batocera-store"))
        .list()
        .expect("store");

    let doc = json!({
        "comment": "Generated by `cargo run -p forma-core --example mock_snapshot`. Do not edit.",
        "apiVersion": SDK_API_VERSION,
        "systems": lib.systems().expect("systems"),
        "views": views,
        "games": games,
        "browse": browse,
        "ra": { "user": ra_user.data, "games": ra_games },
        "lobby": rooms,
        "network": network,
        "batocera": batocera,
        "music": music,
        "options": options,
        "system": system,
        "store": store,
    });
    serde_json::to_string_pretty(&doc).expect("json") + "\n"
}

/// Emulator options: the engine's page for global, and for every fixture system with each
/// emulator and core it can be given (`system|emulator|core`, empty for "not set"). Values are
/// left out: the mock keeps its own `batocera.conf` and fills them in.
fn options(paths: &Paths) -> Value {
    let dir = tempfile::tempdir().expect("temp dir");
    let conf = dir.path().join("batocera.conf");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/usr-share-batocera");
    let engine = forma_core::options::Options::new(
        Box::new(FixtureShell),
        &conf,
        &paths.systems_cfg.with_file_name("es_features.cfg"),
        Path::new("/nonexistent"),
    )
    .with_preset_dirs(
        vec![root.join("shaders/configs")],
        vec![root.join("decorations")],
    );
    let bare = |mut page: OptionsPage| {
        for o in &mut page.options {
            (o.value, o.inherited) = (None, None);
        }
        serde_json::to_value(page).expect("json")
    };
    let mut pages = Map::new();
    std::fs::write(&conf, "").expect("conf");
    pages.insert(
        "global".into(),
        bare(engine.page(&Scope::Global, None).expect("global options")),
    );
    for system in load_systems(paths).expect("systems").systems {
        let mut choices = vec![(None, None)];
        for em in &system.emulators {
            choices.push((Some(em.name.as_str()), None));
            for core in &em.cores {
                choices.push((Some(em.name.as_str()), Some(core.name.as_str())));
            }
        }
        for (em, core) in choices {
            let mut text = String::new();
            for (key, value) in [("emulator", em), ("core", core)] {
                if let Some(v) = value {
                    text.push_str(&format!("{}.{key}={v}\n", system.name));
                }
            }
            std::fs::write(&conf, text).expect("conf");
            let page = engine
                .page(&Scope::System(system.name.clone()), Some(&system))
                .expect("system options");
            pages.insert(
                format!(
                    "{}|{}|{}",
                    system.name,
                    em.unwrap_or(""),
                    core.unwrap_or("")
                ),
                bare(page),
            );
        }
    }
    Value::Object(pages)
}

/// Answers from `fixtures/http`.
struct FixtureHttp;

impl Http for FixtureHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let file = [
            ("API_GetUserSummary", "ra-user.json"),
            ("API_GetGameInfoAndUserProgress", "ra-game.json"),
            ("lobby.libretro.com", "lobby.json"),
        ]
        .into_iter()
        .find(|(needle, _)| url.contains(needle))
        .map(|(_, file)| file)
        .ok_or_else(|| HttpError {
            status: Some(404),
            message: url.to_owned(),
        })?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/http")
            .join(file);
        std::fs::read(path).map_err(|e| HttpError {
            status: None,
            message: e.to_string(),
        })
    }
}

/// `connmanctl` answers from `fixtures/connman`. Nothing that changes the box is run here.
struct FixtureShell;

impl Shell for FixtureShell {
    fn run(&self, program: &str, args: &[&str]) -> forma_core::Result<String> {
        let file = match (program, args) {
            ("connmanctl", ["technologies" | "services"]) => format!("{}.txt", args[0]),
            ("connmanctl", ["services", id]) => format!("{id}.txt"),
            ("connmanctl", ["scan", "wifi"]) => return Ok(String::new()),
            // Batocera's own tools, from `fixtures/batocera`: `batocera-audio list` is
            // `batocera-audio_list.txt`.
            (tool, _) if tool.starts_with("batocera-") && !tool.ends_with("-set") => {
                let name = std::iter::once(program)
                    .chain(args.iter().copied())
                    .collect::<Vec<_>>()
                    .join("_");
                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/batocera")
                    .join(format!("{name}.txt"));
                return Ok(std::fs::read_to_string(path).unwrap_or_default());
            }
            _ => panic!("the snapshot only reads: {program} {args:?}"),
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/connman")
            .join(file);
        Ok(std::fs::read_to_string(path).unwrap_or_default())
    }
}

/// Every page of a view, keyed by `JSON.stringify([view, ...path])`.
fn walk(lib: &Library, view: &View, path: &mut Vec<String>, out: &mut Map<String, Value>) {
    let page = Page {
        offset: 0,
        limit: u32::MAX,
    };
    let result: Browse = lib.browse(view, path, page).expect("browse");
    let children: Vec<String> = match &result.listing {
        Listing::Groups { items, .. } => items.iter().map(|g| g.key.clone()).collect(),
        Listing::Branches { items } => items.iter().map(|b| b.id.clone()).collect(),
        Listing::Games { .. } => Vec::new(),
    };
    let key = serde_json::to_string(
        &std::iter::once(&view.id)
            .chain(path.iter())
            .collect::<Vec<_>>(),
    )
    .expect("key");
    out.insert(key, scrub(serde_json::to_value(&result).expect("json")));
    for child in children {
        path.push(child);
        walk(lib, view, path, out);
        path.pop();
    }
}

/// Media becomes kinds only (the mock draws placeholders), and paths on this machine go.
fn scrub(mut value: Value) -> Value {
    match &mut value {
        Value::Object(map) => {
            if let Some(Value::Object(media)) = map.get_mut("media") {
                for (kind, v) in media.iter_mut() {
                    *v = Value::String(kind.clone());
                }
            }
            for v in map.values_mut() {
                *v = scrub(std::mem::take(v));
            }
        }
        Value::Array(items) => {
            for v in items.iter_mut() {
                *v = scrub(std::mem::take(v));
            }
        }
        _ => {}
    }
    value
}

#[test]
fn the_committed_snapshot_is_current() {
    let file: std::path::PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/sdk/src/mock/library.json");
    let committed = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        committed == snapshot(),
        "{} is stale: run `cargo run -p forma-core --example mock_snapshot > {}`",
        file.display(),
        "packages/sdk/src/mock/library.json"
    );
}
