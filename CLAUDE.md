# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Pastoid - A lightweight clipboard manager built with Tauri + React + TypeScript.

## Common Commands

```bash
# Development
npm run tauri dev

# Production build
npm run tauri build

# TypeScript check
npm run tsc

# Rust tests
cd src-tauri && cargo test

# Release (requires GITHUB_TOKEN)
npm run release           # Interactive release
npm run release:patch     # 0.1.0 -> 0.1.1
npm run release:minor     # 0.1.0 -> 0.2.0
npm run release:major     # 0.1.0 -> 1.0.0
```

## Architecture

### Frontend (src/)
- **React + TypeScript + Vite** with HeroUI v3 + Tailwind CSS
- **i18next** for internationalization (`src/i18n.ts`, `src/locales/`)
- **Zustand** for state management

Key directories:
- `components/` - Reusable UI components
- `pages/` - Page components (Main app, Preferences)
- `hooks/` - Custom React hooks
- `locales/` - Translation JSON files (en.json, zh.json)

### Backend (src-tauri/)
- **Tauri 2** (Rust) for native desktop functionality
- **rust-i18n** for tray menu internationalization (`locales/app.yml`)

Key files:
- `src/lib.rs` - App initialization, plugin setup
- `src/commands.rs` - Tauri commands (IPC between frontend and backend)
- `src/clipboard.rs` - Clipboard monitoring logic
- `src/tray.rs` - System tray menu
- `src/storage.rs` - Data persistence (JSON files)
- `src/models.rs` - Data structures (Settings, ClipboardItem)

## Internationalization

- Frontend: Uses `react-i18next` with `src/locales/en.json` and `src/locales/zh.json`
- Backend tray: Uses `rust-i18n` with `src-tauri/locales/app.yml`
- Language codes: `zh` (Chinese), `en` (English)

When adding new UI text:
1. Add key to both `src/locales/en.json` and `src/locales/zh.json`
2. Use `t('key')` in React components

When adding tray menu text:
1. Add key to `src-tauri/locales/app.yml` under `tray:` section
2. Use `t!("tray.key")` in Rust code