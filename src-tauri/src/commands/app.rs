use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    backend: &'static str,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        name: "Acumod",
        version: env!("CARGO_PKG_VERSION"),
        backend: "Rust via Tauri command",
    }
}
