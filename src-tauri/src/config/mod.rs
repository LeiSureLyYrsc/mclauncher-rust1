use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 外置登录（authlib-injector）服务器条目，设置 → 外置登录 中管理。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthServer {
    pub name: String,
    pub api_root: String,
}

/// 启动器全部持久化设置（melody-launcher/settings.json）。
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

/// 启动器根目录（exe / 开发时 cwd）。
pub fn launcher_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 启动器自身数据目录：cwd/melody-launcher（配置、账户、窗口状态、日志、authlib 等）。
pub fn data_dir() -> PathBuf {
    ensure_layout_migrated();
    let d = launcher_root().join("melody-launcher");
    let _ = fs::create_dir_all(&d);
    d
}

/// Minecraft 根目录：cwd/.minecraft（与其他启动器兼容；不放在 melody-launcher 下）。
pub fn minecraft_dir() -> PathBuf {
    ensure_layout_migrated();
    let d = launcher_root().join(".minecraft");
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
    ensure_layout_migrated();
    let p = settings_path();
    if p.exists() {
        if let Ok(t) = fs::read_to_string(&p) {
            if let Ok(s) = serde_json::from_str::<Settings>(&t) {
                return rewrite_legacy_game_dir(s.migrated().normalized());
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
    ensure_layout_migrated();
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
    minecraft_dir()
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

/// 一次性目录布局迁移结果（可观察、可测）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationReport {
    pub ran: bool,
    pub already_done: bool,
    pub moved_minecraft: bool,
    pub copied_minecraft: bool,
    pub moved_files: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub notes: Vec<String>,
}

const MIGRATE_MARKER: &str = ".migrated-to-melody-launcher";

fn paths_equal_loose(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => {
            let as_ = a.to_string_lossy().replace('\\', "/");
            let bs = b.to_string_lossy().replace('\\', "/");
            as_.eq_ignore_ascii_case(&bs)
        }
    }
}

/// 若 settings.game_dir 仍指向旧默认 data/.minecraft，清空以改用新默认 cwd/.minecraft。
fn rewrite_legacy_game_dir(mut s: Settings) -> Settings {
    let g = s.game_dir.trim();
    if g.is_empty() {
        return s;
    }
    let root = launcher_root();
    let legacy = root.join("data").join(".minecraft");
    let p = PathBuf::from(g);
    if paths_equal_loose(&p, &legacy)
        || g.replace('\\', "/").ends_with("/data/.minecraft")
        || g.replace('\\', "/").ends_with("/data/.minecraft/")
    {
        s.game_dir.clear();
    }
    s
}

fn file_mtime(p: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn move_or_copy_dir(src: &Path, dest: &Path, report: &mut MigrationReport) -> bool {
    if dest.exists() {
        report.skipped.push(format!(
            ".minecraft 目标已存在，跳过：{}",
            dest.display()
        ));
        return false;
    }
    match fs::rename(src, dest) {
        Ok(()) => {
            report.moved_minecraft = true;
            report.notes.push(format!(
                "已移动 data/.minecraft → {}",
                dest.display()
            ));
            true
        }
        Err(e) => {
            report.notes.push(format!(
                "rename .minecraft 失败（{}），尝试递归复制",
                e
            ));
            match copy_dir_recursive(src, dest) {
                Ok(()) => {
                    report.copied_minecraft = true;
                    // 复制成功后尽量删掉源，避免双份；失败则留下并记笔记
                    match fs::remove_dir_all(src) {
                        Ok(()) => report.notes.push(
                            "已复制 .minecraft 到新位置并移除旧目录".into(),
                        ),
                        Err(e2) => report.notes.push(format!(
                            "已复制 .minecraft，但未能删除旧目录：{}",
                            e2
                        )),
                    }
                    true
                }
                Err(e2) => {
                    report.errors.push(format!(
                        "移动/复制 .minecraft 失败：{} / {}",
                        e, e2
                    ));
                    let _ = fs::remove_dir_all(dest);
                    false
                }
            }
        }
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for ent in fs::read_dir(src).map_err(|e| e.to_string())? {
        let ent = ent.map_err(|e| e.to_string())?;
        let ty = ent.file_type().map_err(|e| e.to_string())?;
        let to = dest.join(ent.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&ent.path(), &to)?;
        } else {
            fs::copy(ent.path(), &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn migrate_one_file(src: &Path, dest_dir: &Path, report: &mut MigrationReport) {
    let name = match src.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => {
            report.skipped.push(format!("跳过异常文件名：{}", src.display()));
            return;
        }
    };
    if name == MIGRATE_MARKER || name == ".minecraft" {
        return;
    }
    let dest = dest_dir.join(&name);
    if !dest.exists() {
        match fs::rename(src, &dest) {
            Ok(()) => {
                report.moved_files.push(name);
            }
            Err(e) => {
                // cross-device：复制后删源
                match fs::copy(src, &dest).and_then(|_| fs::remove_file(src)) {
                    Ok(()) => report.moved_files.push(name),
                    Err(e2) => report.errors.push(format!(
                        "迁移 {} 失败：{} / {}",
                        name, e, e2
                    )),
                }
            }
        }
        return;
    }
    // 目标已有：保留较新；另一份备份为 *.from-data-bak
    let src_m = file_mtime(src);
    let dest_m = file_mtime(&dest);
    let src_newer = match (src_m, dest_m) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    };
    if src_newer {
        let bak = dest_dir.join(format!("{name}.bak"));
        let _ = fs::rename(&dest, &bak);
        match fs::rename(src, &dest).or_else(|_| {
            fs::copy(src, &dest).and_then(|_| fs::remove_file(src)).map(|_| ())
        }) {
            Ok(()) => {
                report.moved_files.push(format!("{name}（覆盖，旧目标已备份 .bak）"));
                report.notes.push(format!("保留较新的 data/{name}，原 melody-launcher/{name} → .bak"));
            }
            Err(e) => report.errors.push(format!("覆盖迁移 {name} 失败：{e}")),
        }
    } else {
        let bak = dest_dir.join(format!("{name}.from-data-bak"));
        match fs::rename(src, &bak).or_else(|_| {
            fs::copy(src, &bak).and_then(|_| fs::remove_file(src)).map(|_| ())
        }) {
            Ok(()) => {
                report.skipped.push(format!(
                    "保留已有 melody-launcher/{name}，旧文件 → {name}.from-data-bak"
                ));
            }
            Err(e) => report.errors.push(format!("备份旧 {name} 失败：{e}")),
        }
    }
}

/// 在指定 root 下执行旧 `data/` → `.minecraft` + `melody-launcher/` 迁移（可单测）。
pub fn migrate_legacy_layout_at(root: &Path) -> MigrationReport {
    let mut report = MigrationReport::default();
    let legacy = root.join("data");
    let marker = legacy.join(MIGRATE_MARKER);
    if marker.is_file() {
        report.already_done = true;
        report.notes.push("已存在迁移标记，跳过".into());
        return report;
    }
    if !legacy.is_dir() {
        report.notes.push("无旧 data/ 目录，无需迁移".into());
        return report;
    }
    report.ran = true;
    let launcher_data = root.join("melody-launcher");
    let mc = root.join(".minecraft");
    if let Err(e) = fs::create_dir_all(&launcher_data) {
        report.errors.push(format!("创建 melody-launcher 失败：{e}"));
        return report;
    }

    let legacy_mc = legacy.join(".minecraft");
    if legacy_mc.is_dir() {
        let _ = move_or_copy_dir(&legacy_mc, &mc, &mut report);
    }

    match fs::read_dir(&legacy) {
        Ok(rd) => {
            for ent in rd.flatten() {
                let path = ent.path();
                let name = ent.file_name();
                let name_s = name.to_string_lossy();
                if name_s == MIGRATE_MARKER {
                    continue;
                }
                if name_s == ".minecraft" {
                    // 若仍在（移动失败或目标已存在），不往 melody-launcher 塞
                    continue;
                }
                if path.is_dir() {
                    // 子目录整夹迁到 melody-launcher/<name>
                    let dest = launcher_data.join(name_s.as_ref());
                    if dest.exists() {
                        report.skipped.push(format!("目录已存在，跳过：{name_s}"));
                    } else {
                        match fs::rename(&path, &dest) {
                            Ok(()) => report.moved_files.push(name_s.to_string()),
                            Err(e) => match copy_dir_recursive(&path, &dest) {
                                Ok(()) => {
                                    let _ = fs::remove_dir_all(&path);
                                    report.moved_files.push(name_s.to_string());
                                    report.notes.push(format!("目录 {name_s} 经复制迁移（rename 失败：{e}）"));
                                }
                                Err(e2) => report.errors.push(format!(
                                    "迁移目录 {name_s} 失败：{e} / {e2}"
                                )),
                            },
                        }
                    }
                } else if path.is_file() {
                    migrate_one_file(&path, &launcher_data, &mut report);
                }
            }
        }
        Err(e) => report.errors.push(format!("读取 data/ 失败：{e}")),
    }

    // 写迁移标记（即使有部分错误也写，避免反复折腾；错误已记在 report）
    match fs::write(&marker, format!(
        "migrated\nmoved_minecraft={}\ncopied_minecraft={}\nfiles={}\n",
        report.moved_minecraft, report.copied_minecraft, report.moved_files.len()
    )) {
        Ok(()) => report.notes.push(format!("已写入标记 {}", marker.display())),
        Err(e) => report.errors.push(format!("写迁移标记失败：{e}")),
    }

    // 尝试移除空的 data（只在几乎清空时；保留标记文件则不删目录）
    if let Ok(rd) = fs::read_dir(&legacy) {
        let left: Vec<_> = rd.flatten().collect();
        let only_marker = left.len() == 1
            && left[0].file_name().to_string_lossy() == MIGRATE_MARKER;
        if left.is_empty() {
            let _ = fs::remove_dir(&legacy);
            report.notes.push("已移除空的 data/".into());
        } else if only_marker {
            report.notes.push("data/ 仅剩迁移标记，保留目录".into());
        } else {
            report.notes.push(format!(
                "data/ 仍有 {} 项，保留",
                left.len()
            ));
        }
    }

    report
}

/// 对当前 cwd 执行迁移，并 eprintln 摘要（启动时可观察）。
pub fn migrate_legacy_layout() -> MigrationReport {
    let root = launcher_root();
    let report = migrate_legacy_layout_at(&root);
    if report.ran || !report.errors.is_empty() {
        eprintln!(
            "[layout] migration: ran={} moved_mc={} copied_mc={} files={:?} skipped={:?} errors={:?} notes={:?}",
            report.ran,
            report.moved_minecraft,
            report.copied_minecraft,
            report.moved_files,
            report.skipped,
            report.errors,
            report.notes
        );
    }
    report
}

static LAYOUT_ONCE: std::sync::Once = std::sync::Once::new();

/// 进程内只跑一次布局迁移（读配置 / 取路径前调用）。
pub fn ensure_layout_migrated() {
    LAYOUT_ONCE.call_once(|| {
        let _ = migrate_legacy_layout();
    });
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

    #[test]
    fn migrate_legacy_data_layout() {
        let root = std::env::temp_dir().join(format!(
            "mcl-mig-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/.minecraft/versions/1.20.1")).unwrap();
        fs::write(root.join("data/.minecraft/options.txt"), b"old").unwrap();
        fs::write(root.join("data/settings.json"), br#"{"download_source":"bmclapi"}"#).unwrap();
        fs::write(root.join("data/accounts.json"), br#"{"accounts":[]}"#).unwrap();
        fs::write(root.join("data/window-state.json"), br#"{"x":1}"#).unwrap();
        fs::write(root.join("data/输出日志.txt"), b"log").unwrap();
        fs::create_dir_all(root.join("data/extra_dir")).unwrap();
        fs::write(root.join("data/extra_dir/a.txt"), b"a").unwrap();

        let r1 = migrate_legacy_layout_at(&root);
        assert!(r1.ran, "{r1:?}");
        assert!(r1.moved_minecraft || r1.copied_minecraft, "{r1:?}");
        assert!(root.join(".minecraft/options.txt").is_file());
        assert!(root.join(".minecraft/versions/1.20.1").is_dir());
        assert!(!root.join("data/.minecraft").exists());
        assert!(root.join("melody-launcher/settings.json").is_file());
        assert!(root.join("melody-launcher/accounts.json").is_file());
        assert!(root.join("melody-launcher/window-state.json").is_file());
        assert!(root.join("melody-launcher/输出日志.txt").is_file());
        assert!(root.join("melody-launcher/extra_dir/a.txt").is_file());
        assert!(root.join("data").join(MIGRATE_MARKER).is_file());

        // 幂等：再跑一次不应重复搬家
        let r2 = migrate_legacy_layout_at(&root);
        assert!(r2.already_done, "{r2:?}");
        assert!(!r2.ran);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_keeps_existing_minecraft_and_settings() {
        let root = std::env::temp_dir().join(format!(
            "mcl-mig2-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/.minecraft")).unwrap();
        fs::write(root.join("data/.minecraft/from-old.txt"), b"old").unwrap();
        fs::create_dir_all(root.join(".minecraft")).unwrap();
        fs::write(root.join(".minecraft/from-new.txt"), b"new").unwrap();
        // 先写旧 settings，再写目标，保证目标 mtime 更新 → 保留目标
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/settings.json"), b"old-settings").unwrap();
        fs::create_dir_all(root.join("melody-launcher")).unwrap();
        fs::write(root.join("melody-launcher/settings.json"), b"keep-me").unwrap();

        let r = migrate_legacy_layout_at(&root);
        assert!(r.ran, "{r:?}");
        assert!(!r.moved_minecraft && !r.copied_minecraft, "{r:?}");
        assert!(root.join(".minecraft/from-new.txt").is_file());
        assert!(root.join("data/.minecraft/from-old.txt").is_file()); // 目标已存在则跳过
        assert_eq!(
            fs::read_to_string(root.join("melody-launcher/settings.json")).unwrap(),
            "keep-me"
        );
        assert!(
            root.join("melody-launcher/settings.json.from-data-bak").is_file(),
            "{r:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
