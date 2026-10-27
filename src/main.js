/**
 * MCLauncher 5.31 Rust+Tauri rewrite — vanilla frontend
 * Pixel layout mirrors yan-mc/melody-launcher WPF.
 */
const isTauri = !!(window.__TAURI_INTERNALS__ || window.__TAURI__);

async function invoke(cmd, args = {}) {
  if (!isTauri) {
    console.warn("[mock]", cmd, args);
    return mockInvoke(cmd, args);
  }
  const { invoke: inv } = await import("@tauri-apps/api/core");
  return inv(cmd, args);
}

function mockInvoke(cmd, args) {
  const mocks = {
    list_local_versions: () => [],
    get_settings: () => ({
      download_source: "bmclapi",
      game_dir: "",
      java_path: "",
      max_memory_mb: 2048,
      selected_version: "",
    }),
    list_accounts: () => ({ accounts: [], current: null }),
    get_download_sources: () => [
      { id: "official", name: "Mojang 官方源" },
      { id: "bmclapi", name: "BMCLAPI 镜像源" },
    ],
    list_remote_versions: () => [
      { id: "1.21.3", type: "release", url: "", release_time: "2024-01-01" },
      { id: "1.20.4", type: "release", url: "", release_time: "2023-12-01" },
    ],
    detect_java: () => ({ found: false, path: "", version: "" }),
    offline_login: ({ username }) => ({
      mode: "offline",
      username,
      uuid: "00000000-0000-3000-8000-000000000000",
      access_token: "0",
      user_type: "legacy",
    }),
    yggdrasil_login: () => {
      throw new Error("浏览器预览模式：请用 tauri 运行完整登录");
    },
    set_settings: (s) => s.settings || s,
    launch_game: () => ({ ok: false, command: "java ...", message: "预览模式" }),
    download_version: () => ({ ok: true, message: "预览模式" }),
    logout: () => ({ ok: true }),
    open_game_dir: () => ({ ok: true }),
  };
  const fn = mocks[cmd];
  if (!fn) return Promise.resolve(null);
  return Promise.resolve(fn(args));
}

const state = {
  settings: null,
  account: null,
  localVersions: [],
  remoteVersions: [],
  selectedRemote: null,
  overlay: null,
};

const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];

function xIcon(size = 12, color = "rgba(255,255,255,0.902)") {
  return `<svg width="${size}" height="${size}" viewBox="0 0 12 12">
    <path d="M0,0 L12,12 M12,0 L0,12" stroke="${color}" stroke-width="2" fill="none"/>
  </svg>`;
}

function renderApp() {
  const app = $("#app");
  app.innerHTML = `
    <div class="content-card" id="contentCard"></div>
    <div class="drag-region" data-tauri-drag-region></div>
    <div class="layer" id="layer">
      <button class="btn btn-big no-drag" id="btnLaunch">启动游戏</button>
      <button class="btn btn-update no-drag" id="btnUpdate">更新游戏</button>
      <button class="btn btn-settings no-drag" id="btnSettings">设置</button>
      <div class="version-box no-drag" id="versionBox" title="当前版本">未安装版本</div>
      <button class="btn btn-text no-drag" id="btnVersionSettings">版本设置</button>
      <div class="main-status" id="mainStatus"></div>
      <div class="credit">MineCraft Launcher 5.31 by 忘却的旋律</div>
      <button class="btn-close-win no-drag" id="btnClose" title="关闭">${xIcon(12, "rgba(255,255,255,0.902)")}</button>
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
      <div class="ctx-item" data-act="close-overlay">关闭当前浮层</div>
    </div>

    <div class="download-page" id="downloadPage"></div>
    <div class="overlay-host" id="overlayHost"></div>
    <div class="msg-host" id="msgHost"></div>
  `;
  bindMain();
}

function bindMain() {
  $("#btnClose").onclick = async () => {
    if (!isTauri) { window.close(); return; }
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  };
  $("#btnLaunch").onclick = () => doLaunch();
  $("#btnUpdate").onclick = () => showDownload();
  $("#btnSettings").onclick = () => showOverlay("settings");
  $("#btnVersionSettings").onclick = () => showOverlay("version");

  const root = $("#app");
  root.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const menu = $("#ctxMenu");
    menu.style.left = Math.min(e.clientX, 720 - 190) + "px";
    menu.style.top = Math.min(e.clientY, 450 - 220) + "px";
    menu.classList.add("show");
  });
  document.addEventListener("click", () => $("#ctxMenu").classList.remove("show"));
  $$("#ctxMenu .ctx-item").forEach((el) => {
    el.onclick = () => {
      const act = el.dataset.act;
      if (act === "download") showDownload();
      else if (act === "version") showOverlay("version");
      else if (act === "account") showOverlay("account");
      else if (act === "source") showOverlay("source");
      else if (act === "about") showOverlay("about");
      else if (act === "open-dir") invoke("open_game_dir").catch(showError);
      else if (act === "close-overlay") hideOverlays();
    };
  });
}

function setStatus(text, ms = 4000) {
  const el = $("#mainStatus");
  el.textContent = text || "";
  el.classList.toggle("show", !!text);
  if (text && ms > 0) setTimeout(() => { if (el.textContent === text) el.classList.remove("show"); }, ms);
}

function showError(err) {
  const msg = err?.message || String(err);
  showMessage("错误", msg);
}

function showMessage(title, body) {
  const host = $("#msgHost");
  host.innerHTML = `
    <div class="msg-box">
      <div class="msg-title">${escapeHtml(title)}</div>
      <div class="msg-body">${escapeHtml(body)}</div>
      <div class="msg-actions"><button class="btn" id="msgOk">确定</button></div>
    </div>`;
  host.classList.add("show");
  $("#msgOk").onclick = () => host.classList.remove("show");
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function hideOverlays() {
  $("#overlayHost").classList.remove("show");
  $("#overlayHost").innerHTML = "";
  $("#downloadPage").classList.remove("show");
  state.overlay = null;
}

function showOverlay(name) {
  $("#downloadPage").classList.remove("show");
  const host = $("#overlayHost");
  host.classList.add("show");
  state.overlay = name;
  if (name === "account") renderAccount(host);
  else if (name === "source") renderSource(host);
  else if (name === "settings") renderSettings(host);
  else if (name === "version") renderVersionSettings(host);
  else if (name === "about") renderAbout(host);
}

function panelShell(inner) {
  return `
    <div class="overlay-panel">
      <button class="overlay-close" id="ovClose">${xIcon(11, "rgba(255,255,255,0.949)")}</button>
      ${inner}
    </div>`;
}

function renderAccount(host) {
  const acc = state.account;
  const status = acc
    ? `已登录：${acc.username}（${acc.mode === "authlib" ? "外置登录" : "离线"}）`
    : "未登录（可使用离线或外置登录）";
  host.innerHTML = panelShell(`
    <div class="group-box" style="left:15px;top:30px;width:385px;height:180px;"></div>
    <div class="group-title" style="left:25px;top:22px;">账户</div>
    <div class="group-box" style="left:422px;top:30px;width:251px;height:180px;"></div>
    <div class="group-title" style="left:432px;top:22px;">说明 / 下载源</div>

    <div class="acc-left">
      <div id="accState" style="margin:18px 16px 6px;font-size:13px;color:#10222F;">${escapeHtml(status)}</div>
      <div style="margin:0 16px 4px;font-size:12px;color:#17323D;" id="accName">${acc ? escapeHtml(acc.username) : ""}</div>
      <div style="margin:0 16px 8px;font-size:10px;font-family:Consolas,monospace;color:#173C44;" id="accUuid">${acc ? escapeHtml(acc.uuid || "") : ""}</div>

      <div style="margin:4px 16px;font-size:11px;color:#173C44;">离线登录</div>
      <input class="field" id="offlineName" placeholder="玩家名" style="margin:0 16px;width:200px;" value="${acc?.mode === "offline" ? escapeHtml(acc.username) : ""}" />
      <button class="btn" id="btnOffline" style="position:static;margin:6px 16px;width:110px;height:28px;font-size:12px;">离线登录</button>

      <div style="margin:8px 16px 2px;font-size:11px;color:#173C44;">authlib-injector 外置登录</div>
      <input class="field" id="yggApi" placeholder="API Root，如 https://littleskin.cn/api/yggdrasil" style="margin:2px 16px;width:340px;" value="${escapeHtml(acc?.authlib_api || "https://littleskin.cn/api/yggdrasil")}" />
      <input class="field" id="yggUser" placeholder="用户名 / 邮箱" style="margin:4px 16px;width:160px;" />
      <input class="field" id="yggPass" type="password" placeholder="密码" style="margin:4px 4px;width:140px;" />
      <button class="btn" id="btnYgg" style="position:static;margin:4px 16px;width:110px;height:28px;font-size:12px;">外置登录</button>
      <button class="btn" id="btnLogout" style="position:static;margin:4px 4px;width:90px;height:28px;font-size:12px;">退出登录</button>
    </div>

    <div class="acc-right">
      <div class="note" style="margin:18px 12px;line-height:1.5;">
        第一版支持离线与 authlib-injector（Yggdrasil）外置登录。<br/>
        微软正版登录将在后续版本提供。
      </div>
      <div style="margin:8px 12px;font-size:11px;color:#173C44;">下载源快捷选择</div>
      <label class="radio-row" style="margin:4px 12px;"><input type="radio" name="accSrc" value="official" ${state.settings?.download_source === "official" ? "checked" : ""} /><span class="gfx"></span><span>官方源</span></label>
      <label class="radio-row" style="margin:4px 12px;"><input type="radio" name="accSrc" value="bmclapi" ${state.settings?.download_source !== "official" ? "checked" : ""} /><span class="gfx"></span><span>BMCLAPI</span></label>
      <button class="btn" disabled title="即将支持" style="position:static;margin:12px;width:120px;height:28px;font-size:12px;opacity:0.45;">微软登录（即将支持）</button>
      <div id="accMsg" class="note" style="margin:4px 12px;"></div>
    </div>
  `);
  $("#ovClose").onclick = hideOverlays;
  $("#btnOffline").onclick = async () => {
    try {
      const username = $("#offlineName").value.trim() || "Player";
      const s = await invoke("offline_login", { username });
      state.account = s;
      setStatus(`离线登录：${s.username}`);
      showOverlay("account");
    } catch (e) { showError(e); }
  };
  $("#btnYgg").onclick = async () => {
    try {
      const api_root = $("#yggApi").value.trim();
      const username = $("#yggUser").value.trim();
      const password = $("#yggPass").value;
      const s = await invoke("yggdrasil_login", { apiRoot: api_root, username, password });
      state.account = s;
      $("#yggPass").value = "";
      setStatus(`外置登录：${s.username}`);
      showOverlay("account");
    } catch (e) { showError(e); }
  };
  $("#btnLogout").onclick = async () => {
    await invoke("logout");
    state.account = null;
    showOverlay("account");
  };
  $$('input[name="accSrc"]').forEach((r) => {
    r.onchange = async () => {
      if (!r.checked) return;
      const settings = { ...state.settings, download_source: r.value };
      state.settings = await invoke("set_settings", { settings });
      setStatus(`下载源：${r.value}`);
    };
  });
}

function renderSource(host) {
  const src = state.settings?.download_source || "bmclapi";
  host.innerHTML = panelShell(`
    <div class="group-box" style="left:15px;top:30px;width:385px;height:180px;"></div>
    <div class="group-title" style="left:25px;top:22px;">下载源预设</div>
    <div class="group-box" style="left:422px;top:30px;width:251px;height:180px;"></div>
    <div class="group-title" style="left:432px;top:22px;">说明</div>

    <label class="radio-row" style="position:absolute;left:32px;top:60px;width:220px;"><input type="radio" name="src" value="official" ${src === "official" ? "checked" : ""} /><span class="gfx"></span><span>Mojang 官方源</span></label>
    <label class="radio-row" style="position:absolute;left:32px;top:90px;width:220px;"><input type="radio" name="src" value="bmclapi" ${src === "bmclapi" ? "checked" : ""} /><span class="gfx"></span><span>BMCLAPI 镜像源</span></label>
    <div class="note" style="position:absolute;left:32px;top:130px;width:350px;line-height:1.45;">
      官方：launchermeta / libraries / resources<br/>
      BMCLAPI：https://bmclapi2.bangbang93.com<br/>
      国内网络建议使用 BMCLAPI。
    </div>
    <button class="btn" id="btnSrcSave" style="position:absolute;left:32px;top:216px;width:90px;height:32px;font-size:12px;">保存</button>
    <div class="note" style="position:absolute;left:432px;top:50px;width:230px;line-height:1.5;">
      切换下载源后，版本列表与客户端 / libraries / assets 都会走对应镜像映射。
    </div>
    <div id="srcStatus" class="note" style="position:absolute;left:140px;top:224px;"></div>
  `);
  $("#ovClose").onclick = hideOverlays;
  $("#btnSrcSave").onclick = async () => {
    const v = $$('input[name="src"]').find((x) => x.checked)?.value || "bmclapi";
    state.settings = await invoke("set_settings", { settings: { ...state.settings, download_source: v } });
    $("#srcStatus").textContent = "已保存：" + v;
    setStatus("下载源已更新");
  };
}

function renderSettings(host) {
  const s = state.settings || {};
  host.innerHTML = panelShell(`
    <div class="group-box" style="left:15px;top:30px;width:658px;height:180px;"></div>
    <div class="group-title" style="left:25px;top:22px;">设置</div>
    <div class="label" style="position:absolute;left:32px;top:50px;">游戏目录</div>
    <input class="field" id="setGameDir" style="position:absolute;left:100px;top:48px;width:420px;" value="${escapeHtml(s.game_dir || "")}" placeholder="空=可执行文件旁 .minecraft（开发时 data/.minecraft）" />
    <div class="label" style="position:absolute;left:32px;top:84px;">Java 路径</div>
    <input class="field" id="setJava" style="position:absolute;left:100px;top:82px;width:420px;" value="${escapeHtml(s.java_path || "")}" placeholder="空=自动检测 JAVA_HOME / PATH" />
    <button class="btn" id="btnDetectJava" style="position:absolute;left:530px;top:80px;width:90px;height:28px;font-size:12px;">检测</button>
    <div class="label" style="position:absolute;left:32px;top:118px;">最大内存 (MB)</div>
    <input class="field" id="setMem" type="number" style="position:absolute;left:140px;top:116px;width:100px;" value="${s.max_memory_mb || 2048}" />
    <div class="label" style="position:absolute;left:32px;top:152px;">下载源</div>
    <label class="radio-row" style="position:absolute;left:100px;top:150px;"><input type="radio" name="setSrc" value="official" ${s.download_source === "official" ? "checked" : ""} /><span class="gfx"></span><span>官方</span></label>
    <label class="radio-row" style="position:absolute;left:180px;top:150px;"><input type="radio" name="setSrc" value="bmclapi" ${s.download_source !== "official" ? "checked" : ""} /><span class="gfx"></span><span>BMCLAPI</span></label>
    <button class="btn" id="btnSetSave" style="position:absolute;left:32px;top:216px;width:90px;height:32px;font-size:12px;">保存</button>
    <div id="setStatus" class="note" style="position:absolute;left:140px;top:224px;"></div>
  `);
  $("#ovClose").onclick = hideOverlays;
  $("#btnDetectJava").onclick = async () => {
    try {
      const j = await invoke("detect_java");
      if (j.found) {
        $("#setJava").value = j.path;
        $("#setStatus").textContent = `检测到 Java：${j.version || j.path}`;
      } else {
        $("#setStatus").textContent = "未检测到 Java，请手动填写路径";
      }
    } catch (e) { showError(e); }
  };
  $("#btnSetSave").onclick = async () => {
    const download_source = $$('input[name="setSrc"]').find((x) => x.checked)?.value || "bmclapi";
    const settings = {
      ...state.settings,
      game_dir: $("#setGameDir").value.trim(),
      java_path: $("#setJava").value.trim(),
      max_memory_mb: parseInt($("#setMem").value, 10) || 2048,
      download_source,
    };
    state.settings = await invoke("set_settings", { settings });
    $("#setStatus").textContent = "已保存";
    setStatus("设置已保存");
  };
}

function renderVersionSettings(host) {
  const vers = state.localVersions || [];
  const cur = state.settings?.selected_version || "";
  const options = vers.length
    ? vers.map((v) => `<option value="${escapeHtml(v)}" ${v === cur ? "selected" : ""}>${escapeHtml(v)}</option>`).join("")
    : `<option value="">未安装版本</option>`;
  host.innerHTML = panelShell(`
    <div class="group-box" style="left:15px;top:30px;width:658px;height:180px;"></div>
    <div class="group-title" style="left:25px;top:22px;">版本设置</div>
    <div class="label" style="position:absolute;left:32px;top:60px;">已安装版本</div>
    <select class="field" id="verSelect" style="position:absolute;left:120px;top:58px;width:280px;height:26px;">${options}</select>
    <button class="btn" id="btnRefreshVer" style="position:absolute;left:420px;top:56px;width:90px;height:28px;font-size:12px;">刷新</button>
    <button class="btn" id="btnUseVer" style="position:absolute;left:32px;top:216px;width:90px;height:32px;font-size:12px;">应用</button>
    <div class="note" style="position:absolute;left:32px;top:100px;width:600px;">若列表为空，请先打开「下载中心」安装版本。缺文件时启动会提示补齐。</div>
  `);
  $("#ovClose").onclick = hideOverlays;
  $("#btnRefreshVer").onclick = async () => {
    await refreshLocalVersions();
    showOverlay("version");
  };
  $("#btnUseVer").onclick = async () => {
    const id = $("#verSelect").value;
    state.settings = await invoke("set_settings", { settings: { ...state.settings, selected_version: id } });
    updateVersionBox();
    setStatus(id ? `已选择版本 ${id}` : "未选择版本");
    hideOverlays();
  };
}

function renderAbout(host) {
  host.innerHTML = panelShell(`
    <div style="padding:36px 28px;line-height:1.7;color:#10222F;font-size:13px;">
      <div style="font-size:16px;font-weight:bold;margin-bottom:8px;">MineCraft Launcher 5.31（Rust + Tauri 重写）</div>
      <div>UI 布局参考「旋律启动器」风格（yan-mc/melody-launcher）。</div>
      <div>本版本：离线登录 · authlib-injector 外置登录 · 官方 / BMCLAPI 下载。</div>
      <div>微软正版登录：路线图中，尚未实现。</div>
      <div style="margin-top:10px;color:#173C44;font-size:12px;">原作署名：MineCraft Launcher 5.31 by 忘却的旋律 · mcsky.net</div>
    </div>
  `);
  $("#ovClose").onclick = hideOverlays;
}

async function showDownload() {
  hideOverlays();
  const page = $("#downloadPage");
  page.classList.add("show");
  page.innerHTML = `
    <div class="content-card" style="background-image:url('/assets/backgrounds/main.png');opacity:0.35;filter:blur(0px);"></div>
    <div class="dl-titlebar"></div>
    <button class="btn-close-win no-drag" id="dlClose" style="z-index:2;">${xIcon(12, "rgba(255,255,255,0.902)")}</button>
    <div class="dl-card">
      <div class="dl-head">
        <span style="width:50px;">类型</span>
        <span style="width:100px;">版本</span>
        <span>时间</span>
        <span class="status" id="dlStatus">加载中…</span>
      </div>
      <div class="dl-list" id="dlList"></div>
    </div>
    <div class="dl-bottom">
      <label class="radio-row"><input type="radio" name="dlType" value="release" checked /><span class="gfx"></span><span>正式版</span></label>
      <label class="radio-row"><input type="radio" name="dlType" value="snapshot" /><span class="gfx"></span><span>快照</span></label>
      <label class="radio-row"><input type="radio" name="dlType" value="all" /><span class="gfx"></span><span>全部</span></label>
      <button class="btn" id="btnDlRefresh">刷新列表</button>
      <button class="btn" id="btnDlInstall">下载选中版本</button>
    </div>
  `;
  $("#dlClose").onclick = hideOverlays;
  $("#btnDlRefresh").onclick = () => loadRemoteVersions();
  $("#btnDlInstall").onclick = () => installSelected();
  $$('input[name="dlType"]').forEach((r) => { r.onchange = () => renderRemoteList(); });
  await loadRemoteVersions();
}

async function loadRemoteVersions() {
  const st = $("#dlStatus");
  if (st) st.textContent = "正在拉取版本清单…";
  try {
    state.remoteVersions = await invoke("list_remote_versions");
    if (st) st.textContent = `共 ${state.remoteVersions.length} 个版本`;
    renderRemoteList();
  } catch (e) {
    if (st) st.textContent = "拉取失败";
    showError(e);
  }
}

function renderRemoteList() {
  const type = $$('input[name="dlType"]').find((x) => x.checked)?.value || "release";
  let list = state.remoteVersions || [];
  if (type !== "all") list = list.filter((v) => v.type === type);
  const host = $("#dlList");
  if (!host) return;
  host.innerHTML = list.slice(0, 200).map((v) => `
    <div class="dl-row ${state.selectedRemote === v.id ? "selected" : ""}" data-id="${escapeHtml(v.id)}">
      <span class="col-ver">${escapeHtml(v.type)}</span>
      <span class="col-name">${escapeHtml(v.id)}</span>
      <span class="col-desc">${escapeHtml(v.release_time || v.releaseTime || "")}</span>
    </div>`).join("") || `<div class="dl-row">无匹配版本</div>`;
  $$(".dl-row[data-id]", host).forEach((row) => {
    row.onclick = () => {
      state.selectedRemote = row.dataset.id;
      renderRemoteList();
    };
  });
}

async function installSelected() {
  const id = state.selectedRemote;
  if (!id) { showMessage("提示", "请先选择一个版本"); return; }
  const st = $("#dlStatus");
  if (st) st.textContent = `正在下载 ${id}…`;
  try {
    const r = await invoke("download_version", { versionId: id });
    if (st) st.textContent = r.message || "完成";
    await refreshLocalVersions();
    state.settings = await invoke("set_settings", { settings: { ...state.settings, selected_version: id } });
    updateVersionBox();
    showMessage("下载", r.message || `${id} 已处理`);
  } catch (e) {
    if (st) st.textContent = "下载失败";
    showError(e);
  }
}

async function doLaunch() {
  try {
    const r = await invoke("launch_game");
    if (r.ok) {
      setStatus("已尝试启动游戏");
      showMessage("启动", r.message || "已启动进程");
    } else {
      showMessage("启动", (r.message || "无法启动") + (r.command ? `\n\n命令预览：\n${r.command}` : ""));
    }
  } catch (e) { showError(e); }
}

function updateVersionBox() {
  const id = state.settings?.selected_version;
  $("#versionBox").textContent = id || "未安装版本";
}

async function refreshLocalVersions() {
  try {
    state.localVersions = await invoke("list_local_versions");
    if (!state.settings?.selected_version && state.localVersions.length) {
      state.settings = await invoke("set_settings", {
        settings: { ...state.settings, selected_version: state.localVersions[0] },
      });
    }
    updateVersionBox();
  } catch (e) {
    console.error(e);
  }
}

async function boot() {
  renderApp();
  try {
    state.settings = await invoke("get_settings");
    const acc = await invoke("list_accounts");
    state.account = acc.current || null;
    await refreshLocalVersions();
    updateVersionBox();
  } catch (e) {
    console.error(e);
    setStatus("后端未就绪（浏览器预览可用）", 6000);
  }
}

boot();
