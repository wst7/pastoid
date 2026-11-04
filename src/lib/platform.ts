/** 运行平台判断：Tauri 的 Windows 端 UA / platform 都不含 Mac */
export const isMac: boolean =
  typeof navigator !== "undefined" &&
  (navigator.userAgent.includes("Mac") || /Mac/i.test(navigator.platform ?? ""));

/** 默认呼出快捷键，与后端 models::default_shortcut 保持一致 */
export const defaultShortcut = isMac ? "Cmd+Shift+V" : "Ctrl+Shift+V";

/** 主修饰键在界面上的叫法 */
export const modKeyLabel = isMac ? "⌘" : "Ctrl";
