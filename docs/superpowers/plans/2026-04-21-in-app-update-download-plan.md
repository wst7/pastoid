# In-App Update Download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add in-app update download functionality with progress display, allowing users to download and open installers directly from within Pastoid.

**Architecture:**
- Backend: Add Tauri commands in commands.rs to handle download streaming and installer opening
- Frontend: Extend Preferences.tsx with state machine for download progress tracking
- Cross-platform: Auto-select correct installer (.dmg for macOS, .exe/.msi for Windows) from GitHub releases
- Event-based: Use Tauri's event system to stream download progress from Rust to React

**Tech Stack:** Rust, Tauri 2, reqwest, React + TypeScript

---

### Task 1: Add translation keys

**Files:**
- Modify: `src/locales/en.json`
- Modify: `src/locales/zh.json`

- [ ] **Step 1: Add new keys to en.json**

Add these to `src/locales/en.json` at the end, before the closing brace:

```json
  "downloading": "Downloading...",
  "downloadUpdate": "Download Update",
  "installUpdate": "Install Update",
  "downloadFailed": "Download failed, please retry"
```

- [ ] **Step 2: Add new keys to zh.json**

Add these to `src/locales/zh.json` at the end, before the closing brace:

```json
  "downloading": "下载中...",
  "downloadUpdate": "下载更新",
  "installUpdate": "安装更新",
  "downloadFailed": "下载失败，请重试"
```

- [ ] **Step 3: Commit**

```bash
git add src/locales/en.json src/locales/zh.json
git commit -m "i18n: add download update translation keys"
```

---

### Task 2: Add backend download and installer opening commands

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add required imports**

At the top of `commands.rs`, add:

```rust
use std::io::Write;
use tauri::AppHandle;
```

- [ ] **Step 2: Add DownloadProgress struct**

After `UpdateInfo` struct (around line 189), add:

```rust
#[derive(serde::Serialize, Clone)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f32,
}
```

- [ ] **Step 3: Add find_asset_for_platform helper function**

After `compare_versions` function, add:

```rust
fn find_asset_for_platform(release: &GitHubRelease) -> Option<&GitHubAsset> {
    let os = std::env::consts::OS;

    release.assets.iter().find(|asset| {
        match os {
            "macos" => asset.name.ends_with(".dmg"),
            "windows" => asset.name.ends_with(".exe") || asset.name.ends_with(".msi"),
            "linux" => asset.name.ends_with(".AppImage") || asset.name.ends_with(".deb"),
            _ => false,
        }
    })
}
```

- [ ] **Step 4: Add start_download_update command**

Add this new command at the end of the file:

```rust
#[tauri::command]
pub async fn start_download_update(app_handle: AppHandle) -> Result<String, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/wst7/pastoid/releases/latest")
        .header("User-Agent", "Pastoid")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err("Failed to fetch release info".to_string());
    }

    let release: GitHubRelease = response.json().await.map_err(|e| e.to_string())?;

    let asset = find_asset_for_platform(&release)
        .ok_or_else(|| "No installer found for your platform".to_string())?;

    let download_response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "Pastoid")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let total_size = download_response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut stream = download_response.bytes_stream();

    let temp_path = std::env::temp_dir().join(&asset.name);
    let mut file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        let percent = if total_size > 0 {
            (downloaded as f32 / total_size as f32) * 100.0
        } else {
            0.0
        };

        app_handle.emit("download_progress", DownloadProgress {
            downloaded,
            total: total_size,
            percent,
        }).map_err(|e| e.to_string())?;
    }

    Ok(temp_path.to_string_lossy().to_string())
}
```

- [ ] **Step 5: Add open_installer command**

Add this command after start_download_update:

```rust
#[tauri::command]
pub fn open_installer(path: String) -> Result<(), String> {
    open::that(&path).map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 6: Check if we need to add open crate to Cargo.toml**

First check existing dependencies. If `open` crate is not present, it will need to be added.

```bash
cd src-tauri && cargo check 2>&1 | head -20
```

If compilation fails with "unresolved import `open`", add `open = "5.0.0"` to Cargo.toml dependencies and re-check.

- [ ] **Step 7: Verify compile**

```bash
cd src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs
# Also add Cargo.toml if modified
git add src-tauri/Cargo.toml || true
git commit -m "feat: add in-app download and installer opening commands"
```

---

### Task 3: Register new commands in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Find the invoke_handler call**

Find the `.invoke_handler` call. It should look like:

```rust
.invoke_handler(tauri::generate_handler![
    // ... list of commands
])
```

- [ ] **Step 2: Add new commands**

Add `start_download_update` and `open_installer` to the handler list.

- [ ] **Step 3: Verify compile**

```bash
cd src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: register download commands in invoke handler"
```

---

### Task 4: Update frontend Preferences.tsx with download UI and state

**Files:**
- Modify: `src/pages/Preferences.tsx`

- [ ] **Step 1: Add new import and state types**

At the imports, add:

```typescript
import { listen } from '@tauri-apps/api/event';
```

Add after the UpdateInfo interface:

```typescript
interface DownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
}
```

- [ ] **Step 2: Add new state hooks**

Find existing state hooks (checkingUpdate, updateInfo). Add after them:

```typescript
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadedPath, setDownloadedPath] = useState<string | null>(null);
```

- [ ] **Step 3: Add download handler function**

Add after handleUpdate function:

```typescript
  const handleDownloadUpdate = async () => {
    setIsDownloading(true);
    setDownloadProgress(0);

    const unlisten = await listen<DownloadProgress>('download_progress', (event) => {
      setDownloadProgress(event.payload.percent);
    });

    try {
      const path = await invoke<string>("start_download_update");
      setDownloadedPath(path);
    } catch (error) {
      console.error("Download failed:", error);
      // Fallback: open browser URL
      if (updateInfo?.download_url) {
        openUrl(updateInfo.download_url);
      }
    } finally {
      setIsDownloading(false);
      unlisten();
    }
  };

  const handleInstall = () => {
    if (downloadedPath) {
      invoke("open_installer", { path: downloadedPath });
    }
  };
```

- [ ] **Step 4: Update the About tab render section**

Find the download button section (current: lines 347-353). Replace it with:

```typescript
                    {updateInfo.has_update ? (
                      <>
                        <div className="flex justify-between items-center">
                          <span className="text-sm text-green-500">{t('newVersion')}</span>
                          <span className="font-medium text-green-500">{updateInfo.latest_version}</span>
                        </div>

                        {isDownloading ? (
                          <div className="space-y-2">
                            <div className="w-full bg-muted rounded-full h-2">
                              <div
                                className="bg-primary h-2 rounded-full transition-all duration-300"
                                style={{ width: `${downloadProgress}%` }}
                              />
                            </div>
                            <p className="text-xs text-center text-muted-foreground">
                              {t('downloading')} {downloadProgress.toFixed(1)}%
                            </p>
                          </div>
                        ) : downloadedPath ? (
                          <Button
                            onPress={handleInstall}
                            variant="primary"
                            className="w-full"
                          >
                            {t('installUpdate')}
                          </Button>
                        ) : (
                          <Button
                            onPress={handleDownloadUpdate}
                            variant="primary"
                            className="w-full"
                          >
                            {t('downloadUpdate')}
                          </Button>
                        )}
                      </>
                    ) : (
```

The "已是最新版本" div comes right after this.

- [ ] **Step 5: Also add reset for download state on recheck**

Find the recheck button `onPress={() => setUpdateInfo(null)}` and update it to also clear download state:

```typescript
onPress={() => {
  setUpdateInfo(null);
  setDownloadedPath(null);
  setDownloadProgress(0);
}}
```

- [ ] **Step 6: Type check**

```bash
npm run tsc
```

Expected: No errors.

- [ ] **Step 7: Commit**

```bash
git add src/pages/Preferences.tsx
git commit -m "feat: add download progress UI and state management"
```

---

### Task 5: Test and verify

**Files:** No new files, just verification

- [ ] **Step 1: Full build test**

```bash
npm run tauri build
```

Expected: Build completes successfully.

- [ ] **Step 2: Manual verification plan**

1. Run dev version: `npm run tauri dev`
2. Go to "About" tab
3. Click "Check for updates"
4. If there's an update, verify "Download Update" button appears
5. Click download and verify progress bar updates
6. After download completes, verify "Install Update" button appears
7. Clicking install should open the installer

- [ ] **Step 3: Commit verification (optional)**

No commit needed, just verification. If any issues found, fix and commit fixes.
