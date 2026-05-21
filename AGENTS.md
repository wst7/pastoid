# AGENTS.md

## 项目概览
Pastoid - Tauri + React + TypeScript 构建的轻量级剪贴板管理器

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
  - `src/commands.rs` - Tauri IPC 命令（剪贴板操作、设置管理）
  - `src/clipboard.rs` - 剪贴板监控逻辑
  - `src/tray.rs` - 系统托盘菜单（显示快捷键 accelerator）
  - `src/storage.rs` - 数据持久化
  - `src/models.rs` - 数据结构定义
  - `src/shortcut.rs` - 全局快捷键注册/注销
  - `src/repository.rs` - ClipboardRepository（内存缓存 + 定期 flush）

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
- macOS: `.dmg`（x64 + aarch64 双架构分别构建）
- Windows: `.msi` + `.exe`
- Linux: `.AppImage` + `.deb`
- 更新包: `.tar.gz`（macOS）/ `.nsis.zip`（Windows）/ `.AppImage.tar.gz`（Linux）+ `.sig` 签名文件

## 代码约定
- TypeScript strict 模式
- Rust edition 2021
- 使用 Git 提交信息规范（conventional commits）
