# 快捷键可配置化 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 允许用户在设置页面录制自定义全局快捷键替代硬编码的 `Cmd+Shift+V`

**Architecture:** 前端录制键盘事件 → 组装 `"Cmd+Shift+V"` 字符串 → 存入 `Settings.shortcut` → Rust 端 `save_settings` 检测变化 → 调用 `global_shortcut().unregister(旧)` + `global_shortcut().on_shortcut(新, handler)` 重新注册

**Tech Stack:** Tauri v2 + React + HeroUI v3 + tauri-plugin-global-shortcut (v2.3.1 / global-hotkey v0.7.0)

---

### Task 1: Settings 数据模型扩展

**Files:**
- Modify: `src-tauri/src/models.rs:35-42`
- Test: `src-tauri/src/models.rs:108-115`

- [ ] **Step 1: 在 Settings 中添加 shortcut 字段**

```rust
// src-tauri/src/models.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    #[serde(rename = "max_items")]
    pub max_items: u32,
    pub shortcut: String,
}
```

- [ ] **Step 2: 更新 Default 实现**

```rust
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            autostart: false,
            max_items: 20,
            shortcut: "Cmd+Shift+V".to_string(),
        }
    }
}
```

- [ ] **Step 3: 更新现有测试中的 Settings 构造（新增 shortcut 字段）**

在 `src-tauri/src/models.rs:119-124` 的 `test_settings_clone` 中添加 `shortcut` 字段：

```rust
let settings1 = Settings {
    language: "en".to_string(),
    theme: "dark".to_string(),
    autostart: true,
    max_items: 50,
    shortcut: "Ctrl+Alt+K".to_string(),
};
```

在 `test_settings_serialization` (line 134-153) 中添加：

```rust
let settings = Settings {
    language: "en".to_string(),
    theme: "dark".to_string(),
    autostart: true,
    max_items: 100,
    shortcut: "Cmd+Shift+V".to_string(),
};
// ...
assert!(json.contains("\"shortcut\":\"Cmd+Shift+V\""));
// ...
assert_eq!(decoded.shortcut, "Cmd+Shift+V");
```

- [ ] **Step 4: 运行测试验证**

```bash
cd src-tauri && cargo test models
```

Expected: 8 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs
git commit -m "feat: add shortcut field to Settings model"
```

---

### Task 2: 后端快捷键管理模块

**Files:**
- Create: `src-tauri/src/shortcut.rs`
- Test: `src-tauri/src/shortcut.rs` (module 内嵌测试)

- [ ] **Step 1: 创建 shortcut.rs 模块，定义回调逻辑**

global_hotkey 的 `HotKey::from_str()` 已内置解析 `"Cmd+Shift+V"` 格式（大小写不敏感，支持 `"CMD"|"Cmd"|"cmd"` 等别名），`on_shortcut` 和 `unregister` 直接接受字符串参数，无需手动解析。

```rust
// src-tauri/src/shortcut.rs
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub fn toggle_quick_paste(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("quick-paste") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.center();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("panel-opened", ());
        }
    }
}

pub fn register(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let app_handle = app.clone();
    let shortcut_string = shortcut.to_string();
    app.global_shortcut()
        .on_shortcut(shortcut_string.clone(), move |app, _s, event| {
            use tauri_plugin_global_shortcut::ShortcutState;
            if event.state() == ShortcutState::Pressed {
                toggle_quick_paste(app);
            }
        })
        .map_err(|e| format!("Failed to register shortcut '{}': {}", shortcut_string, e))
}

pub fn unregister(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .unregister(shortcut.to_string())
        .map_err(|e| format!("Failed to unregister shortcut '{}': {}", shortcut, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_quick_paste_does_not_crash_without_window() {
        // toggle_quick_paste 在无窗口时仅打印错误（通过 get_webview_window 返回 None），不 panic
        // 此测试无法直接在测试环境运行（需要 AppHandle），仅确保编译通过
    }
}
```

- [ ] **Step 2: 注册模块**

```rust
// src-tauri/src/lib.rs，在 pub mod storage; 之后添加
pub mod shortcut;
```

- [ ] **Step 3: 编译验证**

```bash
cd src-tauri && cargo check
```

Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/shortcut.rs
git commit -m "feat: add shortcut manager module with register/unregister"
```

---

### Task 3: 修改 lib.rs 启动流程

**Files:**
- Modify: `src-tauri/src/lib.rs:59-78`

- [ ] **Step 1: 替换硬编码快捷键注册为从 settings 读取**

将 `src-tauri/src/lib.rs:59-78` 的快捷键注册块替换为：

```rust
            // 注册全局快捷键
            let shortcut_str = settings.shortcut.clone();
            if let Err(e) = shortcut::register(
                &app.handle(),
                &shortcut_str,
            ) {
                eprintln!(
                    "Failed to register shortcut '{}': {}. Falling back to default.",
                    shortcut_str, e
                );
                let _ = shortcut::register(app.handle(), "Cmd+Shift+V");
            }
```

同时删除不再需要的 import：

```rust
// 删除这一行:
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
// 替换为:
use tauri_plugin_global_shortcut::GlobalShortcutExt;
```

- [ ] **Step 2: 编译验证**

```bash
cd src-tauri && cargo check
```

Expected: 编译通过，无 unused import 警告

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: load shortcut from settings on startup"
```

---

### Task 4: 修改 save_settings 支持快捷键变更

**Files:**
- Modify: `src-tauri/src/commands.rs:78-111`

- [ ] **Step 1: 在 save_settings 中检测 shortcut 变化并重新注册**

在 `src-tauri/src/commands.rs:78-111` 的 `save_settings` 函数中，在 `max_items` 变化检测之后添加 shortcut 变化检测：

```rust
    // shortcut 变更检测（在 max_items 检测之后、*current_settings = settings.clone() 之前添加）
    if current_settings.shortcut != settings.shortcut {
        let old_shortcut = current_settings.shortcut.clone();
        let new_shortcut = settings.shortcut.clone();

        // 先注销旧快捷键
        if let Err(e) = crate::shortcut::unregister(&app_handle, &old_shortcut) {
            eprintln!("Failed to unregister old shortcut '{}': {}", old_shortcut, e);
        }

        // 注册新快捷键
        if let Err(e) = crate::shortcut::register(&app_handle, &new_shortcut) {
            // 注册失败时回滚到旧快捷键
            eprintln!("Failed to register new shortcut '{}': {}", new_shortcut, e);
            let _ = crate::shortcut::register(&app_handle, &old_shortcut);
            return Err(format!("快捷键 '{}' 注册失败，已恢复原设置", new_shortcut));
        }
    }
```

- [ ] **Step 2: 编译验证**

```bash
cd src-tauri && cargo check
```

Expected: 编译通过

- [ ] **Step 3: 运行测试**

```bash
cd src-tauri && cargo test
```

Expected: 26 tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat: re-register shortcut on settings change"
```

---

### Task 5: 前端快捷键录制组件

**Files:**
- Create: `src/components/ShortcutRecordInput.tsx`

- [ ] **Step 1: 创建 ShortcutRecordInput 组件**

```tsx
// src/components/ShortcutRecordInput.tsx
import { useState, useCallback, useRef, useEffect } from "react";
import { Kbd } from "@heroui/react";

interface Props {
  value: string;
  onChange: (shortcut: string) => void;
  disabled?: boolean;
}

const isMac = navigator.platform.toUpperCase().indexOf("MAC") >= 0;

function shortcutToDisplay(s: string): string {
  return s
    .split("+")
    .map((part) => {
      const p = part.trim();
      if (isMac) {
        switch (p.toLowerCase()) {
          case "cmd": return "⌘";
          case "shift": return "⇧";
          case "ctrl": return "⌃";
          case "alt": return "⌥";
          default: return p.toUpperCase();
        }
      }
      return p;
    })
    .join(isMac ? "" : "+");
}

function codeToKey(code: string): string {
  // 去掉 "Key" 前缀
  if (code.startsWith("Key")) return code.slice(3);
  // 去掉 "Digit" 前缀
  if (code.startsWith("Digit")) return code.slice(5);
  // 保持原样（Space, Escape, Enter, Tab, Comma, Period,等等）
  return code;
}

export default function ShortcutRecordInput({ value, onChange, disabled }: Props) {
  const [recording, setRecording] = useState(false);
  const [pendingDisplay, setPendingDisplay] = useState("");
  const keysRef = useRef<{ modifiers: string[]; key: string | null }>({
    modifiers: [],
    key: null,
  });

  const finishRecording = useCallback(() => {
    const { modifiers, key } = keysRef.current;
    if (modifiers.length > 0 && key) {
      onChange([...modifiers, key].join("+"));
    }
    setRecording(false);
    setPendingDisplay("");
    keysRef.current = { modifiers: [], key: null };
  }, [onChange]);

  useEffect(() => {
    if (!recording) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const { modifiers, key: prevKey } = keysRef.current;

      // Esc 取消
      if (e.code === "Escape") {
        setRecording(false);
        setPendingDisplay("");
        keysRef.current = { modifiers: [], key: null };
        return;
      }

      // 收集修饰键
      const newModifiers: string[] = [];
      if (e.metaKey) newModifiers.push("Cmd");
      if (e.ctrlKey) newModifiers.push("Ctrl");
      if (e.altKey) newModifiers.push("Alt");
      if (e.shiftKey) newModifiers.push("Shift");

      // 收集主键（排除纯修饰键按键）
      const modifierCodes = [
        "MetaLeft", "MetaRight", "ControlLeft", "ControlRight",
        "ShiftLeft", "ShiftRight", "AltLeft", "AltRight",
      ];
      let mainKey: string | null = null;
      if (!modifierCodes.includes(e.code)) {
        mainKey = codeToKey(e.code);
      }

      // 更新 ref
      keysRef.current = {
        modifiers: newModifiers.length > 0 ? newModifiers : modifiers,
        key: mainKey || prevKey,
      };

      // 更新显示
      const displayMods = newModifiers.length > 0 ? newModifiers : modifiers;
      const displayKey = mainKey || prevKey;
      if (displayMods.length > 0 && displayKey) {
        setPendingDisplay(shortcutToDisplay(displayMods.join("+") + "+" + displayKey));
      } else if (displayMods.length > 0) {
        setPendingDisplay(shortcutToDisplay(displayMods.join("+")));
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      const { modifiers, key } = keysRef.current;
      // 仅在松开任意键时完成录制（至少一个修饰键 + 一个非修饰键）
      if (modifiers.length > 0 && key) {
        finishRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
    };
  }, [recording, finishRecording]);

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => {
        if (!disabled && !recording) {
          setRecording(true);
        }
      }}
      className={`inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border text-sm transition-colors select-none ${
        disabled
          ? "border-zinc-200 dark:border-zinc-700 text-zinc-400 cursor-not-allowed"
          : recording
            ? "border-blue-400 ring-2 ring-blue-400/30 bg-blue-50 dark:bg-blue-500/10 text-blue-600"
            : "border-zinc-200 dark:border-zinc-700 hover:border-zinc-300 dark:hover:border-zinc-600 cursor-pointer text-zinc-700 dark:text-zinc-300"
      }`}
    >
      {recording ? (
        <span className="flex items-center gap-1">
          {pendingDisplay || (
            <span className="text-zinc-400 animate-pulse">正在录制…</span>
          )}
        </span>
      ) : (
        <span className="flex items-center gap-1">
          {shortcutToDisplay(value).split("").map((ch, i) => (
            <Kbd
              key={i}
              className="bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 text-xs px-1.5 py-0.5"
            >
              {ch}
            </Kbd>
          ))}
        </span>
      )}
    </button>
  );
}
```

- [ ] **Step 2: 编译检查**

```bash
npm run tsc
```

Expected: 无错误

- [ ] **Step 3: Commit**

```bash
git add src/components/ShortcutRecordInput.tsx
git commit -m "feat: add shortcut recording input component"
```

---

### Task 6: 集成到设置页面

**Files:**
- Modify: `src/app/MainApp.tsx` (导入组件、添加 UI、SettingsData 接口)
- Modify: `src/locales/en.json`
- Modify: `src/locales/zh.json`

- [ ] **Step 1: 更新 MainApp.tsx 的 SettingsData 接口**

```tsx
// src/app/MainApp.tsx，在 interface SettingsData 中添加:
interface SettingsData {
  language: string;
  theme: string;
  autostart: boolean;
  max_items: number;
  shortcut: string;          // ← 新增
}
```

- [ ] **Step 2: 导入 ShortcutRecordInput**

```tsx
// src/app/MainApp.tsx，在现有 import 后添加:
import ShortcutRecordInput from "@/components/ShortcutRecordInput";
```

- [ ] **Step 3: 更新 loadSettings 和初始化 state**

```tsx
// 更新 useState 初始化:
const [settings, setSettings] = useState<SettingsData>({
  language: "zh",
  theme: "system",
  autostart: false,
  max_items: 20,
  shortcut: "Cmd+Shift+V",    // ← 新增
});

// 在 loadSettings 中读取:
const data = await invoke<SettingsData>("get_settings");
setSettings({
  language: data.language || "zh",
  theme: data.theme || "light",
  autostart: data.autostart ?? false,
  max_items: data.max_items ?? 20,
  shortcut: data.shortcut || "Cmd+Shift+V",   // ← 新增
});
```

- [ ] **Step 4: 添加 handleShortcutChange 回调**

```tsx
const handleShortcutChange = (shortcut: string) => {
  const newSettings = { ...settings, shortcut };
  setSettings(newSettings);
  saveSettings(newSettings);
};
```

- [ ] **Step 5: 在设置面板 UI 中添加快捷键绑定行**

在设置面板 `{activeTab === "settings" && ...}` 块中，在 `maxItems` 行之后（`</div>` 闭合前）添加：

```tsx
            <div className="flex flex-row items-center justify-between">
              <Label>{t('shortcutLabel')}</Label>
              <ShortcutRecordInput
                value={settings.shortcut}
                onChange={handleShortcutChange}
                disabled={loadingSettings}
              />
            </div>
```

- [ ] **Step 6: 添加国际化文本**

`src/locales/zh.json`:

```json
  "shortcutLabel": "快速粘贴快捷键"
```

`src/locales/en.json`:

```json
  "shortcutLabel": "Quick Paste Shortcut"
```

- [ ] **Step 7: 编译检查**

```bash
npm run tsc
```

Expected: 无错误

- [ ] **Step 8: Commit**

```bash
git add src/app/MainApp.tsx src/locales/en.json src/locales/zh.json
git commit -m "feat: integrate shortcut binding into settings page"
```

---

### Task 7: 更新 AGENTS.md 进度

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: 在 AGENTS.md 的共享进度中添加快捷键功能状态**

无需修改代码，在下一步 Task 8 一起提交。

---

### Task 8: 最终验证

- [ ] **Step 1: 全量 TypeScript 检查**

```bash
npm run tsc
```

Expected: 无错误

- [ ] **Step 2: Rust 编译检查**

```bash
cd src-tauri && cargo check
```

Expected: 无错误（可能有一些无关的 warnings）

- [ ] **Step 3: Rust 测试**

```bash
cd src-tauri && cargo test
```

Expected: 全部测试通过（8 + 18 + 若干 = ~34 tests，含新增的 Settings 序列化测试）

- [ ] **Step 4: 构建验证**

```bash
npm run tauri build
```

Expected: 成功构建

- [ ] **Step 5: 最终 Commit**

```bash
git add -A
git commit -m "feat: complete shortcut binding feature with recording UI"
```
