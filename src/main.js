/**
 * MCLauncher 5.31 Rust+Tauri rewrite — vanilla frontend
 * 设计坐标 720×450，对照原版 MainWindow / MelodyTheme 像素布局。
 */
const T = window.__TAURI__;
const isTauri = !!(window.__TAURI_INTERNALS__ || T);

/* ======================= 后端调用 / 浏览器预览 mock ======================= */
const mockListeners = {};
/* 预览模式：模拟启动进度事件（files → java → args → spawn → watch） */
async function mockLaunch() {
  const emit = (step, st, detail) => (mockListeners["launch-progress"] || []).forEach((cb) => cb({ payload: { step, state: st, detail } }));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const steps = [["files", "5147 个文件完整"], ["java", "Java 17.0.8"], ["args", "内存 4096 MB · 版本隔离"], ["spawn", "PID 1234"], ["watch", "进程运行正常"]];
  for (const [k, d] of steps) { emit(k, "running", "…"); await wait(k === "watch" ? 1200 : 350); emit(k, "done", d); }
  return { ok: true, command: "java ...", message: "已启动（预览模式）" };
}
async function invoke(cmd, args = {}) {
  if (!isTauri) return mockInvoke(cmd, args);
  if (T?.core?.invoke) return T.core.invoke(cmd, args);
  const { invoke: inv } = await import("@tauri-apps/api/core");
  return inv(cmd, args);
}
async function listen(event, cb) {
  if (!isTauri) {
    (mockListeners[event] ||= []).push(cb);
    return () => { mockListeners[event] = mockListeners[event].filter((f) => f !== cb); };
  }
  if (T?.event?.listen) return T.event.listen(event, cb);
  const { listen: l } = await import("@tauri-apps/api/event");
  return l(event, cb);
}
async function currentWindow() {
  if (!isTauri) return null;
  if (T?.window?.getCurrentWindow) return T.window.getCurrentWindow();
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}
async function openUrl(url) {
  if (!isTauri) { window.open(url, "_blank"); return; }
  if (T?.opener?.openUrl) return T.opener.openUrl(url);
  const { openUrl: o } = await import("@tauri-apps/plugin-opener");
  return o(url);
}
async function pickPath(opts) {
  if (!isTauri) return null;
  if (T?.dialog?.open) return T.dialog.open(opts);
  const { open } = await import("@tauri-apps/plugin-dialog");
  return open(opts);
}

const mockDb = {
  settings: {
    download_source: "bmclapi", download_concurrency: 32, game_dir: "", version_isolation: true, settings_rev: 2,
    window_width: 854, window_height: 480, fullscreen: false, after_launch: "keep", debug_mode: false,
    selected_version: "1.20.1", java_path: "", memory_auto: true, max_memory_mb: 2048, jvm_args: "",
    skin: "default", background_path: "",
    auth_servers: [{ name: "LittleSkin", api_root: "https://littleskin.cn/api/yggdrasil" }],
  },
  accounts: {
    accounts: [
      { mode: "offline", username: "Steve", uuid: "5627dd98-e6be-3c21-b8a8-e92344183641", access_token: "0", user_type: "legacy", authlib_api: "" },
      { mode: "authlib", username: "Alex_LS", uuid: "0f3c…", access_token: "t", user_type: "mojang", authlib_api: "https://littleskin.cn/api/yggdrasil" },
    ],
  },
  versions: ["1.20.1", "1.12.2", "1.8.9"],
  dlTimer: null,
};
mockDb.accounts.current = mockDb.accounts.accounts[0];
function mockProgress(id) {
  const cats = [
    ["VersionJson", "版本 JSON", 1, 4096], ["ClientJar", "客户端 Jar", 1, 24_000_000],
    ["Libraries", "依赖库", 48, 61_000_000], ["AssetIndex", "资源索引", 1, 400_000], ["Assets", "资源文件", 3712, 380_000_000],
  ];
  let t = 0;
  return new Promise((resolve) => {
    const tick = () => {
      t += 1;
      const f = Math.min(1, t / 40);
      const categories = cats.map(([category, label, total, bytes], i) => {
        const local = Math.max(0, Math.min(1, f * 5 - i * 0.6));
        const done = Math.floor(total * local);
        return { category, label, total, done, skipped: 0, failed: 0, bytes_total: bytes, bytes_done: Math.floor(bytes * local), state: local >= 1 ? "done" : local > 0 ? "running" : "waiting" };
      });
      const sum = (k) => categories.reduce((a, c) => a + c[k], 0);
      const ev = {
        version_id: id, stage: f >= 1 ? "done" : "files", categories,
        total_files: sum("total"), done_files: sum("done"), skipped_files: 0, failed_files: 0,
        bytes_total: sum("bytes_total"), bytes_done: sum("bytes_done"), speed_bps: 6_400_000,
        current: f >= 1 ? [] : ["minecraft/sounds/ambient/cave/cave13.ogg", "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar"],
        failed: [], message: f >= 1 ? `${id} 安装完成` : "",
      };
      (mockListeners["download-progress"] || []).forEach((cb) => cb({ payload: ev }));
      if (f >= 1 || mockDb.cancel) {
        clearInterval(mockDb.dlTimer);
        if (mockDb.cancel) {
          (mockListeners["download-progress"] || []).forEach((cb) => cb({ payload: { ...ev, stage: "cancelled", message: "已取消" } }));
          resolve({ ok: false, message: "已取消", failed: [] });
        } else resolve({ ok: true, message: `${id} 安装完成`, failed: [] });
      }
    };
    mockDb.cancel = false;
    mockDb.dlTimer = setInterval(tick, 150);
  });
}
function mockInvoke(cmd, args) {
  const m = {
    get_settings: () => ({ ...mockDb.settings }),
    set_settings: ({ settings }) => (mockDb.settings = { ...settings }),
    list_accounts: () => mockDb.accounts,
    offline_login: ({ username }) => {
      const a = { mode: "offline", username: username || "Player", uuid: "00000000-0000-3000-8000-000000000000", access_token: "0", user_type: "legacy", authlib_api: "" };
      mockDb.accounts.accounts.push(a); mockDb.accounts.current = a; return a;
    },
    yggdrasil_login: () => { throw new Error("浏览器预览模式：请用 Tauri 运行完整登录"); },
    microsoft_login: () => { throw new Error("微软正版登录暂未开放"); },
    switch_account: ({ index }) => (mockDb.accounts.current = mockDb.accounts.accounts[index]),
    remove_account: ({ index }) => { mockDb.accounts.accounts.splice(index, 1); return mockDb.accounts; },
    logout: () => { mockDb.accounts.current = null; },
    get_download_sources: () => [{ id: "official", name: "Mojang 官方源" }, { id: "bmclapi", name: "BMCLAPI 镜像源" }],
    list_remote_versions: () => [
      { id: "1.21.3", type: "release", url: "", release_time: "2024-10-23" },
      { id: "24w14potato", type: "snapshot", url: "", release_time: "2024-04-01" },
      { id: "1.20.1", type: "release", url: "", release_time: "2023-06-12" },
      { id: "23w31a", type: "snapshot", url: "", release_time: "2023-08-01" },
      { id: "1.12.2", type: "release", url: "", release_time: "2017-09-18" },
      { id: "b1.7.3", type: "old_beta", url: "", release_time: "2011-07-08" },
    ],
    list_local_versions: () => [...mockDb.versions],
    download_version: ({ versionId }) => mockProgress(versionId),
    repair_version: ({ versionId }) => mockProgress(versionId),
    check_version_files: ({ versionId }) => ({ version_id: versionId, ok: false, checked: 5147, missing: 3219, missing_by_category: [{ category: "Assets", label: "资源文件", count: 3219 }], samples: [], install_complete: false }),
    cancel_download: () => { mockDb.cancel = true; return true; },
    rename_version: ({ old, new: n }) => { mockDb.versions = mockDb.versions.map((v) => (v === old ? n : v)); },
    delete_version: ({ id }) => { mockDb.versions = mockDb.versions.filter((v) => v !== id); },
    detect_java: () => ({ found: true, path: "C:\\Program Files\\Java\\jdk-17\\bin\\javaw.exe", version: 'java version "17.0.8"' }),
    scan_java: () => [
      { found: true, path: "C:\\Program Files\\Eclipse Adoptium\\jdk-21\\bin\\javaw.exe", version: 'openjdk version "21.0.2"' },
      { found: true, path: "C:\\Program Files\\Java\\jre1.8.0_381\\bin\\javaw.exe", version: 'java version "1.8.0_381"' },
    ],
    system_memory: () => ({ available_mb: 9830, auto_mb: 4096 }),
    launch_game: () => mockLaunch(),
    open_game_dir: () => null, open_data_dir: () => null,
    app_info: () => ({ version: "0.1.0", data_dir: "C:\\MCLauncher\\melody-launcher", settings_file: "C:\\MCLauncher\\melody-launcher\\settings.json", game_dir: "C:\\MCLauncher\\.minecraft" }),
    read_image_data_url: () => "/assets/backgrounds/sky.png",
  };
  const fn = m[cmd];
  try { return Promise.resolve(fn ? fn(args) : null); } catch (e) { return Promise.reject(e); }
}

/* ============================== 工具 ============================== */
const state = {
  settings: null, account: null, accounts: { accounts: [], current: null },
  localVersions: [], remoteVersions: [], selectedRemote: null,
  settingsTab: "account", selVersion: null, javaList: [], mem: null, appInfo: null,
  downloading: false,
};
const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
const esc = (s) => String(s ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const errText = (e) => e?.message || (typeof e === "string" ? e : JSON.stringify(e));

/* 淡入淡出（时序来源见 styles.css 顶部注释：exe 0x4431be SetTimer 50ms，0x43e968 每次 alpha+0x33 → 250ms / 5 级） */
const FADE_MS = 250;
function fadeIn(el) {
  if (!el) return;
  el.classList.remove("fade-out");
  void el.offsetWidth; // 重新触发动画
  el.classList.add("fade-in");
}
function fadeOut(el) {
  return new Promise((resolve) => {
    if (!el) return resolve();
    el.classList.remove("fade-in");
    void el.offsetWidth;
    el.classList.add("fade-out");
    let done = false;
    const fin = () => { if (done) return; done = true; el.removeEventListener("animationend", fin); resolve(); };
    el.addEventListener("animationend", fin);
    setTimeout(fin, FADE_MS + 60);
  });
}
async function hideFaded(el, cls = "show") {
  if (!el || !el.classList.contains(cls)) return;
  await fadeOut(el);
  el.classList.remove(cls, "fade-out");
}
function showFaded(el, cls = "show") {
  el.classList.add(cls);
  fadeIn(el);
}

function icon(kind, color = "rgba(255,255,255,0.902)") {
  const p = {
    close: "M1,1 L11,11 M11,1 L1,11",
    min: "M1,9 L11,9",
    max: "M1.5,1.5 H10.5 V10.5 H1.5 Z",
    restore: "M3.5,1.5 H10.5 V8.5 M1.5,3.5 H8.5 V10.5 H1.5 Z",
  }[kind];
  return `<svg width="12" height="12" viewBox="0 0 12 12"><path d="${p}" stroke="${color}" stroke-width="2" fill="none"/></svg>`;
}
function radio(name, value, label, checked, extra = "") {
  return `<label class="radio-row" ${extra}><input type="radio" name="${name}" value="${esc(value)}" ${checked ? "checked" : ""}/><span class="gfx"></span><span>${label}</span></label>`;
}
function check(id, label, checked, extra = "") {
  return `<label class="check-row" ${extra}><input type="checkbox" id="${id}" ${checked ? "checked" : ""}/><span class="gfx"></span><span>${label}</span></label>`;
}
const radioVal = (name) => $$(`input[name="${name}"]`).find((x) => x.checked)?.value;
function fmtBytes(n) {
  if (!n) return "0 B";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0; let v = n;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(i ? 1 : 0)} ${u[i]}`;
}

/* ============================== 主界面 ============================== */
function renderApp() {
  $("#app").innerHTML = `
  <div class="stage" id="stage">
    <div class="content-card" id="contentCard"></div>
    <div class="drag-region" data-tauri-drag-region></div>
    <div class="layer" id="layer">
      <button class="btn btn-big no-drag" id="btnLaunch">启动游戏</button>
      <button class="btn btn-update no-drag" id="btnUpdate">更新游戏</button>
      <button class="btn btn-settings no-drag" id="btnSettings">设置</button>
      <div class="version-box no-drag" id="versionBox" title="当前版本">未安装版本</div>
      <button class="btn btn-text no-drag" id="btnVersionSettings">版本设置</button>
      <div class="main-status" id="mainStatus"></div>
      <div class="credit" id="credit">MineCraft Launcher 5.31 by 忘却的旋律</div>
    </div>
    <div class="ctx-menu" id="ctxMenu">
      <div class="ctx-item" data-act="download">下载中心…</div>
      <div class="ctx-item" data-act="version">版本设置…</div>
      <div class="ctx-item" data-act="account">账户…</div>
      <div class="ctx-item" data-act="source">下载源…</div>
      <div class="ctx-sep"></div>
      <div class="ctx-item" data-act="about">关于…</div>
      <div class="ctx-sep"></div>
      <div class="ctx-item" data-act="open-dir">打开游戏目录</div>
    </div>
    <div class="download-page" id="downloadPage"></div>
    <div class="overlay-host" id="overlayHost"></div>
    <div class="dlg-host" id="dlgHost"></div>
    <div class="msg-host" id="msgHost"></div>
    <div class="win-btns no-drag">
      <button class="win-btn" id="btnMin" title="最小化">${icon("min")}</button>
      <button class="win-btn" id="btnMax" title="最大化">${icon("max")}</button>
      <button class="win-btn" id="btnClose" title="关闭">${icon("close")}</button>
    </div>
  </div>`;
  bindMain();
}

/** 等比缩放：--s = min(w/720, h/450)；最大化时去掉透明外框。 */
async function applyScale() {
  const s = Math.max(0.5, Math.min(window.innerWidth / 720, window.innerHeight / 450));
  $("#app").style.setProperty("--s", String(s));
  let max = false;
  try { const w = await currentWindow(); if (w) max = await w.isMaximized(); } catch { /* ignore */ }
  if (!isTauri) max = window.innerWidth > 720 || window.innerHeight > 450;
  $("#app").classList.toggle("maximized", max);
  const b = $("#btnMax");
  if (b) { b.innerHTML = icon(max ? "restore" : "max"); b.title = max ? "还原" : "最大化"; }
}

async function closeApp() {
  await fadeOut($("#stage")); // 关闭：250ms 淡出后退出（与淡入镜像，推定）
  const w = await currentWindow();
  if (w) await w.close(); else window.close();
}

function bindMain() {
  $("#btnClose").onclick = closeApp;
  $("#btnMin").onclick = async () => { const w = await currentWindow(); if (w) w.minimize(); };
  $("#btnMax").onclick = async () => { const w = await currentWindow(); if (w) { await w.toggleMaximize(); setTimeout(applyScale, 60); } };
  $(".drag-region").addEventListener("dblclick", () => $("#btnMax").click());
  $("#btnLaunch").onclick = () => doLaunch();
  $("#btnUpdate").onclick = () => showDownload();
  $("#btnSettings").onclick = () => openSettings("account");
  $("#btnVersionSettings").onclick = () => openSettings("versions");
  $("#versionBox").onclick = () => openSettings("versions");

  $("#app").addEventListener("contextmenu", (e) => {
    e.preventDefault();
    if (e.target.closest("input,textarea")) return;
    const menu = $("#ctxMenu");
    const s = parseFloat($("#app").style.getPropertyValue("--s")) || 1;
    const W = window.innerWidth / s; const H = window.innerHeight / s;
    menu.style.left = Math.min(e.clientX / s, W - 190) + "px";
    menu.style.top = Math.min(e.clientY / s, H - 190) + "px";
    menu.classList.add("show");
  });
  document.addEventListener("click", () => $("#ctxMenu").classList.remove("show"));
  $$("#ctxMenu .ctx-item").forEach((el) => {
    el.onclick = () => {
      const act = el.dataset.act;
      if (act === "download") showDownload();
      else if (act === "version") openSettings("versions");
      else if (act === "account") openSettings("account");
      else if (act === "source") openSettings("download");
      else if (act === "about") openSettings("about");
      else if (act === "open-dir") invoke("open_game_dir").catch(showError);
    };
  });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if ($("#msgHost").classList.contains("show")) $("#msgOk")?.click();
    else if ($("#dlgHost").classList.contains("show")) { if (!state.downloading) closeDlg(); }
    else if ($("#overlayHost").classList.contains("show")) hideOverlays();
    else if ($("#downloadPage").classList.contains("show")) hideFaded($("#downloadPage"));
  });
  window.addEventListener("resize", applyScale);
}

function setStatus(text, ms = 4000) {
  const el = $("#mainStatus");
  el.textContent = text || "";
  el.classList.toggle("show", !!text);
  if (text && ms > 0) setTimeout(() => { if (el.textContent === text) el.classList.remove("show"); }, ms);
}
function showError(err) { showMessage("错误", errText(err)); }

/** 原版消息框（320×150，带 确认）；opts.input 时作为输入框；返回 Promise。 */
function showMessage(title, body, opts = {}) {
  return new Promise((resolve) => {
    const host = $("#msgHost");
    host.innerHTML = `
      <div class="msg-box">
        <div class="msg-title">${esc(title)}</div>
        <div class="msg-body">${esc(body)}</div>
        ${opts.input !== undefined ? `<input class="field" id="msgInput" value="${esc(opts.input)}"/>` : ""}
        <div class="msg-actions">
          ${opts.cancel ? `<button class="btn" id="msgCancel">取消</button>` : ""}
          <button class="btn" id="msgOk">确认</button>
        </div>
      </div>`;
    showFaded(host);
    const inp = $("#msgInput");
    if (inp) { inp.focus(); inp.select(); inp.onkeydown = (e) => { if (e.key === "Enter") $("#msgOk").click(); }; }
    const done = async (v) => { await hideFaded(host); resolve(v); };
    $("#msgOk").onclick = () => done(inp ? inp.value : true);
    if (opts.cancel) $("#msgCancel").onclick = () => done(null);
  });
}

async function hideOverlays() {
  await hideFaded($("#overlayHost"));
  $("#overlayHost").innerHTML = "";
}

/* ============================== 设置 ============================== */
const TABS = [
  ["account", "账户"], ["authlib", "外置登录"], ["game", "游戏"], ["versions", "游戏管理"],
  ["java", "Java"], ["download", "下载"], ["skin", "外观"], ["about", "关于"],
];

async function openSettings(tab) {
  state.settingsTab = tab || state.settingsTab;
  await hideFaded($("#downloadPage"));
  const host = $("#overlayHost");
  const wasOpen = host.classList.contains("show");
  renderSettings();
  if (!wasOpen) showFaded(host);
}

function renderSettings() {
  const host = $("#overlayHost");
  const tab = state.settingsTab;
  const title = TABS.find((t) => t[0] === tab)?.[1] || "";
  host.innerHTML = `
    <div class="overlay-panel" id="setPanel">
      <button class="overlay-close" id="ovClose" title="关闭">${icon("close", "rgba(255,255,255,0.949)")}</button>
      <div class="group-box" style="left:15px;top:30px;width:120px;height:222px;"></div>
      <div class="group-title" style="left:25px;top:22px;">设置</div>
      <div class="listbox submenu" style="left:23px;top:42px;width:104px;height:200px;">
        ${TABS.map(([k, v]) => `<div class="li ${k === tab ? "sel" : ""}" data-tab="${k}">${v}</div>`).join("")}
      </div>
      <div class="group-box" style="left:147px;top:30px;width:536px;height:222px;"></div>
      <div class="group-title" style="left:157px;top:22px;">${title}</div>
      <div id="pane"></div>
      <div class="note pane-status" id="paneStatus"></div>
    </div>`;
  $("#ovClose").onclick = hideOverlays;
  $$(".submenu .li", host).forEach((li) => {
    li.onclick = () => { state.settingsTab = li.dataset.tab; renderSettings(); };
  });
  const pane = $("#pane");
  ({ account: paneAccount, authlib: paneAuthlib, game: paneGame, versions: paneVersions,
    java: paneJava, download: paneDownload, skin: paneSkin, about: paneAbout })[tab](pane);
}
function paneMsg(t, err = false) {
  const el = $("#paneStatus");
  if (!el) return;
  el.textContent = t || "";
  el.style.color = err ? "#B71C1C" : "";
}
async function saveSettings(patch, msg = "设置已保存") {
  try {
    state.settings = await invoke("set_settings", { settings: { ...state.settings, ...patch } });
    paneMsg(msg);
    applySkin();
    return true;
  } catch (e) { paneMsg(errText(e), true); return false; }
}
async function reloadAccounts() {
  state.accounts = (await invoke("list_accounts")) || { accounts: [], current: null };
  state.account = state.accounts.current || null;
}
const sameAcc = (a, b) => a && b && a.username === b.username && a.mode === b.mode && (a.authlib_api || "") === (b.authlib_api || "");
const modeName = (a) => (a.mode === "authlib" ? "外置" : "离线");

/* --- 账户（原版：游戏名字 / 密码 + 正版登录 / 删除正版信息） --- */
function paneAccount(p) {
  const acc = state.account;
  const list = state.accounts.accounts || [];
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">当前账户：</div>
    <div class="abs" style="left:226px;top:44px;width:340px;line-height:24px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">
      ${acc ? `<b>${esc(acc.username)}</b> <span class="note">（${modeName(acc)}）${esc(acc.uuid || "")}</span>` : `<span class="note">未登录（启动时以 Player 离线进入）</span>`}
    </div>
    <div class="label abs" style="left:160px;top:74px;">游戏名字</div>
    <input class="field abs" id="offName" style="left:220px;top:74px;width:200px;" maxlength="16" value="${acc?.mode === "offline" ? esc(acc.username) : ""}" placeholder="离线玩家名"/>
    <button class="btn sm" id="btnOffline" style="left:430px;top:73px;width:90px;">离线登录</button>
    <div class="label abs" style="left:160px;top:104px;">已保存账户</div>
    <div class="listbox" id="accList" style="left:160px;top:128px;width:410px;height:90px;">
      ${list.length ? list.map((a, i) => `<div class="li ${sameAcc(a, acc) ? "sel" : ""}" data-i="${i}">${esc(a.username)}<span class="sub">${modeName(a)}${a.authlib_api ? " · " + esc(a.authlib_api) : ""}</span></div>`).join("") : `<div class="empty">暂无保存的账户</div>`}
    </div>
    <button class="btn sm" id="btnMs" style="left:580px;top:44px;width:90px;" title="暂未开放">微软登录</button>
    <button class="btn sm" id="btnSwitch" style="left:580px;top:128px;width:90px;">切换</button>
    <button class="btn sm" id="btnRemove" style="left:580px;top:160px;width:90px;">删除</button>
    <button class="btn sm" id="btnLogout" style="left:580px;top:192px;width:90px;">退出登录</button>`;
  let sel = list.findIndex((a) => sameAcc(a, acc));
  $$("#accList .li").forEach((li) => {
    li.onclick = () => { sel = +li.dataset.i; $$("#accList .li").forEach((x) => x.classList.toggle("sel", x === li)); };
    li.ondblclick = () => $("#btnSwitch").click();
  });
  $("#btnMs").onclick = () => paneMsg("微软正版登录：暂未开放", true);
  $("#btnOffline").onclick = async () => {
    try {
      const username = $("#offName").value.trim() || "Player";
      if (!/^[A-Za-z0-9_]{1,16}$/.test(username)) paneMsg("提示：建议玩家名仅用字母、数字、下划线（≤16）");
      await invoke("offline_login", { username });
      await reloadAccounts();
      renderSettings(); paneMsg(`已离线登录：${username}`); setStatus(`离线登录：${username}`);
    } catch (e) { paneMsg(errText(e), true); }
  };
  $("#btnSwitch").onclick = async () => {
    if (sel < 0) return paneMsg("请先选择账户", true);
    try { await invoke("switch_account", { index: sel }); await reloadAccounts(); renderSettings(); paneMsg(`已切换到 ${state.account?.username}`); }
    catch (e) { paneMsg(errText(e), true); }
  };
  $("#btnRemove").onclick = async () => {
    if (sel < 0) return paneMsg("请先选择账户", true);
    try { await invoke("remove_account", { index: sel }); await reloadAccounts(); renderSettings(); paneMsg("已删除账户"); }
    catch (e) { paneMsg(errText(e), true); }
  };
  $("#btnLogout").onclick = async () => {
    await invoke("logout"); await reloadAccounts(); renderSettings(); paneMsg("已退出登录");
  };
}

/* --- 外置登录（authlib-injector，启动时注入 -javaagent） --- */
function paneAuthlib(p) {
  const servers = state.settings.auth_servers || [];
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">认证服务器</div>
    <select class="field abs" id="srvSel" style="left:232px;top:44px;width:338px;">
      ${servers.length ? servers.map((s, i) => `<option value="${i}">${esc(s.name)} — ${esc(s.api_root)}</option>`).join("") : `<option value="">（无，请在下方添加）</option>`}
    </select>
    <button class="btn sm" id="btnSrvDel" style="left:580px;top:43px;width:90px;">删除服务器</button>
    <div class="label abs" style="left:160px;top:76px;">名称</div>
    <input class="field abs" id="srvName" style="left:196px;top:76px;width:100px;" placeholder="LittleSkin"/>
    <div class="label abs" style="left:304px;top:76px;">API</div>
    <input class="field abs" id="srvApi" style="left:332px;top:76px;width:238px;" placeholder="https://littleskin.cn/api/yggdrasil"/>
    <button class="btn sm" id="btnSrvAdd" style="left:580px;top:75px;width:90px;">添加服务器</button>
    <div class="label abs" style="left:160px;top:116px;">账号</div>
    <input class="field abs" id="yggUser" style="left:196px;top:116px;width:220px;" placeholder="邮箱 / 用户名"/>
    <div class="label abs" style="left:160px;top:146px;">密码</div>
    <input class="field abs" id="yggPass" type="password" style="left:196px;top:146px;width:220px;"/>
    <button class="btn sm" id="btnYgg" style="left:430px;top:145px;width:90px;">外置登录</button>
    <div class="note abs" style="left:160px;top:180px;width:510px;line-height:16px;">登录成功后账户保存在「账户」中；启动时自动下载 authlib-injector 并注入<br/>-javaagent:authlib-injector.jar=&lt;API 地址&gt;</div>`;
  $("#btnSrvAdd").onclick = async () => {
    const name = $("#srvName").value.trim(); const api = $("#srvApi").value.trim();
    if (!/^https?:\/\//i.test(api)) return paneMsg("API 地址需以 http(s):// 开头", true);
    const list = [...servers, { name: name || new URL(api).host, api_root: api }];
    if (await saveSettings({ auth_servers: list }, "已添加服务器")) renderSettings();
  };
  $("#btnSrvDel").onclick = async () => {
    const i = $("#srvSel").value; if (i === "") return;
    const list = servers.filter((_, k) => k !== +i);
    if (await saveSettings({ auth_servers: list }, "已删除服务器")) renderSettings();
  };
  $("#btnYgg").onclick = async () => {
    const i = $("#srvSel").value;
    const api = i === "" ? $("#srvApi").value.trim() : servers[+i].api_root;
    if (!api) return paneMsg("请先添加认证服务器", true);
    paneMsg("正在登录…");
    try {
      const a = await invoke("yggdrasil_login", { apiRoot: api, username: $("#yggUser").value.trim(), password: $("#yggPass").value });
      $("#yggPass").value = "";
      await reloadAccounts();
      paneMsg(`外置登录成功：${a.username}`); setStatus(`外置登录：${a.username}`);
    } catch (e) { paneMsg(errText(e), true); }
  };
}

/* --- 游戏（原版：游戏目录 单选 + 调试模式；扩展：窗口大小 / 全屏 / 启动后行为） --- */
function paneGame(p) {
  const s = state.settings;
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">游戏路径</div>
    <input class="field abs" id="gDir" style="left:220px;top:44px;width:250px;" value="${esc(s.game_dir)}" placeholder="空 = 启动器旁 .minecraft"/>
    <button class="btn sm" id="gBrowse" style="left:480px;top:43px;width:90px;">浏览</button>
    <button class="btn sm" id="gOpen" style="left:580px;top:43px;width:90px;">打开</button>
    <div class="label abs" style="left:160px;top:74px;">游戏目录</div>
    <div class="abs" style="left:220px;top:74px;display:flex;gap:14px;">
      ${radio("gIso", "0", "保存到 .minecraft", !s.version_isolation)}
      ${radio("gIso", "1", "保存到 .minecraft\\versions\\版本名称\\", s.version_isolation)}
    </div>
    <div class="note abs" style="left:220px;top:98px;">（只在1.6以上的版本有效）</div>
    <div class="label abs" style="left:160px;top:120px;">窗口大小</div>
    <input class="field abs" id="gW" type="number" min="320" style="left:220px;top:120px;width:64px;" value="${s.window_width}"/>
    <div class="label abs" style="left:290px;top:120px;">×</div>
    <input class="field abs" id="gH" type="number" min="240" style="left:304px;top:120px;width:64px;" value="${s.window_height}"/>
    <div class="abs" style="left:384px;top:120px;">${check("gFull", "全屏", s.fullscreen)}</div>
    <div class="label abs" style="left:160px;top:150px;">启动后</div>
    <div class="abs" style="left:220px;top:150px;display:flex;gap:14px;">
      ${radio("gAfter", "keep", "保持启动器", s.after_launch === "keep")}
      ${radio("gAfter", "minimize", "最小化启动器", s.after_launch === "minimize")}
      ${radio("gAfter", "close", "关闭启动器", s.after_launch === "close")}
    </div>
    <div class="abs" style="left:160px;top:180px;">${check("gDebug", "调试模式（以 java.exe 启动 minecraft，并输出 melody-launcher\\输出日志.txt）", s.debug_mode)}</div>
    <button class="btn sm" id="gSave" style="left:580px;top:218px;width:90px;">保存设置</button>`;
  $("#gBrowse").onclick = async () => {
    const d = await pickPath({ directory: true, title: "选择 .minecraft 目录" });
    if (d) $("#gDir").value = d;
  };
  $("#gOpen").onclick = () => invoke("open_game_dir").catch((e) => paneMsg(errText(e), true));
  $("#gSave").onclick = async () => {
    await saveSettings({
      game_dir: $("#gDir").value.trim(),
      version_isolation: radioVal("gIso") === "1",
      window_width: parseInt($("#gW").value, 10) || 854,
      window_height: parseInt($("#gH").value, 10) || 480,
      fullscreen: $("#gFull").checked,
      after_launch: radioVal("gAfter") || "keep",
      debug_mode: $("#gDebug").checked,
    });
    await refreshLocalVersions();
    paneGame($("#pane"));
  };
}

/* --- 游戏管理（原版：游戏列表 / 重命名 / 删除选中游戏） --- */
function paneVersions(p) {
  const vers = state.localVersions;
  const cur = state.settings.selected_version;
  if (!state.selVersion || !vers.includes(state.selVersion)) state.selVersion = vers.includes(cur) ? cur : vers[0] || null;
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:42px;">游戏列表</div>
    <div class="listbox" id="verList" style="left:160px;top:66px;width:410px;height:120px;">
      ${vers.length ? vers.map((v) => `<div class="li ${v === state.selVersion ? "sel" : ""}" data-v="${esc(v)}">${esc(v)}${v === cur ? `<span class="sub">（当前）</span>` : ""}</div>`).join("") : `<div class="empty">尚未安装任何版本，点主界面「更新游戏」下载</div>`}
    </div>
    <button class="btn sm" id="vUse" style="left:580px;top:66px;width:90px;">设为当前</button>
    <button class="btn sm" id="vRename" style="left:580px;top:98px;width:90px;">重命名</button>
    <button class="btn sm" id="vDelete" style="left:580px;top:130px;width:90px;">删除选中游戏</button>
    <button class="btn sm" id="vRefresh" style="left:580px;top:162px;width:90px;">刷新</button>
    <button class="btn sm" id="vDownload" style="left:580px;top:194px;width:90px;">下载新版本</button>
    <button class="btn sm" id="vRepair" style="left:160px;top:194px;width:150px;">检查并补全游戏文件</button>
    <div class="abs" style="left:322px;top:198px;" title="开启：存档、模组、资源包、options.txt 等保存在 .minecraft\\versions\\版本名称\\（资源与库仍共享）；关闭：所有版本共享 .minecraft">${check("vIso", "版本隔离（各版本独立存档/模组）", state.settings.version_isolation)}</div>`;
  $("#vIso").onchange = async (e) => {
    const on = e.target.checked;
    await saveSettings({ version_isolation: on }, on ? "已开启版本隔离：游戏目录 = versions\\<版本名>\\" : "已关闭版本隔离：所有版本共享 .minecraft");
  };
  $$("#verList .li").forEach((li) => {
    li.onclick = () => { state.selVersion = li.dataset.v; $$("#verList .li").forEach((x) => x.classList.toggle("sel", x === li)); };
    li.ondblclick = () => $("#vUse").click();
  });
  const need = () => { if (!state.selVersion) { paneMsg("请先选择一个版本", true); return false; } return true; };
  $("#vUse").onclick = async () => {
    if (!need()) return;
    await saveSettings({ selected_version: state.selVersion }, `当前版本：${state.selVersion}`);
    updateVersionBox(); paneVersions($("#pane"));
  };
  $("#vRename").onclick = async () => {
    if (!need()) return;
    const old = state.selVersion;
    const n = await showMessage("重命名", `将 ${old} 重命名为：`, { input: old, cancel: true });
    if (!n || n === old) return;
    try {
      await invoke("rename_version", { old, new: n.trim() });
      state.selVersion = n.trim();
      if (state.settings.selected_version === old) await saveSettings({ selected_version: n.trim() });
      await refreshLocalVersions(); paneVersions($("#pane")); paneMsg(`已重命名为 ${n.trim()}`);
    } catch (e) { paneMsg(errText(e), true); }
  };
  $("#vDelete").onclick = async () => {
    if (!need()) return;
    const id = state.selVersion;
    const ok = await showMessage("删除选中游戏", `确定删除 ${id}？\n将删除 versions\\${id} 整个目录（含隔离的存档）。`, { cancel: true });
    if (!ok) return;
    try {
      await invoke("delete_version", { id });
      if (state.settings.selected_version === id) await saveSettings({ selected_version: "" });
      state.selVersion = null;
      await refreshLocalVersions(); paneVersions($("#pane")); paneMsg(`已删除 ${id}`);
    } catch (e) { paneMsg(errText(e), true); }
  };
  $("#vRefresh").onclick = async () => { await refreshLocalVersions(); paneVersions($("#pane")); paneMsg("已刷新"); };
  $("#vDownload").onclick = () => showDownload();
  $("#vRepair").onclick = async () => {
    if (!need()) return;
    const id = state.selVersion;
    let r;
    try { r = await invoke("check_version_files", { versionId: id }); } catch (e) { paneMsg(errText(e), true); return; }
    if (r.ok) { paneMsg(`${id}：已检查 ${r.checked} 个文件，文件完整`); return; }
    if (await askRepair(id, r.missing, r.missing_by_category)) runInstall(id, true);
  };
}

/* --- Java（原版：java路径 + 浏览 / 查找java / 最大内存 自动设置·手动） --- */
function paneJava(p) {
  const s = state.settings;
  const mem = state.mem;
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">java路径</div>
    <input class="field abs" id="jPath" style="left:220px;top:44px;width:350px;" value="${esc(s.java_path)}" placeholder="空 = 自动（JAVA_HOME / PATH / 常见目录）"/>
    <button class="btn sm" id="jBrowse" style="left:580px;top:43px;width:90px;">浏览</button>
    <select class="field abs" id="jList" style="left:220px;top:76px;width:350px;">
      ${state.javaList.length ? `<option value="">— 选择查找到的 Java（${state.javaList.length}）—</option>` + state.javaList.map((j) => `<option value="${esc(j.path)}">${esc((j.version || "").replace(/^.*?"(.+?)".*$/, "$1"))} — ${esc(j.path)}</option>`).join("") : `<option value="">点击「查找java」扫描本机</option>`}
    </select>
    <button class="btn sm" id="jScan" style="left:580px;top:75px;width:90px;">查找java</button>
    <div class="label abs" style="left:160px;top:112px;">最大内存</div>
    <div class="abs" style="left:220px;top:112px;display:flex;gap:14px;align-items:center;">
      ${radio("jMem", "auto", "自动设置", s.memory_auto)}
      ${radio("jMem", "manual", "手动", !s.memory_auto)}
      <input class="field" id="jMb" type="number" min="256" step="256" style="width:76px;" value="${s.max_memory_mb}" ${s.memory_auto ? "disabled" : ""}/>
      <span class="label">MB</span>
    </div>
    <div class="note abs" id="jMemNote" style="left:220px;top:138px;width:450px;">${mem ? `系统可用内存 ${mem.available_mb} MB，自动设置将分配 ${mem.auto_mb} MB（可用的 60%，1024–4096 MB）` : "读取系统内存…"}</div>
    <div class="label abs" style="left:160px;top:160px;">JVM 参数</div>
    <input class="field abs" id="jArgs" style="left:220px;top:160px;width:450px;" value="${esc(s.jvm_args)}" placeholder="例如 -XX:+UseG1GC -Dfile.encoding=UTF-8（可留空）"/>
    <button class="btn sm" id="jSave" style="left:580px;top:218px;width:90px;">保存设置</button>`;
  if (!mem) invoke("system_memory").then((m) => { state.mem = m; if (state.settingsTab === "java" && $("#jMemNote")) $("#jMemNote").textContent = `系统可用内存 ${m.available_mb} MB，自动设置将分配 ${m.auto_mb} MB（可用的 60%，1024–4096 MB）`; }).catch(() => {});
  $$('input[name="jMem"]').forEach((r) => { r.onchange = () => { $("#jMb").disabled = radioVal("jMem") === "auto"; }; });
  $("#jList").onchange = () => { if ($("#jList").value) $("#jPath").value = $("#jList").value; };
  $("#jBrowse").onclick = async () => {
    const f = await pickPath({ multiple: false, title: "选择 javaw.exe / java", filters: navigator.userAgent.includes("Windows") ? [{ name: "Java", extensions: ["exe"] }] : undefined });
    if (f) $("#jPath").value = f;
  };
  $("#jScan").onclick = async () => {
    paneMsg("正在查找 Java…");
    try {
      state.javaList = (await invoke("scan_java")) || [];
      const keep = { path: $("#jPath").value, args: $("#jArgs").value };
      paneJava($("#pane"));
      $("#jPath").value = keep.path || state.javaList[0]?.path || ""; $("#jArgs").value = keep.args;
      paneMsg(state.javaList.length ? `找到 ${state.javaList.length} 个 Java` : "未找到 Java，请手动指定路径", !state.javaList.length);
    } catch (e) { paneMsg(errText(e), true); }
  };
  $("#jSave").onclick = () => saveSettings({
    java_path: $("#jPath").value.trim(),
    memory_auto: radioVal("jMem") === "auto",
    max_memory_mb: parseInt($("#jMb").value, 10) || 2048,
    jvm_args: $("#jArgs").value.trim(),
  });
}

/* --- 下载（下载源 + 并发数） --- */
function paneDownload(p) {
  const s = state.settings;
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">下载源</div>
    <div class="abs" style="left:220px;top:44px;display:flex;gap:18px;">
      ${radio("dSrc", "bmclapi", "BMCLAPI 镜像源（国内推荐）", s.download_source !== "official")}
      ${radio("dSrc", "official", "Mojang 官方源", s.download_source === "official")}
    </div>
    <div class="label abs" style="left:160px;top:80px;">并发数</div>
    <input class="abs" id="dConc" type="range" min="1" max="64" value="${s.download_concurrency}" style="left:220px;top:84px;width:260px;"/>
    <div class="label abs" id="dConcV" style="left:490px;top:80px;">${s.download_concurrency}</div>
    <div class="note abs" style="left:160px;top:118px;width:510px;line-height:18px;">
      BMCLAPI：https://bmclapi2.bangbang93.com，失败时自动回退到官方地址。<br/>
      已存在且 SHA-1 校验通过的文件会跳过；失败的文件会列在下载窗口中，可再次下载重试。<br/>
      默认并发 32（范围 1–64），大文件自动分片下载。BMCLAPI 为公益镜像，有频率限制：<br/>
      若频繁出现 429/5xx 失败请调低并发，启动器也会自动临时降速并退避重试。</div>
    <button class="btn sm" id="dSave" style="left:580px;top:218px;width:90px;">保存设置</button>`;
  $("#dConc").oninput = () => { $("#dConcV").textContent = $("#dConc").value; };
  $("#dSave").onclick = () => saveSettings({ download_source: radioVal("dSrc") || "bmclapi", download_concurrency: +$("#dConc").value });
}

/* --- 外观（原版：启动器皮肤 随机皮肤 / 默认皮肤；增：自定义图片） --- */
const SKINS = ["/assets/backgrounds/main.png", "/assets/backgrounds/sky.png", "/assets/backgrounds/blocks.png"];
async function applySkin() {
  const s = state.settings || {};
  let url = SKINS[0];
  if (s.skin === "random") url = SKINS[Math.floor(Math.random() * SKINS.length)];
  else if (s.skin === "custom" && s.background_path) {
    try { url = await invoke("read_image_data_url", { path: s.background_path }); } catch { url = SKINS[0]; }
  }
  $("#app").style.setProperty("--bg", `url("${url}")`);
  return url;
}
function paneSkin(p) {
  const s = state.settings;
  p.innerHTML = `
    <div class="label abs" style="left:160px;top:44px;">启动器皮肤</div>
    <div class="abs" style="left:160px;top:72px;display:flex;flex-direction:column;gap:6px;">
      ${radio("sSkin", "default", "默认皮肤", s.skin === "default")}
      ${radio("sSkin", "random", "随机皮肤（每次启动随机）", s.skin === "random")}
      ${radio("sSkin", "custom", "自定义图片", s.skin === "custom")}
    </div>
    <input class="field abs" id="sPath" style="left:180px;top:164px;width:230px;" value="${esc(s.background_path)}" placeholder="图片路径（png / jpg）"/>
    <button class="btn sm" id="sBrowse" style="left:418px;top:163px;width:60px;">浏览</button>
    <div class="abs" id="sPrev" style="left:490px;top:46px;width:180px;height:112px;border:1px solid #8C9AA3;background:#1a2a33 center/cover no-repeat;"></div>
    <div class="note abs" style="left:490px;top:162px;">预览（720×450 等比）</div>
    <button class="btn sm" id="sSave" style="left:580px;top:218px;width:90px;">保存设置</button>`;
  const prev = async () => {
    const skin = radioVal("sSkin");
    let url = SKINS[0];
    if (skin === "random") url = SKINS[1];
    if (skin === "custom" && $("#sPath").value.trim()) {
      try { url = await invoke("read_image_data_url", { path: $("#sPath").value.trim() }); } catch (e) { paneMsg(errText(e), true); }
    }
    $("#sPrev").style.backgroundImage = `url("${url}")`;
  };
  prev();
  $$('input[name="sSkin"]').forEach((r) => { r.onchange = prev; });
  $("#sBrowse").onclick = async () => {
    const f = await pickPath({ multiple: false, title: "选择背景图片", filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "bmp", "webp"] }] });
    if (f) { $("#sPath").value = f; $$('input[name="sSkin"]').forEach((r) => { r.checked = r.value === "custom"; }); prev(); }
  };
  $("#sSave").onclick = () => saveSettings({ skin: radioVal("sSkin") || "default", background_path: $("#sPath").value.trim() });
}

/* --- 关于 --- */
function paneAbout(p) {
  const i = state.appInfo;
  p.innerHTML = `
    <div class="abs" style="left:160px;top:44px;width:510px;line-height:20px;">
      <div style="font-size:14px;font-weight:bold;">MineCraft Launcher 5.31（Rust + Tauri 复刻版 ${esc(i?.version || "")}）</div>
      <div>原作：MineCraft Launcher 5.31 by 忘却的旋律 · mcsky.net</div>
      <div class="note">离线登录 · authlib-injector 外置登录 · 官方 / BMCLAPI 下载 · 微软登录暂未开放</div>
      <div class="note" style="margin-top:6px;">数据目录：${esc(i?.data_dir || "")}</div>
      <div class="note">设置文件：${esc(i?.settings_file || "")}</div>
      <div class="note">游戏目录：${esc(i?.game_dir || "")}</div>
    </div>
    <button class="btn sm" id="aData" style="left:160px;top:192px;width:100px;">打开数据目录</button>
    <button class="btn sm" id="aGame" style="left:270px;top:192px;width:100px;">打开游戏目录</button>
    <button class="btn sm" id="aRepo" style="left:380px;top:192px;width:100px;">项目主页</button>`;
  if (!i) invoke("app_info").then((x) => { state.appInfo = x; if (state.settingsTab === "about") paneAbout($("#pane")); }).catch(() => {});
  $("#aData").onclick = () => invoke("open_data_dir").catch((e) => paneMsg(errText(e), true));
  $("#aGame").onclick = () => invoke("open_game_dir").catch((e) => paneMsg(errText(e), true));
  $("#aRepo").onclick = () => openUrl("https://github.com/LeiSureLyYrsc/mclauncher-rust1").catch((e) => paneMsg(errText(e), true));
}

/* ============================== 下载中心 ============================== */
/* 原版愚人节版本 */
const APRIL_FOOLS = new Set(["2.0", "15w14a", "1.RV-Pre1", "3D Shareware v1.34", "20w14infinite", "20w14∞", "22w13oneblockatatime", "23w13a_or_b", "24w14potato", "25w14craftmine"]);
function versionKind(v) {
  if (APRIL_FOOLS.has(v.id)) return "fools";
  if (v.type === "old_alpha" || v.type === "old_beta") return "old";
  return v.type;
}
async function showDownload() {
  await hideOverlays();
  const page = $("#downloadPage");
  page.innerHTML = `
    <div class="content-card"></div>
    <div class="dl-titlebar" data-tauri-drag-region>下载中心</div>
    <div class="dl-card">
      <div class="dl-head">
        <span style="width:62px;">类型</span><span style="width:152px;">版本</span><span>时间</span>
        <span class="status" id="dlStatus">加载中…</span>
      </div>
      <div class="dl-list" id="dlList"></div>
    </div>
    <div class="dl-bottom">
      <div class="radios">
        ${radio("dlType", "release", "正式版", true)}
        ${radio("dlType", "snapshot", "预览版", false)}
        ${radio("dlType", "old", "远古版", false)}
        ${radio("dlType", "fools", "愚人节版", false)}
      </div>
      <button class="btn" id="btnDlCommunity">社区资源</button>
      <button class="btn" id="btnDlInstall">下载</button>
    </div>
    <button class="dl-back no-drag" id="dlBack" title="返回主界面">‹ 返回</button>`;
  showFaded(page);
  $("#dlBack").onclick = () => hideFaded(page);
  $("#btnDlCommunity").onclick = () => openUrl("https://www.mcmod.cn/").catch(showError);
  $("#btnDlInstall").onclick = () => installSelected();
  $$('input[name="dlType"]').forEach((r) => { r.onchange = () => renderRemoteList(); });
  await loadRemoteVersions();
}
async function loadRemoteVersions() {
  const st = $("#dlStatus");
  if (st) st.textContent = "正在拉取版本清单…";
  try {
    state.remoteVersions = (await invoke("list_remote_versions")) || [];
    if (st) st.textContent = `共 ${state.remoteVersions.length} 个版本 · 源：${state.settings?.download_source === "official" ? "官方" : "BMCLAPI"}`;
    renderRemoteList();
  } catch (e) {
    if (st) st.textContent = "拉取失败";
    showError(e);
  }
}
function renderRemoteList() {
  const kind = radioVal("dlType") || "release";
  const list = (state.remoteVersions || []).filter((v) => versionKind(v) === kind || (kind === "snapshot" && v.type === "snapshot" && !APRIL_FOOLS.has(v.id)));
  const names = { release: "正式版", snapshot: "预览版", old_alpha: "远古版", old_beta: "远古版" };
  const host = $("#dlList");
  if (!host) return;
  host.innerHTML = list.slice(0, 400).map((v) => `
    <div class="dl-row ${state.selectedRemote === v.id ? "selected" : ""}" data-id="${esc(v.id)}">
      <span class="col-ver">${APRIL_FOOLS.has(v.id) ? "愚人节" : names[v.type] || esc(v.type)}</span>
      <span class="col-name">${esc(v.id)}</span>
      <span class="col-desc">${esc((v.release_time || v.releaseTime || "").slice(0, 10))}</span>
      <span class="col-inst">${state.localVersions.includes(v.id) ? "已安装" : ""}</span>
    </div>`).join("") || `<div class="dl-row">无匹配版本</div>`;
  $$(".dl-row[data-id]", host).forEach((row) => {
    row.onclick = () => { state.selectedRemote = row.dataset.id; $$(".dl-row", host).forEach((r) => r.classList.toggle("selected", r === row)); };
    row.ondblclick = () => installSelected();
  });
}

/* ---------- 下载进度弹窗（选定版本后弹出：分类清单 + 总进度 + 速度 + 取消/关闭） ---------- */
const CAT_ORDER = [["VersionJson", "版本 JSON"], ["ClientJar", "客户端 Jar"], ["Libraries", "依赖库"], ["AssetIndex", "资源索引"], ["Assets", "资源文件"]];
const ST_TEXT = { waiting: ["等待", "st-wait"], pending: ["等待", "st-wait"], running: ["下载中", "st-run"], done: ["完成", "st-ok"], failed: ["有失败", "st-err"] };
let unlistenProgress = null;

function openDlg(id, repair) {
  const host = $("#dlgHost");
  host.innerHTML = `
    <div class="dlg" id="dlg">
      <div class="dlg-title" id="dlgTitle">${repair ? "正在补全" : "正在更新"} ${esc(id)}</div>
      <table class="dlg-table">
        <thead><tr><th>项目</th><th style="text-align:right">文件</th><th style="text-align:right">大小</th><th>状态</th><th>进度</th></tr></thead>
        <tbody id="dlgRows">
          ${CAT_ORDER.map(([k, l]) => `<tr data-cat="${k}"><td>${l}</td><td class="num c-files">-</td><td class="num c-bytes">-</td><td class="st st-wait">等待</td><td><div class="bar mini"><i></i></div></td></tr>`).join("")}
        </tbody>
      </table>
      <div class="dlg-total"><span class="pct" id="dlgPct">0%</span><div class="bar" id="dlgBar"><i></i></div></div>
      <div class="dlg-info" id="dlgInfo">
        <div><span class="k">正在下载:</span><span class="v" id="dlgCur">准备中…</span></div>
        <div><span class="k">文件名:</span><span class="v" id="dlgCur2">-</span></div>
        <div><span class="k">文件大小:</span><span class="v" id="dlgSize">-</span></div>
        <div><span class="k">下载速度:</span><span class="v" id="dlgSpeed">-</span></div>
      </div>
      <button class="btn" id="dlgBtnCancel">取消</button>
    </div>`;
  showFaded(host);
  $("#dlgBtnCancel").onclick = async () => {
    $("#dlgBtnCancel").disabled = true;
    $("#dlgCur").textContent = "正在取消…";
    try { await invoke("cancel_download"); } catch (e) { console.error(e); }
  };
}
function updateDlg(ev) {
  if (!$("#dlg")) return;
  const pct = ev.bytes_total > 0 ? Math.min(100, (ev.bytes_done / ev.bytes_total) * 100)
    : ev.total_files > 0 ? (ev.done_files / ev.total_files) * 100 : 0;
  $("#dlgBar i").style.width = pct.toFixed(1) + "%";
  $("#dlgPct").textContent = `${pct.toFixed(1)}%  ${ev.done_files}/${ev.total_files}${ev.skipped_files ? `（跳过 ${ev.skipped_files}）` : ""}`;
  for (const c of ev.categories || []) {
    const tr = $(`#dlgRows tr[data-cat="${c.category}"]`);
    if (!tr) continue;
    $(".c-files", tr).textContent = `${c.done}/${c.total}`;
    $(".c-bytes", tr).textContent = c.bytes_total ? `${fmtBytes(c.bytes_done)} / ${fmtBytes(c.bytes_total)}` : "-";
    const st = c.failed > 0 ? "failed" : c.state;
    const [txt, cls] = ST_TEXT[st] || ST_TEXT.waiting;
    const td = $(".st", tr); td.textContent = c.failed ? `失败 ${c.failed}` : txt; td.className = `st ${cls}`;
    const p = c.total ? (c.done / c.total) * 100 : 0;
    $(".bar i", tr).style.width = p + "%";
    $(".bar", tr).classList.toggle("err", c.failed > 0);
  }
  const stageText = { meta: "获取版本信息…", natives: "解压 natives…" }[ev.stage];
  const cur = ev.current || [];
  $("#dlgCur").textContent = stageText || cur[0] || (ev.stage === "files" ? "校验 / 等待中…" : "-");
  $("#dlgCur2").textContent = cur.slice(1).join("  ·  ") || "-";
  $("#dlgSize").textContent = `${fmtBytes(ev.bytes_done)} / ${fmtBytes(ev.bytes_total)}`;
  $("#dlgSpeed").textContent = `${fmtBytes(ev.speed_bps)}/s`;
}
function finishDlg(id, result, lastEv, repair) {
  if (!$("#dlg")) return;
  const failed = result?.failed || lastEv?.failed || [];
  const cancelled = lastEv?.stage === "cancelled" || result?.message === "已取消";
  const verb = repair ? "补全" : "更新";
  $("#dlgTitle").textContent = cancelled ? `已取消${verb} ${id}（游戏文件未完整）` : result?.ok ? `${id} ${verb}完成` : `${id} ${verb}未完成`;
  $("#dlgCur").textContent = result?.message || "";
  $("#dlgSpeed").textContent = "-";
  if (failed.length) {
    $("#dlgInfo").outerHTML = `<div class="dlg-failed"><div class="note" style="color:#C62828;">以下文件更新失败（${failed.length}）— 更新失败文件列表：</div><textarea readonly>${esc(failed.join("\n"))}</textarea></div>`;
  }
  $("#dlgBtnCancel").remove();
  $("#dlg").insertAdjacentHTML("beforeend", `${!result?.ok ? `<button class="btn" id="dlgBtnRetry">重试 / 补全</button>` : ""}<button class="btn" id="dlgBtnClose">关闭</button>`);
  $("#dlgBtnClose").onclick = closeDlg;
  if ($("#dlgBtnRetry")) $("#dlgBtnRetry").onclick = async () => { await closeDlg(); runInstall(id, true); };
}
async function closeDlg() { await hideFaded($("#dlgHost")); $("#dlgHost").innerHTML = ""; }

async function installSelected() {
  const id = state.selectedRemote;
  if (!id) { showMessage("提示", "请先选择一个版本"); return; }
  return runInstall(id, false);
}
/* 安装（repair=false）或检查并补全（repair=true：用本地版本 json，只下缺失/损坏文件） */
async function runInstall(id, repair) {
  if (state.downloading) return;
  state.downloading = true;
  openDlg(id, repair);
  let lastEv = null;
  if (unlistenProgress) { unlistenProgress(); unlistenProgress = null; }
  unlistenProgress = await listen("download-progress", (e) => {
    if (e.payload?.version_id !== id) return;
    lastEv = e.payload; updateDlg(lastEv);
  });
  let result = null;
  try {
    result = await invoke(repair ? "repair_version" : "download_version", { versionId: id });
  } catch (e) {
    result = { ok: false, message: errText(e), failed: [] };
  }
  state.downloading = false;
  if (unlistenProgress) { unlistenProgress(); unlistenProgress = null; }
  if (lastEv) updateDlg(lastEv);
  finishDlg(id, result, lastEv, repair);
  await refreshLocalVersions();
  if (result?.ok && !repair) {
    state.settings = await invoke("set_settings", { settings: { ...state.settings, selected_version: id } });
    updateVersionBox();
    if ($("#dlList")) renderRemoteList();
  }
}
async function askRepair(id, missing, cats) {
  const detail = (cats || []).map((c) => `${c.label} ${c.count}`).join("、");
  return showMessage("游戏文件不完整", `${id}：检测到 ${missing} 个缺失文件${detail ? `（${detail}）` : ""}，是否补全？`, { cancel: true });
}

/* ============================== 启动 ============================== */
const LAUNCH_STEPS = [
  ["files", "检查游戏文件"],
  ["java", "检查 / 定位 Java"],
  ["args", "构建启动参数"],
  ["spawn", "启动进程"],
  ["watch", "等待游戏窗口 / 观察期"],
];
const LST_TEXT = { waiting: ["等待", "st-wait"], running: ["进行中", "st-run"], done: ["完成", "st-ok"], failed: ["失败", "st-err"] };
let unlistenLaunch = null;

function openLaunchDlg(id) {
  const host = $("#dlgHost");
  host.innerHTML = `
    <div class="dlg" id="dlg" data-kind="launch">
      <div class="dlg-title" id="dlgTitle">正在启动 ${esc(id)}</div>
      <table class="dlg-table">
        <thead><tr><th>步骤</th><th>状态</th><th>详情</th></tr></thead>
        <tbody id="dlgRows">
          ${LAUNCH_STEPS.map(([k, l]) => `<tr data-step="${k}"><td class="l-name">${l}</td><td class="st st-wait">等待</td><td class="l-detail"><span>-</span></td></tr>`).join("")}
        </tbody>
      </table>
      <div class="dlg-total"><span class="pct" id="dlgPct">0 / ${LAUNCH_STEPS.length}</span><div class="bar" id="dlgBar"><i></i></div></div>
      <div class="dlg-msg" id="dlgMsg">准备中…</div>
    </div>`;
  showFaded(host);
}
function updateLaunchDlg(ev) {
  if (!$('#dlg[data-kind="launch"]')) return;
  const tr = $(`#dlgRows tr[data-step="${ev.step}"]`);
  if (!tr) return;
  const [txt, cls] = LST_TEXT[ev.state] || LST_TEXT.waiting;
  const td = $(".st", tr); td.textContent = txt; td.className = `st ${cls}`;
  const sp = $(".l-detail span", tr); sp.textContent = ev.detail || "-"; sp.title = ev.detail || "";
  const done = document.querySelectorAll("#dlgRows .st-ok").length;
  $("#dlgBar i").style.width = (done / LAUNCH_STEPS.length) * 100 + "%";
  $("#dlgBar").classList.toggle("err", ev.state === "failed");
  $("#dlgPct").textContent = `${done} / ${LAUNCH_STEPS.length}`;
  if (ev.state === "running") $("#dlgMsg").textContent = `${LAUNCH_STEPS.find(([k]) => k === ev.step)?.[1] || ""}：${ev.detail || ""}`;
}
function finishLaunchDlg(id, r) {
  if (!$('#dlg[data-kind="launch"]')) return;
  // 未收到失败事件（如后端异常）时，把正在进行的步骤标为失败
  if (!r.ok) {
    const failedStep = r.failed_step || $("#dlgRows .st-run")?.closest("tr")?.dataset.step;
    if (failedStep && !$(`#dlgRows tr[data-step="${failedStep}"] .st-err`)) updateLaunchDlg({ step: failedStep, state: "failed", detail: r.message });
  }
  $("#dlgTitle").textContent = r.ok ? `游戏已启动 — ${id}` : r.need_repair ? `${id} 游戏文件不完整` : `${id} 启动失败`;
  let msg = r.message || (r.ok ? "游戏已启动" : "无法启动");
  if (!r.ok && r.exit_code != null && !/退出码/.test(msg)) msg = `退出码 ${r.exit_code}\n${msg}`;
  if (!r.ok && !r.need_repair && r.command && r.exit_code == null && r.failed_step === "spawn") msg += `\n\n命令预览：\n${r.command}`;
  if (r.ok) {
    $("#dlgMsg").textContent = msg;
  } else {
    const detail = (r.missing_by_category || []).map((c) => `${c.label} ${c.count}`).join("、");
    $("#dlgMsg").outerHTML = `<div class="dlg-failed"><div class="note" style="color:#C62828;">${r.need_repair ? `检测到 ${r.missing} 个缺失文件${detail ? `（${esc(detail)}）` : ""}，点击「补全」修复后再启动：` : "失败原因："}</div><textarea readonly>${esc(msg)}</textarea></div>`;
  }
  $("#dlg").insertAdjacentHTML("beforeend", `${r.need_repair ? `<button class="btn" id="dlgBtnRetry">补全</button>` : ""}<button class="btn" id="dlgBtnClose">关闭</button>`);
  $("#dlgBtnClose").onclick = closeDlg;
  if ($("#dlgBtnRetry")) $("#dlgBtnRetry").onclick = async () => { await closeDlg(); runInstall(id, true); };
}

async function doLaunch() {
  if (state.launching || state.downloading) return;
  const id = state.settings?.selected_version || "";
  state.launching = true;
  openLaunchDlg(id || "游戏");
  if (unlistenLaunch) { unlistenLaunch(); unlistenLaunch = null; }
  unlistenLaunch = await listen("launch-progress", (e) => { if (e.payload) updateLaunchDlg(e.payload); });
  let r;
  try {
    setStatus("正在启动…", 0);
    r = await invoke("launch_game");
  } catch (e) {
    r = { ok: false, message: errText(e) };
  }
  state.launching = false;
  if (unlistenLaunch) { unlistenLaunch(); unlistenLaunch = null; }
  setStatus(r.ok ? r.message || "已启动游戏" : "");
  finishLaunchDlg(id || "游戏", r);
  if (r.ok) {
    const after = state.settings?.after_launch;
    const w = await currentWindow();
    if (after === "minimize" && w) w.minimize();
    else if (after === "close") setTimeout(closeApp, 800);
  }
}
function updateVersionBox() {
  const id = state.settings?.selected_version;
  $("#versionBox").textContent = id || "未安装版本";
}
async function refreshLocalVersions() {
  try {
    state.localVersions = (await invoke("list_local_versions")) || [];
    if (!state.settings?.selected_version && state.localVersions.length) {
      state.settings = await invoke("set_settings", { settings: { ...state.settings, selected_version: state.localVersions[0] } });
    }
    updateVersionBox();
  } catch (e) { console.error(e); }
}

async function boot() {
  renderApp();
  await applyScale();
  fadeIn($("#stage")); // 启动淡入：250ms / 5 级（原版 SetTimer 50ms，每次 alpha += 51）
  try {
    state.settings = await invoke("get_settings");
    await applySkin();
    await reloadAccounts();
    await refreshLocalVersions();
  } catch (e) {
    console.error(e);
    setStatus("后端未就绪（浏览器预览可用）", 6000);
  }
  // 截图/调试辅助：?view=settings:java | download | dlg
  const view = new URLSearchParams(location.search).get("view");
  if (view?.startsWith("settings")) openSettings(view.split(":")[1] || "account");
  else if (view === "download") showDownload();
  else if (view === "dlg") { await showDownload(); state.selectedRemote = "1.20.1"; installSelected(); }
}
boot();
