// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod addons;
mod api;
mod bluetooth;
mod boot;
mod catalogue;
mod collections;
mod commands;
mod magazines;
mod music;
mod options;
mod permissions;
mod protocol;
mod ra_match;
mod scraper;
mod shell;
mod splash;
mod state;
mod status;
mod store;
mod system_settings;
mod updates;

#[cfg(test)]
mod ipc_tests;

use tauri::ipc::Invoke;
use tauri::{Manager, Runtime};

use crate::state::AppState;

/// Every command a theme can call through `@ramraiders/forma-sdk`.
fn handlers<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::sdk_api_version,
        commands::theme_ready,
        commands::library_systems,
        commands::library_game,
        commands::library_prepare_media,
        commands::library_prepare_list,
        commands::library_search,
        commands::library_set_favorite,
        commands::library_delete_game,
        commands::nav_views,
        commands::nav_browse,
        commands::nav_default_view,
        commands::nav_set_default_view,
        commands::settings_get,
        commands::settings_set,
        commands::themes_list,
        commands::themes_select,
        commands::themes_remove,
        catalogue::themes_catalogue,
        catalogue::themes_install,
        magazines::magazines_list,
        magazines::magazines_pages,
        magazines::magazines_set_page,
        magazines::magazines_open,
        scraper::scraper_settings,
        scraper::scraper_configure,
        scraper::scraper_set_credential,
        scraper::scraper_start,
        scraper::scraper_status,
        scraper::scraper_stop,
        scraper::scraper_offers,
        scraper::scraper_preview,
        scraper::scraper_apply,
        scraper::scraper_edit,
        addons::addons_list,
        addons::addons_set_enabled,
        addons::addons_remove,
        addons::addons_catalogue,
        addons::addons_install,
        addons::addons_set_setting,
        addons::addons_page,
        addons::addons_action,
        commands::input_controllers,
        commands::system_quit,
        commands::system_power,
        commands::launch_game,
        commands::ra_user,
        commands::ra_game,
        ra_match::ra_matches,
        commands::netplay_lobby,
        commands::netplay_lan,
        collections::collections_list,
        collections::collections_of,
        collections::collections_create,
        collections::collections_delete,
        collections::collections_set,
        commands::network_status,
        commands::network_wifi_scan,
        commands::network_wifi_join,
        commands::network_wifi_set_enabled,
        commands::network_ethernet_configure,
        commands::batocera_settings,
        commands::batocera_set,
        options::options_get,
        options::options_set,
        system_settings::system_info,
        system_settings::system_settings,
        system_settings::system_settings_set,
        bluetooth::bluetooth_status,
        bluetooth::bluetooth_set_enabled,
        bluetooth::bluetooth_pair,
        bluetooth::bluetooth_connect,
        bluetooth::bluetooth_disconnect,
        bluetooth::bluetooth_forget,
        updates::updates_check,
        updates::updates_install,
        store::store_list,
        store::store_refresh,
        store::store_install,
        store::store_remove,
        store::store_update,
        commands::ra_login,
        commands::ra_logout,
        music::music_sources,
        music::music_status,
        music::music_play,
        music::music_toggle,
        music::music_next,
        music::music_previous,
        music::music_stop,
        music::music_set_volume,
        music::music_radios,
        music::music_add_radio,
        music::music_remove_radio,
        music::video_play,
        music::video_stop,
        shell::shell_claim,
        shell::shell_set_open,
        shell::shell_started,
        status::status_get,
        shell::shell_set_context,
        shell::shell_info,
        shell::shell_set_theme_setting,
        shell::shell_open_view,
    ]
}

fn main() {
    boot::mark_process_start();
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("forma", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // File reads (a video can be large) stay off the WebView's thread.
            std::thread::spawn(move || {
                let response = match app.try_state::<AppState>() {
                    Some(state) => protocol::respond(&*state, &request),
                    None => tauri::http::Response::builder()
                        .status(503)
                        .body(b"starting".to_vec())
                        .unwrap_or_default(),
                };
                responder.respond(response);
            });
        })
        .invoke_handler(handlers())
        .on_page_load(|webview, payload| {
            // A new page in the window: the shell it brings claims a new token, and a theme page
            // has its time to report ready from now.
            if payload.event() == tauri::webview::PageLoadEvent::Started
                && let Some(state) = webview.try_state::<AppState>()
            {
                state.shell.new_page();
                // A video the old page played goes with it, and the music comes back.
                if state.music.ducked() || state.video.playing() {
                    let app = webview.app_handle().clone();
                    std::thread::spawn(move || {
                        let state = app.state::<AppState>();
                        let _ = state.video.stop();
                        state.music.duck(false);
                    });
                }
                if payload.url().path().starts_with("/theme/") {
                    boot::watch_ready(webview.app_handle().clone());
                }
            }
        })
        .on_window_event(commands::on_window_event)
        .setup(|app| boot::start(app))
        .run(tauri::generate_context!())
        .expect("error while running Forma");
}
