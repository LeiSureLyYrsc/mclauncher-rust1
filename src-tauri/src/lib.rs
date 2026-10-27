mod auth;
mod config;
mod download;
mod launch;

use config::{Account, Settings};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

/// 当前下载任务的取消标志（同一时间只允许一个安装任务）。
#[derive(Default)]
struct DownloadState {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

#[tauri::command]
fn get_settings() -> Settings {
    config::load_settings()
}

#[tauri::command]
fn set_settings(settings: Settings) -> Result<Settings, String> {
    let s = settings.normalized();
    config::save_settings(&s)?;
    Ok(s)
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

/// 微软正版登录：占位（暂未开放）。
#[tauri::command]
fn microsoft_login() -> Result<Account, String> {
    auth::microsoft_login_stub()
}

#[tauri::command]
fn switch_account(index: usize) -> Result<Account, String> {
    auth::switch_account(index)
}

#[tauri::command]
fn remove_account(index: usize) -> Result<config::AccountsFile, String> {
    auth::remove_account(index)
}

#[tauri::command]
fn get_download_sources() -> Vec<download::DownloadSourceInfo> {
    download::sources()
}

#[tauri::command]
async fn list_remote_versions() -> Result<Vec<download::RemoteVersion>, String> {
    let s = config::load_settings();
    download::list_remote_versions(&s.download_source).await
}

#[tauri::command]
fn list_local_versions() -> Vec<String> {
    let s = config::load_settings();
    let dir = config::resolve_game_dir(&s);
    download::list_local_versions(&dir)
}

/// 安装版本：进度通过事件 `download-progress` 推送（download::ProgressEvent）。
#[tauri::command]
async fn download_version(
    app: tauri::AppHandle,
    state: State<'_, DownloadState>,
    version_id: String,
) -> Result<download::DownloadResult, String> {
    run_install(app, &state, &version_id, false).await
}

/// 检查并补全游戏文件：使用本地版本 json，只下载缺失/损坏的文件（复用下载对话框与进度事件）。
#[tauri::command]
async fn repair_version(
    app: tauri::AppHandle,
    state: State<'_, DownloadState>,
    version_id: String,
) -> Result<download::DownloadResult, String> {
    run_install(app, &state, &version_id, true).await
}

/// 完整性预检（存在性 + 大小）。
#[tauri::command]
async fn check_version_files(version_id: String) -> Result<download::IntegrityReport, String> {
    let s = config::load_settings();
    let dir = config::resolve_game_dir(&s);
    tauri::async_runtime::spawn_blocking(move || download::check_version_files(&dir, &version_id))
        .await
        .map_err(|e| e.to_string())?
}

async fn run_install(
    app: tauri::AppHandle,
    state: &DownloadState,
    version_id: &str,
    repair: bool,
) -> Result<download::DownloadResult, String> {
    let cancel = {
        let mut slot = state.cancel.lock().unwrap();
        if slot.is_some() {
            return Err("已有下载任务正在进行".into());
        }
        let c = Arc::new(AtomicBool::new(false));
        *slot = Some(c.clone());
        c
    };
    let s = config::load_settings();
    let dir = config::resolve_game_dir(&s);
    let emit: download::Emit = Arc::new(move |ev| {
        let _ = app.emit("download-progress", ev);
    });
    let r = download::install_version(
        version_id,
        &s.download_source,
        &dir,
        s.download_concurrency as usize,
        repair,
        cancel,
        emit,
    )
    .await;
    *state.cancel.lock().unwrap() = None;
    r
}

#[tauri::command]
fn cancel_download(state: State<'_, DownloadState>) -> bool {
    if let Some(c) = state.cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

#[tauri::command]
fn rename_version(old: String, new: String) -> Result<(), String> {
    let s = config::load_settings();
    download::rename_version(&config::resolve_game_dir(&s), &old, &new)
}

#[tauri::command]
fn delete_version(id: String) -> Result<(), String> {
    let s = config::load_settings();
    download::delete_version(&config::resolve_game_dir(&s), &id)
}

#[tauri::command]
fn detect_java() -> launch::JavaInfo {
    let s = config::load_settings();
    launch::detect_java(&s.java_path)
}

#[tauri::command]
async fn scan_java() -> Vec<launch::JavaInfo> {
    tokio::task::spawn_blocking(launch::scan_java)
        .await
        .unwrap_or_default()
}

#[derive(Serialize)]
struct MemoryInfo {
    available_mb: u64,
    auto_mb: u32,
}

#[tauri::command]
fn system_memory() -> MemoryInfo {
    let a = launch::available_memory_mb();
    MemoryInfo {
        available_mb: a,
        auto_mb: launch::auto_memory_mb(a),
    }
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

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    launch::open_path(&config::data_dir())
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
    data_dir: String,
    settings_file: String,
    game_dir: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    let s = config::load_settings();
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: config::data_dir().to_string_lossy().into(),
        settings_file: config::settings_path().to_string_lossy().into(),
        game_dir: config::resolve_game_dir(&s).to_string_lossy().into(),
    }
}

/// 自定义背景：读取本地图片为 data URL（避免开放 asset 协议的文件系统范围）。
#[tauri::command]
fn read_image_data_url(path: String) -> Result<String, String> {
    use base64::Engine;
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("图片过大（>20MB）".into());
    }
    let mime = match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return Err("不支持的图片格式".into()),
    };
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(DownloadState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_accounts,
            offline_login,
            yggdrasil_login,
            logout,
            microsoft_login,
            switch_account,
            remove_account,
            get_download_sources,
            list_remote_versions,
            list_local_versions,
            download_version,
            repair_version,
            check_version_files,
            cancel_download,
            rename_version,
            delete_version,
            detect_java,
            scan_java,
            system_memory,
            launch_game,
            open_game_dir,
            open_data_dir,
            app_info,
            read_image_data_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
