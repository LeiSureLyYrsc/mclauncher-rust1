use crate::config::{self, Account, Settings};
use crate::download::{self, VersionJson};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize)]
pub struct LaunchResult {
    pub ok: bool,
    pub command: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct JavaInfo {
    pub found: bool,
    pub path: String,
    pub version: String,
}

pub fn detect_java(preferred: &str) -> JavaInfo {
    if !preferred.trim().is_empty() {
        let p = preferred.trim();
        if Path::new(p).exists() || which::which(p).is_ok() {
            let ver = java_version(p);
            return JavaInfo {
                found: true,
                path: p.to_string(),
                version: ver,
            };
        }
    }
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let cand = if cfg!(windows) {
            PathBuf::from(&home).join("bin").join("java.exe")
        } else {
            PathBuf::from(&home).join("bin").join("java")
        };
        if cand.exists() {
            let path = cand.to_string_lossy().to_string();
            return JavaInfo {
                found: true,
                path: path.clone(),
                version: java_version(&path),
            };
        }
    }
    if let Ok(p) = which::which("java") {
        let path = p.to_string_lossy().to_string();
        return JavaInfo {
            found: true,
            path: path.clone(),
            version: java_version(&path),
        };
    }
    JavaInfo {
        found: false,
        path: String::new(),
        version: String::new(),
    }
}

fn java_version(java: &str) -> String {
    let out = Command::new(java).arg("-version").output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stderr);
            s.lines().next().unwrap_or("").to_string()
        }
        Err(_) => String::new(),
    }
}

fn classpath_sep() -> &'static str {
    if cfg!(windows) { ";" } else { ":" }
}

fn build_classpath(vj: &VersionJson, libs_dir: &Path, jar_path: &Path) -> String {
    let mut entries = Vec::new();
    for lib in &vj.libraries {
        if !download::library_allowed(&lib.rules) {
            continue;
        }
        // skip natives-only
        let path = if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                if !art.path.is_empty() {
                    Some(art.path.clone())
                } else {
                    Some(maven_path_simple(&lib.name))
                }
            } else {
                None
            }
        } else {
            Some(maven_path_simple(&lib.name))
        };
        if let Some(rel) = path {
            let full = libs_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            if full.exists() {
                entries.push(full.to_string_lossy().to_string());
            }
        }
    }
    entries.push(jar_path.to_string_lossy().to_string());
    entries.join(classpath_sep())
}

fn maven_path_simple(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return name.replace(':', "/");
    }
    let group = parts[0].replace('.', "/");
    format!(
        "{}/{}/{}/{}-{}.jar",
        group, parts[1], parts[2], parts[1], parts[2]
    )
}

fn assets_index_name(vj: &VersionJson) -> String {
    if let Some(ai) = &vj.asset_index {
        if !ai.id.is_empty() {
            return ai.id.clone();
        }
    }
    vj.assets.clone().unwrap_or_else(|| "legacy".into())
}

fn expand_token(s: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
}

fn expand_arg_list(values: &[serde_json::Value], vars: &std::collections::HashMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        match v {
            serde_json::Value::String(s) => out.push(expand_token(s, vars)),
            serde_json::Value::Object(m) => {
                // skip rules-based optional args for simplicity unless always allow
                let allowed = match m.get("rules") {
                    Some(serde_json::Value::Array(rules)) => {
                        let mut allow = false;
                        let os = if cfg!(windows) {
                            "windows"
                        } else if cfg!(target_os = "macos") {
                            "osx"
                        } else {
                            "linux"
                        };
                        for r in rules {
                            let action = r.get("action").and_then(|a| a.as_str()).unwrap_or("");
                            let matches = match r.get("os").and_then(|o| o.get("name")).and_then(|n| n.as_str()) {
                                None => true,
                                Some(n) => n == os,
                            };
                            if matches {
                                allow = action == "allow";
                            }
                        }
                        allow
                    }
                    _ => true,
                };
                if !allowed {
                    continue;
                }
                match m.get("value") {
                    Some(serde_json::Value::String(s)) => out.push(expand_token(s, vars)),
                    Some(serde_json::Value::Array(arr)) => {
                        for a in arr {
                            if let Some(s) = a.as_str() {
                                out.push(expand_token(s, vars));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

pub async fn launch_game(settings: &Settings, account: Option<&Account>) -> LaunchResult {
    let game_dir = config::resolve_game_dir(settings);
    let version_id = settings.selected_version.trim();
    if version_id.is_empty() {
        return LaunchResult {
            ok: false,
            command: String::new(),
            message: "未选择版本，请先在下载中心安装并选择版本".into(),
        };
    }
    let versions_dir = game_dir.join("versions").join(version_id);
    let jar_path = versions_dir.join(format!("{version_id}.jar"));
    let json_path = versions_dir.join(format!("{version_id}.json"));
    if !json_path.exists() {
        return LaunchResult {
            ok: false,
            command: String::new(),
            message: format!("缺少版本元数据，请前往下载中心安装 {version_id}"),
        };
    }
    if !jar_path.exists() {
        return LaunchResult {
            ok: false,
            command: String::new(),
            message: format!("缺少客户端 jar，请前往下载中心安装 {version_id}"),
        };
    }

    let vj = match download::load_local_version_json(&game_dir, version_id) {
        Ok(v) => v,
        Err(e) => {
            return LaunchResult {
                ok: false,
                command: String::new(),
                message: e,
            };
        }
    };

    let java_info = detect_java(&settings.java_path);
    if !java_info.found {
        return LaunchResult {
            ok: false,
            command: String::new(),
            message: "未检测到 Java。请在设置中填写 Java 路径，或安装 JDK 并确保在 PATH / JAVA_HOME 中。".into(),
        };
    }

    let session = account.cloned().unwrap_or_else(|| crate::auth::offline_login("Player"));
    let natives_dir = versions_dir.join("natives");
    let assets_dir = game_dir.join("assets");
    let libs_dir = game_dir.join("libraries");
    let cp = build_classpath(&vj, &libs_dir, &jar_path);

    let mut vars = std::collections::HashMap::new();
    vars.insert("auth_player_name".into(), session.username.clone());
    vars.insert("version_name".into(), version_id.to_string());
    vars.insert("game_directory".into(), game_dir.to_string_lossy().to_string());
    vars.insert("assets_root".into(), assets_dir.to_string_lossy().to_string());
    vars.insert("assets_index_name".into(), assets_index_name(&vj));
    vars.insert(
        "auth_uuid".into(),
        session.uuid.replace('-', ""),
    );
    vars.insert("auth_access_token".into(), session.access_token.clone());
    vars.insert("user_type".into(), session.user_type.clone());
    vars.insert("version_type".into(), vj.type_.clone());
    vars.insert("natives_directory".into(), natives_dir.to_string_lossy().to_string());
    vars.insert("launcher_name".into(), "mclauncher-rust1".into());
    vars.insert("launcher_version".into(), "0.1.0".into());
    vars.insert("classpath".into(), cp.clone());
    vars.insert("library_directory".into(), libs_dir.to_string_lossy().to_string());
    vars.insert("classpath_separator".into(), classpath_sep().into());

    let mem = if settings.max_memory_mb == 0 { 2048 } else { settings.max_memory_mb };
    let mut jvm_args = vec![format!("-Xmx{mem}M"), format!("-Djava.library.path={}", natives_dir.display())];

    if session.mode == "authlib" && !session.authlib_api.is_empty() {
        match download::ensure_authlib_jar().await {
            Ok(jar) => {
                jvm_args.push(format!("-javaagent:{}={}", jar.display(), session.authlib_api));
            }
            Err(e) => {
                return LaunchResult {
                    ok: false,
                    command: String::new(),
                    message: format!("准备 authlib-injector 失败: {e}"),
                };
            }
        }
    }

    if let Some(args) = &vj.arguments {
        if !args.jvm.is_empty() {
            jvm_args.extend(expand_arg_list(&args.jvm, &vars));
        }
    }
    if !jvm_args.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm_args.push("-cp".into());
        jvm_args.push(cp.clone());
    } else {
        // replace ${classpath} already done in expand
        for i in 0..jvm_args.len() {
            if (jvm_args[i] == "-cp" || jvm_args[i] == "-classpath") && i + 1 < jvm_args.len() {
                if jvm_args[i + 1].contains("${") || jvm_args[i + 1].is_empty() {
                    jvm_args[i + 1] = cp.clone();
                }
            }
        }
    }

    let main_class = if vj.main_class.is_empty() {
        return LaunchResult {
            ok: false,
            command: String::new(),
            message: "版本 json 缺少 mainClass".into(),
        };
    } else {
        vj.main_class.clone()
    };

    let game_args = if let Some(args) = &vj.arguments {
        if !args.game.is_empty() {
            expand_arg_list(&args.game, &vars)
        } else {
            default_game_args(&vars)
        }
    } else if let Some(legacy) = &vj.minecraft_arguments {
        legacy
            .split_whitespace()
            .map(|s| expand_token(s, &vars))
            .collect()
    } else {
        default_game_args(&vars)
    };

    let mut all_args = jvm_args;
    all_args.push(main_class);
    all_args.extend(game_args);

    let cmd_preview = format!("{} {}", java_info.path, all_args.join(" "));

    match Command::new(&java_info.path)
        .args(&all_args)
        .current_dir(&game_dir)
        .spawn()
    {
        Ok(_child) => LaunchResult {
            ok: true,
            command: cmd_preview,
            message: format!("已尝试启动 {version_id}（进程已 spawn；若立刻退出请检查 natives/资源是否完整）"),
        },
        Err(e) => LaunchResult {
            ok: false,
            command: cmd_preview,
            message: format!("启动失败: {e}"),
        },
    }
}

fn default_game_args(vars: &std::collections::HashMap<String, String>) -> Vec<String> {
    vec![
        "--username".into(),
        vars["auth_player_name"].clone(),
        "--version".into(),
        vars["version_name"].clone(),
        "--gameDir".into(),
        vars["game_directory"].clone(),
        "--assetsDir".into(),
        vars["assets_root"].clone(),
        "--assetIndex".into(),
        vars["assets_index_name"].clone(),
        "--uuid".into(),
        vars["auth_uuid"].clone(),
        "--accessToken".into(),
        vars["auth_access_token"].clone(),
        "--userType".into(),
        vars["user_type"].clone(),
        "--versionType".into(),
        vars["version_type"].clone(),
    ]
}

pub fn open_game_dir(settings: &Settings) -> Result<(), String> {
    let d = config::resolve_game_dir(settings);
    config::ensure_dir(&d)?;
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg(&d).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(&d).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(&d).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}
