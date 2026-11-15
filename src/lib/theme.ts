import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_STYLE, themeById } from "@/themes";

export type ThemeMode = "light" | "dark";
export type ThemePref = ThemeMode | "system";

function darkQuery() {
  return window.matchMedia("(prefers-color-scheme: dark)");
}

/**
 * 把风格 + 明暗画到 DOM 上。
 *
 * `theme` 是用户在「外观」里选的档：light / dark / system。
 * 选 system 时跟随操作系统；单模式风格仍然会兜底到它支持的那一档。
 */
export function paint(style: string = DEFAULT_STYLE, theme: ThemePref = "system"): boolean {
  const styleDef = themeById(style);
  let isDark = theme === "system" ? darkQuery().matches : theme === "dark";
  if (!styleDef.modes.includes(isDark ? "dark" : "light")) {
    isDark = styleDef.modes[0] === "dark";
  }

  const html = document.documentElement;
  html.classList.toggle("dark", isDark);
  html.classList.toggle("light", !isDark);
  html.setAttribute("data-theme", isDark ? "dark" : "light");
  html.setAttribute("data-style", style || DEFAULT_STYLE);
  return isDark;
}

/**
 * 设置窗口用：同时改原生窗口外观。
 * 选了明确的明暗就把窗口外观固定住，选「跟随系统」则交还系统（null）。
 */
export function applyTheme(style: string = DEFAULT_STYLE, theme: ThemePref = "system") {
  let applied: Promise<void> = Promise.resolve();
  try {
    const win = getCurrentWindow();
    if (typeof win.setTheme === "function") {
      applied = win
        .setTheme(theme === "system" ? null : theme)
        .then(() => undefined)
        .catch(() => undefined);
    }
  } catch {
    // 非 Tauri 环境（浏览器里跑组件预览）忽略
  }

  // 先按当前已知状态画一次，避免等原生窗口
  paint(style, theme);

  // 原生外观生效后 media query 才会更新，再画一次把「跟随系统」校正到真正的系统明暗
  void applied.then(() => paint(style, theme));
  requestAnimationFrame(() => paint(style, theme));
  // 兜底：个别平台 setTheme 的 promise 与 media query 更新不同步
  window.setTimeout(() => paint(style, theme), 120);
}

/** 系统明暗切换时通知调用方（双模式风格需要重画） */
export function watchSystemTheme(onChange: () => void): () => void {
  const mq = darkQuery();
  const handler = () => onChange();
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
