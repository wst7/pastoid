# 快捷键可配置化设计

## 概述

允许用户在设置页面自定义快速粘贴面板的全局快捷键，替换当前硬编码的 `Cmd+Shift+V`。采用前端录制模式，字符串序列化存储。

## 数据模型

### Settings (Rust)

```rust
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    pub max_items: u32,
    pub shortcut: String,   // 新增，默认 "Cmd+Shift+V"
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // ...
            shortcut: "Cmd+Shift+V".to_string(),
        }
    }
}
```

### 字符串格式

`修饰键+修饰键+...+按键`，至少一个修饰键。示例：

- `Cmd+V` — 单修饰键
- `Cmd+Shift+V` — 双修饰键
- `Ctrl+Alt+Shift+K` — 三修饰键
- `Ctrl+Shift+Space` — 特殊键名

修饰键名：`Cmd` (macOS SUPER)、`Ctrl` (CONTROL)、`Shift`、`Alt`。按键名遵循 `tauri_plugin_global_shortcut::Code` 的 `Debug` 输出格式（不含 `Key` 前缀时不加），如 `V`、`Space`、`Escape`。

## 架构

### 后端：快捷键管理器 (`src-tauri/src/shortcut.rs`)

新建模块，封装全局快捷键的注册/注销/重注册：

```
parse_shortcut(s: &str) -> Result<(Modifiers, Code), String>
├─ 按 '+' 分割
├─ 映射修饰键: "Cmd"→SUPER, "Ctrl"→CONTROL, "Shift"→SHIFT, "Alt"→ALT
├─ 映射按键: name → Code (如 "V"→KeyV, "Space"→Space, "#Key0"→Digit0)
└─ 校验: 至少一个修饰键 + 一个按键

register(app, shortcut_str, callback) -> Result<ShortcutId>
├─ 调用 parse_shortcut 解析
└─ 调用 global_shortcut().on_shortcut() 注册回调

unregister(app, shortcut_id)
└─ 调用 global_shortcut().unregister(shortcut_id)
```

### 启动流程 (`lib.rs`)

```
1. 加载 settings
2. 从 settings.shortcut 读取快捷键字符串
3. shortcut::register(app, &shortcut_str, callback)
4. 失败 → fallback 到默认 "Cmd+Shift+V" + eprintln warning
```

### 设置保存流程 (`commands::save_settings`)

```
1. 比较 old_shortcut vs new_shortcut
2. 如果变化:
   a. shortcut::unregister(app, old_id)
   b. shortcut::register(app, &new_shortcut, callback)
   c. 注册失败 → 提示用户 + 保留上次有效值 + return Err
3. 持久化 settings 到文件
```

### 后端侧回调内容

与当前 `lib.rs:62-74` 逻辑完全一致：按下列出/隐藏 quick-paste 窗口，显示时 emit `panel-opened` 事件。提取为纯函数 `toggle_quick_paste(app)`。

## 前端

### 录制输入组件 (`src/components/ShortcutRecordInput.tsx`)

**状态：**
- `recording: boolean` — 是否处于录制模式
- `display: string` — 显示文本（macOS 用符号：`⌘⇧V`，其他平台用 `Ctrl+Shift+V`）

**行为：**
1. 组件显示当前快捷键 → 点击进入录制 → placeholder 变为 "正在录制…"
2. `keydown` 事件捕获：
   - 记录按下的修饰键（`e.metaKey/ctrlKey/shiftKey/altKey`）
   - 记录按下的物理键（`e.code` 如 `KeyV`、`Space`、`Digit0`）
   - 必须至少一个修饰键 + 一个非修饰键 → 完成录制
3. 完成录制后 → 组装字符串 → 调用 `save_settings` → 退出录制模式
4. Esc → 取消录制，恢复原值
5. 不在录制时 Escape/功能键不干扰

**平台适配：**
- 显示：macOS 用 `⌘⇧V` 符号，Windows/Linux 用 `Ctrl+Shift+V` 文本
- 修饰键收集：macOS 收集 `metaKey`，Windows/Linux 收集 `ctrlKey`

## 国际化

新增翻译键：

| 键 | zh | en |
|---|---|---|
| `shortcutLabel` | 快速粘贴快捷键 | Quick Paste Shortcut |
| `shortcutPlaceholder` | 点击录制快捷键 | Click to record shortcut |

## 错误处理

| 场景 | 行为 |
|---|---|
| 无效快捷键字符串 | 启动时打印 warning，回退到 `Cmd+Shift+V` |
| 注册冲突/无权限 | `save_settings` 返回错误，前端提示，保留旧值 |
| 录制时仅修饰键 | 忽略，继续等待按键 |
| 录制时按 Esc | 取消录制，恢复原值 |

## 测试

- Rust: `parse_shortcut` 单元测试（合法/非法/边界）
- Rust: `Settings` 默认值包含 `shortcut` 字段
- TypeScript: 无新增运行时逻辑测试（组件级手工验证）
- 集成测试：`cargo test` 通过 (26 tests → 新增 parse 测试)

## 影响范围

| 文件 | 变更 |
|---|---|
| `src-tauri/src/models.rs` | Settings 加 `shortcut` 字段 |
| `src-tauri/src/shortcut.rs` | **新建**，注册/注销/解析 |
| `src-tauri/src/lib.rs` | 用 shortcut 模块替换硬编码注册 |
| `src-tauri/src/commands.rs` | `save_settings` 检测 shortcut 变化 |
| `src-tauri/src/storage.rs` | 无变更（Settings 自动序列化新字段） |
| `src/components/ShortcutRecordInput.tsx` | **新建**录制组件 |
| `src/app/MainApp.tsx` | 设置页面加入快捷键绑定项 |
| `src/locales/en.json` | 新增 2 个键 |
| `src/locales/zh.json` | 新增 2 个键 |
