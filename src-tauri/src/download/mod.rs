use crate::config;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const OFFICIAL_MANIFEST: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
pub const BMCLAPI_BASE: &str = "https://bmclapi2.bangbang93.com";
pub const BMCLAPI_MANIFEST: &str =
    "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, alias = "releaseTime", rename = "release_time")]
    pub release_time: String,
    #[serde(default)]
    pub sha1: String,
}

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<RemoteVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSourceInfo {
    pub id: String,
    pub name: String,
}

pub fn sources() -> Vec<DownloadSourceInfo> {
    vec![
        DownloadSourceInfo {
            id: "official".into(),
            name: "Mojang 官方源".into(),
        },
        DownloadSourceInfo {
            id: "bmclapi".into(),
            name: "BMCLAPI 镜像源".into(),
        },
    ]
}

fn use_bmclapi(settings_source: &str) -> bool {
    settings_source != "official"
}

pub fn mirror_url(raw: &str, bmcl: bool) -> String {
    if !bmcl || raw.is_empty() {
        return raw.to_string();
    }
    let repls = [
        (
            "https://launchermeta.mojang.com",
            BMCLAPI_BASE,
        ),
        ("http://launchermeta.mojang.com", BMCLAPI_BASE),
        ("https://launcher.mojang.com", BMCLAPI_BASE),
        ("http://launcher.mojang.com", BMCLAPI_BASE),
        ("https://piston-meta.mojang.com", BMCLAPI_BASE),
        ("https://piston-data.mojang.com", BMCLAPI_BASE),
        (
            "https://libraries.minecraft.net/",
            &format!("{BMCLAPI_BASE}/maven/"),
        ),
        (
            "http://libraries.minecraft.net/",
            &format!("{BMCLAPI_BASE}/maven/"),
        ),
        (
            "https://resources.download.minecraft.net/",
            &format!("{BMCLAPI_BASE}/assets/"),
        ),
        (
            "http://resources.download.minecraft.net/",
            &format!("{BMCLAPI_BASE}/assets/"),
        ),
        (
            "https://authlib-injector.yushi.moe/",
            &format!("{BMCLAPI_BASE}/mirrors/authlib-injector/"),
        ),
    ];
    let mut out = raw.to_string();
    for (old, neu) in repls {
        if out.contains(old) {
            out = out.replace(old, neu);
        }
    }
    out
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("mclauncher-rust1/0.1")
        .build()
        .map_err(|e| e.to_string())
}

pub async fn list_remote_versions(source: &str) -> Result<Vec<RemoteVersion>, String> {
    let bmcl = use_bmclapi(source);
    let url = if bmcl {
        BMCLAPI_MANIFEST
    } else {
        OFFICIAL_MANIFEST
    };
    let client = http_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("拉取版本清单失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("版本清单 HTTP {}", resp.status()));
    }
    let m: Manifest = resp
        .json()
        .await
        .map_err(|e| format!("解析版本清单失败: {e}"))?;
    Ok(m.versions
        .into_iter()
        .map(|mut v| {
            // normalize for frontend
            v.type_ = v.type_.clone();
            v
        })
        .collect())
}

pub fn list_local_versions(game_dir: &Path) -> Vec<String> {
    let dir = game_dir.join("versions");
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let jar = e.path().join(format!("{name}.jar"));
        let json = e.path().join(format!("{name}.json"));
        if jar.exists() || json.exists() {
            out.push(name);
        }
    }
    out.sort();
    out.reverse();
    out
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct VersionJson {
    pub id: String,
    #[serde(default, rename = "mainClass")]
    pub main_class: String,
    #[serde(default, rename = "type")]
    pub type_: String,
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub downloads: std::collections::HashMap<String, Artifact>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub assets: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
    pub jvm: Vec<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub natives: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<Extract>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Artifact {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AssetIndex {
    pub id: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub total_size: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct AssetIndexFile {
    objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Deserialize, Clone)]
struct AssetObject {
    hash: String,
    #[serde(default)]
    size: i64,
}

fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub fn library_allowed(rules: &Option<Vec<Rule>>) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    if rules.is_empty() {
        return true;
    }
    let os = current_os_name();
    let mut allow = false;
    for r in rules {
        let matches = match &r.os {
            None => true,
            Some(o) => o.name.as_deref() == Some(os),
        };
        if matches {
            allow = r.action == "allow";
        }
    }
    allow
}

fn maven_path(name: &str) -> String {
    // group:artifact:version[:classifier]
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return name.replace(':', "/");
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    if parts.len() >= 4 {
        let classifier = parts[3];
        format!("{group}/{artifact}/{version}/{artifact}-{version}-{classifier}.jar")
    } else {
        format!("{group}/{artifact}/{version}/{artifact}-{version}.jar")
    }
}

fn file_sha1(path: &Path) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 已存在且（大小、SHA-1）校验通过的文件直接跳过。
pub fn existing_file_ok(dest: &Path, sha1hex: &str, expected_size: i64) -> bool {
    let Ok(meta) = fs::metadata(dest) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    if expected_size > 0 && meta.len() as i64 != expected_size {
        return false;
    }
    if sha1hex.is_empty() {
        // 无哈希可校验时，只要非空即认为可用
        return meta.len() > 0;
    }
    file_sha1(dest)
        .map(|h| h.eq_ignore_ascii_case(sha1hex))
        .unwrap_or(false)
}

/// 简单单文件下载（无进度），供 authlib-injector 等使用。
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha1hex: &str,
    expected_size: i64,
    bmcl: bool,
) -> Result<(), String> {
    if existing_file_ok(dest, sha1hex, expected_size) {
        return Ok(());
    }
    let url = mirror_url(url, bmcl);
    let tracker = Tracker::new(String::new());
    let never = AtomicBool::new(false);
    stream_to_file(client, &url, dest, sha1hex, &tracker, &never).await
}

pub async fn fetch_version_json(
    client: &reqwest::Client,
    info: &RemoteVersion,
    bmcl: bool,
) -> Result<VersionJson, String> {
    let mut candidates = Vec::new();
    if !info.url.is_empty() {
        candidates.push(mirror_url(&info.url, bmcl));
    }
    if bmcl && !info.id.is_empty() {
        if !info.sha1.is_empty() {
            candidates.push(format!(
                "{BMCLAPI_BASE}/v1/packages/{}/{}.json",
                info.sha1, info.id
            ));
        }
        candidates.push(format!("{BMCLAPI_BASE}/version/{}/json", info.id));
    }
    let mut last = "无可用 URL".to_string();
    for u in candidates {
        match client.get(&u).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<VersionJson>().await {
                    Ok(mut v) => {
                        if v.id.is_empty() {
                            v.id = info.id.clone();
                        }
                        return Ok(v);
                    }
                    Err(e) => last = e.to_string(),
                }
            }
            Ok(resp) => last = format!("HTTP {}", resp.status()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("获取版本元数据失败: {last}"))
}

fn native_classifier(lib: &Library) -> Option<String> {
    let natives = lib.natives.as_ref()?;
    let key = current_os_name();
    natives.get(key).cloned().map(|s| {
        s.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" })
    })
}

// ---------------------------------------------------------------------------
// 带进度的并发安装
// ---------------------------------------------------------------------------

/// 下载清单分类（弹窗中按此分组显示）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    VersionJson,
    ClientJar,
    Libraries,
    AssetIndex,
    Assets,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::VersionJson,
        Category::ClientJar,
        Category::Libraries,
        Category::AssetIndex,
        Category::Assets,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Category::VersionJson => "版本 JSON",
            Category::ClientJar => "客户端 jar",
            Category::Libraries => "依赖库",
            Category::AssetIndex => "资源索引",
            Category::Assets => "资源文件",
        }
    }
    fn idx(self) -> usize {
        self as usize
    }
}

/// 一个待下载文件。
#[derive(Debug, Clone)]
pub struct FileTask {
    pub category: Category,
    pub name: String,
    pub url: String,
    /// 官方源原始地址（镜像失败时回退）
    pub fallback_url: String,
    pub dest: PathBuf,
    pub sha1: String,
    pub size: i64,
    /// Some(exclude) 表示该文件是 natives，下载后需解压
    pub natives_exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CategoryStat {
    pub category: String,
    pub label: String,
    pub total: u64,
    pub done: u64,
    pub skipped: u64,
    pub failed: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    /// pending | running | done | failed
    pub state: String,
}

/// 推送给前端的进度事件（事件名 `download-progress`）。
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub version_id: String,
    /// meta | files | natives | done | failed | cancelled
    pub stage: String,
    pub categories: Vec<CategoryStat>,
    pub total_files: u64,
    pub done_files: u64,
    pub skipped_files: u64,
    pub failed_files: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub speed_bps: u64,
    pub current: Vec<String>,
    pub failed: Vec<String>,
    pub message: String,
}

/// 线程安全的进度累计器。
pub struct Tracker {
    version_id: String,
    cats: Mutex<Vec<CategoryStat>>,
    bytes_done: AtomicU64,
    net_bytes: AtomicU64,
    active: Mutex<Vec<String>>,
    failed: Mutex<Vec<String>>,
    stage: Mutex<String>,
}

impl Tracker {
    pub fn new(version_id: String) -> Self {
        let cats = Category::ALL
            .iter()
            .map(|c| CategoryStat {
                category: format!("{:?}", c),
                label: c.label().into(),
                state: "pending".into(),
                ..Default::default()
            })
            .collect();
        Self {
            version_id,
            cats: Mutex::new(cats),
            bytes_done: AtomicU64::new(0),
            net_bytes: AtomicU64::new(0),
            active: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
            stage: Mutex::new("meta".into()),
        }
    }

    pub fn set_stage(&self, s: &str) {
        *self.stage.lock().unwrap() = s.into();
    }

    pub fn plan(&self, tasks: &[FileTask]) {
        let mut cats = self.cats.lock().unwrap();
        for t in tasks {
            let c = &mut cats[t.category.idx()];
            c.total += 1;
            c.bytes_total += t.size.max(0) as u64;
        }
    }

    fn cat_mut<R>(&self, c: Category, f: impl FnOnce(&mut CategoryStat) -> R) -> R {
        let mut cats = self.cats.lock().unwrap();
        f(&mut cats[c.idx()])
    }

    fn start(&self, t: &FileTask) {
        self.cat_mut(t.category, |c| {
            if c.state == "pending" {
                c.state = "running".into();
            }
        });
        self.active.lock().unwrap().push(t.name.clone());
    }

    fn add_bytes(&self, n: u64, cat: Option<Category>) {
        self.bytes_done.fetch_add(n, Ordering::Relaxed);
        self.net_bytes.fetch_add(n, Ordering::Relaxed);
        if let Some(c) = cat {
            self.cat_mut(c, |s| s.bytes_done += n);
        }
    }

    fn rollback_bytes(&self, n: u64, cat: Option<Category>) {
        self.bytes_done.fetch_sub(n, Ordering::Relaxed);
        if let Some(c) = cat {
            self.cat_mut(c, |s| s.bytes_done = s.bytes_done.saturating_sub(n));
        }
    }

    /// result: Ok(true)=已下载, Ok(false)=已存在跳过, Err=失败
    fn finish(&self, t: &FileTask, result: &Result<bool, String>) {
        {
            let mut a = self.active.lock().unwrap();
            if let Some(p) = a.iter().position(|n| n == &t.name) {
                a.remove(p);
            }
        }
        let size = t.size.max(0) as u64;
        let mut cats = self.cats.lock().unwrap();
        let c = &mut cats[t.category.idx()];
        match result {
            Ok(true) => c.done += 1,
            Ok(false) => {
                c.done += 1;
                c.skipped += 1;
                c.bytes_done += size;
                self.bytes_done.fetch_add(size, Ordering::Relaxed);
            }
            Err(e) => {
                c.failed += 1;
                self.failed.lock().unwrap().push(format!("{} ({e})", t.name));
            }
        }
        if c.done + c.failed >= c.total {
            c.state = if c.failed > 0 { "failed" } else { "done" }.into();
        }
    }

    pub fn snapshot(&self, speed_bps: u64, message: &str) -> ProgressEvent {
        let cats = self.cats.lock().unwrap().clone();
        let sum = |f: fn(&CategoryStat) -> u64| cats.iter().map(f).sum::<u64>();
        ProgressEvent {
            version_id: self.version_id.clone(),
            stage: self.stage.lock().unwrap().clone(),
            total_files: sum(|c| c.total),
            done_files: sum(|c| c.done),
            skipped_files: sum(|c| c.skipped),
            failed_files: sum(|c| c.failed),
            bytes_total: sum(|c| c.bytes_total),
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            speed_bps,
            current: self.active.lock().unwrap().iter().take(6).cloned().collect(),
            failed: self.failed.lock().unwrap().clone(),
            categories: cats,
            message: message.into(),
        }
    }

    pub fn net_bytes(&self) -> u64 {
        self.net_bytes.load(Ordering::Relaxed)
    }
}

/// 流式下载到 .part，边写边算 SHA-1，校验通过后改名。
async fn stream_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha1hex: &str,
    tracker: &Tracker,
    cancel: &AtomicBool,
) -> Result<(), String> {
    stream_to_file_cat(client, url, dest, sha1hex, tracker, cancel, None).await
}

async fn stream_to_file_cat(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha1hex: &str,
    tracker: &Tracker,
    cancel: &AtomicBool,
    cat: Option<Category>,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        config::ensure_dir(parent)?;
    }
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let tmp = part_path(dest);
    let mut file = File::create(&tmp).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    let res: Result<(), String> = async {
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            let chunk = chunk.map_err(|e| e.to_string())?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            hasher.update(&chunk);
            received += chunk.len() as u64;
            tracker.add_bytes(chunk.len() as u64, cat);
        }
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        tracker.rollback_bytes(received, cat);
        return Err(e);
    }
    if !sha1hex.is_empty() {
        let h = hex::encode(hasher.finalize());
        if !h.eq_ignore_ascii_case(sha1hex) {
            let _ = fs::remove_file(&tmp);
            tracker.rollback_bytes(received, cat);
            return Err("SHA-1 校验失败".into());
        }
    }
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok(())
}

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

pub const CANCELLED: &str = "已取消";

/// 执行单个任务：存在且校验通过→跳过；否则镜像下载，失败重试并回退官方地址。
async fn run_task(
    client: &reqwest::Client,
    t: &FileTask,
    tracker: &Tracker,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    let (dest, sha, size) = (t.dest.clone(), t.sha1.clone(), t.size);
    let exists = tokio::task::spawn_blocking(move || existing_file_ok(&dest, &sha, size))
        .await
        .unwrap_or(false);
    if exists {
        return Ok(false);
    }
    let mut urls = vec![t.url.clone()];
    if !t.fallback_url.is_empty() && t.fallback_url != t.url {
        urls.push(t.fallback_url.clone());
    }
    let mut last = String::new();
    for attempt in 0..3 {
        let url = &urls[attempt.min(urls.len() - 1)];
        match stream_to_file_cat(client, url, &t.dest, &t.sha1, tracker, cancel, Some(t.category)).await {
            Ok(()) => return Ok(true),
            Err(e) if e == CANCELLED => return Err(e),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// 根据版本 JSON 生成 client jar + libraries(+natives) 任务。
pub fn plan_version_files(
    vj: &VersionJson,
    game_dir: &Path,
    version_id: &str,
    bmcl: bool,
) -> Vec<FileTask> {
    let mut tasks = Vec::new();
    let versions_dir = game_dir.join("versions").join(version_id);
    if let Some(c) = vj.downloads.get("client") {
        if !c.url.is_empty() {
            tasks.push(FileTask {
                category: Category::ClientJar,
                name: format!("{version_id}.jar"),
                url: mirror_url(&c.url, bmcl),
                fallback_url: c.url.clone(),
                dest: versions_dir.join(format!("{version_id}.jar")),
                sha1: c.sha1.clone(),
                size: c.size,
                natives_exclude: None,
            });
        }
    }
    let libs_dir = game_dir.join("libraries");
    let to_dest = |p: &str| libs_dir.join(p.replace('/', std::path::MAIN_SEPARATOR_STR));
    for lib in &vj.libraries {
        if !library_allowed(&lib.rules) {
            continue;
        }
        if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                let path = if art.path.is_empty() {
                    maven_path(&lib.name)
                } else {
                    art.path.clone()
                };
                let raw = if art.url.is_empty() {
                    format!("https://libraries.minecraft.net/{path}")
                } else {
                    art.url.clone()
                };
                tasks.push(FileTask {
                    category: Category::Libraries,
                    name: file_name_of(&path),
                    url: mirror_url(&raw, bmcl),
                    fallback_url: raw,
                    dest: to_dest(&path),
                    sha1: art.sha1.clone(),
                    size: art.size,
                    natives_exclude: None,
                });
            }
            if let (Some(classifier), Some(classifiers)) = (native_classifier(lib), &dl.classifiers) {
                if let Some(nat) = classifiers.get(&classifier) {
                    let path = if nat.path.is_empty() {
                        maven_path(&format!("{}:{}", lib.name, classifier))
                    } else {
                        nat.path.clone()
                    };
                    let raw = if nat.url.is_empty() {
                        format!("https://libraries.minecraft.net/{path}")
                    } else {
                        nat.url.clone()
                    };
                    tasks.push(FileTask {
                        category: Category::Libraries,
                        name: file_name_of(&path),
                        url: mirror_url(&raw, bmcl),
                        fallback_url: raw,
                        dest: to_dest(&path),
                        sha1: nat.sha1.clone(),
                        size: nat.size,
                        natives_exclude: Some(
                            lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default(),
                        ),
                    });
                }
            }
        } else if !lib.name.is_empty() {
            let path = maven_path(&lib.name);
            let raw = match &lib.url {
                Some(u) if u.ends_with(".jar") => u.clone(),
                Some(u) => format!("{}/{path}", u.trim_end_matches('/')),
                None => format!("https://libraries.minecraft.net/{path}"),
            };
            tasks.push(FileTask {
                category: Category::Libraries,
                name: file_name_of(&path),
                url: mirror_url(&raw, bmcl),
                fallback_url: raw,
                dest: to_dest(&path),
                sha1: String::new(),
                size: 0,
                natives_exclude: None,
            });
        }
    }
    dedupe(tasks)
}

/// 由 asset index 生成资源对象任务（相同 hash 只下一次）。
pub fn plan_assets(index_json: &str, game_dir: &Path, bmcl: bool) -> Result<Vec<FileTask>, String> {
    let index: AssetIndexFile =
        serde_json::from_str(index_json).map_err(|e| format!("解析 asset index: {e}"))?;
    let objects_dir = game_dir.join("assets").join("objects");
    let mut names: Vec<(String, AssetObject)> = index.objects.into_iter().collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    let mut tasks = Vec::new();
    for (name, obj) in names {
        if obj.hash.len() < 2 {
            continue;
        }
        let prefix = &obj.hash[..2];
        let raw = format!("https://resources.download.minecraft.net/{prefix}/{}", obj.hash);
        tasks.push(FileTask {
            category: Category::Assets,
            name,
            url: mirror_url(&raw, bmcl),
            fallback_url: raw,
            dest: objects_dir.join(prefix).join(&obj.hash),
            sha1: obj.hash.clone(),
            size: obj.size,
            natives_exclude: None,
        });
    }
    Ok(dedupe(tasks))
}

fn dedupe(tasks: Vec<FileTask>) -> Vec<FileTask> {
    let mut seen = std::collections::HashSet::new();
    tasks
        .into_iter()
        .filter(|t| seen.insert(t.dest.clone()))
        .collect()
}

fn file_name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn extract_natives(zip_path: &Path, dest: &Path, exclude: &[String]) -> Result<(), String> {
    config::ensure_dir(dest)?;
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if exclude.iter().any(|e| name.starts_with(e)) {
            continue;
        }
        if name.ends_with('/') {
            continue;
        }
        let out = dest.join(Path::new(&name).file_name().unwrap_or_default());
        let mut outfile = File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct DownloadResult {
    pub ok: bool,
    pub message: String,
    pub failed: Vec<String>,
}

pub type Emit = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// 安装一个版本：元数据 → 资源索引 → 并发下载全部文件 → 解压 natives。
/// 进度通过 `emit` 周期性推送（约每 150ms 一次），可经 `cancel` 取消。
pub async fn install_version(
    version_id: &str,
    source: &str,
    game_dir: &Path,
    concurrency: usize,
    cancel: Arc<AtomicBool>,
    emit: Emit,
) -> Result<DownloadResult, String> {
    let tracker = Arc::new(Tracker::new(version_id.to_string()));
    let done_flag = Arc::new(AtomicBool::new(false));

    // 进度推送循环：计算瞬时速度（最近一个周期的网络字节）
    let reporter = {
        let tracker = tracker.clone();
        let emit = emit.clone();
        let done_flag = done_flag.clone();
        tokio::spawn(async move {
            let mut last_bytes = tracker.net_bytes();
            let mut last_t = std::time::Instant::now();
            let mut speed: f64 = 0.0;
            while !done_flag.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let now = std::time::Instant::now();
                let b = tracker.net_bytes();
                let dt = now.duration_since(last_t).as_secs_f64().max(0.001);
                let inst = (b.saturating_sub(last_bytes)) as f64 / dt;
                // 指数平滑，避免数字乱跳
                speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                last_bytes = b;
                last_t = now;
                emit(tracker.snapshot(speed as u64, ""));
            }
        })
    };

    let result = install_inner(version_id, source, game_dir, concurrency, &cancel, &tracker).await;
    done_flag.store(true, Ordering::Relaxed);
    let _ = reporter.await;

    let cancelled = cancel.load(Ordering::Relaxed);
    let final_event = match &result {
        _ if cancelled => {
            tracker.set_stage("cancelled");
            tracker.snapshot(0, "下载已取消")
        }
        Ok(r) if r.ok => {
            tracker.set_stage("done");
            tracker.snapshot(0, &r.message)
        }
        Ok(r) => {
            tracker.set_stage("failed");
            tracker.snapshot(0, &r.message)
        }
        Err(e) => {
            tracker.set_stage("failed");
            tracker.snapshot(0, e)
        }
    };
    emit(final_event);
    if cancelled {
        return Err(CANCELLED.into());
    }
    result
}

async fn install_inner(
    version_id: &str,
    source: &str,
    game_dir: &Path,
    concurrency: usize,
    cancel: &Arc<AtomicBool>,
    tracker: &Arc<Tracker>,
) -> Result<DownloadResult, String> {
    let bmcl = use_bmclapi(source);
    let client = http_client()?;
    tracker.set_stage("meta");

    // 1) 版本 JSON
    let vj_task_name = format!("{version_id}.json");
    tracker.plan(&[FileTask {
        category: Category::VersionJson,
        name: vj_task_name.clone(),
        url: String::new(),
        fallback_url: String::new(),
        dest: PathBuf::new(),
        sha1: String::new(),
        size: 0,
        natives_exclude: None,
    }]);
    let vj_stub = FileTask {
        category: Category::VersionJson,
        name: vj_task_name,
        url: String::new(),
        fallback_url: String::new(),
        dest: PathBuf::new(),
        sha1: String::new(),
        size: 0,
        natives_exclude: None,
    };
    tracker.start(&vj_stub);
    let vj = async {
        let versions = list_remote_versions(source).await?;
        let info = versions
            .into_iter()
            .find(|v| v.id == version_id)
            .ok_or_else(|| format!("版本清单中未找到 {version_id}"))?;
        fetch_version_json(&client, &info, bmcl).await
    }
    .await;
    let vj = match vj {
        Ok(v) => {
            tracker.finish(&vj_stub, &Ok(true));
            v
        }
        Err(e) => {
            tracker.finish(&vj_stub, &Err(e.clone()));
            return Err(e);
        }
    };
    let versions_dir = game_dir.join("versions").join(version_id);
    config::ensure_dir(&versions_dir)?;
    let meta = serde_json::to_vec_pretty(&vj).map_err(|e| e.to_string())?;
    fs::write(versions_dir.join(format!("{version_id}.json")), meta).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    // 2) 资源索引
    let mut tasks = plan_version_files(&vj, game_dir, version_id, bmcl);
    if let Some(ai) = &vj.asset_index {
        let raw = if ai.url.is_empty() {
            format!("{BMCLAPI_BASE}/index/{}.json", ai.id)
        } else {
            ai.url.clone()
        };
        let idx_task = FileTask {
            category: Category::AssetIndex,
            name: format!("{}.json", ai.id),
            url: mirror_url(&raw, bmcl),
            fallback_url: raw,
            dest: game_dir.join("assets").join("indexes").join(format!("{}.json", ai.id)),
            sha1: ai.sha1.clone(),
            size: ai.size,
            natives_exclude: None,
        };
        tracker.plan(std::slice::from_ref(&idx_task));
        tracker.start(&idx_task);
        let r = run_task(&client, &idx_task, tracker, cancel).await;
        tracker.finish(&idx_task, &r);
        r?;
        let text = fs::read_to_string(&idx_task.dest).map_err(|e| e.to_string())?;
        tasks.extend(plan_assets(&text, game_dir, bmcl)?);
    }

    // 3) 并发下载
    tracker.plan(&tasks);
    tracker.set_stage("files");
    let conc = concurrency.clamp(1, 64);
    let results: Vec<(FileTask, Result<bool, String>)> = futures_util::stream::iter(tasks.into_iter())
        .map(|t| {
            let client = client.clone();
            let tracker = tracker.clone();
            let cancel = cancel.clone();
            async move {
                tracker.start(&t);
                let r = run_task(&client, &t, &tracker, &cancel).await;
                tracker.finish(&t, &r);
                (t, r)
            }
        })
        .buffer_unordered(conc)
        .collect()
        .await;
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    // 4) natives
    tracker.set_stage("natives");
    let natives_dir = versions_dir.join("natives");
    for (t, r) in &results {
        if let (Some(ex), Ok(_)) = (&t.natives_exclude, r) {
            extract_natives(&t.dest, &natives_dir, ex)
                .map_err(|e| format!("解压 natives {}: {e}", t.name))?;
        }
    }

    let failed: Vec<String> = results
        .iter()
        .filter_map(|(t, r)| r.as_ref().err().map(|e| format!("{} ({e})", t.name)))
        .collect();
    if failed.is_empty() {
        Ok(DownloadResult {
            ok: true,
            message: format!("版本 {version_id} 安装完成"),
            failed,
        })
    } else {
        Ok(DownloadResult {
            ok: false,
            message: format!(
                "以下 {} 个文件更新失败，可再次下载重试",
                failed.len()
            ),
            failed,
        })
    }
}

/// 兼容旧调用：无进度推送的安装。
#[allow(dead_code)]
pub async fn download_version(
    version_id: &str,
    source: &str,
    game_dir: &Path,
) -> Result<DownloadResult, String> {
    install_version(
        version_id,
        source,
        game_dir,
        16,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
}

// ---------------------------------------------------------------------------
// 游戏管理（原版设置「游戏管理」：重命名 / 删除选中游戏）
// ---------------------------------------------------------------------------

fn valid_version_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.trim() == name
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && name != "."
        && name != ".."
}

pub fn rename_version(game_dir: &Path, old: &str, new: &str) -> Result<(), String> {
    if !valid_version_name(old) || !valid_version_name(new) {
        return Err("版本名称不合法".into());
    }
    let vdir = game_dir.join("versions");
    let src = vdir.join(old);
    let dst = vdir.join(new);
    if !src.is_dir() {
        return Err(format!("版本 {old} 不存在"));
    }
    if dst.exists() {
        return Err(format!("版本 {new} 已存在"));
    }
    fs::rename(&src, &dst).map_err(|e| e.to_string())?;
    let oj = dst.join(format!("{old}.json"));
    let nj = dst.join(format!("{new}.json"));
    if oj.exists() {
        let text = fs::read_to_string(&oj).map_err(|e| e.to_string())?;
        let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        v["id"] = serde_json::Value::String(new.to_string());
        fs::write(&nj, serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&oj);
    }
    let ojar = dst.join(format!("{old}.jar"));
    if ojar.exists() {
        fs::rename(&ojar, dst.join(format!("{new}.jar"))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn delete_version(game_dir: &Path, id: &str) -> Result<(), String> {
    if !valid_version_name(id) {
        return Err("版本名称不合法".into());
    }
    let dir = game_dir.join("versions").join(id);
    if !dir.is_dir() {
        return Err(format!("版本 {id} 不存在"));
    }
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}

/// Ensure authlib-injector.jar cached under data/
pub async fn ensure_authlib_jar() -> Result<PathBuf, String> {
    let dest = config::data_dir().join("authlib-injector.jar");
    if dest.exists() && fs::metadata(&dest).map(|m| m.len() > 1000).unwrap_or(false) {
        return Ok(dest);
    }
    let client = http_client()?;
    // Prefer BMCLAPI mirror of latest.json
    let meta_url = format!("{BMCLAPI_BASE}/mirrors/authlib-injector/artifact/latest.json");
    #[derive(Deserialize)]
    struct Latest {
        #[serde(default)]
        download_url: Option<String>,
        #[serde(default)]
        url: Option<String>,
    }
    let meta: Latest = client
        .get(&meta_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let url = meta
        .download_url
        .or(meta.url)
        .unwrap_or_else(|| {
            "https://bmclapi2.bangbang93.com/mirrors/authlib-injector/artifact/latest.jar".into()
        });
    let url = mirror_url(&url, true);
    download_file(&client, &url, &dest, "", 0, false).await?;
    Ok(dest)
}

pub fn load_local_version_json(game_dir: &Path, id: &str) -> Result<VersionJson, String> {
    let p = game_dir.join("versions").join(id).join(format!("{id}.json"));
    let t = fs::read_to_string(&p).map_err(|e| format!("读取版本 json 失败: {e}"))?;
    serde_json::from_str(&t).map_err(|e| format!("解析版本 json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcl-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn mirror_rewrites() {
        assert_eq!(
            mirror_url("https://libraries.minecraft.net/a/b.jar", true),
            "https://bmclapi2.bangbang93.com/maven/a/b.jar"
        );
        assert_eq!(
            mirror_url("https://libraries.minecraft.net/a/b.jar", false),
            "https://libraries.minecraft.net/a/b.jar"
        );
    }

    #[test]
    fn plan_files_and_assets() {
        let vj: VersionJson = serde_json::from_str(r#"{
          "id":"1.12.2","mainClass":"M",
          "downloads":{"client":{"sha1":"aa","size":10,"url":"https://piston-data.mojang.com/c.jar"}},
          "libraries":[
            {"name":"a:b:1","downloads":{"artifact":{"path":"a/b/1/b-1.jar","sha1":"s","size":5,"url":"https://libraries.minecraft.net/a/b/1/b-1.jar"}}},
            {"name":"a:b:1","downloads":{"artifact":{"path":"a/b/1/b-1.jar","sha1":"s","size":5,"url":"https://libraries.minecraft.net/a/b/1/b-1.jar"}}},
            {"name":"x:only-mac:1","rules":[{"action":"allow","os":{"name":"osx"}}],"downloads":{"artifact":{"path":"x/m.jar","sha1":"","size":1,"url":"u"}}},
            {"name":"n:nat:2","natives":{"linux":"natives-linux","windows":"natives-windows","osx":"natives-osx"},
             "downloads":{"classifiers":{"natives-linux":{"path":"n/nat/2/nat-2-natives-linux.jar","sha1":"","size":3,"url":"https://libraries.minecraft.net/n/nat/2/nat-2-natives-linux.jar"},
               "natives-windows":{"path":"n/nat/2/nat-2-natives-windows.jar","sha1":"","size":3,"url":"https://libraries.minecraft.net/n/nat/2/nat-2-natives-windows.jar"},
               "natives-osx":{"path":"n/nat/2/nat-2-natives-osx.jar","sha1":"","size":3,"url":"https://libraries.minecraft.net/n/nat/2/nat-2-natives-osx.jar"}}}}
          ]}"#).unwrap();
        let t = plan_version_files(&vj, Path::new("/g"), "1.12.2", true);
        let jars: Vec<_> = t.iter().filter(|x| x.category == Category::ClientJar).collect();
        assert_eq!(jars.len(), 1);
        assert!(jars[0].url.starts_with(BMCLAPI_BASE));
        assert_eq!(jars[0].fallback_url, "https://piston-data.mojang.com/c.jar");
        let libs: Vec<_> = t.iter().filter(|x| x.category == Category::Libraries).collect();
        // dup removed, mac-only excluded on linux, one native
        let expect = if cfg!(target_os = "macos") { 3 } else { 2 };
        assert_eq!(libs.len(), expect);
        assert_eq!(libs.iter().filter(|x| x.natives_exclude.is_some()).count(), 1);

        let idx = r#"{"objects":{"a.ogg":{"hash":"ab12","size":4},"b.ogg":{"hash":"ab12","size":4},"c.png":{"hash":"cd34","size":7}}}"#;
        let a = plan_assets(idx, Path::new("/g"), false).unwrap();
        assert_eq!(a.len(), 2);
        assert!(a.iter().any(|x| x.url == "https://resources.download.minecraft.net/ab/ab12"));
    }

    #[test]
    fn existing_file_check() {
        let d = tmpdir("exist");
        let f = d.join("x.bin");
        fs::write(&f, b"hello").unwrap();
        let h = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"; // sha1("hello")
        assert!(existing_file_ok(&f, h, 5));
        assert!(existing_file_ok(&f, h, 0));
        assert!(!existing_file_ok(&f, h, 6));
        assert!(!existing_file_ok(&f, "0000", 5));
        assert!(!existing_file_ok(&d.join("nope"), h, 5));
    }

    #[test]
    fn rename_and_delete() {
        let g = tmpdir("ren");
        let v = g.join("versions").join("1.20.1");
        fs::create_dir_all(&v).unwrap();
        fs::write(v.join("1.20.1.json"), r#"{"id":"1.20.1","mainClass":"M"}"#).unwrap();
        fs::write(v.join("1.20.1.jar"), b"jar").unwrap();
        assert!(rename_version(&g, "1.20.1", "../evil").is_err());
        rename_version(&g, "1.20.1", "my-pack").unwrap();
        let nv = g.join("versions").join("my-pack");
        assert!(nv.join("my-pack.jar").exists());
        let j: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(nv.join("my-pack.json")).unwrap()).unwrap();
        assert_eq!(j["id"], "my-pack");
        assert_eq!(list_local_versions(&g), vec!["my-pack".to_string()]);
        delete_version(&g, "my-pack").unwrap();
        assert!(list_local_versions(&g).is_empty());
    }

    /// 本地 HTTP 服务器验证：并发下载 + SHA-1 校验 + 跳过已存在 + 失败计数。
    #[tokio::test]
    async fn tasks_download_verify_skip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let body: &[u8] = if req.starts_with("GET /hello") { b"hello" } else { b"bad!!" };
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(body).await;
                });
            }
        });
        let d = tmpdir("dl");
        let h = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let mk = |name: &str, path: &str, sha: &str| FileTask {
            category: Category::Assets,
            name: name.into(),
            url: format!("http://{addr}/{path}"),
            fallback_url: String::new(),
            dest: d.join(name),
            sha1: sha.into(),
            size: 5,
            natives_exclude: None,
        };
        let tasks = vec![mk("ok1", "hello", h), mk("ok2", "hello", h), mk("bad", "other", h)];
        let tracker = Tracker::new("t".into());
        tracker.plan(&tasks);
        let client = http_client().unwrap();
        let cancel = AtomicBool::new(false);
        for t in &tasks {
            tracker.start(t);
            let r = run_task(&client, t, &tracker, &cancel).await;
            tracker.finish(t, &r);
        }
        let s = tracker.snapshot(0, "");
        assert_eq!((s.done_files, s.failed_files, s.skipped_files), (2, 1, 0));
        assert_eq!(fs::read(d.join("ok1")).unwrap(), b"hello");
        assert!(!d.join("bad").exists());
        assert!(!d.join("bad.part").exists());
        assert_eq!(s.bytes_done, 10);
        // second run: skipped
        let t2 = Tracker::new("t".into());
        t2.plan(&tasks[..1]);
        let r = run_task(&client, &tasks[0], &t2, &cancel).await;
        assert_eq!(r, Ok(false));
        // cancel
        cancel.store(true, Ordering::Relaxed);
        let r = run_task(&client, &tasks[2], &t2, &cancel).await;
        assert_eq!(r, Err(CANCELLED.to_string()));
    }
}
