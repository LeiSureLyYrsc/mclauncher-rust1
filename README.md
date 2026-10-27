# mclauncher-rust1

「旋律启动器」MCLauncher **5.31** 的 **Rust + Tauri 2** 重写版。

目标：主界面坐标与交互尽量一比一复刻参考布局（`yan-mc/melody-launcher` WPF），并实现第一版可用的：

- 离线登录
- authlib-injector / Yggdrasil 外置登录
- 官方源 + BMCLAPI 游戏下载
- 多平台 GitHub Actions 构建

> **诚实说明**：启动链路已实现「拼出 Java 命令并 `spawn`」；若缺 jar / libraries / natives / assets，会提示去下载中心。完整可玩性依赖本机 Java 与下载完整性，未在本仓库 CI 中做端到端进游戏验证。微软正版登录**未实现**（界面灰显「即将支持」）。

## 功能

| 模块 | 状态 |
|------|------|
| 720×450 无边框主界面（坐标对齐参考） | ✅ |
| 账户浮层：离线 + 外置登录 | ✅ |
| 下载源：官方 / BMCLAPI | ✅ |
| 下载中心：版本列表 + 安装 client/libraries/assets | ✅ |
| 设置（游戏目录 / Java / 内存 / 下载源） | ✅ |
| 启动游戏（组命令 + spawn） | ✅（缺文件时提示） |
| 微软登录 | ❌ 路线图 |

## WebView 运行时（多平台一致性）

| 平台 | 渲染引擎 | 说明 |
|------|----------|------|
| **Linux** | **CEF（捆绑 Chromium）** | alpha；与 Win/mac 的 Chromium/Blink 渲染更接近，避免 WebKitGTK 差异 |
| Windows | WebView2（系统） | 现有 wry 路径，不变 |
| macOS | WKWebView（系统） | 现有 wry 路径，不变 |

**为何 Linux 用 CEF**：系统 WebKitGTK 在滚动、CSS、GPU 合成上与 WebView2 / WKWebView 差异大，UI 很难统一。CEF 自带 Chromium，Linux 表现可对齐其它桌面端。方案对齐 [Readest](https://github.com/readest/readest) / Tauri `feat/cef`（本仓库 patch 使用 Readest fork：上游 `feat/cef` 已去掉 `tauri/wry` feature，且 AppImage 打包依赖 Readest CLI 修复）。

> **诚实标注**：CEF 仍为 alpha。Linux 上通常是「主程序 + CEF 运行时目录」，或打进 AppImage / deb；**不是**「一个纯静态单文件 exe」。产物体积会显著变大（数百 MB 级），属预期。

### Linux 构建注意

- **命令**（Linux）：`npm run tauri:linux-cef -- build`（内部交换 `Cargo.cef.lock`、启用 `--features cef`、加载 `src-tauri/.cargo/cef.toml`）
- **Win / mac**：`npm run tauri build`（wry + 现有 `Cargo.lock`）
- **X11 / Wayland**：CEF 运行时强制 X11；纯 Wayland 需 **XWayland**（`DISPLAY` 非空）。Niri 等可装 `xwayland-satellite`
- **首次启动**：代码已传 Chromium `--no-first-run`，避免空 profile 立刻退出（exit 28）
- **文件对话框**：Linux 使用 `xdg-portal`（避免 rfd/gtk 与 CEF 抢 GLib 主循环）
- **无边框拖拽**：CEF+X11 上系统 `startDragging` 可能受限（已知上游限制）；若拖不动窗口属已知问题

### 锁文件

- `src-tauri/Cargo.lock` → Win/mac（wry）
- `src-tauri/Cargo.cef.lock` → Linux CEF
- 中断的 CEF 构建若留下 `Cargo.lock.wry`：`mv src-tauri/Cargo.lock.wry src-tauri/Cargo.lock`

## UI 说明

布局严格参考：

- 窗口：`720×450`，无边框、不可缩放、居中、透明圆角内容区
- 背景：`Assets/backgrounds/main.png`，ContentCard Margin 6、CornerRadius 2
- 按钮坐标（窗口 Canvas）：
  - 启动游戏 `430,310` `150×75`，约 25px 粗体
  - 更新游戏 `590,310` `100×35`
  - 设置 `590,350` `100×35`
  - 版本框 `430,390` `185×30`
  - 版本设置 `620,393`
  - 关闭 X `682,4`
  - 署名 `4,433`：`MineCraft Launcher 5.31 by 忘却的旋律`（SimSun/serif，`#BEBEBF`）
- 按钮风格：浅灰渐变、细描边、轻微阴影（见 `MelodyTheme.xaml`）
- 浮层约 `698×268`，半透明蓝青渐变 `#9474C2E5 → #94A6DAEF`

资源风格来源于旋律启动器 / `yan-mc/melody-launcher`，仅作 UI 复刻学习用途。

## 开发

### 依赖

- Node.js 18+
- Rust stable
- **Linux CEF**：GTK3、NSS、GBM、ALSA、udev、libxkbcommon-x11、patchelf 等（见 CI）；**不再依赖 webkit2gtk** 作为发布路径
- Win：WebView2；macOS：系统 WebKit

```bash
# 拉取大图资源（若 public/assets 为空）
bash scripts/fetch-assets.sh

npm install

# Win / mac
npm run tauri dev

# Linux（CEF）
npm run tauri:linux-cef -- dev
```

本地数据目录：`data/`（`settings.json`、`accounts.json`、`.minecraft/`、`authlib-injector.jar`）。**不要把含令牌的 accounts 提交到 git**。

### 构建

```bash
# Win / mac
npm run tauri build

# Linux CEF
npm run tauri:linux-cef -- build --bundles appimage,deb
```

产物在 `src-tauri/target/release/bundle/`。Linux 另可能有旁路 CEF 资源目录 / tarball。

## 下载源

- **官方**：`launchermeta.mojang.com` / `libraries.minecraft.net` / `resources.download.minecraft.net`
- **BMCLAPI**：`https://bmclapi2.bangbang93.com`（manifest / maven / assets 映射）

## 登录

### 离线

输入玩家名；UUID 按 Minecraft 离线算法：

`UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`（MD5 + version 3）。

### authlib-injector 外置

填写 API Root（例如 `https://littleskin.cn/api/yggdrasil`）、用户名/邮箱与密码，走 Yggdrasil `authenticate`。启动时注入：

```text
-javaagent:authlib-injector.jar=<apiRoot>
```

jar 首次会尝试从 BMCLAPI authlib-injector 镜像缓存到 `data/`。

### 微软登录

**明确不做（第一版）**，界面保留灰显入口，后续路线图再实现。

## 单文件 / 运行时说明

| 平台 | 产物 | 备注 |
|------|------|------|
| Windows | NSIS / MSI + `release` 目录下可执行文件 | 需要 WebView2；CI 使用 Evergreen bootstrapper |
| Linux | AppImage / deb（含 CEF）+ 可选 runtime tarball | **主程序 + Chromium 运行时**；体积远大于 WebKitGTK 版 |
| macOS | `.app` / dmg | 非单一裸二进制 |

## GitHub Actions

`.github/workflows/build.yml`：

- `windows-latest` / `macos-latest`：`tauri-apps/tauri-action` + wry
- `ubuntu-22.04`：安装 CEF 依赖与 `cargo tauri`（readest/tauri）、`scripts/tauri-linux-cef.mjs` 构建；缓存 `~/.cache/tauri-cef`；artifact 名带 `-cef`

CI 第一步会跑 `scripts/fetch-assets.sh` 拉取背景与 UI 小图。

## 路线图

1. 微软设备码 / OAuth 正版登录
2. 下载进度条与并行加速
3. Mod / 资源管理
4. 更完整的 natives / 参数规则兼容

## 致谢

- 原作：忘却的旋律 · MCLauncher 5.31 · mcsky.net
- UI 参考开源复刻：https://github.com/yan-mc/melody-launcher
- Linux CEF 方案参考：https://github.com/readest/readest 、Tauri `feat/cef`

本项目为学习与兼容性重写，与原作者无隶属关系。
