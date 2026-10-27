use crate::config;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

#[derive(Deserialize)]
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
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha1hex: &str,
    expected_size: i64,
    bmcl: bool,
) -> Result<(), String> {
    let url = mirror_url(url, bmcl);
    if let Some(parent) = dest.parent() {
        config::ensure_dir(parent)?;
    }
    if dest.exists() {
        let ok_size = expected_size <= 0
            || fs::metadata(dest)
                .map(|m| m.len() as i64 == expected_size)
                .unwrap_or(false);
        if ok_size {
            if sha1hex.is_empty() {
                return Ok(());
            }
            if let Ok(h) = file_sha1(dest) {
                if h.eq_ignore_ascii_case(sha1hex) {
                    return Ok(());
                }
            }
        }
    }
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败 {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败 {url}: HTTP {}", resp.status()));
    }
    let mut stream = resp.bytes_stream();
    let tmp = dest.with_extension("part");
    let mut file = File::create(&tmp).map_err(|e| e.to_string())?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    drop(file);
    if !sha1hex.is_empty() {
        let h = file_sha1(&tmp)?;
        if !h.eq_ignore_ascii_case(sha1hex) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("SHA1 校验失败: {url}"));
        }
    }
    fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok(())
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

async fn install_library(
    client: &reqwest::Client,
    lib: &Library,
    libs_dir: &Path,
    natives_dir: &Path,
    bmcl: bool,
) -> Result<(), String> {
    if !library_allowed(&lib.rules) {
        return Ok(());
    }
    if let Some(dl) = &lib.downloads {
        if let Some(art) = &dl.artifact {
            let path = if art.path.is_empty() {
                maven_path(&lib.name)
            } else {
                art.path.clone()
            };
            let dest = libs_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let url = if art.url.is_empty() {
                format!("{BMCLAPI_BASE}/maven/{path}")
            } else {
                art.url.clone()
            };
            download_file(client, &url, &dest, &art.sha1, art.size, bmcl).await?;
        }
    } else if !lib.name.is_empty() {
        let path = maven_path(&lib.name);
        let dest = libs_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let url = match &lib.url {
            Some(u) if u.ends_with(".jar") => u.clone(),
            Some(u) => format!("{}/{path}", u.trim_end_matches('/')),
            None => format!("{BMCLAPI_BASE}/maven/{path}"),
        };
        download_file(client, &url, &dest, "", 0, bmcl).await?;
    }

    // natives
    let Some(classifier) = native_classifier(lib) else {
        return Ok(());
    };
    let Some(dl) = &lib.downloads else {
        return Ok(());
    };
    let Some(classifiers) = &dl.classifiers else {
        return Ok(());
    };
    let Some(nat) = classifiers.get(&classifier) else {
        return Ok(());
    };
    let path = if nat.path.is_empty() {
        maven_path(&format!("{}:{}", lib.name, classifier))
    } else {
        nat.path.clone()
    };
    // Fix maven path for classifier form group:art:ver:classifier already handled;
    // if path empty above used wrong — rebuild:
    let path = if nat.path.is_empty() {
        let parts: Vec<&str> = lib.name.split(':').collect();
        if parts.len() >= 3 {
            let group = parts[0].replace('.', "/");
            format!(
                "{}/{}/{}/{}-{}-{}.jar",
                group, parts[1], parts[2], parts[1], parts[2], classifier
            )
        } else {
            path
        }
    } else {
        path
    };
    let dest = libs_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let url = if nat.url.is_empty() {
        format!("{BMCLAPI_BASE}/maven/{path}")
    } else {
        nat.url.clone()
    };
    download_file(client, &url, &dest, &nat.sha1, nat.size, bmcl).await?;
    extract_natives(&dest, natives_dir, lib.extract.as_ref().map(|e| e.exclude.as_slice()).unwrap_or(&[]))?;
    Ok(())
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

async fn install_assets(
    client: &reqwest::Client,
    vj: &VersionJson,
    game_dir: &Path,
    bmcl: bool,
) -> Result<(), String> {
    let Some(ai) = &vj.asset_index else {
        return Ok(());
    };
    let indexes = game_dir.join("assets/indexes");
    config::ensure_dir(&indexes)?;
    let index_path = indexes.join(format!("{}.json", ai.id));
    let url = if ai.url.is_empty() {
        format!("{BMCLAPI_BASE}/index/{}.json", ai.id)
    } else {
        ai.url.clone()
    };
    download_file(client, &url, &index_path, &ai.sha1, ai.size, bmcl).await?;
    let text = fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
    let index: AssetIndexFile =
        serde_json::from_str(&text).map_err(|e| format!("解析 asset index: {e}"))?;
    let objects_dir = game_dir.join("assets/objects");
    // Limit concurrent simplicity: sequential (first version — honest about speed)
    let mut count = 0usize;
    let total = index.objects.len();
    for (_name, obj) in index.objects {
        count += 1;
        if count % 50 == 0 {
            eprintln!("assets {count}/{total}");
        }
        if obj.hash.len() < 2 {
            continue;
        }
        let prefix = &obj.hash[..2];
        let dest = objects_dir.join(prefix).join(&obj.hash);
        let url = if bmcl {
            format!("{BMCLAPI_BASE}/assets/{prefix}/{}", obj.hash)
        } else {
            format!("https://resources.download.minecraft.net/{prefix}/{}", obj.hash)
        };
        download_file(client, &url, &dest, &obj.hash, obj.size, false).await?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct DownloadResult {
    pub ok: bool,
    pub message: String,
}

pub async fn download_version(version_id: &str, source: &str, game_dir: &Path) -> Result<DownloadResult, String> {
    let bmcl = use_bmclapi(source);
    let client = http_client()?;
    let versions = list_remote_versions(source).await?;
    let info = versions
        .into_iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("版本清单中未找到 {version_id}"))?;

    let versions_dir = game_dir.join("versions").join(version_id);
    config::ensure_dir(&versions_dir)?;

    let vj = fetch_version_json(&client, &info, bmcl).await?;
    let meta_path = versions_dir.join(format!("{version_id}.json"));
    let meta = serde_json::to_vec_pretty(&vj).map_err(|e| e.to_string())?;
    fs::write(&meta_path, meta).map_err(|e| e.to_string())?;

    if let Some(client_art) = vj.downloads.get("client") {
        let jar = versions_dir.join(format!("{version_id}.jar"));
        if !client_art.url.is_empty() {
            download_file(
                &client,
                &client_art.url,
                &jar,
                &client_art.sha1,
                client_art.size,
                bmcl,
            )
            .await
            .map_err(|e| format!("客户端 jar: {e}"))?;
        }
    }

    let libs_dir = game_dir.join("libraries");
    let natives_dir = versions_dir.join("natives");
    config::ensure_dir(&natives_dir)?;
    for lib in &vj.libraries {
        install_library(&client, lib, &libs_dir, &natives_dir, bmcl)
            .await
            .map_err(|e| format!("依赖 {}: {e}", lib.name))?;
    }

    install_assets(&client, &vj, game_dir, bmcl)
        .await
        .map_err(|e| format!("资源: {e}"))?;

    Ok(DownloadResult {
        ok: true,
        message: format!("版本 {version_id} 安装完成"),
    })
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
