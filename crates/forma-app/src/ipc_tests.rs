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
use forma_core::options::Options;
use forma_core::system_settings::SystemSettings;
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
            bluetooth: forma_core::bluetooth::Bluetooth::new(Box::new(SystemShell), &paths),
            batocera: BatoceraSettings::new(Box::new(SystemShell), &paths.batocera_conf()),
            store: forma_core::store::Store::default(),
            scraper: crate::scraper::Scraping::new(Arc::new(FixtureHttp), Arc::new(FakeShell)),
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
            music: forma_core::music::Player::new(
                forma_core::music::Library {
                    music: paths.music_dir(),
                    fallback: root.join("no-music"),
                    addon_radios: Default::default(),
                },
                Box::new(forma_core::music::Mpv::new(root.join("mpv.sock"))),
                root.join("run"),
                60,
            ),
            magazines: forma_core::magazines::Reader::new(
                &paths,
                Box::new(forma_core::magazines::HostTools),
            ),
            thumbnails: forma_core::thumbs::Thumbnailer::spawn(forma_core::thumbs::Thumbs::new(
                root.join("thumbs"),
                forma_core::thumbs::DEFAULT_MAX_BYTES,
            )),
            video: forma_core::video::VideoPlayer::new(Box::new(forma_core::music::Mpv::video(
                root.join("video.sock"),
            ))),
            playing: AtomicBool::new(false),
            list_filter: Mutex::default(),
            theme_ready: AtomicBool::new(false),
            ready_file: Some(root.join("forma.ready")),
            on_batocera: false,
            see_through: false,
            input: Mutex::new(None),
            shell: crate::shell::ShellState::default(),
            shell_dir: None,
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
        self.call_with(url, cmd, args, Default::default())
    }

    /// A call from Forma's shell, carrying `token`.
    fn call_as_shell(&self, token: &str, cmd: &str, args: Value) -> Result<Value, Value> {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert(crate::shell::SHELL_HEADER, token.parse().unwrap());
        self.call_with(PAGE, cmd, args, headers)
    }

    fn call_with(
        &self,
        url: &str,
        cmd: &str,
        args: Value,
        headers: tauri::http::HeaderMap,
    ) -> Result<Value, Value> {
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: url.parse().unwrap(),
                body: InvokeBody::Json(args),
                headers,
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

/// Runs nothing; says what it was asked to run.
struct FakeShell;

impl forma_core::host::Shell for FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> forma_core::Result<String> {
        Ok(format!("{program} {}", args.join(" ")))
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
        } else if url.contains("forma-themes") {
            "themes-catalogue.json"
        } else if url.ends_with("/theme-neon-1.0.0.zip") {
            "theme-neon-1.0.0.zip"
        } else if url.ends_with("/addon-example-boxes-1.0.0.zip") {
            "addon-example-boxes-1.0.0.zip"
        } else if url.ends_with("/addon-radio-box-1.0.0.zip") {
            "addon-radio-box-1.0.0.zip"
        } else if url.starts_with("https://boxes.example.com/")
            || url.contains("thumbnails.libretro.com")
                && url.contains("/Named_Boxarts/Super%20Mario%20Bros.%20%28World%29.png")
        {
            // The add-on's box host, and libretro's box for one game.
            "libretro-boxart.png"
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
    assert_eq!(
        h.call("sdk_api_version", json!({})).unwrap(),
        forma_core::SDK_API_VERSION
    );
}

#[test]
fn the_list_on_screen_is_filtered_until_the_theme_leaves_it() {
    let h = Harness::new(&[]);
    let total = |path: serde_json::Value| {
        h.call("nav_browse", json!({ "view": "systems", "path": path }))
            .unwrap()["listing"]["total"]
            .clone()
    };
    assert_eq!(
        h.call("nav_filter_options", json!({})).unwrap(),
        json!(null)
    );
    assert_eq!(total(json!(["nes"])), 3);

    let mario = json!({ "filter": { "name": "mario" } });
    assert_eq!(h.call("nav_set_filter", mario.clone()).unwrap(), 1);
    assert_eq!(total(json!(["nes"])), 1);
    let options = h.call("nav_filter_options", json!({})).unwrap();
    assert_eq!((&options["total"], &options["all"]), (&json!(1), &json!(3)));

    // Nothing would be left: the list stays as it was.
    let nothing = json!({ "filter": { "name": "no such game" } });
    assert_eq!(h.call("nav_set_filter", nothing).unwrap(), 0);
    assert_eq!(total(json!(["nes"])), 1);

    // The theme stays on the games, then goes elsewhere.
    h.call(
        "shell_set_context",
        json!({ "context": { "list": "games" } }),
    )
    .unwrap();
    assert_eq!(total(json!(["nes"])), 1);
    h.call(
        "shell_set_context",
        json!({ "context": { "list": "systems" } }),
    )
    .unwrap();
    assert_eq!(total(json!(["nes"])), 3);

    assert_eq!(h.call("nav_set_filter", mario).unwrap(), 1);
    assert_eq!(
        h.call("nav_set_filter", json!({ "filter": null })).unwrap(),
        3
    );
    assert_eq!(total(json!(["nes"])), 3);
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
fn prepares_pictures_ahead_and_ignores_what_it_cannot() {
    let h = Harness::new(&[]);
    let hits = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = hits[0]["id"].clone();
    h.call(
        "library_prepare_media",
        json!({ "items": [
            { "gameId": id, "kind": "image", "width": 320 },
            { "gameId": id, "kind": "video", "width": 320 },
            { "gameId": 999_999, "kind": "image", "width": 320 }
        ] }),
    )
    .unwrap();
}

#[test]
fn prepares_a_lists_pictures_around_the_selection_wherever_it_is() {
    let h = Harness::new(&[]);
    for around in [0, 1, 999] {
        h.call(
            "library_prepare_list",
            json!({ "view": "systems", "path": ["nes"], "around": around,
                    "pictures": [{ "kind": "image", "width": 320 }, { "kind": "video", "width": 1 }] }),
        )
        .unwrap();
    }
    // Not a game list: nothing to do.
    h.call(
        "library_prepare_list",
        json!({ "view": "systems", "path": [], "around": 0, "pictures": [] }),
    )
    .unwrap();
    let err = h
        .call(
            "library_prepare_list",
            json!({ "view": "nope", "path": [], "around": 0, "pictures": [] }),
        )
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
    assert_eq!(
        views.as_array().unwrap().len(),
        9,
        "the bundled ones and collections"
    );
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
fn bluetooth_needs_permission_and_is_refused_outside_batocera() {
    let h = Harness::new(&[]);
    let address = json!({ "address": "9C:DE:B3:05:06:07" });
    for (cmd, args) in [
        ("bluetooth_status", json!({})),
        ("bluetooth_set_enabled", json!({ "enabled": false })),
        ("bluetooth_pair", json!({})),
        ("bluetooth_connect", address.clone()),
        ("bluetooth_disconnect", address.clone()),
        ("bluetooth_forget", address.clone()),
    ] {
        let err = h.call(cmd, args).unwrap_err();
        assert_eq!(code(&err), "permissionDenied", "{cmd}");
    }
    let allowed = Harness::new(&["system.controllers"]);
    let err = allowed.call("bluetooth_status", json!({})).unwrap_err();
    assert_eq!(code(&err), "unsupported", "{err}");
}

#[test]
fn updates_need_permission_and_are_refused_outside_batocera() {
    let h = Harness::new(&[]);
    for cmd in ["updates_check", "updates_install"] {
        let err = h.call(cmd, json!({})).unwrap_err();
        assert_eq!(code(&err), "permissionDenied", "{cmd}");
    }
    let allowed = Harness::new(&["system.update"]);
    let err = allowed.call("updates_install", json!({})).unwrap_err();
    assert_eq!(code(&err), "unsupported", "{err}");
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
fn removes_an_installed_theme_but_not_the_bundled_or_the_one_in_use() {
    let write = |h: &Harness, id: &str| {
        let dir = h.state().paths.userdata.join("themes-forma").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("theme.json"),
            json!({ "id": id, "name": "Neon", "version": "1.0.0", "sdk": "^1",
                    "author": "Ana", "permissions": ["system.power"] })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>").unwrap();
        dir
    };
    let denied = Harness::new(&[]);
    write(&denied, "neon");
    let err = denied
        .call("themes_remove", json!({ "id": "neon" }))
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");

    let h = Harness::new(&["themes.manage"]);
    let neon = write(&h, "neon");
    let themes = h.call("themes_list", json!({})).unwrap();
    let listed = themes
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "neon")
        .unwrap();
    assert_eq!(listed["author"], "Ana");
    assert_eq!(listed["removable"], true);
    assert_eq!(
        listed["permissionDescriptions"][0],
        "quit Forma, restart or shut down the machine"
    );
    assert_eq!(themes[0]["removable"], false, "the bundled theme stays");
    let err = h
        .call("themes_remove", json!({ "id": "default" }))
        .unwrap_err();
    assert_eq!(code(&err), "invalid");

    h.call("themes_remove", json!({ "id": "neon" })).unwrap();
    assert!(!neon.exists());
    let themes = h.call("themes_list", json!({})).unwrap();
    assert!(themes.as_array().unwrap().iter().all(|t| t["id"] != "neon"));
    let err = h
        .call("themes_remove", json!({ "id": "neon" }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
}

#[test]
fn installs_a_theme_from_the_catalogue_and_marks_updates() {
    let h = Harness::new(&["themes.manage"]);
    let fetched = h.call("themes_catalogue", json!({})).unwrap();
    let themes = fetched["data"]["themes"].as_array().unwrap();
    assert_eq!(themes.len(), 2, "the broken entry is skipped");
    assert_eq!(fetched["data"]["skipped"].as_array().unwrap().len(), 1);
    let neon = &themes[0];
    assert_eq!(neon["installed"], Value::Null);
    assert_eq!(neon["runsHere"], true);
    assert_eq!(
        neon["permissionDescriptions"][0],
        "quit Forma, restart or shut down the machine"
    );
    assert_eq!(themes[1]["runsHere"], false);

    h.call("themes_install", json!({ "id": "neon" })).unwrap();
    let dir = h.state().paths.userdata.join("themes-forma/neon");
    assert!(dir.join("index.html").is_file());
    let listed = h.call("themes_list", json!({})).unwrap();
    let neon = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "neon")
        .unwrap();
    assert_eq!(neon["author"], "RAM Raiders");
    assert_eq!(
        neon["update"],
        Value::Null,
        "the catalogue has this version"
    );

    // An older copy on disk: the catalogue's 1.0.0 is an update.
    let manifest = dir.join("theme.json");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("1.0.0", "0.9.0")).unwrap();
    let listed = h.call("themes_list", json!({})).unwrap();
    let neon = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "neon")
        .unwrap();
    assert_eq!(neon["update"], "1.0.0");
    let fetched = h.call("themes_catalogue", json!({})).unwrap();
    assert_eq!(fetched["data"]["themes"][0]["update"], true);

    let err = h
        .call("themes_install", json!({ "id": "future" }))
        .unwrap_err();
    assert!(err["message"].as_str().unwrap().contains("needs Forma SDK"));
    let err = h
        .call("themes_install", json!({ "id": "nope" }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
    let denied = Harness::new(&[]);
    let err = denied
        .call("themes_install", json!({ "id": "neon" }))
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
}

#[test]
fn lists_magazines_serves_their_pages_and_remembers_the_page() {
    let h = Harness::new(&[]);
    let paths = h.state().paths.clone();
    let issue = paths.userdata.join("magazines/Edge/Edge 9");
    std::fs::create_dir_all(&issue).unwrap();
    // A 1×1 PNG.
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 248, 207, 80,
        15, 0, 3, 134, 1, 128, 90, 52, 125, 107, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    for page in ["001.png", "002.png"] {
        std::fs::write(issue.join(page), PNG).unwrap();
    }

    let list = h.call("magazines_list", json!({})).unwrap();
    assert_eq!(list[0]["id"], "Edge/Edge 9");
    assert_eq!(list[0]["folder"], "Edge");
    assert_eq!(list[0]["format"], "folder");
    assert_eq!(list[0]["page"], Value::Null);
    assert_eq!(
        list[0]["pageUrl"],
        "forma://localhost/magazine/{page}/Edge/Edge%209"
    );
    let pages = h
        .call("magazines_pages", json!({ "id": "Edge/Edge 9" }))
        .unwrap();
    assert_eq!(pages, 2);
    let page = crate::protocol::Resolver::magazine_page(&*h.state(), "Edge/Edge 9", 1, 640);
    assert!(page.is_some_and(|f| f.is_file()));

    h.call(
        "magazines_set_page",
        json!({ "id": "Edge/Edge 9", "page": 1 }),
    )
    .unwrap();
    let list = h.call("magazines_list", json!({})).unwrap();
    assert_eq!(list[0]["page"], 1);
    let err = h
        .call("magazines_pages", json!({ "id": "../roms" }))
        .unwrap_err();
    assert_eq!(code(&err), "invalid");
}

#[test]
fn a_games_manual_is_read_as_a_magazine_is_and_opened_in_the_shells_reader() {
    let h = Harness::new(&[]);
    let paths = h.state().paths.clone();
    let nes = paths.userdata.join("roms/nes");
    std::fs::create_dir_all(nes.join("manuals")).unwrap();
    // A 1×1 PNG: a manual scraped as one picture.
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 248, 207, 80,
        15, 0, 3, 134, 1, 128, 90, 52, 125, 107, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    std::fs::write(nes.join("manuals/zelda.png"), PNG).unwrap();
    let list = nes.join("gamelist.xml");
    let xml = std::fs::read_to_string(&list).unwrap().replacen(
        "<name>The Legend of Zelda</name>",
        "<name>The Legend of Zelda</name><manual>./manuals/zelda.png</manual>",
        1,
    );
    std::fs::write(&list, xml).unwrap();
    h.state().library().refresh(&paths).unwrap();
    let found = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = found[0]["id"].as_i64().unwrap();

    let manual = h.call("library_manual", json!({ "gameId": id })).unwrap();
    assert_eq!(manual["pages"], 1);
    assert_eq!(
        manual["pageUrl"],
        format!("forma://localhost/manual/{{page}}/{id}")
    );
    let page = crate::protocol::Resolver::manual_page(&*h.state(), id, 0, 640);
    assert!(page.is_some_and(|f| f.is_file()));
    assert!(crate::protocol::Resolver::manual_page(&*h.state(), id, 1, 640).is_none());

    let other = h
        .call("library_search", json!({ "query": "homebrew" }))
        .unwrap();
    let none = h
        .call("library_manual", json!({ "gameId": other[0]["id"] }))
        .unwrap();
    assert_eq!(none, Value::Null);

    let opened = Arc::new(Mutex::new(None));
    let seen = opened.clone();
    h.window
        .listen_any(crate::magazines::EVENT_MANUAL_OPEN, move |e| {
            *seen.lock().unwrap() = Some(e.payload().to_owned());
        });
    h.call("library_open_manual", json!({ "gameId": id }))
        .unwrap();
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some(format!(r#"{{"gameId":{id}}}"#).as_str())
    );
}

#[test]
fn a_game_is_edited_by_hand_with_the_permission() {
    let h = Harness::new(&["scraper.manage"]);
    let found = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = found[0]["id"].as_i64().unwrap();
    let system = found[0]["system"].as_str().unwrap().to_owned();
    h.call(
        "scraper_edit",
        json!({ "id": id, "fields": { "genre": "Platform" } }),
    )
    .unwrap();
    let list = std::fs::read_to_string(
        h.state()
            .paths
            .userdata
            .join(format!("roms/{system}/gamelist.xml")),
    )
    .unwrap();
    assert!(list.contains("<genre>Platform</genre>"), "{list}");
    let bad = h.call(
        "scraper_edit",
        json!({ "id": id, "fields": { "releaseDate": "soon" } }),
    );
    assert_eq!(code(&bad.unwrap_err()), "invalid");
    let denied = Harness::new(&[]);
    let err = denied
        .call("scraper_edit", json!({ "id": id, "fields": {} }))
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
}

#[test]
fn a_games_picture_is_removed_and_its_file_deleted() {
    let h = Harness::new(&["scraper.manage"]);
    let found = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = found[0]["id"].as_i64().unwrap();
    let system = found[0]["system"].as_str().unwrap().to_owned();
    let path = found[0]["path"].as_str().unwrap().to_owned();
    let dir = h.state().paths.userdata.join(format!("roms/{system}"));
    let stem = std::path::Path::new(&path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(dir.join("images")).unwrap();
    let picture = dir.join(format!("images/{stem}-image.png"));
    std::fs::write(&picture, b"png").unwrap();
    h.call(
        "scraper_edit",
        json!({ "id": id, "fields": { "genre": "Adventure" } }),
    )
    .unwrap();
    let list = dir.join("gamelist.xml");
    let xml = std::fs::read_to_string(&list).unwrap();
    let xml = xml.replace(
        "<genre>Adventure</genre>",
        &format!("<genre>Adventure</genre><image>./images/{stem}-image.png</image>"),
    );
    std::fs::write(&list, xml).unwrap();
    h.state().library().refresh(&h.state().paths).unwrap();
    let problems = h
        .call(
            "scraper_remove_media",
            json!({ "id": id, "kinds": ["image"] }),
        )
        .unwrap();
    assert_eq!(problems, json!([]));
    assert!(!picture.exists(), "the file is deleted");
    let xml = std::fs::read_to_string(&list).unwrap();
    assert!(!xml.contains(&format!("{stem}-image.png")), "{xml}");
    let denied = Harness::new(&[]);
    let err = denied
        .call(
            "scraper_remove_media",
            json!({ "id": id, "kinds": ["image"] }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
}

#[test]
fn scraper_settings_accounts_and_a_run_over_a_system() {
    let h = Harness::new(&["scraper.manage"]);
    let settings = h.call("scraper_settings", json!({})).unwrap();
    let sources = settings["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 9);
    assert_eq!(sources[0]["id"], "screenscraper");
    assert_eq!(sources[0]["ready"], false);
    assert_eq!(sources[7]["id"], "launchbox");
    assert_eq!(sources[7]["ready"], true);
    assert_eq!(sources[8]["id"], "libretro");
    assert_eq!(sources[8]["ready"], true);
    assert_eq!(settings["mediaKinds"].as_array().unwrap().len(), 19);

    // Credentials: a secret is never sent back, only that it is set.
    h.call(
        "scraper_set_credential",
        json!({ "source": "screenscraper", "key": "devId", "value": "forma" }),
    )
    .unwrap();
    let after = h
        .call(
            "scraper_set_credential",
            json!({ "source": "screenscraper", "key": "devPassword", "value": "hush" }),
        )
        .unwrap();
    let needs = &after["sources"][0]["needs"];
    assert_eq!(needs[0]["value"], "forma");
    assert_eq!(needs[1]["set"], true);
    assert_eq!(needs[1]["value"], Value::Null);
    assert_eq!(after["sources"][0]["ready"], true);
    let err = h
        .call(
            "scraper_set_credential",
            json!({ "source": "screenscraper", "key": "apiKey", "value": "x" }),
        )
        .unwrap_err();
    assert_eq!(code(&err), "invalid");

    // Only libretro, replacing what is there, Europe's names first whatever the ROM says.
    let only_libretro: Vec<Value> = sources
        .iter()
        .map(|s| json!({ "id": s["id"], "enabled": s["id"] == "libretro" }))
        .collect();
    let configured = h
        .call(
            "scraper_configure",
            json!({ "sources": only_libretro, "replace": true, "ownRegion": false, "regions": ["Europe", "Mars"] }),
        )
        .unwrap();
    assert_eq!(configured["replace"], true);
    assert_eq!(configured["ownRegion"], false);
    assert_eq!(
        configured["regions"],
        json!(["Europe"]),
        "unknown regions left out"
    );
    assert_eq!(configured["allRegions"][1], "USA");
    assert_eq!(configured["manuals"], true);

    let events = Arc::new(Mutex::new(0));
    let seen = events.clone();
    h.window
        .listen_any("scraper:progress", move |_| *seen.lock().unwrap() += 1);
    let started = h
        .call(
            "scraper_start",
            json!({ "scope": { "kind": "systems", "names": ["nes"] } }),
        )
        .unwrap();
    assert_eq!(started["running"], true);
    let total = started["total"].as_u64().unwrap();
    assert!(total >= 3, "{started}");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let status = loop {
        let status = h.call("scraper_status", json!({})).unwrap();
        if status["running"] == false {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "still running: {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status["done"], total);
    assert_eq!(status["found"], 1, "{status}");
    assert_eq!(status["notFound"].as_u64().unwrap(), total - 1);
    let nes = h.state().paths.userdata.join("roms/nes");
    let saved = std::fs::read(nes.join("images/Super Mario Bros. (World)-thumb.png")).unwrap();
    assert!(saved.starts_with(b"\x89PNG"));
    let list = std::fs::read_to_string(nes.join("gamelist.xml")).unwrap();
    assert!(list.contains("<thumbnail>./images/Super Mario Bros. (World)-thumb.png</thumbnail>"));
    assert!(*events.lock().unwrap() > 0);

    let denied = Harness::new(&[]);
    let err = denied.call("scraper_settings", json!({})).unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
}

#[test]
fn an_addon_installs_from_the_catalogue_joins_the_scraper_and_goes() {
    let h = Harness::new(&["addons.manage", "scraper.manage"]);
    assert_eq!(h.call("addons_list", json!({})).unwrap(), json!([]));
    let catalogue = h.call("addons_catalogue", json!({})).unwrap();
    let entry = &catalogue["data"]["addons"][0];
    assert_eq!(entry["id"], "example-boxes");
    assert_eq!(entry["installed"], Value::Null);
    assert_eq!(
        entry["permissionDescriptions"][0],
        "reach the internet sites it lists, and only those"
    );

    let installed = h
        .call("addons_install", json!({ "id": "example-boxes" }))
        .unwrap();
    assert_eq!(installed["version"], "1.0.0");
    assert_eq!(installed["enabled"], true);
    assert_eq!(installed["hosts"], json!(["https://boxes.example.com"]));
    assert_eq!(installed["scraperSource"], "addon:example-boxes");
    let catalogue = h.call("addons_catalogue", json!({})).unwrap();
    assert_eq!(catalogue["data"]["addons"][0]["installed"], "1.0.0");

    // It is the scraper's last source, and set up (its token is optional).
    let settings = h.call("scraper_settings", json!({})).unwrap();
    let sources = settings["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 10);
    assert_eq!(sources[9]["id"], "addon:example-boxes");
    assert_eq!(sources[9]["ready"], true);
    assert_eq!(sources[9]["needs"][0]["label"], "Token");

    // Only the add-on, boxes only: every NES game gets one.
    let only: Vec<Value> = sources
        .iter()
        .map(|s| json!({ "id": s["id"], "enabled": s["id"] == "addon:example-boxes" }))
        .collect();
    h.call(
        "scraper_configure",
        json!({ "sources": only, "replace": true }),
    )
    .unwrap();
    let started = h
        .call(
            "scraper_start",
            json!({ "scope": { "kind": "system", "name": "nes" } }),
        )
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let status = loop {
        let status = h.call("scraper_status", json!({})).unwrap();
        if status["running"] == false {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "still running: {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status["found"], started["total"], "{status}");
    let nes = h.state().paths.userdata.join("roms/nes");
    let made: Vec<String> = std::fs::read_dir(nes.join("images"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with("-thumb.png"))
        .collect();
    assert!(
        made.contains(&"Homebrew Demo-thumb.png".to_owned()),
        "{made:?} {status}"
    );

    // Off: gone from the scraper, still installed.
    let listed = h
        .call(
            "addons_set_enabled",
            json!({ "id": "example-boxes", "enabled": false }),
        )
        .unwrap();
    assert_eq!(listed[0]["enabled"], false);
    let settings = h.call("scraper_settings", json!({})).unwrap();
    assert_eq!(settings["sources"].as_array().unwrap().len(), 9);
    h.call(
        "addons_set_enabled",
        json!({ "id": "example-boxes", "enabled": true }),
    )
    .unwrap();
    let settings = h.call("scraper_settings", json!({})).unwrap();
    assert_eq!(settings["sources"].as_array().unwrap().len(), 10);

    h.call("addons_remove", json!({ "id": "example-boxes" }))
        .unwrap();
    assert_eq!(h.call("addons_list", json!({})).unwrap(), json!([]));
    let settings = h.call("scraper_settings", json!({})).unwrap();
    assert_eq!(settings["sources"].as_array().unwrap().len(), 9);
    let err = h
        .call("addons_install", json!({ "id": "nope" }))
        .unwrap_err();
    assert_eq!(code(&err), "notFound");
    let denied = Harness::new(&["scraper.manage"]);
    let err = denied.call("addons_list", json!({})).unwrap_err();
    assert_eq!(code(&err), "permissionDenied");
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
    assert_eq!(
        rooms[1]["join"]["session"], "a1b2c3d4e5",
        "through its relay"
    );

    let owned = h
        .call("netplay_lobby", json!({ "filter": { "owned": true } }))
        .unwrap();
    let owned = owned["data"].as_array().unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0]["localGameId"], mario);
}

#[test]
fn collections_are_made_filled_and_browsed_by_any_theme() {
    let h = Harness::new(&[]);
    let mario = h
        .call("library_search", json!({ "query": "super mario" }))
        .unwrap()[0]["id"]
        .clone();
    assert_eq!(h.call("collections_list", json!({})).unwrap(), json!([]));
    assert_eq!(
        h.call("collections_create", json!({ "name": "Couch co-op" }))
            .unwrap(),
        "Couch co-op"
    );
    h.call(
        "collections_set",
        json!({ "name": "couch CO-OP", "gameId": mario, "member": true }),
    )
    .unwrap();
    assert_eq!(
        h.call("collections_list", json!({})).unwrap(),
        json!([{ "name": "Couch co-op", "games": 1 }])
    );
    assert_eq!(
        h.call("collections_of", json!({ "gameId": mario }))
            .unwrap(),
        json!(["Couch co-op"])
    );
    let page = h
        .call(
            "nav_browse",
            json!({ "view": "collections", "path": ["couch-co-op"] }),
        )
        .unwrap();
    assert_eq!(page["listing"]["items"][0]["id"], mario);
    let err = h
        .call("collections_create", json!({ "name": "a/b" }))
        .unwrap_err();
    assert_eq!(err["code"], "invalid");
    let err = h
        .call("collections_delete", json!({ "name": "nope" }))
        .unwrap_err();
    assert_eq!(err["code"], "notFound");
    h.call("collections_delete", json!({ "name": "Couch co-op" }))
        .unwrap();
    assert_eq!(h.call("collections_list", json!({})).unwrap(), json!([]));
}

#[test]
fn the_shell_claims_its_token_once_and_then_may_do_what_forma_offers() {
    // A theme with no permissions at all: Forma's menu still works over it.
    let h = Harness::new(&[]);
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    let token = token.as_str().unwrap();
    assert_eq!(
        code(&h.call("shell_claim", json!({})).unwrap_err()),
        "permissionDenied"
    );

    let err = h
        .call_as_shell(token, "system_power", json!({ "action": "shutdown" }))
        .unwrap_err();
    assert_eq!(
        code(&err),
        "unsupported",
        "past the permission, refused off Batocera"
    );
    let err = h
        .call_as_shell("guess", "system_power", json!({ "action": "shutdown" }))
        .unwrap_err();
    assert_eq!(code(&err), "permissionDenied");

    h.call_as_shell(
        token,
        "settings_set",
        json!({ "key": "display.margins", "value": 3, "scope": "system" }),
    )
    .unwrap();
    assert_eq!(
        h.call(
            "settings_get",
            json!({ "key": "display.margins", "scope": "system" })
        )
        .unwrap_err()["code"],
        "permissionDenied",
        "the theme itself still cannot"
    );
    assert_eq!(
        h.call_as_shell(
            token,
            "settings_get",
            json!({ "key": "display.margins", "scope": "system" })
        )
        .unwrap(),
        3
    );

    // Only the shell opens its menu, or reads what it needs.
    assert_eq!(
        code(
            &h.call("shell_set_open", json!({ "open": true }))
                .unwrap_err()
        ),
        "permissionDenied"
    );
    assert_eq!(
        code(&h.call("shell_info", json!({})).unwrap_err()),
        "permissionDenied"
    );
    h.call_as_shell(token, "shell_set_open", json!({ "open": true }))
        .unwrap();

    // The screensaver: only the shell starts it, and every press is the shell's meanwhile.
    assert_eq!(
        code(
            &h.call("shell_screensaver", json!({ "active": true }))
                .unwrap_err()
        ),
        "permissionDenied"
    );
    h.call_as_shell(token, "shell_set_open", json!({ "open": false }))
        .unwrap();
    h.call_as_shell(token, "shell_screensaver", json!({ "active": true }))
        .unwrap();
    assert!(h.state().shell.is_open());
}

#[test]
fn the_theme_tells_the_shell_what_is_selected() {
    let h = Harness::new(&[]);
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    let token = token.as_str().unwrap();
    let context =
        json!({ "view": "systems", "system": { "name": "snes", "fullname": "Super Nintendo" } });
    h.call("shell_set_context", json!({ "context": context }))
        .unwrap();
    let info = h.call_as_shell(token, "shell_info", json!({})).unwrap();
    assert_eq!(info["context"], context);
    assert_eq!(info["theme"]["id"], "default");
    assert_eq!(info["theme"]["settings"], json!([]));
    assert_eq!(
        code(
            &h.call("shell_set_context", json!({ "context": 3 }))
                .unwrap_err()
        ),
        "invalid"
    );
}

#[test]
fn the_shell_opens_a_view_in_the_theme() {
    let h = Harness::new(&[]);
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    let token = token.as_str().unwrap();
    let opened = Arc::new(Mutex::new(None));
    let seen = opened.clone();
    h.window.listen_any(crate::shell::EVENT_NAV_OPEN, move |e| {
        *seen.lock().unwrap() = Some(e.payload().to_owned());
    });
    h.call_as_shell(token, "shell_open_view", json!({ "view": "genre" }))
        .unwrap();
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some(r#"{"view":"genre"}"#)
    );
    assert_eq!(
        code(
            &h.call_as_shell(token, "shell_open_view", json!({ "view": "nope" }))
                .unwrap_err()
        ),
        "notFound"
    );
}

#[test]
fn deleting_the_one_game_a_filter_kept_shows_every_game_from_the_one_beside_it() {
    let h = Harness::new(&[]);
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    let token = token.as_str().unwrap();
    let list = |h: &Harness| {
        h.call("nav_browse", json!({ "view": "systems", "path": ["nes"] }))
            .unwrap()
    };
    let names = |page: &Value| -> Vec<String> {
        page["listing"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let all = list(&h);
    let id_of = |name: &str| {
        all["listing"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let (mario, zelda) = (id_of("Super Mario Bros."), id_of("The Legend of Zelda"));
    let mario_only = json!({ "filter": { "name": "mario" } });
    assert_eq!(h.call("nav_set_filter", mario_only).unwrap(), 1);

    let opened = Arc::new(Mutex::new(None));
    let seen = opened.clone();
    h.window.listen_any(crate::shell::EVENT_NAV_GAME, move |e| {
        *seen.lock().unwrap() = Some(e.payload().to_owned());
    });
    h.call_as_shell(token, "library_delete_game", json!({ "id": mario }))
        .unwrap();
    // Every game again, the theme sent to the one after it.
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some(format!(r#"{{"game":{zelda},"system":"nes"}}"#).as_str())
    );
    assert_eq!(list(&h)["filtered"], json!(false));

    // Once the library has read the disk again: gone, and no filter brought back.
    {
        let state = h.state();
        state.library().refresh(&state.paths).unwrap();
    }
    let after = list(&h);
    assert!(!names(&after).contains(&"Super Mario Bros.".to_owned()));
    assert_eq!(after["filtered"], json!(false));
}

#[test]
fn a_filter_that_keeps_nothing_any_more_is_dropped() {
    let h = Harness::new(&[]);
    let browse = || {
        h.call("nav_browse", json!({ "view": "systems", "path": ["nes"] }))
            .unwrap()
    };
    let all = browse()["listing"]["total"].as_i64().unwrap();
    let mario_only = json!({ "filter": { "name": "mario" } });
    assert_eq!(h.call("nav_set_filter", mario_only).unwrap(), 1);
    // Its game goes behind Forma's back.
    {
        let state = h.state();
        std::fs::remove_file(
            state
                .paths
                .userdata
                .join("roms/nes/Super Mario Bros. (World).nes"),
        )
        .unwrap();
        state.library().refresh(&state.paths).unwrap();
    }
    let page = browse();
    assert_eq!(page["filtered"], json!(false));
    assert_eq!(page["listing"]["total"].as_i64(), Some(all - 1));
    let options = h.call("nav_filter_options", json!({})).unwrap();
    assert_eq!(options["filter"]["name"], json!(null));
}

#[test]
fn the_shell_opens_a_game_in_the_theme() {
    let h = Harness::new(&[]);
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    let token = token.as_str().unwrap();
    let hits = h
        .call("library_search", json!({ "query": "zelda" }))
        .unwrap();
    let id = hits[0]["id"].as_i64().unwrap();
    let system = hits[0]["system"].as_str().unwrap().to_owned();
    let opened = Arc::new(Mutex::new(None));
    let seen = opened.clone();
    h.window.listen_any(crate::shell::EVENT_NAV_GAME, move |e| {
        *seen.lock().unwrap() = Some(e.payload().to_owned());
    });
    h.call_as_shell(token, "shell_open_game", json!({ "id": id }))
        .unwrap();
    assert_eq!(
        opened.lock().unwrap().as_deref(),
        Some(format!(r#"{{"game":{id},"system":"{system}"}}"#).as_str())
    );
    assert_eq!(
        code(
            &h.call_as_shell(token, "shell_open_game", json!({ "id": 999_999 }))
                .unwrap_err()
        ),
        "notFound"
    );
}

#[test]
fn library_versions_answers_for_a_system() {
    let h = Harness::new(&[]);
    let groups = h
        .call("library_versions", json!({ "system": "nes" }))
        .unwrap();
    assert!(groups.is_array());
}

#[test]
fn ra_matches_reports_each_ra_system_of_the_library() {
    let h = Harness::new(&[]);
    let report = h.call("ra_matches", json!({})).unwrap();
    assert_eq!(report["running"], false);
    let systems = report["systems"].as_array().unwrap();
    let nes = systems.iter().find(|s| s["system"] == "nes").unwrap();
    assert!(nes["games"].as_i64().unwrap() > 0);
    // The fixture imports one hash and its id; the worker has not run.
    assert_eq!(
        (nes["hashed"].as_i64(), nes["matched"].as_i64()),
        (Some(1), Some(1))
    );
    assert!(
        systems.iter().all(|s| s["system"] != "ports"),
        "no RA console"
    );
}

#[test]
fn a_radio_addon_adds_stations_and_pages_and_runs_its_system_part() {
    let h = Harness::new(&["addons.manage", "music.manage"]);
    let installed = h
        .call("addons_install", json!({ "id": "radio-box" }))
        .unwrap();
    assert_eq!(installed["verified"], true);
    assert_eq!(installed["system"], true);
    assert_eq!(installed["menu"]["label"], "Radio box");
    assert_eq!(
        installed["permissionDescriptions"][1],
        "install and run its own programs with full control of this machine"
    );

    // Its stations are in Music, after the user's own.
    let radio = |sources: &Value| -> Vec<String> {
        sources
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["id"].as_str().unwrap().starts_with("addon:radio-box:"))
            .map(|s| s["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let sources = h.call("music_sources", json!({})).unwrap();
    assert_eq!(radio(&sources), ["Chip FM", "Arcade Lounge"]);

    // A setting it reads: a third station.
    let after = h
        .call(
            "addons_set_setting",
            json!({ "id": "radio-box", "key": "station", "value": "https://radio.example.com/mine" }),
        )
        .unwrap();
    assert_eq!(
        after[0]["settings"][0]["value"],
        "https://radio.example.com/mine"
    );
    let sources = h.call("music_sources", json!({})).unwrap();
    assert_eq!(
        radio(&sources),
        ["Chip FM", "Arcade Lounge", "Your station"]
    );
    assert!(
        h.call(
            "addons_set_setting",
            json!({ "id": "radio-box", "key": "nope", "value": "x" })
        )
        .is_err()
    );

    // Its pages, drawn by Forma.
    let page = h
        .call("addons_page", json!({ "id": "radio-box", "path": [] }))
        .unwrap();
    assert_eq!(page["title"], "Radio box");
    assert_eq!(page["rows"][0]["heading"], "Stations");
    assert_eq!(page["rows"][3]["action"], "tune");
    assert_eq!(page["rows"][3]["ask"]["title"], "Frequency");
    assert_eq!(page["rows"][4]["open"], json!(["about"]));
    let about = h
        .call(
            "addons_page",
            json!({ "id": "radio-box", "path": ["about"] }),
        )
        .unwrap();
    assert_eq!(about["rows"][0]["label"], "A test add-on");

    // An action runs its system command (the fake shell says what it ran).
    let done = h
        .call(
            "addons_action",
            json!({ "id": "radio-box", "action": "tune", "input": "99.1" }),
        )
        .unwrap();
    let notify = done["notify"].as_str().unwrap();
    assert!(
        notify.starts_with("sh ") && notify.ends_with("system/tune.sh 99.1"),
        "{notify}"
    );

    // Off: no stations, no pages.
    h.call(
        "addons_set_enabled",
        json!({ "id": "radio-box", "enabled": false }),
    )
    .unwrap();
    assert!(radio(&h.call("music_sources", json!({})).unwrap()).is_empty());
    assert!(
        h.call("addons_page", json!({ "id": "radio-box", "path": [] }))
            .is_err()
    );

    h.call("addons_remove", json!({ "id": "radio-box" }))
        .unwrap();
    assert_eq!(h.call("addons_list", json!({})).unwrap(), json!([]));
}

#[test]
fn random_games_for_the_screensaver_have_the_media_asked_for() {
    let h = Harness::new(&[]);
    let games = h
        .call("library_random", json!({ "media": ["video"], "limit": 10 }))
        .unwrap();
    let games = games.as_array().unwrap();
    assert!(!games.is_empty());
    assert!(games.iter().all(|g| g["media"].get("video").is_some()));
    let favourites = h
        .call("library_random", json!({ "favorites": true }))
        .unwrap();
    assert!(
        favourites
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["favorite"] == true)
    );
}

#[test]
fn the_dump_check_reports_systems_a_catalogue_covers_and_can_be_turned_off() {
    let h = Harness::new(&[]);
    let checks = h.call("library_dump_checks", json!({})).unwrap();
    assert_eq!(checks["on"], true);
    let systems: Vec<&str> = checks["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["system"].as_str().unwrap())
        .collect();
    assert!(systems.contains(&"nes"), "{systems:?}");
    let games = h
        .call("library_search", json!({ "query": "mario" }))
        .unwrap();
    assert!(
        games[0].get("dump").is_some(),
        "every game says, null while unknown"
    );
    h.state().shell.new_page();
    let token = h.call("shell_claim", json!({})).unwrap();
    h.call_as_shell(
        token.as_str().unwrap(),
        "settings_set",
        json!({ "key": "library.verifyDumps", "value": false, "scope": "system" }),
    )
    .unwrap();
    assert_eq!(
        h.call("library_dump_checks", json!({})).unwrap()["on"],
        false
    );
}
