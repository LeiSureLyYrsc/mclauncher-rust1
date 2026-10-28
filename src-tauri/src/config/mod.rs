use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 外置登录（authlib-injector）服务器条目，设置 → 外置登录 中管理。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthServer {
    pub name: String,
    pub api_root: String,
}

/// 启动器全部持久化设置（data/settings.json）。
/// 所有字段都带 serde default，旧版 settings.json 可无损读入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // ---- 下载 ----
    #[serde(default = "default_source")]
    pub download_source: String,
    #[serde(default = "default_concurrency")]
    pub download_concurrency: u32,

    // ---- 游戏 ----
    #[serde(default)]
    pub game_dir: String,
    /// 版本隔离（默认开启）：true = 存档/模组/配置保存到 .minecraft\versions\版本名称\；
    /// false = 共享 .minecraft 根目录。assets / libraries / versions 始终共享。
    #[serde(default = "default_true")]
    pub version_isolation: bool,
    /// 设置文件结构版本（用于一次性迁移）。
    #[serde(default)]
    pub settings_rev: u32,
    #[serde(default = "default_width")]
    pub window_width: u32,
    #[serde(default = "default_height")]
    pub window_height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    /// 启动后行为：keep | minimize | close
    #[serde(default = "default_after_launch")]
    pub after_launch: String,
    /// 原版「调试模式」：以 java.exe 启动并把输出写入 输出日志.txt
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub selected_version: String,

    // ---- Java ----
    #[serde(default)]
    pub java_path: String,
    /// 原版「最大内存」：自动设置 / 手动
    #[serde(default = "default_true")]
    pub memory_auto: bool,
    #[serde(default = "default_mem")]
    pub max_memory_mb: u32,
    #[serde(default)]
    pub jvm_args: String,

    // ---- 外观（原版「启动器皮肤」：随机皮肤 / 默认皮肤，另加自定义图片）----
    #[serde(default = "default_skin")]
    pub skin: String,
    #[serde(default)]
    pub background_path: String,

    // ---- 外置登录服务器 ----
    #[serde(default)]
    pub auth_servers: Vec<AuthServer>,
}

fn default_source() -> String {
    "bmclapi".into()
}
fn default_concurrency() -> u32 {
    32
}
fn default_mem() -> u32 {
    2048
}
fn default_width() -> u32 {
    854
}
fn default_height() -> u32 {
    480
}
fn default_after_launch() -> String {
    "keep".into()
}
fn default_true() -> bool {
    true
}
/// 当前设置结构版本：2 = 引入「版本隔离默认开启」。
pub const SETTINGS_REV: u32 = 2;
fn default_skin() -> String {
    "default".into()
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str("{}").expect("all fields have defaults")
    }
}

impl Settings {
    /// 一次性迁移：旧版设置（rev < 2）中的 version_isolation=false 是旧默认值而非用户选择，
    /// 迁移为新默认（开启）。只改设置，不移动/删除任何游戏数据；用户可随时在设置中关闭。
    pub fn migrated(mut self) -> Self {
        if self.settings_rev < 2 {
            self.version_isolation = true;
        }
        self.settings_rev = SETTINGS_REV;
        self
    }

    /// 规范化：把越界值拉回合理范围（UI 与手改 json 都走这里）。
    pub fn normalized(mut self) -> Self {
        self.download_concurrency = self.download_concurrency.clamp(1, 64);
        if self.max_memory_mb < 256 {
            self.max_memory_mb = default_mem();
        }
        if self.window_width < 320 {
            self.window_width = default_width();
        }
        if self.window_height < 240 {
            self.window_height = default_height();
        }
        if !matches!(self.after_launch.as_str(), "keep" | "minimize" | "close") {
            self.after_launch = default_after_launch();
        }
        if !matches!(self.download_source.as_str(), "official" | "bmclapi") {
            self.download_source = default_source();
        }
        if !matches!(self.skin.as_str(), "default" | "random" | "custom") {
            self.skin = default_skin();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            if let Ok(s) = serde_json::from_str::<Settings>(&t) {
                return s.migrated().normalized();
            }
        }
    }
    Settings::default().migrated()
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    // 由 UI 保存的设置均为当前结构（避免下次加载时再次迁移覆盖用户选择）
    let mut s = s.clone();
    s.settings_rev = SETTINGS_REV;
    let t = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
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

/// 游戏运行目录（--gameDir）：开启版本隔离时为 versions/<id>，否则为 .minecraft 根。
pub fn run_dir(game_dir: &Path, version_id: &str, isolation: bool) -> PathBuf {
    if isolation {
        game_dir.join("versions").join(version_id)
    } else {
        game_dir.to_path_buf()
    }
}

pub fn ensure_dir(p: &Path) -> Result<(), String> {
    fs::create_dir_all(p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_json_still_loads() {
        let s: Settings = serde_json::from_str(
            r#"{"download_source":"official","game_dir":"X","java_path":"","max_memory_mb":3000,"selected_version":"1.20.1"}"#,
        )
        .unwrap();
        assert_eq!(s.download_source, "official");
        assert_eq!(s.max_memory_mb, 3000);
        assert_eq!(s.download_concurrency, 32);
        assert!(s.memory_auto);
        assert_eq!(s.after_launch, "keep");
        assert_eq!((s.window_width, s.window_height), (854, 480));
    }

    #[test]
    fn normalize_clamps() {
        let mut s = Settings::default();
        s.download_concurrency = 999;
        s.after_launch = "weird".into();
        s.max_memory_mb = 10;
        let s = s.normalized();
        assert_eq!(s.download_concurrency, 64);
        assert_eq!(s.after_launch, "keep");
        assert_eq!(s.max_memory_mb, 2048);
    }

    #[test]
    fn isolation_migration() {
        // 旧文件（无 rev，保存过旧默认 false）→ 迁移为开启
        let s: Settings = serde_json::from_str(r#"{"version_isolation":false}"#).unwrap();
        let s = s.migrated();
        assert!(s.version_isolation);
        assert_eq!(s.settings_rev, SETTINGS_REV);
        // 新文件中用户显式关闭 → 保持关闭
        let s: Settings =
            serde_json::from_str(r#"{"version_isolation":false,"settings_rev":2}"#).unwrap();
        assert!(!s.migrated().version_isolation);
        // 缺字段 → 默认开启
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.version_isolation);
    }

    #[test]
    fn run_dir_isolation() {
        let g = Path::new("/mc");
        assert_eq!(run_dir(g, "1.20.1", false), PathBuf::from("/mc"));
        assert_eq!(run_dir(g, "1.20.1", true), PathBuf::from("/mc/versions/1.20.1"));
    }
}
