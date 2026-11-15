import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_STYLE, themeById } from "@/themes";

function darkQuery() {
  return window.matchMedia("(prefers-color-scheme: dark)");
}

/**
 * 把当前风格画到 DOM 上。
 *
 * 现在没有「明暗」这个设置了：
 * - 双模式风格（玻璃、瑞士、M3…）跟随系统明暗
 * - 单模式风格（OLED 只有深色、侘寂只有浅色）固定用自己的那一档
 */
export function paint(style: string = DEFAULT_STYLE): boolean {
  const styleDef = themeById(style);
  let isDark = darkQuery().matches;
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
 * 没有明暗开关之后，窗口外观一律交还系统（null），让双模式风格跟着系统走。
 */
export function applyTheme(style: string = DEFAULT_STYLE) {
  try {
    const win = getCurrentWindow();
    if (typeof win.setTheme === "function") {
      void win.setTheme(null).catch(() => {});
    }
  } catch {
    // 非 Tauri 环境（浏览器里跑组件预览）忽略
  }

  paint(style);

  // 原生外观是异步生效的，media query 下一帧才更新
  requestAnimationFrame(() => paint(style));
  window.setTimeout(() => paint(style), 120);
}

/** 系统明暗切换时通知调用方（双模式风格需要重画） */
export function watchSystemTheme(onChange: () => void): () => void {
  const mq = darkQuery();
  const handler = () => onChange();
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
