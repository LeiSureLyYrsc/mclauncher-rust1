use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_source")]
    pub download_source: String,
    #[serde(default)]
    pub game_dir: String,
    #[serde(default)]
    pub java_path: String,
    #[serde(default = "default_mem")]
    pub max_memory_mb: u32,
    #[serde(default)]
    pub selected_version: String,
}

fn default_source() -> String {
    "bmclapi".into()
}
fn default_mem() -> u32 {
    2048
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_source: default_source(),
            game_dir: String::new(),
            java_path: String::new(),
            max_memory_mb: default_mem(),
            selected_version: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub mode: String, // offline | authlib
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    #[serde(default)]
    pub client_token: String,
    #[serde(default = "default_user_type")]
    pub user_type: String,
    #[serde(default)]
    pub authlib_api: String,
}

fn default_user_type() -> String {
    "legacy".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountsFile {
    #[serde(default)]
    pub accounts: Vec<Account>,
    pub current: Option<Account>,
}

pub fn data_dir() -> PathBuf {
    // Prefer cwd/data for portable + dev; fall back beside exe.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let d = cwd.join("data");
    let _ = fs::create_dir_all(&d);
    d
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn accounts_path() -> PathBuf {
    data_dir().join("accounts.json")
}

pub fn load_settings() -> Settings {
    let p = settings_path();
    if p.exists() {
        if let Ok(t) = fs::read_to_string(&p) {
            if let Ok(s) = serde_json::from_str(&t) {
                return s;
            }
        }
    }
    Settings::default()
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    let t = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    fs::write(settings_path(), t).map_err(|e| e.to_string())
}

pub fn load_accounts() -> AccountsFile {
    let p = accounts_path();
    if p.exists() {
        if let Ok(t) = fs::read_to_string(&p) {
            if let Ok(a) = serde_json::from_str(&t) {
                return a;
            }
        }
    }
    AccountsFile::default()
}

pub fn save_accounts(a: &AccountsFile) -> Result<(), String> {
    let t = serde_json::to_string_pretty(a).map_err(|e| e.to_string())?;
    fs::write(accounts_path(), t).map_err(|e| e.to_string())
}

pub fn resolve_game_dir(settings: &Settings) -> PathBuf {
    if !settings.game_dir.trim().is_empty() {
        return PathBuf::from(settings.game_dir.trim());
    }
    // Dev / portable: data/.minecraft
    let d = data_dir().join(".minecraft");
    let _ = fs::create_dir_all(&d);
    d
}

pub fn ensure_dir(p: &Path) -> Result<(), String> {
    fs::create_dir_all(p).map_err(|e| e.to_string())
}
