// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod boot;
mod commands;
mod permissions;
mod protocol;
mod state;

#[cfg(test)]
mod ipc_tests;

use tauri::ipc::Invoke;
use tauri::{Manager, Runtime};

use crate::state::AppState;

/// Every command a theme can call through `@romdex/sdk`.
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
    ]
}

fn main() {
    boot::mark_process_start();
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("romdex", |ctx, request, responder| {
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
        .setup(|app| boot::start(app))
        .run(tauri::generate_context!())
        .expect("error while running RomDex");
}
