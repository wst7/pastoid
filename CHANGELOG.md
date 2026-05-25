# Changelog

## [1.7.0](https://github.com/wst7/pastoid/compare/v1.6.1...v1.7.0) (2026-05-25)

### ✨ Features | 新功能

* add log rotation (5MB max, keep 3 backups) ([85d800f](https://github.com/wst7/pastoid/commit/85d800f91670d5e17749cffa50c2af27c57b7061))
* add startup and shutdown logs ([bc5dbef](https://github.com/wst7/pastoid/commit/bc5dbef0f64764686138b4c00803a2c730c01bea))
* add structured logging with file output ([2e0ac27](https://github.com/wst7/pastoid/commit/2e0ac2703bf90c2472a002422be9173790595c0c))
* auto-migrate data from old identifier com.pastoid.app to com.wst7.pastoid ([c8ad25e](https://github.com/wst7/pastoid/commit/c8ad25e02f0ddcffe9d1a874df224bdf969d24d5))
* platform-aware shortcuts display with i18n in quick-paste panel ([d6ba0ca](https://github.com/wst7/pastoid/commit/d6ba0caea7c6684e7654da01d5ed524f72a7ccfe))
* remove clear-all button from quick-paste panel header ([1a35a3b](https://github.com/wst7/pastoid/commit/1a35a3b1899c4591531f0d65860e3963605c19ff))
* use RFC3339 format for log timestamps ([a9303a3](https://github.com/wst7/pastoid/commit/a9303a3fa22bb2b27eaa013cd7cdbbe751d0836a))

### 🐛 Bug Fixes | Bug 修复

* normalize global shortcut for Windows/Linux platform ([ce3a52e](https://github.com/wst7/pastoid/commit/ce3a52ec7270be28c6e147cdc6d97ef473193140))
* remove incorrect time_level override so log timestamps appear ([b685700](https://github.com/wst7/pastoid/commit/b68570051b2d8c0a3b7fe52465622c60be25c4c6))

## [1.6.1](https://github.com/wst7/pastoid/compare/v1.6.0...v1.6.1) (2026-05-21)

### 🐛 Bug Fixes | Bug 修复

* update updater public key for new signing key pair ([9975971](https://github.com/wst7/pastoid/commit/9975971cf40bdd60517c2c404e74717896b35eeb))

## [1.6.0](https://github.com/wst7/clipon/compare/v1.5.0...v1.6.0) (2026-05-21)

### ✨ Features | 新功能

* migrate to Tauri official updater plugin ([c7b725b](https://github.com/wst7/clipon/commit/c7b725bb223c0676030120ffc8eeab4b58552c6a))

### 🐛 Bug Fixes | Bug 修复

* correct quick-paste build and rename main window to settings ([a88c541](https://github.com/wst7/clipon/commit/a88c541e6263fb62c184445f2da106239a621351))
* handle HeroUI Select Set key and add theme debug logging ([af79ab9](https://github.com/wst7/clipon/commit/af79ab9e9f052d78f3167bf6dba775e1497edacf))
* improve autostart error messages for macOS development ([41bb154](https://github.com/wst7/clipon/commit/41bb15493405f2a19ee083c38e969c95fc97fd31))
* remove 'system' theme option due to WebView matchMedia inaccuracy ([975ddc0](https://github.com/wst7/clipon/commit/975ddc0e75e11ea0fb50524b9f6af23e5c3f859e))
* restrict tauri-nspanel to macOS-only to fix Linux build ([d54ae3c](https://github.com/wst7/clipon/commit/d54ae3cd15227f0ab4c19616322f6472a0aa5b10))

## [1.5.0](https://github.com/wst7/clipon/compare/v1.4.1...v1.5.0) (2026-05-20)

### ✨ Features | 新功能

* add NSPanel level and style mask for fullscreen overlay ([5155054](https://github.com/wst7/clipon/commit/5155054b9a14485799aeac6dd701913f72ee5e21))
* add shortcut field to Settings model ([403817d](https://github.com/wst7/clipon/commit/403817d176646f832f8786a7d767fb5413c3d2d9))
* add shortcut manager module with register/unregister ([40de7d6](https://github.com/wst7/clipon/commit/40de7d605d5c8964fdc34506a28d184a95047579))
* add shortcut recording input component ([3b0c8e6](https://github.com/wst7/clipon/commit/3b0c8e681123cedc9fd22dae6bdceb9f7bbda927))
* integrate shortcut binding into settings page ([08aa033](https://github.com/wst7/clipon/commit/08aa0337a9378f1828f5a3efe05ef48afe478cad))
* re-register shortcut on settings change ([8bce97c](https://github.com/wst7/clipon/commit/8bce97c1c918ef6e1b7183c1439a4ded38172d26))
* tray accelerator, quick-paste clear, autostart fix, theme sync ([7df52f3](https://github.com/wst7/clipon/commit/7df52f3965bc1b391a421817038a9fb0810a6de0))
* use shortcut module for shortcut registration on startup ([898e99c](https://github.com/wst7/clipon/commit/898e99c43fb18c05682e0f79fb6e13ba32d9f6ce))
* use tauri-nspanel for fullscreen overlay ([5a02cc2](https://github.com/wst7/clipon/commit/5a02cc2334d61832cedb46da20e2010fb6b70240))

### 🐛 Bug Fixes | Bug 修复

* use Settings default for shortcut fallback, log fallback failure ([ec27c4a](https://github.com/wst7/clipon/commit/ec27c4a60528af87cb0a1ee103c06e1419089edb))

## [1.4.1](https://github.com/wst7/clipon/compare/v1.4.0...v1.4.1) (2026-05-18)

## [1.4.0](https://github.com/wst7/clipon/compare/v1.3.0...v1.4.0) (2026-04-21)

### ✨ Features | 新功能

* add core:window:allow-set-theme permission ([b22cafa](https://github.com/wst7/clipon/commit/b22cafa54f1fd91c074c3706f3452431800a140d))

### 🐛 Bug Fixes | Bug 修复

* adapt title bar symbol color to theme on macOS ([df5bc08](https://github.com/wst7/clipon/commit/df5bc08db95bb7b59082754022be4158013a104a))
* add padding top for overlay title bar to avoid overlap ([d9f6e59](https://github.com/wst7/clipon/commit/d9f6e597d3342f90f828db5f6a239f1eafdcf60f))
* also update window theme when system theme changes ([e7ac50e](https://github.com/wst7/clipon/commit/e7ac50e11f0c9dd2ce123e599c91524fc9f21ab7))
* apply system theme on initialization for title bar ([d545f8e](https://github.com/wst7/clipon/commit/d545f8ebcaf2507a63252f181cb213cf5f14a6d9))
* ensure tab text color is correct in dark mode ([e898bc5](https://github.com/wst7/clipon/commit/e898bc57aa67c24d58205a229f4f7abc4e7f5408))
* improve search placeholder visibility in light theme ([627c254](https://github.com/wst7/clipon/commit/627c25457cf950256d7dcae719ad1266755dfe39))
* match correct architecture for macOS dmg download ([2b98982](https://github.com/wst7/clipon/commit/2b989829a7732371f569fe35a984fca188c27aea))
* prefer x64 dmg on macOS to avoid quarantine damage error ([f7987a0](https://github.com/wst7/clipon/commit/f7987a091951407425f29e5216cff0688b721cad))
* re-apply theme when settings.theme changes (fixes system theme switch) ([216a9de](https://github.com/wst7/clipon/commit/216a9de0d5b6335373eac64fd0668fe161252f8d))
* remove border bottom from tabs header ([e3ee44c](https://github.com/wst7/clipon/commit/e3ee44c5e285d2671699144f7ba954b3497d2e65))
* theme change updates window title bar color ([b186a94](https://github.com/wst7/clipon/commit/b186a94ec0d2fae907fd3e286995e65510da6eef))

## [1.3.0](https://github.com/wst7/clipon/compare/v1.2.0...v1.3.0) (2026-04-21)

### ✨ Features | 新功能

* add download progress UI and state management ([d149a0d](https://github.com/wst7/clipon/commit/d149a0d99b07535fdbd4f23f4e77c6b9669d2017))
* add in-app download and installer opening commands ([b74642f](https://github.com/wst7/clipon/commit/b74642fc93e78f3138291632a50686d95e6b8e1a))

### 🐛 Bug Fixes | Bug 修复

* correct GitHub repo URL and UI improvements ([3d7fd0f](https://github.com/wst7/clipon/commit/3d7fd0f39a7fc7a750e9b7c3ab5bd91ace9f0ee4))
* use consistent GitHub repo URL (wst7) ([b81643c](https://github.com/wst7/clipon/commit/b81643ce3c4f84df1124fd0897ea53ceb1a21500))
* use streaming download with incremental progress updates ([1c3aa0b](https://github.com/wst7/clipon/commit/1c3aa0bc345847ab289842eef2d05181f07f8342))

## [1.2.0](https://github.com/wst7/clipon/compare/v1.1.0...v1.2.0) (2026-04-21)

### ✨ Features | 新功能

* tray menu support i18n ([38d577c](https://github.com/wst7/clipon/commit/38d577c35855e505655073849062476f9264efef))

## [1.1.0](https://github.com/wst7/clipon/compare/v1.0.6...v1.1.0) (2026-04-21)

### ✨ Features | 新功能

* 样式优化 ([8592699](https://github.com/wst7/clipon/commit/859269901f1b57a9297bd95e392796faa716136c))

## [1.0.6](https://github.com/wst7/clipon/compare/v1.0.5...v1.0.6) (2026-04-20)

### 🐛 Bug Fixes | Bug 修复

* 样式问题优化 ([28781be](https://github.com/wst7/clipon/commit/28781be7c4beb85cddd8fd03d093e78e8445f547))

## [1.0.5](https://github.com/wst7/clipminister/compare/v1.0.4...v1.0.5) (2026-04-20)

### 🐛 Bug Fixes | Bug 修复

* 前端类型校验失败 ([eb98511](https://github.com/wst7/clipminister/commit/eb985112e98dd3c09366b27550cd3375ff738a31))

## [1.0.4](https://github.com/wst7/clipminister/compare/v1.0.3...v1.0.4) (2026-04-20)

## [1.0.3](https://github.com/wst7/clipminister/compare/v1.0.2...v1.0.3) (2026-04-20)

## [1.0.2](https://github.com/wst7/clipminister/compare/v1.0.1...v1.0.2) (2026-04-20)

## [1.0.1](https://github.com/wst7/clipminister/compare/v0.1.2...v1.0.1) (2026-04-20)
