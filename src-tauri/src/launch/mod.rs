use crate::config::{self, Account, Settings};
use crate::download::{self, VersionJson};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Default, Debug)]
pub struct LaunchResult {
    pub ok: bool,
    pub command: String,
    pub message: String,
    /// 预检发现缺失文件，需要补全（前端据此询问「是否补全」）
    #[serde(default)]
    pub need_repair: bool,
    /// 缺失文件数
    #[serde(default)]
    pub missing: u64,
    /// 各类别缺失数
    #[serde(default)]
    pub missing_by_category: Vec<download::MissingCount>,
    /// 启动后迅速退出时的退出码
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// 失败所在步骤（files | java | args | spawn | watch），成功为空
    #[serde(default)]
    pub failed_step: String,
}

/// 启动后观察窗口：此时间内进程退出视为「启动即退出」。
pub const EARLY_EXIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// 在 `dur` 内轮询子进程是否已退出；退出则返回 Some(退出码，被信号杀死时为 -1)。
pub async fn wait_early_exit(child: &mut std::process::Child, dur: std::time::Duration) -> Option<i32> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(st.code().unwrap_or(-1)),
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() >= dur {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn log_tail(path: &Path, max_lines: usize) -> String {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

#[derive(Serialize, Clone, Debug)]
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
    let mut cmd = Command::new(java);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：避免闪黑框
    }
    let out = cmd.output();
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

/// 参数规则判定：os 规则按当前系统；features 规则只认可 has_custom_resolution（由设置决定），
/// 其余（is_demo_user / quick_play_* 等）一律视为 false，避免误加 --demo 等参数。
fn rules_allow(rules: &[serde_json::Value], features: &std::collections::HashMap<&str, bool>) -> bool {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let mut allow = false;
    for r in rules {
        let action = r.get("action").and_then(|a| a.as_str()).unwrap_or("");
        let os_ok = match r.get("os").and_then(|o| o.get("name")).and_then(|n| n.as_str()) {
            None => true,
            Some(n) => n == os,
        };
        let feat_ok = match r.get("features").and_then(|f| f.as_object()) {
            None => true,
            Some(m) => m.iter().all(|(k, v)| {
                features.get(k.as_str()).copied().unwrap_or(false) == v.as_bool().unwrap_or(false)
            }),
        };
        if os_ok && feat_ok {
            allow = action == "allow";
        }
    }
    allow
}

fn expand_arg_list(
    values: &[serde_json::Value],
    vars: &std::collections::HashMap<String, String>,
    features: &std::collections::HashMap<&str, bool>,
) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        match v {
            serde_json::Value::String(s) => out.push(expand_token(s, vars)),
            serde_json::Value::Object(m) => {
                let allowed = match m.get("rules") {
                    Some(serde_json::Value::Array(rules)) => rules_allow(rules, features),
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

/// 把用户填写的 JVM 参数串拆成数组（支持双引号包裹含空格的值）。
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut has = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            c if c.is_whitespace() && !in_q => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// 原版「最大内存-自动设置」：按系统当前剩余内存动态分配。
/// 原版规则为「最大 1300M，最小 500M」（2014 年的 1.7 时代）；现代版本最低需约 1G，
/// 因此按剩余内存的 60% 计算，并夹在 [1024, 4096] MB，按 256MB 取整。
pub fn auto_memory_mb(available_mb: u64) -> u32 {
    let v = (available_mb * 6 / 10) / 256 * 256;
    v.clamp(1024, 4096) as u32
}

pub fn available_memory_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.available_memory() / 1024 / 1024
}

pub fn effective_memory_mb(settings: &Settings) -> u32 {
    if settings.memory_auto {
        auto_memory_mb(available_memory_mb())
    } else if settings.max_memory_mb == 0 {
        2048
    } else {
        settings.max_memory_mb
    }
}

/// 启动所需的全部输入（纯数据，便于单元测试）。
pub struct LaunchPlan<'a> {
    pub settings: &'a Settings,
    pub vj: &'a VersionJson,
    pub version_id: &'a str,
    pub game_dir: &'a Path,
    pub session: &'a Account,
    pub authlib_jar: Option<&'a Path>,
    pub memory_mb: u32,
}

/// 生成完整参数（不含 java 可执行本身）。
pub fn build_args(p: &LaunchPlan) -> Vec<String> {
    let settings = p.settings;
    let vj = p.vj;
    let version_id = p.version_id;
    let game_dir = p.game_dir;
    let versions_dir = game_dir.join("versions").join(version_id);
    let jar_path = versions_dir.join(format!("{version_id}.jar"));
    let natives_dir = versions_dir.join("natives");
    let assets_dir = game_dir.join("assets");
    let libs_dir = game_dir.join("libraries");
    let run_dir = config::run_dir(game_dir, version_id, settings.version_isolation);
    let cp = build_classpath(vj, &libs_dir, &jar_path);
    let session = p.session;

    let mut vars = std::collections::HashMap::new();
    vars.insert("auth_player_name".into(), session.username.clone());
    vars.insert("version_name".into(), version_id.to_string());
    vars.insert("game_directory".into(), run_dir.to_string_lossy().to_string());
    vars.insert("assets_root".into(), assets_dir.to_string_lossy().to_string());
    vars.insert("game_assets".into(), assets_dir.to_string_lossy().to_string());
    vars.insert("assets_index_name".into(), assets_index_name(vj));
    vars.insert("auth_uuid".into(), session.uuid.replace('-', ""));
    vars.insert("auth_access_token".into(), session.access_token.clone());
    vars.insert("auth_session".into(), session.access_token.clone());
    vars.insert("user_type".into(), session.user_type.clone());
    vars.insert("user_properties".into(), "{}".into());
    vars.insert("version_type".into(), vj.type_.clone());
    vars.insert("natives_directory".into(), natives_dir.to_string_lossy().to_string());
    vars.insert("launcher_name".into(), "mclauncher-rust1".into());
    vars.insert("launcher_version".into(), "0.1.0".into());
    vars.insert("classpath".into(), cp.clone());
    vars.insert("library_directory".into(), libs_dir.to_string_lossy().to_string());
    vars.insert("classpath_separator".into(), classpath_sep().into());
    vars.insert("resolution_width".into(), settings.window_width.to_string());
    vars.insert("resolution_height".into(), settings.window_height.to_string());

    let mut features = std::collections::HashMap::new();
    features.insert("has_custom_resolution", !settings.fullscreen);

    let mut jvm_args = vec![
        format!("-Xmx{}M", p.memory_mb),
        format!("-Djava.library.path={}", natives_dir.display()),
    ];
    if let Some(jar) = p.authlib_jar {
        jvm_args.push(format!("-javaagent:{}={}", jar.display(), session.authlib_api));
    }
    jvm_args.extend(split_args(&settings.jvm_args));

    if let Some(args) = &vj.arguments {
        if !args.jvm.is_empty() {
            jvm_args.extend(expand_arg_list(&args.jvm, &vars, &features));
        }
    }
    if !jvm_args.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm_args.push("-cp".into());
        jvm_args.push(cp.clone());
    } else {
        for i in 0..jvm_args.len() {
            if (jvm_args[i] == "-cp" || jvm_args[i] == "-classpath") && i + 1 < jvm_args.len() {
                if jvm_args[i + 1].contains("${") || jvm_args[i + 1].is_empty() {
                    jvm_args[i + 1] = cp.clone();
                }
            }
        }
    }

    let mut game_args = if let Some(args) = &vj.arguments {
        if !args.game.is_empty() {
            expand_arg_list(&args.game, &vars, &features)
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
    if settings.fullscreen {
        if !game_args.iter().any(|a| a == "--fullscreen") {
            game_args.push("--fullscreen".into());
        }
    } else if !game_args.iter().any(|a| a == "--width") {
        game_args.push("--width".into());
        game_args.push(settings.window_width.to_string());
        game_args.push("--height".into());
        game_args.push(settings.window_height.to_string());
    }

    let mut all = jvm_args;
    all.push(vj.main_class.clone());
    all.extend(game_args);
    all
}

/// Windows：调试模式用 java.exe（带控制台输出），否则优先 javaw.exe。
pub fn pick_java_exe(path: &str, debug: bool) -> String {
    if !cfg!(windows) {
        return path.to_string();
    }
    let lower = path.to_ascii_lowercase();
    if debug && lower.ends_with("javaw.exe") {
        let alt = format!("{}java.exe", &path[..path.len() - "javaw.exe".len()]);
        if Path::new(&alt).exists() {
            return alt;
        }
    }
    if !debug && lower.ends_with("\\java.exe") {
        let alt = format!("{}javaw.exe", &path[..path.len() - "java.exe".len()]);
        if Path::new(&alt).exists() {
            return alt;
        }
    }
    path.to_string()
}

/// 启动进度步骤（推送给前端 `launch-progress` 事件）。
pub const STEP_FILES: &str = "files";
pub const STEP_JAVA: &str = "java";
pub const STEP_ARGS: &str = "args";
pub const STEP_SPAWN: &str = "spawn";
pub const STEP_WATCH: &str = "watch";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LaunchProgress {
    /// files | java | args | spawn | watch
    pub step: String,
    /// running | done | failed
    pub state: String,
    pub detail: String,
}

pub type LaunchEmit = std::sync::Arc<dyn Fn(LaunchProgress) + Send + Sync>;

fn step(emit: &LaunchEmit, step: &str, state: &str, detail: impl Into<String>) {
    emit(LaunchProgress {
        step: step.into(),
        state: state.into(),
        detail: detail.into(),
    });
}

/// 无进度回调的便捷入口（测试/旧调用）。
#[allow(dead_code)]
pub async fn launch_game(settings: &Settings, account: Option<&Account>) -> LaunchResult {
    let emit: LaunchEmit = std::sync::Arc::new(|_| {});
    launch_game_with_progress(settings, account, emit).await
}

/// 创建（必要时）并返回本次启动的 --gameDir 目录。
pub fn prepare_run_dir(game_dir: &Path, version_id: &str, isolation: bool) -> Result<PathBuf, String> {
    let d = config::run_dir(game_dir, version_id, isolation);
    config::ensure_dir(&d).map_err(|e| format!("无法创建游戏运行目录 {}: {e}", d.display()))?;
    Ok(d)
}

pub async fn launch_game_with_progress(
    settings: &Settings,
    account: Option<&Account>,
    emit: LaunchEmit,
) -> LaunchResult {
    let fail_at = |s: &str, m: String| {
        step(&emit, s, "failed", m.clone());
        LaunchResult {
            message: m,
            failed_step: s.to_string(),
            ..Default::default()
        }
    };
    // ---- 1. 检查游戏文件 ----
    step(&emit, STEP_FILES, "running", "正在校验版本文件…");
    let game_dir = config::resolve_game_dir(settings);
    let version_id = settings.selected_version.trim();
    if version_id.is_empty() {
        return fail_at(STEP_FILES, "未选择版本，请先在下载中心安装并选择版本".into());
    }
    let versions_dir = game_dir.join("versions").join(version_id);
    let jar_path = versions_dir.join(format!("{version_id}.jar"));
    let json_path = versions_dir.join(format!("{version_id}.json"));
    if !json_path.exists() {
        return fail_at(STEP_FILES, format!("缺少版本元数据，请前往下载中心安装 {version_id}"));
    }
    // 启动前预检：库（仅本平台）、client jar、资源索引及全部资源对象的存在性与大小
    let precheck = {
        let g = game_dir.clone();
        let id = version_id.to_string();
        tokio::task::spawn_blocking(move || download::check_version_files(&g, &id))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r)
    };
    let checked = match precheck {
        Ok(r) if !r.ok => {
            let message = format!("检测到 {} 个缺失文件（{}），是否补全？", r.missing, r.summary());
            step(&emit, STEP_FILES, "failed", message.clone());
            return LaunchResult {
                ok: false,
                command: String::new(),
                message,
                need_repair: true,
                missing: r.missing,
                missing_by_category: r.missing_by_category,
                exit_code: None,
                failed_step: STEP_FILES.into(),
            };
        }
        Ok(r) => {
            if !r.install_complete {
                // 旧版本启动器安装、但文件已齐全：补写完成标记
                let _ = download::write_install_marker(&game_dir, version_id, r.checked as usize);
            }
            r.checked
        }
        Err(e) => return fail_at(STEP_FILES, e),
    };
    if !jar_path.exists() {
        return fail_at(STEP_FILES, format!("缺少客户端 jar，请前往下载中心安装 {version_id}"));
    }
    let vj = match download::load_local_version_json(&game_dir, version_id) {
        Ok(v) => v,
        Err(e) => return fail_at(STEP_FILES, e),
    };
    if vj.main_class.is_empty() {
        return fail_at(STEP_FILES, "版本 json 缺少 mainClass".into());
    }
    step(&emit, STEP_FILES, "done", format!("{checked} 个文件完整"));

    // ---- 2. 检查 / 定位 Java ----
    step(&emit, STEP_JAVA, "running", "正在定位 Java…");
    let java_info = {
        let p = settings.java_path.clone();
        tokio::task::spawn_blocking(move || detect_java(&p))
            .await
            .unwrap_or(JavaInfo { found: false, path: String::new(), version: String::new() })
    };
    if !java_info.found {
        return fail_at(STEP_JAVA, "未检测到 Java。请在 设置 → Java 中填写路径或点「查找java」。".into());
    }
    step(
        &emit,
        STEP_JAVA,
        "done",
        if java_info.version.is_empty() {
            java_info.path.clone()
        } else {
            format!("Java {}", java_info.version)
        },
    );

    // ---- 3. 构建启动参数 ----
    step(&emit, STEP_ARGS, "running", "正在构建启动参数…");
    let session = account.cloned().unwrap_or_else(|| crate::auth::offline_login("Player"));
    let authlib_jar = if session.mode == "authlib" && !session.authlib_api.is_empty() {
        step(&emit, STEP_ARGS, "running", "正在准备 authlib-injector…");
        match download::ensure_authlib_jar().await {
            Ok(j) => Some(j),
            Err(e) => return fail_at(STEP_ARGS, format!("准备 authlib-injector 失败: {e}")),
        }
    } else {
        None
    };
    let run_dir = match prepare_run_dir(&game_dir, version_id, settings.version_isolation) {
        Ok(d) => d,
        Err(e) => return fail_at(STEP_ARGS, e),
    };
    let memory_mb = effective_memory_mb(settings);
    let all_args = build_args(&LaunchPlan {
        settings,
        vj: &vj,
        version_id,
        game_dir: &game_dir,
        session: &session,
        authlib_jar: authlib_jar.as_deref(),
        memory_mb,
    });
    let java = pick_java_exe(&java_info.path, settings.debug_mode);
    let cmd_preview = format!("{} {}", java, all_args.join(" "));
    step(
        &emit,
        STEP_ARGS,
        "done",
        format!(
            "内存 {memory_mb} MB · {}",
            if settings.version_isolation { "版本隔离" } else { "共享 .minecraft" }
        ),
    );

    // ---- 4. 启动进程 ----
    step(&emit, STEP_SPAWN, "running", "正在启动 Java 进程…");
    let mut cmd = Command::new(&java);
    cmd.args(&all_args).current_dir(&run_dir);
    let log = config::data_dir().join("输出日志.txt");
    if settings.debug_mode {
        // 原版：开启调试模式将以 java.exe 启动 minecraft，并输出 输出日志.txt
        match std::fs::File::create(&log) {
            Ok(f) => {
                let f2 = f.try_clone().ok();
                cmd.stdout(f);
                if let Some(f2) = f2 {
                    cmd.stderr(f2);
                }
            }
            Err(e) => return fail_at(STEP_SPAWN, format!("无法创建日志文件: {e}")),
        }
    }
    #[cfg(windows)]
    if !settings.debug_mode {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let mut r = fail_at(STEP_SPAWN, format!("启动失败: {e}"));
            r.command = cmd_preview;
            return r;
        }
    };
    step(&emit, STEP_SPAWN, "done", format!("PID {}", child.id()));

    // ---- 5. 等待游戏窗口 / 观察期 ----
    step(
        &emit,
        STEP_WATCH,
        "running",
        format!("观察 {} 秒内是否异常退出…", EARLY_EXIT_WINDOW.as_secs()),
    );
    match wait_early_exit(&mut child, EARLY_EXIT_WINDOW).await {
        Some(code) => {
            let mut message = format!(
                "游戏启动后立即退出（退出码 {code}），请开启调试模式查看输出日志（data\\输出日志.txt）"
            );
            if settings.debug_mode {
                let tail = log_tail(&log, 15);
                if !tail.trim().is_empty() {
                    message.push_str("\n\n");
                    message.push_str(&tail);
                }
            }
            step(&emit, STEP_WATCH, "failed", format!("进程已退出（退出码 {code}）"));
            LaunchResult {
                ok: false,
                command: cmd_preview,
                message,
                exit_code: Some(code),
                failed_step: STEP_WATCH.into(),
                ..Default::default()
            }
        }
        None => {
            step(&emit, STEP_WATCH, "done", "进程运行正常");
            LaunchResult {
                ok: true,
                command: cmd_preview,
                message: format!("已启动 {version_id}（内存 {memory_mb} MB）"),
                ..Default::default()
            }
        }
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

/// 原版「查找java」：扫描常见安装位置，返回去重后的可执行路径（版本高的在前）。
pub fn scan_java() -> Vec<JavaInfo> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_file() && !found.iter().any(|f| f == &p) {
            found.push(p);
        }
    };
    let exe = if cfg!(windows) { "javaw.exe" } else { "java" };
    if let Ok(home) = std::env::var("JAVA_HOME") {
        push(PathBuf::from(home).join("bin").join(exe));
    }
    if let Ok(p) = which::which(if cfg!(windows) { "javaw" } else { "java" }) {
        push(p);
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for base in ["C:\\Program Files", "C:\\Program Files (x86)"] {
            for v in [
                "Java",
                "Eclipse Adoptium",
                "Microsoft",
                "Zulu",
                "BellSoft",
                "Amazon Corretto",
                "AdoptOpenJDK",
                "Semeru",
            ] {
                roots.push(PathBuf::from(base).join(v));
            }
        }
        if let Some(d) = dirs::data_local_dir() {
            roots.push(d.join("Programs"));
        }
        if let Some(d) = dirs::data_dir() {
            roots.push(d.join(".minecraft").join("runtime"));
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        if let Some(h) = dirs::home_dir() {
            roots.push(h.join("Library/Java/JavaVirtualMachines"));
        }
    } else {
        roots.push(PathBuf::from("/usr/lib/jvm"));
        roots.push(PathBuf::from("/opt"));
    }
    for r in roots {
        walk_for(&r, exe, 5, &mut push);
    }
    let mut list: Vec<JavaInfo> = found
        .into_iter()
        .map(|p| {
            let path = p.to_string_lossy().to_string();
            let version = java_version(&pick_java_exe(&path, true));
            JavaInfo {
                found: true,
                path,
                version,
            }
        })
        .collect();
    list.sort_by_key(|j| std::cmp::Reverse(java_major(&j.version)));
    list
}

fn walk_for(dir: &Path, exe: &str, depth: u32, push: &mut impl FnMut(PathBuf)) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().map(|n| n == "bin").unwrap_or(false) {
                push(p.join(exe));
            } else {
                walk_for(&p, exe, depth - 1, push);
            }
        }
    }
}

/// 从 `java -version` 首行解析主版本号："1.8.0_381" → 8，"17.0.2" → 17。
pub fn java_major(version_line: &str) -> u32 {
    let Some(start) = version_line.find('"') else {
        return 0;
    };
    let rest = &version_line[start + 1..];
    let v = rest.split('"').next().unwrap_or("");
    let mut parts = v.split(['.', '_', '-', '+']);
    let first: u32 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    if first == 1 {
        parts.next().and_then(|x| x.parse().ok()).unwrap_or(0)
    } else {
        first
    }
}

pub fn open_game_dir(settings: &Settings) -> Result<(), String> {
    let d = config::resolve_game_dir(settings);
    open_path(&d)
}

pub fn open_path(d: &Path) -> Result<(), String> {
    config::ensure_dir(d)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::VersionJson;

    fn vj_modern() -> VersionJson {
        serde_json::from_str(r#"{
            "id":"1.20.1","mainClass":"net.minecraft.client.main.Main","type":"release",
            "assetIndex":{"id":"5","sha1":"","size":0,"url":""},
            "arguments":{
              "game":["--username","${auth_player_name}","--uuid","${auth_uuid}","--gameDir","${game_directory}","--assetsDir","${assets_root}",
                {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"},
                {"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],"value":["--width","${resolution_width}","--height","${resolution_height}"]}],
              "jvm":[{"rules":[{"action":"allow","os":{"name":"osx"}}],"value":["-XstartOnFirstThread"]},"-cp","${classpath}"]
            },
            "libraries":[]
        }"#).unwrap()
    }

    fn plan_args(settings: &Settings, acc: &Account, jar: Option<&Path>) -> Vec<String> {
        let vj = vj_modern();
        build_args(&LaunchPlan {
            settings,
            vj: &vj,
            version_id: "1.20.1",
            game_dir: Path::new("/mc"),
            session: acc,
            authlib_jar: jar,
            memory_mb: 3072,
        })
    }

    fn arg_after(a: &[String], flag: &str) -> String {
        let i = a.iter().position(|x| x == flag).unwrap_or_else(|| panic!("missing {flag}"));
        a[i + 1].clone()
    }

    #[test]
    fn isolation_off_uses_shared_game_dir() {
        let mut s = Settings::default();
        s.version_isolation = false;
        let acc = crate::auth::offline_login("Steve");
        let a = plan_args(&s, &acc, None);
        assert_eq!(Path::new(&arg_after(&a, "--gameDir")), Path::new("/mc"));
        assert_eq!(Path::new(&arg_after(&a, "--assetsDir")), Path::new("/mc").join("assets"));
        let natives = Path::new("/mc").join("versions").join("1.20.1").join("natives");
        assert!(a.contains(&format!("-Djava.library.path={}", natives.display())));
    }

    #[test]
    fn isolation_on_uses_version_dir_but_shared_assets_and_libs() {
        let mut s = Settings::default();
        s.version_isolation = true;
        let acc = crate::auth::offline_login("Steve");
        let a = plan_args(&s, &acc, None);
        let vdir = Path::new("/mc").join("versions").join("1.20.1");
        assert_eq!(Path::new(&arg_after(&a, "--gameDir")), vdir.as_path());
        // 资源与库仍共享
        assert_eq!(Path::new(&arg_after(&a, "--assetsDir")), Path::new("/mc").join("assets"));
        let cp = arg_after(&a, "-cp");
        // classpath 不受隔离影响：客户端 jar 仍取自共享 versions 目录
        assert_eq!(Path::new(&cp), vdir.join("1.20.1.jar").as_path(), "{cp}");
        assert!(a.contains(&format!("-Djava.library.path={}", vdir.join("natives").display())));
    }

    #[test]
    fn isolation_default_is_on() {
        assert!(Settings::default().version_isolation);
    }

    #[test]
    fn prepare_run_dir_creates_dirs() {
        let g = std::env::temp_dir().join(format!("mcl-rundir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&g);
        std::fs::create_dir_all(&g).unwrap();
        let on = prepare_run_dir(&g, "1.20.1", true).unwrap();
        assert_eq!(on, g.join("versions").join("1.20.1"));
        assert!(on.is_dir());
        let off = prepare_run_dir(&g, "1.20.1", false).unwrap();
        assert_eq!(off, g);
        assert!(off.is_dir());
        // 已有数据不被破坏
        std::fs::write(on.join("options.txt"), "lang:zh_cn").unwrap();
        prepare_run_dir(&g, "1.20.1", true).unwrap();
        assert_eq!(std::fs::read_to_string(on.join("options.txt")).unwrap(), "lang:zh_cn");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[tokio::test]
    async fn launch_progress_reports_failed_files_step() {
        let g = std::env::temp_dir().join(format!("mcl-launch-prog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&g);
        let v = g.join("versions").join("1.0");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(
            v.join("1.0.json"),
            r#"{"id":"1.0","mainClass":"M","downloads":{"client":{"sha1":"","size":3,"url":"u"}},"assetIndex":{"id":"1","sha1":"","size":5,"url":"u"}}"#,
        )
        .unwrap();
        let mut s = Settings::default();
        s.game_dir = g.to_string_lossy().to_string();
        s.selected_version = "1.0".into();
        let evs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<LaunchProgress>::new()));
        let e2 = evs.clone();
        let emit: LaunchEmit = std::sync::Arc::new(move |e| e2.lock().unwrap().push(e));
        let r = launch_game_with_progress(&s, None, emit).await;
        assert!(r.need_repair);
        assert_eq!(r.failed_step, STEP_FILES);
        let evs = evs.lock().unwrap();
        assert_eq!(evs.first().unwrap().state, "running");
        let last = evs.last().unwrap();
        assert_eq!((last.step.as_str(), last.state.as_str()), (STEP_FILES, "failed"));
        assert!(!evs.iter().any(|e| e.step == STEP_JAVA), "后续步骤不应开始");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[tokio::test]
    async fn launch_progress_no_version_fails_first_step() {
        let mut s = Settings::default();
        s.selected_version = String::new();
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let emit: LaunchEmit = std::sync::Arc::new(move |e| {
            assert_eq!(e.step, STEP_FILES);
            n2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let r = launch_game_with_progress(&s, None, emit).await;
        assert!(!r.ok);
        assert_eq!(r.failed_step, STEP_FILES);
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn offline_args_basic() {
        let s = Settings::default();
        let acc = crate::auth::offline_login("Steve");
        let a = plan_args(&s, &acc, None);
        assert!(a.contains(&"-Xmx3072M".to_string()));
        assert!(a.contains(&"5627dd98e6be3c21b8a8e92344183641".to_string()));
        assert!(!a.contains(&"--demo".to_string()), "demo must be excluded");
        let w = a.iter().position(|x| x == "--width").unwrap();
        assert_eq!(a[w + 1], "854");
        assert!(!a.iter().any(|x| x.starts_with("-javaagent")));
        // 默认开启版本隔离：gameDir 为版本目录
        let g = a.iter().position(|x| x == "--gameDir").unwrap();
        assert_eq!(Path::new(&a[g + 1]), Path::new("/mc").join("versions").join("1.20.1"));
    }

    #[test]
    fn authlib_injects_javaagent() {
        let s = Settings::default();
        let mut acc = crate::auth::offline_login("x");
        acc.mode = "authlib".into();
        acc.authlib_api = "https://littleskin.cn/api/yggdrasil".into();
        let a = plan_args(&s, &acc, Some(Path::new("/d/authlib-injector.jar")));
        assert!(a.contains(&"-javaagent:/d/authlib-injector.jar=https://littleskin.cn/api/yggdrasil".to_string()));
    }

    #[test]
    fn settings_reach_args() {
        let mut s = Settings::default();
        s.version_isolation = true;
        s.fullscreen = true;
        s.jvm_args = r#"-XX:+UseG1GC "-Dfoo=a b""#.into();
        let acc = crate::auth::offline_login("Steve");
        let a = plan_args(&s, &acc, None);
        assert!(a.contains(&"-XX:+UseG1GC".to_string()));
        assert!(a.contains(&"-Dfoo=a b".to_string()));
        assert!(a.contains(&"--fullscreen".to_string()));
        assert!(!a.contains(&"--width".to_string()));
        let g = a.iter().position(|x| x == "--gameDir").unwrap();
        assert_eq!(Path::new(&a[g + 1]), Path::new("/mc").join("versions").join("1.20.1"));
    }

    #[test]
    fn memory_auto_rule() {
        assert_eq!(auto_memory_mb(500), 1024);
        assert_eq!(auto_memory_mb(4000), 2304);
        assert_eq!(auto_memory_mb(64000), 4096);
    }

    #[test]
    fn split_and_major() {
        assert_eq!(split_args("  -Xss1M   -Da=\"x y\" "), vec!["-Xss1M", "-Da=x y"]);
        assert!(split_args("").is_empty());
        assert_eq!(java_major(r#"java version "1.8.0_381""#), 8);
        assert_eq!(java_major(r#"openjdk version "17.0.2" 2022-01-18"#), 17);
        assert_eq!(java_major("garbage"), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn early_exit_detected() {
        let mut c = std::process::Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        assert_eq!(wait_early_exit(&mut c, EARLY_EXIT_WINDOW).await, Some(3));
        let mut c = std::process::Command::new("sh").args(["-c", "sleep 2"]).spawn().unwrap();
        let t = std::time::Instant::now();
        assert_eq!(wait_early_exit(&mut c, std::time::Duration::from_millis(300)).await, None);
        assert!(t.elapsed() < std::time::Duration::from_secs(1));
        let _ = c.kill();
    }

    #[tokio::test]
    async fn launch_blocks_when_files_missing() {
        let g = std::env::temp_dir().join(format!("mcl-launch-pre-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&g);
        let v = g.join("versions").join("1.0");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(
            v.join("1.0.json"),
            r#"{"id":"1.0","mainClass":"M","downloads":{"client":{"sha1":"","size":3,"url":"u"}},"assetIndex":{"id":"1","sha1":"","size":5,"url":"u"}}"#,
        )
        .unwrap();
        let mut s = Settings::default();
        s.game_dir = g.to_string_lossy().to_string();
        s.selected_version = "1.0".into();
        let r = launch_game(&s, None).await;
        assert!(!r.ok && r.need_repair);
        assert_eq!(r.missing, 2);
        assert!(r.message.contains("检测到 2 个缺失文件"), "{}", r.message);
    }
}
