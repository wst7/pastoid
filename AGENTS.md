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

# 发布版本（需要 GITHUB_TOKEN）
npm run release           # 交互式
npm run release:patch     # 0.1.0 -> 0.1.1
npm run release:minor     # 0.1.0 -> 0.2.0
npm run release:major     # 0.1.0 -> 1.0.0
```

**重要：** 项目使用 `bun` 作为包管理器（tauri 内部调用 bun run dev/build）

## 架构

### 前端 (src/)
- **React 19 + TypeScript + Vite**
- **HeroUI v3 + Tailwind CSS v4**
- **i18next** 国际化: `src/locales/{en,zh}.json`
- **Zustand** 状态管理
- 路径别名: `@/*` -> `src/*`

### 后端 (src-tauri/)
- **Tauri 2 (Rust)**
- **rust-i18n** 托盘菜单国际化: `src-tauri/locales/app.yml`
- 核心文件:
  - `src/lib.rs` - 应用初始化、插件注册
  - `src/commands.rs` - Tauri IPC 命令
  - `src/clipboard.rs` - 剪贴板监控逻辑
  - `src/tray.rs` - 系统托盘菜单
  - `src/storage.rs` - 数据持久化
  - `src/models.rs` - 数据结构定义

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
- 自动创建 GitHub Release

## 代码约定
- TypeScript strict 模式
- Rust edition 2021
- 使用 Git 提交信息规范（conventional commits）
