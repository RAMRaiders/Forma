//! The commands as a theme calls them: by name, with JSON arguments, from a `forma://` page,
//! through Tauri's real IPC layer on its mock runtime.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use forma_core::batocera::BatoceraSettings;
use forma_core::hierarchy::load_views;
use forma_core::launch::MockLauncher;
use forma_core::net::{Http, HttpError};
use forma_core::network::{Network, SystemShell};
use forma_core::themes::discover;
use forma_core::{Library, Paths};
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::state::AppState;

const PAGE: &str = "forma://localhost/theme/default/index.html";

struct Harness {
    _dir: tempfile::TempDir,
    _app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
}

impl Harness {
    /// The fixture library, with a bundled `default` theme holding `permissions`.
    fn new(permissions: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        copy_dir(&fixtures.join("userdata"), &root.join("userdata"));
        let paths = Paths {
            userdata: root.join("userdata"),
            systems_cfg: fixtures.join("usr-share-emulationstation/es_systems.cfg"),
        };

        let theme_dir = root.join("bundled/default");
        std::fs::create_dir_all(&theme_dir).unwrap();
        std::fs::write(theme_dir.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(
            theme_dir.join("theme.json"),
            json!({ "id": "default", "name": "Default", "version": "1", "sdk": "^1",
                    "permissions": permissions })
            .to_string(),
        )
        .unwrap();

        let mut library = Library::open_in_memory().unwrap();
        library.refresh(&paths).unwrap();
        let bundled = vec![theme_dir];
        let state = AppState {
            views: RwLock::new(load_views(&paths)),
            library: Mutex::new(library),
            themes: RwLock::new(discover(&bundled, &paths)),
            bundled_themes: bundled,
            current_theme: RwLock::new("default".into()),
            launcher: Box::new(MockLauncher::new(Duration::from_millis(20))),
            http: Box::new(FixtureHttp),
            network: Network::new(Box::new(SystemShell), &paths),
            batocera: BatoceraSettings::new(Box::new(SystemShell), &paths.batocera_conf()),
            music: forma_core::music::Player::new(
                forma_core::music::Library {
                    music: paths.music_dir(),
                    fallback: root.join("no-music"),
                },
                Box::new(forma_core::music::Mpv::new(root.join("mpv.sock"))),
                root.join("run"),
                60,
            ),
            thumbnails: forma_core::thumbs::Thumbnailer::spawn(forma_core::thumbs::Thumbs::new(
                root.join("thumbs"),
                forma_core::thumbs::DEFAULT_MAX_BYTES,
            )),
            playing: AtomicBool::new(false),
            theme_ready: AtomicBool::new(false),
            ready_file: Some(root.join("forma.ready")),
            on_batocera: false,
            input: Mutex::new(None),
            sync: Mutex::new(None),
            paths,
        };

        let app = mock_builder()
            .register_uri_scheme_protocol("forma", |_, _| {
                tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap()
            })
            .invoke_handler(crate::handlers())
            .build(mock_context(noop_assets()))
            .unwrap();
        app.manage(state);
        let window = WebviewWindowBuilder::new(
            &app,
            "main",
            WebviewUrl::CustomProtocol(PAGE.parse().unwrap()),
        )
        .build()
        .unwrap();
        Self {
            _dir: dir,
            _app: app,
            window,
        }
    }

    fn call_from(&self, url: &str, cmd: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: url.parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
    }

    fn call(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        self.call_from(PAGE, cmd, args)
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.window.state::<AppState>()
    }
}

/// Answers from `fixtures/http`, like the services would.
struct FixtureHttp;

impl Http for FixtureHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let file = if url.contains("API_GetUserSummary") {
            "ra-user.json"
        } else if url.contains("API_GetGameInfoAndUserProgress") {
            "ra-game.json"
        } else if url.contains("lobby.libretro.com") {
            "lobby.json"
        } else {
            return Err(HttpError {
                status: Some(404),
                message: url.into(),
            });
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/http")
            .join(file);
        Ok(std::fs::read(path).unwrap())
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target: PathBuf = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn code(err: &Value) -> &str {
    err["code"].as_str().unwrap_or_default()
}

#[test]
fn answers_the_version_handshake() {
    let h = Harness::new(&[]);
    assert_eq!(h.call("sdk_api_version", json!({})).unwrap(), "1.0.0");
}

#[test]
fn browses_a_view_and_hands_out_media_as_urls_not_paths() {
    let h = Harness::new(&[]);
    let root = h
        .call("nav_browse", json!({ "view": "systems", "path": [] }))
        .unwrap();
    assert_eq!(root["listing"]["kind"], "groups");
    assert_eq!(root["listing"]["items"][0]["key"], "nes");
    assert_eq!(root["listing"]["items"][0]["gameCount"], 3);

    let nes = h
        .call(
            "nav_browse",
            json!({ "view": "systems", "path": ["nes"], "page": { "offset": 1, "limit": 1 } }),
        )
        .unwrap();
    let mario = &nes["listing"]["items"][0];
    assert_eq!(mario["name"], "Super Mario Bros.");
    let id = mario["id"].as_i64().unwrap();
    assert_eq!(
        mario["media"]["image"],
        format!("forma://localhost/media/{id}/image")
    );
    assert!(
        !nes.to_string().contains("/userdata/"),
        "no filesystem path reaches the theme"
    );

    let err = h
        .call("nav_browse", json!({ "view": "nope", "path": [] }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
}

#[test]
fn search_and_favourites() {
    let h = Harness::new(&[]);
    let hits = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = hits[0]["id"].clone();
    h.call(
        "library_set_favorite",
        json!({ "id": id, "favorite": true }),
    )
    .unwrap();
    let game = h.call("library_game", json!({ "id": id })).unwrap();
    assert_eq!(game["favorite"], true);
    let err = h
        .call(
            "library_set_favorite",
            json!({ "id": 9999, "favorite": true }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
}

#[test]
fn theme_settings_are_free_and_system_settings_need_permission() {
    let h = Harness::new(&[]);
    h.call(
        "settings_set",
        json!({ "key": "layout", "value": { "grid": 4 } }),
    )
    .unwrap();
    assert_eq!(
        h.call("settings_get", json!({ "key": "layout" })).unwrap(),
        json!({ "grid": 4 })
    );
    assert_eq!(
        h.call("settings_get", json!({ "key": "missing" })).unwrap(),
        Value::Null
    );

    let err = h
        .call(
            "settings_set",
            json!({ "key": "views.default", "value": "genre", "scope": "system" }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");

    let admin = Harness::new(&["settings.system"]);
    admin
        .call(
            "settings_set",
            json!({ "key": "anything", "value": 1, "scope": "system" }),
        )
        .unwrap();
}

#[test]
fn the_boot_view_can_be_chosen_from_the_ui() {
    let h = Harness::new(&[]);
    assert_eq!(
        h.call("nav_default_view", json!({})).unwrap()["id"],
        "systems"
    );
    let err = h
        .call("nav_set_default_view", json!({ "id": "nope" }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
    h.call(
        "nav_set_default_view",
        json!({ "id": "manufacturer-system" }),
    )
    .unwrap();
    assert_eq!(
        h.call("nav_default_view", json!({})).unwrap()["id"],
        "manufacturer-system"
    );
    let views = h.call("nav_views", json!({})).unwrap();
    assert_eq!(views.as_array().unwrap().len(), 6);
}

#[test]
fn reporting_ready_writes_the_ready_file_for_the_batocera_service() {
    let h = Harness::new(&[]);
    let file = h.state().ready_file.clone().unwrap();
    assert!(!file.exists());
    h.call("theme_ready", json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap(), "default");
}

#[test]
fn power_needs_permission_and_is_refused_outside_batocera() {
    let h = Harness::new(&[]);
    let err = h
        .call("system_power", json!({ "action": "shutdown" }))
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
    let err = h.call("system_quit", json!({})).unwrap_err();
    assert_eq!(code(&err), "permissionDenied");

    let allowed = Harness::new(&["system.power"]);
    let err = allowed
        .call("system_power", json!({ "action": "shutdown" }))
        .unwrap_err();
    assert_eq!(code(&err), "unsupported");
}

#[test]
fn network_settings_need_permission_and_are_refused_outside_batocera() {
    let h = Harness::new(&[]);
    for (cmd, args) in [
        ("network_status", json!({})),
        ("network_wifi_scan", json!({})),
        (
            "network_wifi_join",
            json!({ "ssid": "Home", "passphrase": "secret-key" }),
        ),
        ("network_wifi_set_enabled", json!({ "enabled": false })),
        (
            "network_ethernet_configure",
            json!({ "config": { "method": "dhcp" } }),
        ),
    ] {
        let err = h.call(cmd, args.clone()).unwrap_err();
        assert_eq!(code(&err), "permissionDenied", "{cmd}");
    }

    let allowed = Harness::new(&["system.network"]);
    let err = allowed.call("network_status", json!({})).unwrap_err();
    assert_eq!(code(&err), "unsupported", "{err}");
    let err = allowed
        .call(
            "network_ethernet_configure",
            json!({ "config": { "method": "manual", "address": "10.0.0.2", "netmask": "255.0.0.0", "gateway": null } }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "unsupported");
}

#[test]
fn batocera_settings_need_permission_and_are_refused_outside_batocera() {
    let h = Harness::new(&[]);
    for (cmd, args) in [
        ("batocera_settings", json!({ "prefix": "global.netplay" })),
        (
            "batocera_set",
            json!({ "key": "global.netplay", "value": "1" }),
        ),
        ("ra_login", json!({ "username": "u", "password": "p" })),
        ("ra_logout", json!({})),
    ] {
        let err = h.call(cmd, args.clone()).unwrap_err();
        assert_eq!(code(&err), "permissionDenied", "{cmd}");
    }
    let allowed = Harness::new(&["settings.batocera"]);
    let err = allowed
        .call("batocera_settings", json!({ "prefix": "" }))
        .unwrap_err();
    assert_eq!(code(&err), "unsupported", "{err}");
}

#[test]
fn music_lists_sources_reports_stopped_and_guards_the_radio_list() {
    let h = Harness::new(&[]);
    // The fixture's music folder: loose tracks, a sub-folder, and a radio.
    let sources = h.call("music_sources", json!({})).unwrap();
    let ids: Vec<&str> = sources
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "folder:",
            "folder:Chiptune",
            "radio:https://ice1.somafm.com/groovesalad-128-mp3"
        ]
    );
    let status = h.call("music_status", json!({})).unwrap();
    assert_eq!(status["state"], "stopped");
    assert_eq!(status["volume"], 60);
    let err = h
        .call(
            "music_add_radio",
            json!({ "name": "x", "url": "https://x" }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");

    let allowed = Harness::new(&["music.manage"]);
    allowed
        .call(
            "music_add_radio",
            json!({ "name": "Lofi", "url": "https://lofi.example/live" }),
        )
        .unwrap();
    let radios = allowed.call("music_radios", json!({})).unwrap();
    // Added after the fixture's own radio.
    assert_eq!(radios[1]["name"], "Lofi");
}

#[test]
fn lists_themes_and_refuses_an_unknown_one() {
    let h = Harness::new(&[]);
    let themes = h.call("themes_list", json!({})).unwrap();
    assert_eq!(themes[0]["id"], "default");
    assert_eq!(themes[0]["current"], true);
    assert!(
        themes.to_string().find("/bundled/").is_none(),
        "no theme paths either"
    );
    let err = h
        .call("themes_select", json!({ "id": "nope" }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
}

#[test]
fn a_game_session_runs_records_and_reports_its_events() {
    let h = Harness::new(&[]);
    let events = Arc::new(Mutex::new(Vec::new()));
    for name in ["game:start", "game:end"] {
        let events = events.clone();
        h.window.listen_any(name, move |e| {
            events
                .lock()
                .unwrap()
                .push(serde_json::from_str::<Value>(e.payload()).unwrap());
        });
    }

    let zelda = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap()[0]["id"]
        .clone();
    h.call("launch_game", json!({ "id": zelda })).unwrap();
    let busy = h.call("launch_game", json!({ "id": zelda })).unwrap_err();
    assert_eq!(code(&busy), "busy");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while h.state().playing.load(Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < deadline,
            "the session never ended"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["type"], "game:start");
    assert_eq!(events[1]["type"], "game:end");
    assert_eq!(events[1]["exitCode"], 0);

    // The mock "played" for 20 ms and exited 0: counted, since a clean exit is never a crash.
    let game = h.call("library_game", json!({ "id": zelda })).unwrap();
    assert_eq!(game["playCount"], 1);

    let err = h.call("launch_game", json!({ "id": 9999 })).unwrap_err();
    assert_eq!(code(&err), "launch");
    assert!(
        !h.state().playing.load(Ordering::SeqCst),
        "a failed launch frees the slot"
    );
}

#[test]
fn a_page_from_another_origin_cannot_call_commands() {
    let h = Harness::new(&[]);
    let refused = h.call_from(
        "https://evil.example.com/",
        "library_search",
        json!({ "query": "a" }),
    );
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn achievements_need_an_api_key_and_then_show_progress_through_the_proxy() {
    let h = Harness::new(&[]);
    let err = h.call("ra_user", json!({})).unwrap_err();
    assert_eq!(code(&err), "notConfigured");
    assert!(err["message"].as_str().unwrap().contains("ra.apiKey"));

    h.state()
        .library()
        .set_setting("ra.apiKey", Some("\"key\""))
        .unwrap();
    let user = h.call("ra_user", json!({})).unwrap();
    assert_eq!(user["data"]["username"], "fixture-user");
    assert_eq!(user["stale"], false);
    assert_eq!(
        user["data"]["avatar"],
        "forma://localhost/ra-media/UserPic/fixture-user.png"
    );

    let mario = h
        .call("library_search", json!({ "query": "super mario" }))
        .unwrap()[0]["id"]
        .clone();
    let game = h.call("ra_game", json!({ "id": mario })).unwrap();
    assert_eq!(game["data"]["achievements"].as_array().unwrap().len(), 3);
    assert_eq!(game["data"]["earned"], 2);

    let zelda = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap()[0]["id"]
        .clone();
    let err = h.call("ra_game", json!({ "id": zelda })).unwrap_err();
    assert_eq!(code(&err), "notFound");
}

#[test]
fn the_netplay_lobby_is_matched_to_local_games() {
    let h = Harness::new(&[]);
    let lobby = h.call("netplay_lobby", json!({})).unwrap();
    let rooms = lobby["data"].as_array().unwrap();
    assert_eq!(rooms.len(), 2);
    let mario = h
        .call("library_search", json!({ "query": "super mario" }))
        .unwrap()[0]["id"]
        .clone();
    assert_eq!(rooms[0]["localGameId"], mario);
    assert_eq!(rooms[1]["localGameId"], Value::Null);
}
