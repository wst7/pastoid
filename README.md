<p align="center">
  <img src="public/logo.svg" alt="Pastoid" width="120" height="120">
</p>

<h1 align="center">Pastoid</h1>

<p align="center">
  <img src="https://img.shields.io/github/v/release/wst7/pastoid?label=version" alt="Latest Release">
  <img src="https://github.com/wst7/pastoid/actions/workflows/release.yml/badge.svg" alt="Build Status">
</p>

Pastoid is a lightweight, fast clipboard manager for macOS, Windows, and Linux. Built with Tauri and React, it runs quietly in the background and keeps your clipboard history always within reach.

## Why Pastoid?

Your clipboard is one of the most used tools on your computer, yet it only remembers one item at a time. Pastoid changes that by automatically saving everything you copy, so you never lose a snippet again. Whether it's a code block, a URL, a password, or a piece of text you copied hours ago — Pastoid keeps it safe and makes it easy to find.

## Key Features

- **📋 Automatic History** — Everything you copy is recorded in the background without any extra steps.
- **📌 Pin Important Items** — Keep frequently used snippets at the top of your list for instant access.
- **🔍 Instant Search** — Find any copied item in seconds with real-time search as you type.
- **🌙 Theme Support** — Choose between Light, Dark, or follow your system automatically.
- **🌍 Bilingual** — Full support for both English and Chinese interfaces.
- **🚀 Start with System** — Optional auto-start so Pastoid is always ready when you are.
- **⌨️ Keyboard-First** — Open the quick paste panel with a global shortcut, navigate with arrow keys, and paste without touching your mouse.
- **⚙️ Configurable Limits** — Set how many items to keep (1–100) to balance history depth and performance.

## Screenshots

<p align="center">
  <img src="snapshots/quick-paste.png" alt="Pastoid Quick Paste Panel (Light)" width="500">
  <br><br>
  <img src="snapshots/quick-paste-dark.png" alt="Pastoid Quick Paste Panel (Dark)" width="500">
</p>

## Quick Paste Panel

Press your configured shortcut (default: `Cmd+Shift+V`) anywhere to instantly summon the quick paste panel. It appears over any app — even full-screen games or videos — so you can paste without breaking your flow.

Inside the panel:
- **↑↓** to navigate
- **↵** to paste
- **⌘P** to pin or unpin
- **⌘⌫** to delete an item
- **⌘⇧X** to clear all history
- **Esc** to close

## Privacy by Design

Pastoid stores your clipboard history locally on your machine. No cloud, no account, no tracking. Your data stays yours.

## Installation

### macOS

1. Download the `.dmg` file from the [latest release](https://github.com/wst7/pastoid/releases/latest).
2. Open the `.dmg` and drag **Pastoid** into your **Applications** folder.
3. If you see the message **"Pastoid.app" is damaged and can't be opened**, this is due to macOS Gatekeeper blocking unsigned apps. Run this command in Terminal to allow it:

   ```bash
   xattr -cr /Applications/Pastoid.app
   ```

4. Open Pastoid from your Applications folder. You may need to go to **System Settings → Privacy & Security** and click **Open Anyway** on the first launch.

### Windows

1. Download the `.msi` or `.exe` installer from the [latest release](https://github.com/wst7/pastoid/releases/latest).
2. Run the installer and follow the prompts.

### Linux

Download the `.AppImage` or `.deb` package from the [latest release](https://github.com/wst7/pastoid/releases/latest).

- **AppImage**: Make it executable and run it directly.
- **.deb**: Install with `sudo dpkg -i pastoid_*.deb`.
