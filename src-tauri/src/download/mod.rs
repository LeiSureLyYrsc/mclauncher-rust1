use crate::config;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

const USER_AGENT: &str = "mclauncher-rust1/0.1";

/// 公共连接参数：连接 10s 超时、连接池（每主机保留 64 条空闲连接，90s 回收）、
/// TCP_NODELAY、TCP keepalive；启用 HTTP/2（TLS ALPN 协商，服务器支持时多路复用）。
fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(60))
        .http2_adaptive_window(true)
}

/// 元数据 / 列表等小请求：保留 60s 总超时，允许 gzip。
fn http_client() -> Result<reqwest::Client, String> {
    client_builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// 文件下载：不设总超时（慢网下大文件不会被误杀），改用读空闲超时；
/// 关闭 gzip，保证 Content-Length / Range 以原始字节计、SHA-1 针对原文件。
fn download_client(idle: Duration) -> Result<reqwest::Client, String> {
    client_builder()
        .read_timeout(idle)
        .no_gzip()
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
    #[serde(default, rename = "totalSize", alias = "total_size")]
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
async fn download_file(url: &str, dest: &Path, sha1hex: &str, expected_size: i64, bmcl: bool) -> Result<(), String> {
    if existing_file_ok(dest, sha1hex, expected_size) {
        return Ok(());
    }
    let opts = DlOptions::new(4);
    let dl = Downloader::new(
        download_client(opts.idle_timeout)?,
        opts,
        Arc::new(Tracker::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let task = FileTask {
        category: Category::Libraries,
        name: file_name_of(&dest.to_string_lossy()),
        url: mirror_url(url, bmcl),
        fallback_url: String::new(),
        dest: dest.to_path_buf(),
        sha1: sha1hex.to_string(),
        size: expected_size,
        natives_exclude: None,
    };
    dl.run_task(&task).await.map(|_| ())
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
    /// 预占的各类别字节（展开实际任务前先计入总量，避免进度回退）
    reserved: Mutex<[u64; 5]>,
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
            reserved: Mutex::new([0; 5]),
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

    /// 预占某类别的字节总量（实际任务展开前）。
    pub fn reserve_bytes(&self, c: Category, n: u64) {
        self.reserved.lock().unwrap()[c.idx()] += n;
        self.cat_mut(c, |s| s.bytes_total += n);
    }

    /// 撤销预占（随后 plan 实际任务）。
    pub fn unreserve(&self, c: Category) {
        let n = std::mem::take(&mut self.reserved.lock().unwrap()[c.idx()]);
        self.cat_mut(c, |s| s.bytes_total = s.bytes_total.saturating_sub(n));
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

// ---------------------------------------------------------------------------
// 下载引擎：共享连接预算（信号量）、大文件 Range 分片、空闲超时、可立即取消、
// 指数退避 + 抖动重试、遇 429/5xx 自动临时降并发。
// ---------------------------------------------------------------------------

/// 默认并发连接数（设置可调 1–64）。
pub const DEFAULT_CONCURRENCY: usize = 32;

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

pub const CANCELLED: &str = "已取消";

/// 下载参数。
#[derive(Clone, Debug)]
pub struct DlOptions {
    /// 同时进行的 HTTP 连接上限（分片也计入）。
    pub concurrency: usize,
    /// 读空闲超时：等待响应头或两次数据块之间超过该时长即判失败。
    pub idle_timeout: Duration,
    /// 已知大小 ≥ 该值的文件尝试分片。
    pub split_threshold: u64,
    /// 单文件最多分片数。
    pub max_parts: usize,
    /// 每片最小字节数（自适应片数 = min(max_parts, size / min_part_size)）。
    pub min_part_size: u64,
    /// 每个文件（及每个分片）的最多尝试次数。
    pub attempts: usize,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
}

impl DlOptions {
    pub fn new(concurrency: usize) -> Self {
        Self {
            concurrency: concurrency.clamp(1, 64),
            idle_timeout: Duration::from_secs(30),
            split_threshold: 8 * 1024 * 1024,
            max_parts: 4,
            min_part_size: 2 * 1024 * 1024,
            attempts: 3,
            backoff_base: Duration::from_millis(500),
            backoff_max: Duration::from_secs(8),
        }
    }
}

#[derive(Debug)]
enum DlErr {
    Cancelled,
    /// HTTP 状态码 + Retry-After
    Status(u16, Option<Duration>),
    /// 服务器不支持（或不一致地支持）Range，需回退单连接
    NoRange,
    Msg(String),
}

impl DlErr {
    fn text(&self) -> String {
        match self {
            DlErr::Cancelled => CANCELLED.into(),
            DlErr::Status(c, _) => format!("HTTP {c}"),
            DlErr::NoRange => "服务器不支持分段下载".into(),
            DlErr::Msg(m) => m.clone(),
        }
    }
    fn retry_after(&self) -> Option<Duration> {
        match self {
            DlErr::Status(_, r) => *r,
            _ => None,
        }
    }
}

fn io_err(e: std::io::Error) -> DlErr {
    DlErr::Msg(e.to_string())
}

/// 解析 `Content-Range: bytes a-b/total` → (a, total)
fn content_range(resp: &reqwest::Response) -> Option<(u64, Option<u64>)> {
    let v = resp.headers().get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (a, _b) = range.split_once('-')?;
    Some((a.trim().parse().ok()?, total.trim().parse().ok()))
}

pub struct Downloader {
    client: reqwest::Client,
    sem: Arc<Semaphore>,
    opts: DlOptions,
    tracker: Arc<Tracker>,
    cancel: Arc<AtomicBool>,
    /// 因 429/5xx 暂时收回的连接数
    reduced: AtomicUsize,
    ok_streak: AtomicUsize,
    last_overload: Mutex<Option<Instant>>,
    seed: AtomicU64,
}

impl Downloader {
    pub fn new(
        client: reqwest::Client,
        opts: DlOptions,
        tracker: Arc<Tracker>,
        cancel: Arc<AtomicBool>,
    ) -> Arc<Self> {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9)
            | 1;
        Arc::new(Self {
            client,
            sem: Arc::new(Semaphore::new(opts.concurrency)),
            opts,
            tracker,
            cancel,
            reduced: AtomicUsize::new(0),
            ok_streak: AtomicUsize::new(0),
            last_overload: Mutex::new(None),
            seed: AtomicU64::new(seed),
        })
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 取消标志置位后 ≤50ms 返回，用于 select! 让所有等待点都能立即取消。
    async fn wait_cancel(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn sleep(&self, d: Duration) -> Result<(), DlErr> {
        tokio::select! {
            biased;
            _ = self.wait_cancel() => Err(DlErr::Cancelled),
            _ = tokio::time::sleep(d) => Ok(()),
        }
    }

    fn rand01(&self) -> f64 {
        // xorshift64，抖动用，无需密码学强度
        let mut x = self.seed.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed.store(x, Ordering::Relaxed);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 第 n 次重试前的等待：base·2^n（封顶 backoff_max），乘 [0.5,1) 抖动；Retry-After 取较大者（≤30s）。
    fn backoff(&self, n: usize, hint: Option<Duration>) -> Duration {
        let exp = self
            .opts
            .backoff_base
            .saturating_mul(1u32 << n.min(10))
            .min(self.opts.backoff_max);
        let d = exp.mul_f64(0.5 + 0.5 * self.rand01());
        match hint {
            Some(h) => d.max(h.min(Duration::from_secs(30))),
            None => d,
        }
    }

    /// 当前有效并发（测试/诊断用）
    #[allow(dead_code)]
    pub fn effective_concurrency(&self) -> usize {
        self.opts.concurrency - self.reduced.load(Ordering::SeqCst)
    }

    /// 429/5xx：收回该连接的许可（降并发 1，下限为设定值的 1/4）。
    fn on_overload(&self, permit: OwnedSemaphorePermit) {
        *self.last_overload.lock().unwrap() = Some(Instant::now());
        self.ok_streak.store(0, Ordering::Relaxed);
        let floor = (self.opts.concurrency / 4).max(1);
        let cur = self.reduced.load(Ordering::SeqCst);
        if self.opts.concurrency - cur > floor
            && self
                .reduced
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            permit.forget();
        }
    }

    /// 连续成功 16 个文件且距上次过载 ≥3s → 归还 1 个连接。
    fn on_success(&self) {
        let streak = self.ok_streak.fetch_add(1, Ordering::Relaxed) + 1;
        let cur = self.reduced.load(Ordering::SeqCst);
        if cur == 0 || streak < 16 {
            return;
        }
        let calm = self
            .last_overload
            .lock()
            .unwrap()
            .map(|t| t.elapsed() >= Duration::from_secs(3))
            .unwrap_or(true);
        if calm
            && self
                .reduced
                .compare_exchange(cur, cur - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            self.ok_streak.store(0, Ordering::Relaxed);
            self.sem.add_permits(1);
        }
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, DlErr> {
        tokio::select! {
            biased;
            _ = self.wait_cancel() => Err(DlErr::Cancelled),
            p = self.sem.clone().acquire_owned() => p.map_err(|_| DlErr::Msg("下载已结束".into())),
        }
    }

    /// 占用一个连接许可发起 GET；成功返回 (响应, 许可)，许可随响应体读取结束释放。
    async fn send(
        &self,
        url: &str,
        range: Option<(u64, u64)>,
    ) -> Result<(reqwest::Response, OwnedSemaphorePermit), DlErr> {
        let permit = self.acquire().await?;
        let mut req = self.client.get(url);
        if let Some((a, b)) = range {
            req = req.header(reqwest::header::RANGE, format!("bytes={a}-{b}"));
        }
        let idle = self.opts.idle_timeout;
        let resp = tokio::select! {
            biased;
            _ = self.wait_cancel() => return Err(DlErr::Cancelled),
            r = tokio::time::timeout(idle, req.send()) => match r {
                Err(_) => return Err(DlErr::Msg(format!("等待响应超时（{:.1}s）", idle.as_secs_f32()))),
                Ok(Err(e)) => return Err(DlErr::Msg(reqwest_err_text(&e))),
                Ok(Ok(r)) => r,
            },
        };
        let st = resp.status();
        if st.is_success() {
            return Ok((resp, permit));
        }
        let code = st.as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        if code == 429 || code >= 500 {
            self.on_overload(permit);
        }
        Err(DlErr::Status(code, retry_after))
    }

    /// 逐块读取响应体：每块等待受 idle_timeout 约束，取消立即生效。
    async fn pump(
        &self,
        resp: &mut reqwest::Response,
        mut sink: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), DlErr> {
        let idle = self.opts.idle_timeout;
        loop {
            let next = tokio::select! {
                biased;
                _ = self.wait_cancel() => return Err(DlErr::Cancelled),
                r = tokio::time::timeout(idle, resp.chunk()) => r,
            };
            match next {
                Err(_) => {
                    return Err(DlErr::Msg(format!(
                        "读取超时（{:.1}s 无数据）",
                        idle.as_secs_f32()
                    )))
                }
                Ok(Err(e)) => return Err(DlErr::Msg(reqwest_err_text(&e))),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(b))) => sink(&b).map_err(DlErr::Msg)?,
            }
        }
    }

    /// 单连接整流下载到 .part，边写边算 SHA-1，校验通过后改名。
    async fn stream_whole(
        &self,
        mut resp: reqwest::Response,
        permit: OwnedSemaphorePermit,
        t: &FileTask,
    ) -> Result<(), DlErr> {
        let tmp = part_path(&t.dest);
        let mut file = File::create(&tmp).map_err(io_err)?;
        let mut hasher = Sha1::new();
        let mut received: u64 = 0;
        let cat = Some(t.category);
        let tracker = &self.tracker;
        let r = self
            .pump(&mut resp, |b| {
                file.write_all(b).map_err(|e| e.to_string())?;
                hasher.update(b);
                received += b.len() as u64;
                tracker.add_bytes(b.len() as u64, cat);
                Ok(())
            })
            .await;
        drop(permit);
        drop(file);
        let fail = |e: DlErr| {
            let _ = fs::remove_file(&tmp);
            tracker.rollback_bytes(received, cat);
            Err(e)
        };
        if let Err(e) = r {
            return fail(e);
        }
        if t.sha1.is_empty() {
            if t.size > 0 && received != t.size as u64 {
                return fail(DlErr::Msg(format!("文件大小不符（{received}/{}）", t.size)));
            }
        } else {
            let h = hex::encode(hasher.finalize());
            if !h.eq_ignore_ascii_case(&t.sha1) {
                return fail(DlErr::Msg("SHA-1 校验失败".into()));
            }
        }
        if let Err(e) = replace_file(&tmp, &t.dest) {
            return fail(DlErr::Msg(e));
        }
        Ok(())
    }

    async fn fetch_whole(&self, url: &str, t: &FileTask) -> Result<(), DlErr> {
        let (resp, permit) = self.send(url, None).await?;
        self.stream_whole(resp, permit, t).await
    }

    /// 下载一个分片 [start, end]，写入预分配文件对应偏移；失败按已写位置续传重试。
    #[allow(clippy::too_many_arguments)]
    async fn fetch_range(
        &self,
        url: &str,
        path: &Path,
        start: u64,
        end: u64,
        cat: Category,
        mut first: Option<(reqwest::Response, OwnedSemaphorePermit)>,
        written: &AtomicU64,
    ) -> Result<(), DlErr> {
        use std::io::{Seek, SeekFrom};
        let limit = end + 1;
        let mut pos = start;
        let mut attempt = 0usize;
        loop {
            let res: Result<(), DlErr> = async {
                let (mut resp, _permit) = match first.take() {
                    Some(x) => x,
                    None => self.send(url, Some((pos, end))).await?,
                };
                if resp.status().as_u16() != 206 {
                    return Err(DlErr::NoRange);
                }
                match content_range(&resp) {
                    Some((s, _)) if s == pos => {}
                    _ => return Err(DlErr::NoRange),
                }
                let mut f = fs::OpenOptions::new().write(true).open(path).map_err(io_err)?;
                f.seek(SeekFrom::Start(pos)).map_err(io_err)?;
                let tracker = &self.tracker;
                let pos_ref = &mut pos;
                self.pump(&mut resp, |b| {
                    let n = b.len() as u64;
                    if *pos_ref + n > limit {
                        return Err("服务器返回的数据超出分段范围".into());
                    }
                    f.write_all(b).map_err(|e| e.to_string())?;
                    *pos_ref += n;
                    written.fetch_add(n, Ordering::Relaxed);
                    tracker.add_bytes(n, Some(cat));
                    Ok(())
                })
                .await?;
                if pos != limit {
                    return Err(DlErr::Msg("分段数据不完整（连接提前关闭）".into()));
                }
                Ok(())
            }
            .await;
            match res {
                Ok(()) => return Ok(()),
                Err(e @ (DlErr::Cancelled | DlErr::NoRange)) => return Err(e),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.opts.attempts.max(1) {
                        return Err(e);
                    }
                    self.sleep(self.backoff(attempt - 1, e.retry_after())).await?;
                }
            }
        }
    }

    /// 大文件分片：首片请求兼作探测（206 → 并行其余片；200 → 直接用这条连接整流）。
    async fn fetch_split(&self, url: &str, t: &FileTask, size: u64) -> Result<(), DlErr> {
        let parts = (size / self.opts.min_part_size.max(1)).clamp(1, self.opts.max_parts as u64) as usize;
        if parts < 2 {
            return Err(DlErr::NoRange);
        }
        let ranges: Vec<(u64, u64)> = (0..parts as u64)
            .map(|i| (size * i / parts as u64, size * (i + 1) / parts as u64 - 1))
            .collect();
        let (resp0, permit0) = self.send(url, Some(ranges[0])).await?;
        if resp0.status().as_u16() != 206 {
            return self.stream_whole(resp0, permit0, t).await;
        }
        match content_range(&resp0) {
            Some((0, Some(total))) if total == size => {}
            _ => return Err(DlErr::NoRange),
        }
        let tmp = part_path(&t.dest);
        {
            let f = File::create(&tmp).map_err(io_err)?;
            f.set_len(size).map_err(io_err)?;
        }
        let written = AtomicU64::new(0);
        let mut first = Some((resp0, permit0));
        let mut futs = Vec::with_capacity(parts);
        for (i, &(a, b)) in ranges.iter().enumerate() {
            let f0 = if i == 0 { first.take() } else { None };
            futs.push(self.fetch_range(url, &tmp, a, b, t.category, f0, &written));
        }
        let r = futures_util::future::try_join_all(futs).await;
        let cat = Some(t.category);
        let fail = |e: DlErr| {
            let _ = fs::remove_file(&tmp);
            self.tracker.rollback_bytes(written.load(Ordering::Relaxed), cat);
            Err(e)
        };
        if let Err(e) = r {
            return fail(e);
        }
        if !t.sha1.is_empty() {
            let p = tmp.clone();
            let h = tokio::task::spawn_blocking(move || file_sha1(&p))
                .await
                .map_err(|e| DlErr::Msg(e.to_string()))
                .and_then(|r| r.map_err(DlErr::Msg));
            match h {
                Ok(h) if h.eq_ignore_ascii_case(&t.sha1) => {}
                Ok(_) => return fail(DlErr::Msg("SHA-1 校验失败".into())),
                Err(e) => return fail(e),
            }
        }
        if let Err(e) = replace_file(&tmp, &t.dest) {
            return fail(DlErr::Msg(e));
        }
        Ok(())
    }

    async fn fetch_file(&self, url: &str, t: &FileTask) -> Result<(), DlErr> {
        if let Some(parent) = t.dest.parent() {
            config::ensure_dir(parent).map_err(DlErr::Msg)?;
        }
        let size = t.size.max(0) as u64;
        if self.opts.max_parts > 1 && size >= self.opts.split_threshold {
            match self.fetch_split(url, t, size).await {
                Err(DlErr::NoRange) => {}
                other => return other,
            }
        }
        self.fetch_whole(url, t).await
    }

    /// 执行单个任务：存在且校验通过→跳过；否则镜像下载，失败退避重试并回退官方地址。
    /// Ok(true)=已下载, Ok(false)=已存在跳过。
    pub async fn run_task(&self, t: &FileTask) -> Result<bool, String> {
        if self.is_cancelled() {
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
        let mut last = DlErr::Msg("未尝试".into());
        for attempt in 0..self.opts.attempts.max(1) {
            if attempt > 0 {
                if let Err(e) = self.sleep(self.backoff(attempt - 1, last.retry_after())).await {
                    return Err(e.text());
                }
            }
            let url = &urls[attempt.min(urls.len() - 1)];
            match self.fetch_file(url, t).await {
                Ok(()) => {
                    self.on_success();
                    return Ok(true);
                }
                Err(DlErr::Cancelled) => return Err(CANCELLED.into()),
                Err(e) => last = e,
            }
        }
        Err(last.text())
    }

    async fn run_tracked(&self, t: FileTask) -> (FileTask, Result<bool, String>) {
        self.tracker.start(&t);
        let r = self.run_task(&t).await;
        self.tracker.finish(&t, &r);
        (t, r)
    }

    /// 并发下载一组任务：大文件优先（避免长尾）；文件级并行 = 2×并发
    /// （让存在性校验 / 等待响应与传输重叠），实际网络连接数受信号量严格限制。
    pub async fn download_all(&self, mut tasks: Vec<FileTask>) -> Vec<(FileTask, Result<bool, String>)> {
        tasks.sort_by(|a, b| b.size.cmp(&a.size));
        let slots = (self.opts.concurrency * 2).max(2);
        futures_util::stream::iter(tasks)
            .map(|t| self.run_tracked(t))
            .buffer_unordered(slots)
            .collect()
            .await
    }
}

fn reqwest_err_text(e: &reqwest::Error) -> String {
    use std::error::Error as _;
    if e.is_timeout() {
        return if e.is_connect() { "连接超时".into() } else { "读取超时（无数据）".into() };
    }
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(x) = src {
        s.push_str(": ");
        s.push_str(&x.to_string());
        src = x.source();
    }
    s
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(tmp, dest).map_err(|e| e.to_string())
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
    plan_assets_with_base(index_json, game_dir, bmcl, None)
}

/// `base` 覆盖资源下载根地址（测试用）；None = Mojang 官方地址（再经镜像改写）。
fn plan_assets_with_base(
    index_json: &str,
    game_dir: &Path,
    bmcl: bool,
    base: Option<&str>,
) -> Result<Vec<FileTask>, String> {
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
        let raw = format!(
            "{}/{prefix}/{}",
            base.unwrap_or("https://resources.download.minecraft.net"),
            obj.hash
        );
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

/// 安装配置（测试可覆盖资源下载根地址与下载参数）。
#[derive(Clone, Debug)]
pub struct InstallCfg {
    pub opts: DlOptions,
    /// true：优先使用本地 versions/<id>/<id>.json（补全模式，适用于已改名的版本）
    pub prefer_local_json: bool,
    /// 资源文件下载根地址覆盖（仅测试使用）
    pub asset_base: Option<String>,
}

impl InstallCfg {
    pub fn new(concurrency: usize, prefer_local_json: bool) -> Self {
        Self {
            opts: DlOptions::new(concurrency),
            prefer_local_json,
            asset_base: None,
        }
    }
}

/// 安装一个版本：元数据 → (版本文件 ∥ 资源索引→资源文件) 并发下载 → 解压 natives → 写完成标记。
/// 进度通过 `emit` 周期性推送（约每 150ms 一次），可经 `cancel` 取消。
/// `prefer_local_json=true` 即「检查并补全」：使用本地版本 json，只补缺失/损坏的文件。
pub async fn install_version(
    version_id: &str,
    source: &str,
    game_dir: &Path,
    concurrency: usize,
    prefer_local_json: bool,
    cancel: Arc<AtomicBool>,
    emit: Emit,
) -> Result<DownloadResult, String> {
    install_version_cfg(
        version_id,
        source,
        game_dir,
        InstallCfg::new(concurrency, prefer_local_json),
        cancel,
        emit,
    )
    .await
}

pub async fn install_version_cfg(
    version_id: &str,
    source: &str,
    game_dir: &Path,
    cfg: InstallCfg,
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

    let result = install_inner(version_id, source, game_dir, &cfg, &cancel, &tracker).await;
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

fn asset_index_task(ai: &AssetIndex, game_dir: &Path, bmcl: bool) -> FileTask {
    let raw = if ai.url.is_empty() {
        format!("{BMCLAPI_BASE}/index/{}.json", ai.id)
    } else {
        ai.url.clone()
    };
    FileTask {
        category: Category::AssetIndex,
        name: format!("{}.json", ai.id),
        url: mirror_url(&raw, bmcl),
        fallback_url: raw,
        dest: game_dir.join("assets").join("indexes").join(format!("{}.json", ai.id)),
        sha1: ai.sha1.clone(),
        size: ai.size,
        natives_exclude: None,
    }
}

fn stub_task(category: Category, name: String) -> FileTask {
    FileTask {
        category,
        name,
        url: String::new(),
        fallback_url: String::new(),
        dest: PathBuf::new(),
        sha1: String::new(),
        size: 0,
        natives_exclude: None,
    }
}

async fn install_inner(
    version_id: &str,
    source: &str,
    game_dir: &Path,
    cfg: &InstallCfg,
    cancel: &Arc<AtomicBool>,
    tracker: &Arc<Tracker>,
) -> Result<DownloadResult, String> {
    let bmcl = use_bmclapi(source);
    tracker.set_stage("meta");
    let versions_dir = game_dir.join("versions").join(version_id);
    // 开始安装/补全即作废完成标记：中途失败、取消或进程被关掉都不会被当作「已安装完整」
    clear_install_marker(game_dir, version_id);

    // 1) 版本 JSON（补全模式优先用本地文件）
    let vj_stub = stub_task(Category::VersionJson, format!("{version_id}.json"));
    tracker.plan(std::slice::from_ref(&vj_stub));
    tracker.start(&vj_stub);
    let local = if cfg.prefer_local_json {
        load_local_version_json(game_dir, version_id).ok()
    } else {
        None
    };
    let vj = match local {
        Some(v) => {
            tracker.finish(&vj_stub, &Ok(false));
            v
        }
        None => {
            let fetched = async {
                let client = http_client()?;
                let versions = list_remote_versions(source).await?;
                let info = versions
                    .into_iter()
                    .find(|v| v.id == version_id)
                    .ok_or_else(|| format!("版本清单中未找到 {version_id}"))?;
                fetch_version_json(&client, &info, bmcl).await
            }
            .await;
            match fetched {
                Ok(v) => {
                    tracker.finish(&vj_stub, &Ok(true));
                    config::ensure_dir(&versions_dir)?;
                    let meta = serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?;
                    fs::write(versions_dir.join(format!("{version_id}.json")), meta)
                        .map_err(|e| e.to_string())?;
                    v
                }
                Err(e) => {
                    tracker.finish(&vj_stub, &Err(e.clone()));
                    return Err(e);
                }
            }
        }
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    // 2) 规划：版本文件立即开始；资源索引与之并行获取，随后展开资源文件
    let vtasks = plan_version_files(&vj, game_dir, version_id, bmcl);
    tracker.plan(&vtasks);
    let idx_task = vj.asset_index.as_ref().map(|ai| asset_index_task(ai, game_dir, bmcl));
    if let Some(it) = &idx_task {
        tracker.plan(std::slice::from_ref(it));
        // 预占资源总字节（来自版本 json 的 totalSize），避免展开资源后总进度回退
        if let Some(ts) = vj.asset_index.as_ref().and_then(|a| a.total_size) {
            tracker.reserve_bytes(Category::Assets, ts.max(0) as u64);
        }
    }
    tracker.set_stage("files");
    let dl = Downloader::new(
        download_client(cfg.opts.idle_timeout)?,
        cfg.opts.clone(),
        tracker.clone(),
        cancel.clone(),
    );

    let assets_branch = async {
        let Some(it) = idx_task.clone() else {
            return Vec::new();
        };
        let (it, r) = dl.run_tracked(it).await;
        if let Err(e) = r {
            tracker.unreserve(Category::Assets);
            return vec![(it, Err(e))];
        }
        let planned = fs::read_to_string(&it.dest)
            .map_err(|e| e.to_string())
            .and_then(|text| plan_assets_with_base(&text, game_dir, bmcl, cfg.asset_base.as_deref()));
        tracker.unreserve(Category::Assets);
        match planned {
            Ok(assets) => {
                tracker.plan(&assets);
                let mut out = vec![(it, r)];
                out.extend(dl.download_all(assets).await);
                out
            }
            Err(e) => vec![(it, Err(e))],
        }
    };
    let (mut results, assets_results) = tokio::join!(dl.download_all(vtasks), assets_branch);
    results.extend(assets_results);
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    // 3) natives
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
        // 4) 全部成功才写完成标记
        write_install_marker(game_dir, version_id, results.len())?;
        Ok(DownloadResult {
            ok: true,
            message: if cfg.prefer_local_json {
                format!("版本 {version_id} 文件已补全")
            } else {
                format!("版本 {version_id} 安装完成")
            },
            failed,
        })
    } else {
        Ok(DownloadResult {
            ok: false,
            message: format!(
                "以下 {} 个文件更新失败，游戏文件未完整，可点「重试 / 补全」",
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
        DEFAULT_CONCURRENCY,
        false,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
}

// ---------------------------------------------------------------------------
// 安装完成标记 + 完整性预检（存在性 + 大小，不做全量 SHA-1，保证启动速度）
// ---------------------------------------------------------------------------

pub const INSTALL_MARKER: &str = ".install-complete";

pub fn install_marker_path(game_dir: &Path, id: &str) -> PathBuf {
    game_dir.join("versions").join(id).join(INSTALL_MARKER)
}

pub fn is_install_complete(game_dir: &Path, id: &str) -> bool {
    install_marker_path(game_dir, id).is_file()
}

fn clear_install_marker(game_dir: &Path, id: &str) {
    let _ = fs::remove_file(install_marker_path(game_dir, id));
}

/// 写入完成标记（versions/<id>/.install-complete）。
pub fn write_install_marker(game_dir: &Path, id: &str, files: usize) -> Result<(), String> {
    let dir = game_dir.join("versions").join(id);
    config::ensure_dir(&dir)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let body = serde_json::json!({ "version": id, "completed_at": secs, "files": files });
    fs::write(install_marker_path(game_dir, id), body.to_string()).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct MissingCount {
    pub category: String,
    pub label: String,
    pub count: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct IntegrityReport {
    pub version_id: String,
    /// 无缺失
    pub ok: bool,
    pub checked: u64,
    pub missing: u64,
    pub missing_by_category: Vec<MissingCount>,
    /// 前若干个缺失文件名（便于提示）
    pub samples: Vec<String>,
    /// 是否存在完成标记
    pub install_complete: bool,
}

impl IntegrityReport {
    pub fn summary(&self) -> String {
        self.missing_by_category
            .iter()
            .map(|m| format!("{} {}", m.label, m.count))
            .collect::<Vec<_>>()
            .join("、")
    }
}

fn file_present(dest: &Path, size: i64) -> bool {
    match fs::metadata(dest) {
        Ok(m) if m.is_file() => {
            if size > 0 {
                m.len() == size as u64
            } else {
                m.len() > 0
            }
        }
        _ => false,
    }
}

/// 按版本 json 检查：本平台允许的 libraries（含 natives）、client jar、asset index 及索引内全部 objects。
pub fn check_version_files(game_dir: &Path, id: &str) -> Result<IntegrityReport, String> {
    let vj = load_local_version_json(game_dir, id)?;
    let mut tasks = plan_version_files(&vj, game_dir, id, false);
    let mut missing_counts = [0u64; 5];
    let mut samples = Vec::new();
    let mut checked = 0u64;
    let mut miss = |c: Category, name: &str, counts: &mut [u64; 5]| {
        counts[c.idx()] += 1;
        if samples.len() < 20 {
            samples.push(name.to_string());
        }
    };
    if let Some(ai) = &vj.asset_index {
        let it = asset_index_task(ai, game_dir, false);
        checked += 1;
        let parsed = if file_present(&it.dest, it.size) {
            fs::read_to_string(&it.dest)
                .ok()
                .and_then(|t| plan_assets(&t, game_dir, false).ok())
        } else {
            None
        };
        match parsed {
            Some(a) => tasks.extend(a),
            None => miss(Category::AssetIndex, &it.name, &mut missing_counts),
        }
    }
    for t in &tasks {
        checked += 1;
        if !file_present(&t.dest, t.size) {
            miss(t.category, &t.name, &mut missing_counts);
        }
    }
    let missing: u64 = missing_counts.iter().sum();
    let missing_by_category = Category::ALL
        .iter()
        .filter(|c| missing_counts[c.idx()] > 0)
        .map(|c| MissingCount {
            category: format!("{:?}", c),
            label: c.label().into(),
            count: missing_counts[c.idx()],
        })
        .collect();
    Ok(IntegrityReport {
        version_id: id.to_string(),
        ok: missing == 0,
        checked,
        missing,
        missing_by_category,
        samples,
        install_complete: is_install_complete(game_dir, id),
    })
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
    download_file(&url, &dest, "", 0, false).await?;
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

    // ------------------------------------------------------------------
    // 本地 HTTP/1.1 测试服务器：keep-alive、可选 Range、限速、卡死、前 N 次 503
    // ------------------------------------------------------------------
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone, Default)]
    struct Route {
        body: Arc<Vec<u8>>,
        range: bool,
        /// 前 N 次请求返回 503
        fail_first: Arc<AtomicUsize>,
        /// 前 N 次请求在发送 stall_after 字节后卡住不再发送
        stall_times: Arc<AtomicUsize>,
        stall_after: usize,
        /// 每连接限速（字节/秒），0 = 不限
        bps: u64,
    }

    impl Route {
        fn new(body: Vec<u8>) -> Self {
            Route { body: Arc::new(body), ..Default::default() }
        }
        fn ranged(mut self) -> Self {
            self.range = true;
            self
        }
        fn fail(self, n: usize) -> Self {
            self.fail_first.store(n, Ordering::SeqCst);
            self
        }
        fn stall(mut self, times: usize, after: usize) -> Self {
            self.stall_times.store(times, Ordering::SeqCst);
            self.stall_after = after;
            self
        }
        fn throttle(mut self, bps: u64) -> Self {
            self.bps = bps;
            self
        }
    }

    #[derive(Clone, Default)]
    struct Srv {
        base: String,
        /// (path, Range 头)
        hits: Arc<Mutex<Vec<(String, Option<String>)>>>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    impl Srv {
        fn hits_for(&self, path: &str) -> Vec<Option<String>> {
            self.hits.lock().unwrap().iter().filter(|h| h.0 == path).map(|h| h.1.clone()).collect()
        }
    }

    async fn serve(routes: HashMap<String, Route>) -> Srv {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let srv = Srv {
            base: format!("http://{}", listener.local_addr().unwrap()),
            ..Default::default()
        };
        let routes = Arc::new(routes);
        let s2 = srv.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let routes = routes.clone();
                let srv = s2.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        // 读取一个请求头
                        let head_end = loop {
                            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break Some(i);
                            }
                            let mut tmp = [0u8; 4096];
                            match sock.read(&mut tmp).await {
                                Ok(0) | Err(_) => break None,
                                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                            }
                        };
                        let Some(he) = head_end else { return };
                        let head = String::from_utf8_lossy(&buf[..he]).to_string();
                        buf.drain(..he + 4);
                        let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                        let range = head
                            .lines()
                            .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                            .map(|l| l[6..].trim().to_string());
                        srv.hits.lock().unwrap().push((path.clone(), range.clone()));
                        let Some(rt) = routes.get(&path).cloned() else {
                            let _ = sock
                                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                                .await;
                            continue;
                        };
                        #[allow(deprecated)]
                        let take = |a: &AtomicUsize| {
                            a.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_sub(1)).is_ok()
                        };
                        if take(&rt.fail_first) {
                            let _ = sock
                                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
                                .await;
                            continue;
                        }
                        let stall = take(&rt.stall_times);
                        let total = rt.body.len();
                        let (status, a, b) = match (&range, rt.range) {
                            (Some(r), true) => {
                                let spec = r.trim_start_matches("bytes=");
                                let (x, y) = spec.split_once('-').unwrap();
                                let a: usize = x.parse().unwrap();
                                let b: usize = if y.is_empty() { total - 1 } else { y.parse::<usize>().unwrap().min(total - 1) };
                                (206, a, b)
                            }
                            _ => (200, 0, total - 1),
                        };
                        let len = b + 1 - a;
                        let mut h = format!(
                            "HTTP/1.1 {} {}\r\nContent-Length: {len}\r\n",
                            status,
                            if status == 206 { "Partial Content" } else { "OK" }
                        );
                        if rt.range {
                            h.push_str("Accept-Ranges: bytes\r\n");
                        }
                        if status == 206 {
                            h.push_str(&format!("Content-Range: bytes {a}-{b}/{total}\r\n"));
                        }
                        h.push_str("\r\n");
                        let now = srv.active.fetch_add(1, Ordering::SeqCst) + 1;
                        srv.max_active.fetch_max(now, Ordering::SeqCst);
                        let mut ok = sock.write_all(h.as_bytes()).await.is_ok();
                        let body = &rt.body[a..=b];
                        let chunk = 16 * 1024;
                        let mut sent = 0usize;
                        while ok && sent < body.len() {
                            let mut end = (sent + chunk).min(body.len());
                            if stall && sent < rt.stall_after {
                                end = end.min(rt.stall_after);
                            }
                            if stall && sent >= rt.stall_after {
                                tokio::time::sleep(Duration::from_secs(60)).await;
                                ok = false;
                                break;
                            }
                            ok = sock.write_all(&body[sent..end]).await.is_ok();
                            if rt.bps > 0 {
                                tokio::time::sleep(Duration::from_secs_f64((end - sent) as f64 / rt.bps as f64)).await;
                            }
                            sent = end;
                        }
                        srv.active.fetch_sub(1, Ordering::SeqCst);
                        if !ok {
                            return;
                        }
                    }
                });
            }
        });
        srv
    }

    fn sha1_hex(b: &[u8]) -> String {
        hex::encode(Sha1::digest(b))
    }

    fn pseudo_random(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    fn test_opts(conc: usize) -> DlOptions {
        let mut o = DlOptions::new(conc);
        o.idle_timeout = Duration::from_millis(400);
        o.backoff_base = Duration::from_millis(10);
        o.backoff_max = Duration::from_millis(50);
        o.split_threshold = 1024 * 1024;
        o.min_part_size = 256 * 1024;
        o
    }

    fn mk_dl(opts: DlOptions) -> (Arc<Downloader>, Arc<Tracker>, Arc<AtomicBool>) {
        let tracker = Arc::new(Tracker::new("t".into()));
        let cancel = Arc::new(AtomicBool::new(false));
        let dl = Downloader::new(
            download_client(opts.idle_timeout).unwrap(),
            opts,
            tracker.clone(),
            cancel.clone(),
        );
        (dl, tracker, cancel)
    }

    fn task(srv: &Srv, d: &Path, name: &str, path: &str, body: &[u8]) -> FileTask {
        FileTask {
            category: Category::Libraries,
            name: name.into(),
            url: format!("{}{path}", srv.base),
            fallback_url: String::new(),
            dest: d.join(name),
            sha1: sha1_hex(body),
            size: body.len() as i64,
            natives_exclude: None,
        }
    }

    /// 并发下载 + SHA-1 校验 + 跳过已存在 + 失败计数 + 取消。
    #[tokio::test]
    async fn tasks_download_verify_skip() {
        let srv = serve(HashMap::from([
            ("/hello".to_string(), Route::new(b"hello".to_vec())),
            ("/other".to_string(), Route::new(b"bad!!".to_vec())),
        ]))
        .await;
        let d = tmpdir("dl");
        let h = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let mk = |name: &str, path: &str| FileTask {
            category: Category::Assets,
            name: name.into(),
            url: format!("{}/{path}", srv.base),
            fallback_url: String::new(),
            dest: d.join(name),
            sha1: h.into(),
            size: 5,
            natives_exclude: None,
        };
        let tasks = vec![mk("ok1", "hello"), mk("ok2", "hello"), mk("bad", "other")];
        let (dl, tracker, cancel) = mk_dl(test_opts(8));
        tracker.plan(&tasks);
        let res = dl.download_all(tasks.clone()).await;
        assert_eq!(res.len(), 3);
        let s = tracker.snapshot(0, "");
        assert_eq!((s.done_files, s.failed_files, s.skipped_files), (2, 1, 0));
        assert_eq!(fs::read(d.join("ok1")).unwrap(), b"hello");
        assert!(!d.join("bad").exists());
        assert!(!d.join("bad.part").exists());
        assert_eq!(s.bytes_done, 10);
        assert!(s.failed[0].contains("SHA-1"));
        // 第二次：已存在 → 跳过
        assert_eq!(dl.run_task(&tasks[0]).await, Ok(false));
        // 取消
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(dl.run_task(&tasks[2]).await, Err(CANCELLED.to_string()));
    }

    /// 大文件：服务器支持 Range → 4 片并行写入预分配文件，整文件 SHA-1，进度不重复。
    #[tokio::test]
    async fn chunked_range_download() {
        let body = pseudo_random(3 * 1024 * 1024 + 123, 7);
        let srv = serve(HashMap::from([("/big".to_string(), Route::new(body.clone()).ranged())])).await;
        let d = tmpdir("split");
        let t = task(&srv, &d, "big.jar", "/big", &body);
        let (dl, tracker, _c) = mk_dl(test_opts(8));
        tracker.plan(std::slice::from_ref(&t));
        assert_eq!(dl.run_task(&t).await, Ok(true));
        assert_eq!(fs::read(&t.dest).unwrap(), body);
        assert!(!part_path(&t.dest).exists());
        let hits = srv.hits_for("/big");
        assert_eq!(hits.len(), 4, "{hits:?}");
        assert!(hits.iter().all(|r| r.is_some()));
        let s = tracker.snapshot(0, "");
        assert_eq!(s.bytes_done, body.len() as u64);
        assert_eq!(tracker.net_bytes(), body.len() as u64);
    }

    /// 服务器不支持 Range（忽略 Range 返回 200）→ 直接用首个连接整流下载，仅 1 次请求。
    #[tokio::test]
    async fn no_range_fallback() {
        let body = pseudo_random(2 * 1024 * 1024, 9);
        let srv = serve(HashMap::from([("/big".to_string(), Route::new(body.clone()))])).await;
        let d = tmpdir("norange");
        let t = task(&srv, &d, "big.jar", "/big", &body);
        let (dl, tracker, _c) = mk_dl(test_opts(8));
        tracker.plan(std::slice::from_ref(&t));
        assert_eq!(dl.run_task(&t).await, Ok(true));
        assert_eq!(fs::read(&t.dest).unwrap(), body);
        assert_eq!(srv.hits_for("/big").len(), 1);
        assert_eq!(tracker.snapshot(0, "").bytes_done, body.len() as u64);
    }

    /// 读空闲超时：服务器发一部分后卡住 → 400ms 判超时并重试成功；只尝试 1 次时报超时错误。
    #[tokio::test]
    async fn idle_timeout_and_retry() {
        let body = pseudo_random(200 * 1024, 3);
        let srv = serve(HashMap::from([
            ("/s1".to_string(), Route::new(body.clone()).stall(1, 50_000)),
            ("/s2".to_string(), Route::new(body.clone()).stall(5, 50_000)),
        ]))
        .await;
        let d = tmpdir("idle");
        let (dl, tracker, _c) = mk_dl(test_opts(4));
        let t1 = task(&srv, &d, "a.bin", "/s1", &body);
        tracker.plan(std::slice::from_ref(&t1));
        let t0 = Instant::now();
        assert_eq!(dl.run_task(&t1).await, Ok(true));
        assert!(t0.elapsed() < Duration::from_secs(5));
        assert_eq!(fs::read(&t1.dest).unwrap(), body);
        assert_eq!(srv.hits_for("/s1").len(), 2);
        // 进度：失败那次的字节已回滚，不重复计数
        assert_eq!(tracker.snapshot(0, "").bytes_done, body.len() as u64);

        let mut o = test_opts(4);
        o.attempts = 1;
        let (dl1, _t, _c) = mk_dl(o);
        let t2 = task(&srv, &d, "b.bin", "/s2", &body);
        let t0 = Instant::now();
        let e = dl1.run_task(&t2).await.unwrap_err();
        assert!(e.contains("超时"), "{e}");
        assert!(t0.elapsed() < Duration::from_secs(3));
        assert!(!part_path(&t2.dest).exists());
    }

    /// 取消：限速慢下载过程中置取消标志 → 立即返回「已取消」，删除 .part，回滚进度。
    #[tokio::test]
    async fn cancel_is_immediate() {
        let body = pseudo_random(4 * 1024 * 1024, 5);
        let srv = serve(HashMap::from([(
            "/slow".to_string(),
            Route::new(body.clone()).ranged().throttle(256 * 1024),
        )]))
        .await;
        let d = tmpdir("cancel");
        let (dl, tracker, cancel) = mk_dl(test_opts(8));
        let t = task(&srv, &d, "slow.jar", "/slow", &body);
        tracker.plan(std::slice::from_ref(&t));
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            c2.store(true, Ordering::Relaxed);
        });
        let t0 = Instant::now();
        assert_eq!(dl.run_task(&t).await, Err(CANCELLED.to_string()));
        let el = t0.elapsed();
        assert!(el < Duration::from_millis(2000), "{el:?}");
        assert!(!t.dest.exists());
        assert!(!part_path(&t.dest).exists());
        assert_eq!(tracker.snapshot(0, "").bytes_done, 0);
    }

    /// 503 → 指数退避重试成功，并临时降低并发。
    #[tokio::test]
    async fn retry_on_503_with_backoff() {
        let srv = serve(HashMap::from([("/f".to_string(), Route::new(b"hello".to_vec()).fail(2))])).await;
        let d = tmpdir("retry");
        let (dl, _tracker, _c) = mk_dl(test_opts(8));
        let t = task(&srv, &d, "f.txt", "/f", b"hello");
        assert_eq!(dl.run_task(&t).await, Ok(true));
        assert_eq!(srv.hits_for("/f").len(), 3);
        assert_eq!(dl.effective_concurrency(), 6);
        // 一直 503 → 失败并报告状态码
        let srv2 = serve(HashMap::from([("/g".to_string(), Route::new(b"x".to_vec()).fail(99))])).await;
        let t2 = task(&srv2, &d, "g.txt", "/g", b"x");
        let e = dl.run_task(&t2).await.unwrap_err();
        assert_eq!(e, "HTTP 503");
        assert!(dl.effective_concurrency() >= 2);
    }

    /// 分片中途断流 → 该分片从已写位置续传（Range 起点非分片边界），整文件仍正确、进度不重复。
    #[tokio::test]
    async fn range_part_resume() {
        let body = pseudo_random(2 * 1024 * 1024, 11);
        let srv = serve(HashMap::from([(
            "/big".to_string(),
            Route::new(body.clone()).ranged().stall(1, 100_000),
        )]))
        .await;
        let d = tmpdir("resume");
        let (dl, tracker, _c) = mk_dl(test_opts(8));
        let t = task(&srv, &d, "big.jar", "/big", &body);
        tracker.plan(std::slice::from_ref(&t));
        assert_eq!(dl.run_task(&t).await, Ok(true));
        assert_eq!(fs::read(&t.dest).unwrap(), body);
        let hits = srv.hits_for("/big");
        assert_eq!(hits.len(), 5, "{hits:?}");
        assert!(hits.iter().any(|r| r.as_deref() == Some("bytes=100000-524287")), "{hits:?}");
        assert_eq!(tracker.snapshot(0, "").bytes_done, body.len() as u64);
    }

    /// 连接数严格受并发上限约束（分片也计入）。
    #[tokio::test]
    async fn concurrency_is_bounded() {
        let mut routes = HashMap::new();
        let big = pseudo_random(2 * 1024 * 1024, 13);
        routes.insert("/big".to_string(), Route::new(big.clone()).ranged().throttle(4 * 1024 * 1024));
        let small = pseudo_random(64 * 1024, 17);
        for i in 0..30 {
            routes.insert(format!("/s{i}"), Route::new(small.clone()).throttle(512 * 1024));
        }
        let srv = serve(routes).await;
        let d = tmpdir("bound");
        let (dl, tracker, _c) = mk_dl(test_opts(4));
        let mut tasks: Vec<FileTask> = (0..30).map(|i| task(&srv, &d, &format!("s{i}"), &format!("/s{i}"), &small)).collect();
        tasks.push(task(&srv, &d, "big", "/big", &big));
        tracker.plan(&tasks);
        let res = dl.download_all(tasks).await;
        assert!(res.iter().all(|(_, r)| r == &Ok(true)));
        let m = srv.max_active.load(Ordering::SeqCst);
        assert!(m <= 4 && m >= 2, "max_active={m}");
        // 大文件优先 → big 的首片在第一批并发窗口内发出
        assert!(srv.hits.lock().unwrap().iter().take(4).any(|h| h.0 == "/big"));
        assert_eq!(tracker.snapshot(0, "").bytes_done, (30 * small.len() + big.len()) as u64);
    }

    // ------------------------------------------------------------------
    // 完整性预检 + 补全 + 完成标记
    // ------------------------------------------------------------------

    struct Fixture {
        game: PathBuf,
        srv: Srv,
        assets: Vec<Vec<u8>>,
    }

    /// 构造：版本 json（client jar + 1 个库 + 资源索引 3 个对象）写入本地；
    /// 服务器提供全部文件。`bad_asset=true` 时第 3 个资源服务端返回错误内容。
    async fn fixture(tag: &str, id: &str, bad_asset: bool) -> Fixture {
        let jar = pseudo_random(30_000, 21);
        let lib = pseudo_random(10_000, 22);
        let assets: Vec<Vec<u8>> = (0..3).map(|i| pseudo_random(1000 + i * 10, 30 + i as u64)).collect();
        let objects: serde_json::Map<String, serde_json::Value> = assets
            .iter()
            .enumerate()
            .map(|(i, a)| (format!("minecraft/a{i}.ogg"), serde_json::json!({"hash": sha1_hex(a), "size": a.len()})))
            .collect();
        let index = serde_json::to_vec(&serde_json::json!({ "objects": objects })).unwrap();
        let mut routes = HashMap::from([
            ("/c.jar".to_string(), Route::new(jar.clone())),
            ("/lib/a/b/1/b-1.jar".to_string(), Route::new(lib.clone())),
            ("/idx/1.json".to_string(), Route::new(index.clone())),
        ]);
        for (i, a) in assets.iter().enumerate() {
            let h = sha1_hex(a);
            let served = if bad_asset && i == 2 { b"corrupt".to_vec() } else { a.clone() };
            routes.insert(format!("/assets/{}/{h}", &h[..2]), Route::new(served));
        }
        let srv = serve(routes).await;
        let game = tmpdir(tag);
        let b = &srv.base;
        let vj = serde_json::json!({
            "id": id, "mainClass": "net.minecraft.client.main.Main",
            "downloads": {"client": {"sha1": sha1_hex(&jar), "size": jar.len(), "url": format!("{b}/c.jar")}},
            "libraries": [{"name": "a:b:1", "downloads": {"artifact": {"path": "a/b/1/b-1.jar", "sha1": sha1_hex(&lib), "size": lib.len(), "url": format!("{b}/lib/a/b/1/b-1.jar")}}},
                          {"name": "x:mac-only:1", "rules": [{"action": "allow", "os": {"name": "nonexistent-os"}}],
                           "downloads": {"artifact": {"path": "x/m.jar", "sha1": "", "size": 1, "url": format!("{b}/nope")}}}],
            "assetIndex": {"id": "1", "sha1": sha1_hex(&index), "size": index.len(), "url": format!("{b}/idx/1.json"), "totalSize": 3030}
        });
        let vdir = game.join("versions").join(id);
        fs::create_dir_all(&vdir).unwrap();
        fs::write(vdir.join(format!("{id}.json")), serde_json::to_vec(&vj).unwrap()).unwrap();
        // 预置：jar、库、索引、前两个资源；缺第 3 个资源
        fs::write(vdir.join(format!("{id}.jar")), &jar).unwrap();
        let lp = game.join("libraries/a/b/1");
        fs::create_dir_all(&lp).unwrap();
        fs::write(lp.join("b-1.jar"), &lib).unwrap();
        let ip = game.join("assets/indexes");
        fs::create_dir_all(&ip).unwrap();
        fs::write(ip.join("1.json"), &index).unwrap();
        for a in &assets[..2] {
            let h = sha1_hex(a);
            let p = game.join("assets/objects").join(&h[..2]);
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join(&h), a).unwrap();
        }
        Fixture { game, srv, assets }
    }

    async fn repair(f: &Fixture, id: &str) -> Result<DownloadResult, String> {
        let mut cfg = InstallCfg::new(8, true);
        cfg.opts = test_opts(8);
        cfg.asset_base = Some(format!("{}/assets", f.srv.base));
        install_version_cfg(id, "official", &f.game, cfg, Arc::new(AtomicBool::new(false)), Arc::new(|_| {})).await
    }

    #[tokio::test]
    async fn precheck_reports_missing_then_repair_passes() {
        let f = fixture("pre", "26.3", false).await;
        let r = check_version_files(&f.game, "26.3").unwrap();
        // jar + 1 库（其它 OS 的库跳过）+ 索引 + 3 资源
        assert_eq!(r.checked, 6);
        assert_eq!(r.missing, 1);
        assert!(!r.ok);
        assert_eq!(r.missing_by_category, vec![MissingCount { category: "Assets".into(), label: "资源文件".into(), count: 1 }]);
        assert_eq!(r.samples, vec!["minecraft/a2.ogg".to_string()]);
        assert!(!r.install_complete);

        // 大小不符同样视为缺失
        let h0 = sha1_hex(&f.assets[0]);
        let p0 = f.game.join("assets/objects").join(&h0[..2]).join(&h0);
        fs::write(&p0, b"short").unwrap();
        assert_eq!(check_version_files(&f.game, "26.3").unwrap().missing, 2);

        let res = repair(&f, "26.3").await.unwrap();
        assert!(res.ok, "{:?}", res.failed);
        assert!(is_install_complete(&f.game, "26.3"));
        let r2 = check_version_files(&f.game, "26.3").unwrap();
        assert!(r2.ok && r2.missing == 0 && r2.install_complete);
        // 只补缺失的：jar / 库 / 索引 / 完好资源都未重新请求
        let hits = f.srv.hits.lock().unwrap().clone();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(fs::read(&p0).unwrap(), f.assets[0]);
    }

    #[tokio::test]
    async fn marker_not_written_on_failure() {
        let f = fixture("fail", "26.3", true).await;
        // 旧的完成标记在新一轮安装开始时即作废
        write_install_marker(&f.game, "26.3", 1).unwrap();
        let res = repair(&f, "26.3").await.unwrap();
        assert!(!res.ok);
        assert_eq!(res.failed.len(), 1);
        assert!(res.failed[0].contains("minecraft/a2.ogg"), "{:?}", res.failed);
        assert!(!is_install_complete(&f.game, "26.3"));
        assert_eq!(check_version_files(&f.game, "26.3").unwrap().missing, 1);
    }

    #[test]
    fn precheck_missing_index_counts() {
        let g = tmpdir("noidx");
        let v = g.join("versions").join("x");
        fs::create_dir_all(&v).unwrap();
        fs::write(v.join("x.json"), r#"{"id":"x","mainClass":"M","assetIndex":{"id":"9","sha1":"","size":10,"url":"u"}}"#).unwrap();
        let r = check_version_files(&g, "x").unwrap();
        assert_eq!(r.missing, 1);
        assert_eq!(r.missing_by_category[0].category, "AssetIndex");
    }
}
