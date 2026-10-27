mod auth;
mod config;
mod download;
mod launch;

use config::{Account, Settings};
use serde::Serialize;

#[tauri::command]
fn get_settings() -> Settings {
    config::load_settings()
}

#[tauri::command]
fn set_settings(settings: Settings) -> Result<Settings, String> {
    config::save_settings(&settings)?;
    Ok(settings)
}

#[tauri::command]
fn list_accounts() -> config::AccountsFile {
    auth::list()
}

#[tauri::command]
fn offline_login(username: String) -> Result<Account, String> {
    let a = auth::offline_login(&username);
    auth::persist_current(a)
}

#[tauri::command]
async fn yggdrasil_login(
    api_root: String,
    username: String,
    password: String,
) -> Result<Account, String> {
    let a = auth::yggdrasil_login(&api_root, &username, &password).await?;
    auth::persist_current(a)
}

#[tauri::command]
fn logout() -> Result<(), String> {
    auth::logout()
}

#[tauri::command]
fn get_download_sources() -> Vec<download::DownloadSourceInfo> {
    download::sources()
}

#[tauri::command]
async fn list_remote_versions() -> Result<Vec<download::RemoteVersion>, String> {
    let s = config::load_settings();
    let mut list = download::list_remote_versions(&s.download_source).await?;
    // expose release_time field name expected by frontend
    for v in &mut list {
        if v.release_time.is_empty() {
            // keep as-is
        }
    }
    Ok(list)
}

#[tauri::command]
fn list_local_versions() -> Vec<String> {
    let s = config::load_settings();
    let dir = config::resolve_game_dir(&s);
    download::list_local_versions(&dir)
}

#[tauri::command]
async fn download_version(version_id: String) -> Result<download::DownloadResult, String> {
    let s = config::load_settings();
    let dir = config::resolve_game_dir(&s);
    download::download_version(&version_id, &s.download_source, &dir).await
}

#[tauri::command]
fn detect_java() -> launch::JavaInfo {
    let s = config::load_settings();
    launch::detect_java(&s.java_path)
}

#[tauri::command]
async fn launch_game() -> launch::LaunchResult {
    let s = config::load_settings();
    let accounts = auth::list();
    launch::launch_game(&s, accounts.current.as_ref()).await
}

#[tauri::command]
fn open_game_dir() -> Result<(), String> {
    let s = config::load_settings();
    launch::open_game_dir(&s)
}

#[derive(Serialize)]
struct OkMsg {
    ok: bool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_accounts,
            offline_login,
            yggdrasil_login,
            logout,
            get_download_sources,
            list_remote_versions,
            list_local_versions,
            download_version,
            detect_java,
            launch_game,
            open_game_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
