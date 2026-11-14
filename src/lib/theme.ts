import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_STYLE, themeById } from "@/themes";

export type ThemePref = "system" | "light" | "dark";

function darkQuery() {
  return window.matchMedia("(prefers-color-scheme: dark)");
}

function isDarkPref(theme: ThemePref): boolean {
  return theme === "dark" || (theme === "system" && darkQuery().matches);
}

/** 只负责把当前该用的明暗 + 风格画到 DOM 上 */
export function paint(theme: ThemePref, style: string = DEFAULT_STYLE): boolean {
  // 单模式风格（OLED 只有深色、侘寂只有浅色）不允许出现"浅色 + OLED"这种不成立的组合：
  // 这里兜底把明暗掰到它支持的那一档，设置页的自动切换只是让 UI 保持一致。
  const styleDef = themeById(style);
  let isDark = isDarkPref(theme);
  if (!styleDef.modes.includes(isDark ? "dark" : "light")) {
    isDark = styleDef.modes[0] === "dark";
  }
  const html = document.documentElement;
  html.classList.toggle("dark", isDark);
  html.classList.toggle("light", !isDark);
  html.setAttribute("data-theme", isDark ? "dark" : "light");
  // 主题 token 挂在 html[data-style] 上，见 index.css
  html.setAttribute("data-style", style || DEFAULT_STYLE);
  return isDark;
}

/**
 * 设置窗口用：同时改原生窗口外观。
 * 坑：setTheme("light") 会把窗口的 preferred-color-scheme 也钉成 light，
 * 所以从「浅色」切回「跟随系统」时，matchMedia 读到的还是刚才那个 light。
 * 必须先把外观交还系统（null），等它生效后再重画一次。
 */
export function applyTheme(theme: ThemePref, style: string = DEFAULT_STYLE) {
  try {
    const win = getCurrentWindow();
    if (typeof win.setTheme === "function") {
      void win.setTheme(theme === "system" ? null : theme).catch(() => {});
    }
  } catch {
    // 非 Tauri 环境（浏览器里跑组件预览）忽略
  }

  paint(theme, style);

  if (theme === "system") {
    // 原生外观切换是异步的，media query 下一帧才更新
    requestAnimationFrame(() => paint(theme, style));
    window.setTimeout(() => paint(theme, style), 120);
  }
}

/** 系统明暗切换时通知调用方（只在 system 档需要处理） */
export function watchSystemTheme(onChange: () => void): () => void {
  const mq = darkQuery();
  const handler = () => onChange();
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
