# AGENTS.md

## 项目概览
Pastoid - Tauri + React + TypeScript 构建的轻量级剪贴板管理器，支持 **macOS 和 Windows**。
两端共用同一套前端与数据格式；平台差异集中在快捷键写法、粘贴按键、托盘图标和面板窗口上。
新增平台相关代码请沿用 `cfg!(target_os = "macos")` 的方式，保持单文件可读，不要再拆出整套 cfg 分支。

## 常用命令

```bash
# 开发模式（启动前端 + Tauri）
npm run tauri dev

# 生产构建
npm run tauri build

# TypeScript 检查
npm run tsc

# Rust 测试
cd src-tauri && cargo test

# 发布版本（需要 GITHUB_TOKEN 环境变量）
npm run release           # 交互式
npm run release:patch     # 0.1.0 -> 0.1.1
npm run release:minor     # 0.1.0 -> 0.2.0
npm run release:major     # 0.1.0 -> 1.0.0
```

**重要：** 项目使用 `bun` 作为包管理器（tauri 内部调用 bun run dev/build）

## 架构

### 前端 (src/)
- **React 19 + TypeScript + Vite**（多入口构建）
- **HeroUI v3 + Tailwind CSS v4**
- **i18next** 国际化: `src/locales/{en,zh}.json`
- **Zustand** 状态管理
- 路径别名: `@/*` -> `src/*`

**窗口入口：**
- `src/settings-entry.tsx` → `index.html` → `src/app/SettingsApp.tsx`（设置/关于页面）
- `src/quick-paste-entry.tsx` → `quick-paste.html` → `src/app/QuickPaste.tsx`（快速粘贴面板）
- Vite 配置多入口: `vite.config.ts` 中 `build.rollupOptions.input`

### 后端 (src-tauri/)
- **Tauri 2 (Rust)**
- **rust-i18n** 托盘菜单国际化: `src-tauri/locales/app.yml`
- 核心文件:
  - `src/lib.rs` - 应用初始化、插件注册
  - `src/commands.rs` - Tauri IPC 命令（剪贴板操作、设置管理、图片读取）
  - `src/clipboard.rs` - 剪贴板监控逻辑（文本 + 图片，按内容哈希去重）
  - `src/tray.rs` - 系统托盘菜单（显示快捷键 accelerator）
  - `src/storage.rs` - 数据持久化、日志轮转、孤儿图片回收
  - `src/models.rs` - 数据结构定义
  - `src/shortcut.rs` - 全局快捷键注册/注销、健康监控、面板定位
  - `src/repository.rs` - ClipboardRepository（内存缓存 + 定期 flush）

**剪贴板采集约定（重要）：**
- 文本按「内容 + 类型」去重，图片按 `imageHash`（blake3）去重
- 图片文件名直接使用哈希（`images/<hash>.png`），同一张图不会在磁盘上出现第二份
- **先判重、再落盘**：只有确定新增时才写 PNG。历史上的 bug 就是每轮轮询都另存一张新图，
  21 小时写了 5.9 万个文件（29GB），把历史冲垮，看起来像「复制记录不生效」
- 用户删除/清空时会把内容键记进 `AppState.suppressed`，避免剪贴板里残留的同一条内容被重新加回来；
  一旦剪贴板内容真的变了，抑制表整体清空（否则旧键越积越多，语义也难推理）
- 启动时后台跑一次 `cleanup_orphan_images`，回收没有被任何条目引用的图片文件

**采集优先级与轮询（踩过坑，别改回去）：**
- 文本轮询 250ms，位图探测单独限流到 1 秒。位图 `get_image()` 每次都要把剪贴板解码成 RGBA，
  跟着文本一起加密会白烧 CPU；但文本轮询太慢会漏记——实测 1 秒轮询下间隔 250ms 连续复制的 10 条只记到 3 条
- **文本优先于位图**：剪贴板同时带文本和位图很常见（浏览器复制图片会附带 URL/HTML）。先取位图会让用户
  复制的文本凭空消失；只有当文本是 URL / data URI / HTML 片段这类「复制图片的副产物」时才让位给图片
- 超长文本（>512KB）、超大图片（>40MP 或 >64MB）会被跳过，但**必须写一条 WARN 日志**，
  否则用户只会看到「复制了却没进历史」而完全无从排查

**关键插件：**
- `tauri-plugin-global-shortcut` — 全局快捷键（呼出 quick-paste）
- `tauri-plugin-updater` — 官方自动更新（签名验证 + 自动安装重启）
- `tauri-plugin-autostart` — 开机自启动（macOS AppleScript 模式）
- `tauri-nspanel` — macOS NSPanel（quick-paste 覆盖全屏应用）
- `tauri-plugin-clipboard-manager` — 剪贴板读写
- `tauri-plugin-notification` — 系统通知

## 国际化规范

**前端新增文本:**
1. 同时添加到 `src/locales/en.json` 和 `src/locales/zh.json`
2. React 中使用 `t('key')`

**后端托盘新增文本:**
1. 添加到 `src-tauri/locales/app.yml` 的 `tray:` 下
2. Rust 中使用 `t!("tray.key")`

## 发布流程
- `release-it` 自动同步 3 处版本: `package.json` → `src-tauri/Cargo.toml` → `src-tauri/tauri.conf.json`
- 自动生成基于 conventional commits 的 CHANGELOG
- 自动创建 GitHub Release（需要 `GITHUB_TOKEN` 环境变量）
- CI 自动构建并上传签名更新包（需要 `TAURI_SIGNING_PRIVATE_KEY` Secret）

## 构建产物
- macOS: `.app` + `.dmg`（x64 + aarch64 双架构分别构建）
- Windows: `.msi` + `.exe`（NSIS）
- 更新包: `.tar.gz` + `.sig` 签名文件

## 代码约定
- TypeScript strict 模式
- Rust edition 2021
- 使用 Git 提交信息规范（conventional commits）

## 平台差异速查

| 关注点 | macOS | Windows |
| --- | --- | --- |
| 默认快捷键 | `Cmd+Shift+V` | `Ctrl+Shift+V`（`normalize_shortcut` 负责 Cmd→Ctrl） |
| 粘贴 | Cmd+Tab 切回原应用后 `Key::Unicode('v')` | 直接 Ctrl+V，且必须用 `Key::V`（见下） |
| 面板窗口 | `tauri-nspanel` 转 NSPanel，浮在全屏应用之上 | 普通 always-on-top 窗口 + 所有虚拟桌面可见 |
| 托盘图标 | `icons/tray/32x32.png`，`icon_as_template(true)` | `icons/tray-dark/32x32.png`，模板图标不反色 |
| 面板底部留白 | 110pt（避开 Dock） | 60pt（避开任务栏） |
| 数据目录 | `~/Library/Application Support/<id>` | `%LOCALAPPDATA%\<id>`（不用 Roaming，避免漫游同步图片） |
| 安装包 | `.dmg` | `.msi` / NSIS `.exe` |

Windows 上「复制文件」会以多行路径（有时带引号）出现在剪贴板文本里，
`clipboard::first_image_path` 负责逐行去引号后识别，别退化回「整段当路径」。

**enigo 的坑**：Windows 上 `Key::Unicode(c)` 无法映射成虚拟键，enigo 会打一条 warn 后
降级成「文本输入」（`queue_char`），Ctrl 和字符变成两条独立事件，应用收不到 Ctrl+V，
表现是回车粘贴没反应、或者往编辑器里插了个 `v`。Windows 必须用平台专属的虚拟键
（`Key::V` / `Key::A` …），它们带 `#[cfg(target_os = "windows")]`，用 cfg 分平台取。

**数据目录与迁移**：
- 默认目录由 `storage::default_data_dir` 决定；用户可在设置里改成任意路径，
  记录方式是默认目录下的 `location.txt`（第一行目标路径，可选第二行 `from=<旧目录>`）
- 切换目录一律延后到下次启动：仓库、图片目录、监控线程都持有旧路径，运行中切换没有意义
- 启动时 `get_data_dir` 先解析指针，再把 `from=` 指定的旧目录搬过去，然后清掉 `from=` 那行
- 自定义目录不可用（移动硬盘没插）时回落默认目录，指针保留，等磁盘插回来再切回去
- `get_data_dir` 在 logger 初始化之前执行，所以它把要打的提示作为返回值带出来，别在里面直接 log
