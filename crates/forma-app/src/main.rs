// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod boot;
mod commands;
mod music;
mod options;
mod permissions;
mod protocol;
mod shell;
mod state;
mod status;
mod store;
mod system_settings;

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
        commands::library_search,
        commands::library_set_favorite,
        commands::nav_views,
        commands::nav_browse,
        commands::nav_default_view,
        commands::nav_set_default_view,
        commands::settings_get,
        commands::settings_set,
        commands::themes_list,
        commands::themes_select,
        commands::input_controllers,
        commands::system_quit,
        commands::system_power,
        commands::launch_game,
        commands::ra_user,
        commands::ra_game,
        commands::netplay_lobby,
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
