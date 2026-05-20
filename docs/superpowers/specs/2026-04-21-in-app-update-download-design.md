# In-App Update Download Design

## Overview

Implement in-app downloading of updates for Pastoid. User can check for updates, download the installer within the app (with progress display), and open it directly for installation. No code signing certificate required, works with existing GitHub Releases workflow.

## Requirements

- User experience: Check update → Find update → Download in-app with progress → Click install → Open installer
- Full package download (not incremental), ~50MB - acceptable for this app size
- Auto-select correct installer based on current platform
- Progress bar in UI
- Fallback to browser if no matching installer found
- No external dependencies, use existing reqwest

## Architecture

### Backend (Rust - commands.rs)

New command: `download_update(app_handle) -> Result<(), String>`

Steps:
1. Fetch latest release from GitHub API
2. Find asset matching current platform:
   - macOS: looks for `.dmg`
   - Windows: looks for `.exe` or `.msi`
   - Linux: looks for `.AppImage` or `.deb`
3. Stream download to system temp directory
4. Emit progress events via `app_handle.emit("download_progress", progress)`
5. When complete, save to temp file and notify frontend
6. Frontend triggers `open_installer()` which opens the file via `shell::open`

### Frontend (React - Preferences.tsx)

New state:
```typescript
type UpdateState =
  | { status: 'idle' }
  | { status: 'checking' }
  | { status: 'update_available', info: UpdateInfo }
  | { status: 'downloading', percent: number }
  | { status: 'downloaded', path: string }
  | { status: 'no_update' }
  | { status: 'error', message: string }
```

Listen to Tauri events for `download_progress` to update progress bar.

### Data Flow

```
User clicks "Check Update"
  ↓
Frontend calls check_update() (existing)
  ↓
Backend returns UpdateInfo with has_update
  ↓
If has_update: UI shows "Download Update" button
  ↓
User clicks "Download Update"
  ↓
Backend streams download, emits download_progress events
  ↓
Frontend updates progress bar
  ↓
Download complete: UI enables "Install Update"
  ↓
User clicks "Install Update"
  ↓
Backend opens the installer file with system default app
```

## Files Modified

| File | Change Type |
|------|-------------|
| `src-tauri/src/commands.rs` | Add `start_download_update` and `open_installer` commands |
| `src/pages/Preferences.tsx` | Add download progress UI and state handling |
| `src/locales/en.json` | Add new translation keys |
| `src/locales/zh.json` | Add new translation keys |

## Translation Keys

| Key | en | zh |
|-----|----|----|
| `downloading` | Downloading... | 下载中... |
| `downloadUpdate` | Download Update | 下载更新 |
| `installUpdate` | Install Update | 安装更新 |
| `downloadFailed` | Download failed, please retry | 下载失败，请重试 |

## Error Handling

- 404/network error: Show error message, allow retry
- No matching asset for platform: Open browser to releases page (existing behavior)
- Partial download: Clean up temp file on failure
- User cancels: Can re-initiate download

## Security Considerations

- Download directly from GitHub releases over HTTPS → content integrity protected by TLS
- No execution of arbitrary code, just opening the downloaded installer → user confirms installation at OS level
- This is same security level as downloading via browser then opening

## Open Questions

None - design approved.
