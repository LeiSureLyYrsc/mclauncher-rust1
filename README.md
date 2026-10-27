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
- Tauri 2 系统依赖（Linux 需 `webkit2gtk` 等）

```bash
# 拉取大图资源（若 public/assets 为空）
bash scripts/fetch-assets.sh

npm install
npm run tauri dev
```

本地数据目录：`data/`（`settings.json`、`accounts.json`、`.minecraft/`、`authlib-injector.jar`）。**不要把含令牌的 accounts 提交到 git**。

### 构建

```bash
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/`。

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
| Windows | NSIS / MSI + `release` 目录下可执行文件 | 需要 WebView2；CI 使用 Evergreen bootstrapper 配置，也可另下运行时 |
| Linux | AppImage / deb | AppImage 相对接近「单文件」分发 |
| macOS | `.app` / dmg | 非单一裸二进制 |

Tauri 应用依赖系统 WebView，无法在所有平台做到「绝对零依赖单文件」；文档与 workflow 已尽量提供便携产物。

## GitHub Actions

`.github/workflows/build.yml`：`windows-latest` / `ubuntu-latest` / `macos-latest` 矩阵，使用 `tauri-apps/tauri-action`，并上传 artifacts。CI 第一步会跑 `scripts/fetch-assets.sh` 拉取背景与 UI 小图。

## 路线图

1. 微软设备码 / OAuth 正版登录
2. 下载进度条与并行加速
3. Mod / 资源管理
4. 更完整的 natives / 参数规则兼容

## 致谢

- 原作：忘却的旋律 · MCLauncher 5.31 · mcsky.net
- UI 参考开源复刻：https://github.com/yan-mc/melody-launcher

本项目为学习与兼容性重写，与原作者无隶属关系。
