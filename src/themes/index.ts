/**
 * 视觉风格注册表。
 *
 * 每个风格在 src/index.css 里有对应的 token 覆盖
 * （`:root[data-style="<id>"].light/.dark`），这里只描述元数据：
 * 叫什么、支持哪些明暗模式。
 *
 * 每套风格都做了明暗两版（结构/圆角/边框不变，只换颜色）。
 * `modes` 留作能力声明：目前全是 ["light","dark"]，paint() 会跟随系统明暗。
 * 侘寂的米纸色在深色下也不成立。切到只支持单一模式的主题时，明暗会自动跟过去。
 */
export type ThemeMode = "light" | "dark";

export interface ThemeStyle {
  id: string;
  /** 中文名 */
  name: string;
  /** 英文名（英文界面用） */
  nameEn: string;
  /** 这个风格支持的明暗模式 */
  modes: ThemeMode[];
}

export const THEMES: ThemeStyle[] = [
  { id: "default", name: "默认", nameEn: "Default", modes: ["light", "dark"] },
  { id: "glass", name: "玻璃拟态", nameEn: "Glassmorphism", modes: ["light", "dark"] },
  { id: "swiss", name: "瑞士国际主义", nameEn: "Swiss Style", modes: ["light", "dark"] },
  { id: "material", name: "Material 3", nameEn: "Material 3", modes: ["light", "dark"] },
  { id: "notion", name: "文档工作台", nameEn: "Document", modes: ["light", "dark"] },
  { id: "oled", name: "OLED 纯粹", nameEn: "OLED Pure", modes: ["light", "dark"] },
  { id: "soft", name: "柔雾渐层", nameEn: "Soft Gradient", modes: ["light", "dark"] },
  { id: "mica", name: "Fluent 云母", nameEn: "Fluent Mica", modes: ["light", "dark"] },
  { id: "aurora", name: "极光界面", nameEn: "Aurora", modes: ["light", "dark"] },
  { id: "zen", name: "日式侘寂", nameEn: "Wabi-sabi", modes: ["light", "dark"] },
  { id: "academia", name: "学院手稿", nameEn: "Academia", modes: ["light", "dark"] },
  { id: "brutal", name: "新粗野主义", nameEn: "Neo Brutalism", modes: ["light", "dark"] },
];

export const DEFAULT_STYLE = "default";

export function themeById(id: string | undefined | null): ThemeStyle {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

/**
 * 主题不支持当前明暗时，给出它支持的那一档。
 *
 * 例：用户在浅色模式下选 OLED → 自动切到深色；
 * 反过来在深色模式下选侘寂 → 自动切回浅色。
 * 支持双模式的主题原样返回。
 */
export function modeForTheme(style: ThemeStyle, current: ThemeMode): ThemeMode {
  if (style.modes.includes(current)) return current;
  return style.modes[0];
}
