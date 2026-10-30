// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Reports the API version the host implements, so `@romdex/sdk` can refuse an incompatible
/// theme with a clear error instead of failing on its first missing command.
#[tauri::command]
fn sdk_api_version() -> &'static str {
    romdex_core::SDK_API_VERSION
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![sdk_api_version])
        .run(tauri::generate_context!())
        .expect("error while running RomDex");
}
