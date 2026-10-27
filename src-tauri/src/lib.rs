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

fn register_plugins_and_handlers<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
) -> tauri::Builder<R> {
    builder
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
}

#[cfg(all(feature = "cef", target_os = "linux"))]
fn run_cef() {
    // CEF runtime forces X11 even on Wayland (needs XWayland / DISPLAY).
    if std::env::var_os("DISPLAY").filter(|v| !v.is_empty()).is_none() {
        eprintln!(
            "MCLauncher 的 Linux CEF 运行时需要 X11 DISPLAY。\n\
             Wayland 请启用 XWayland；Niri 可安装 xwayland-satellite 并在该会话中启动。"
        );
        std::process::exit(1);
    }

    // Fresh Chromium profiles on Linux otherwise exit with code 28 (EULA / first-run).
    // Sandbox disabled: AppImage cannot ship setuid chrome-sandbox; Ubuntu 24.04+ may
    // restrict user namespaces. Deb installs can revisit SandboxPolicy::Auto later.
    let cef = tauri_runtime_cef::Cef::default()
        .sandbox(tauri_runtime_cef::SandboxPolicy::Disabled)
        .command_line_arg("no-first-run", None::<String>);

    let builder = tauri::Builder::<tauri_runtime_cef::CefRuntime>::new().runtime(cef);
    register_plugins_and_handlers(builder)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(not(all(feature = "cef", target_os = "linux")))]
fn run_wry() {
    register_plugins_and_handlers(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg_attr(
    all(feature = "cef", target_os = "linux"),
    tauri_runtime_cef::cef_entry_point
)]
pub fn run() {
    #[cfg(all(feature = "cef", target_os = "linux"))]
    run_cef();
    #[cfg(not(all(feature = "cef", target_os = "linux")))]
    run_wry();
}
