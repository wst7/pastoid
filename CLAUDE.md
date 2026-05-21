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

# Release (requires GITHUB_TOKEN environment variable)
npm run release           # Interactive release
npm run release:patch     # 0.1.0 -> 0.1.1
npm run release:minor     # 0.1.0 -> 0.2.0
npm run release:major     # 0.1.0 -> 1.0.0
```

## Architecture

### Frontend (src/)
- **React 19 + TypeScript + Vite** with HeroUI v3 + Tailwind CSS v4
- **i18next** for internationalization (`src/i18n.ts`, `src/locales/`)
- **Zustand** for state management
- **Multi-entry Vite build**: `index.html` (settings) + `quick-paste.html` (quick paste panel)

Key directories:
- `app/` - App components (SettingsApp.tsx, QuickPaste.tsx)
- `components/` - Reusable UI components
- `locales/` - Translation JSON files (en.json, zh.json)

### Backend (src-tauri/)
- **Tauri 2** (Rust) for native desktop functionality
- **rust-i18n** for tray menu internationalization (`locales/app.yml`)

Key files:
- `src/lib.rs` - App initialization, plugin setup (updater, autostart, global shortcut, NSPanel)
- `src/commands.rs` - Tauri commands (IPC between frontend and backend)
- `src/clipboard.rs` - Clipboard monitoring logic
- `src/tray.rs` - System tray menu (with keyboard accelerator display)
- `src/storage.rs` - Data persistence (JSON files)
- `src/models.rs` - Data structures (Settings, ClipboardItem)
- `src/shortcut.rs` - Global shortcut registration/unregistration
- `src/repository.rs` - ClipboardRepository (in-memory cache + periodic flush)

### Key Plugins
- `tauri-plugin-global-shortcut` - Global hotkey to toggle quick-paste panel
- `tauri-plugin-updater` - Official auto-updater with signature verification
- `tauri-plugin-autostart` - Auto-start on boot (macOS AppleScript mode)
- `tauri-nspanel` - macOS NSPanel for quick-paste fullscreen overlay
- `tauri-plugin-clipboard-manager` - Clipboard read/write

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

## Release & Updates

- **release-it**: Bumps version across `package.json`, `Cargo.toml`, `tauri.conf.json`, generates CHANGELOG, creates GitHub tag and Release
- **GitHub Actions**: Builds on push to `v*` tags, creates signed update artifacts
- **Updater**: Uses `tauri-plugin-updater` with `latest.json` endpoint on GitHub Releases
- **Signing**: Requires `TAURI_SIGNING_PRIVATE_KEY` secret in GitHub Actions

## Important Notes

- macOS `tauri-nspanel` uses git dependency (not published to crates.io v2)
- Quick-paste window uses `tauri-nspanel` on macOS for fullscreen overlay; `alwaysOnTop` on other platforms
- The `settings` window label was formerly `main` — all references use `"settings"`
- Theme options: `light` / `dark` (no "system" due to WebView `matchMedia` inaccuracy on macOS)
