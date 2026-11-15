#!/usr/bin/env node
/**
 * 主题体检：确保每套风格都有明暗两版、核心 token 齐全、关键对比度达标。
 * 用法：node scripts/check-themes.mjs
 */
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(resolve(root, "src/index.css"), "utf8");
const themesTs = readFileSync(resolve(root, "src/themes/index.ts"), "utf8");

const activeIds = [...themesTs.matchAll(/\{\s*id:\s*"([a-z]+)"/g)].map((m) => m[1]);

const blocks = new Map();
for (const m of css.matchAll(/:root\[data-style="([a-z]+)"\]\.(light|dark)\{([^}]*)\}/g)) {
  blocks.set(`${m[1]}/${m[2]}`, m[3]);
}

/** 返回 [r,g,b,a]；半透明卡片色要先与背景合成再算对比度 */
const parse = (v) => {
  if (!v) return null;
  v = v.trim();
  if (v.startsWith("#")) {
    let h = v.slice(1);
    if (h.length === 3) h = [...h].map((c) => c + c).join("");
    const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    return [r, g, b, 1];
  }
  const m = v.match(/rgba?\(([^)]+)\)/);
  if (!m) return null;
  const parts = m[1].split(",").map(Number);
  return [parts[0], parts[1], parts[2], parts.length > 3 ? parts[3] : 1];
};
const blend = (fg, bg, a = fg[3] ?? 1) => [0, 1, 2].map((i) => fg[i] * a + bg[i] * (1 - a));
const lum = (rgb) => {
  const f = (v) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = rgb.map(f);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => {
  const la = lum(a), lb = lum(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
};

const CORE = ["background", "foreground", "card", "card-foreground", "primary", "muted-foreground", "border"];
const rows = [];
const problems = [];

for (const id of activeIds) {
  for (const mode of ["light", "dark"]) {
    const key = `${id}/${mode}`;
    const body = blocks.get(key);
    if (!body) {
      problems.push(`${key}: 缺少主题块`);
      continue;
    }
    const t = Object.fromEntries([...body.matchAll(/--([a-z-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2]]));
    for (const token of CORE) if (!t[token]) problems.push(`${key}: 缺少 --${token}`);

    const bg = blend(parse(t.background) ?? [255, 255, 255, 1], [255, 255, 255]);
    const card = blend(parse(t.card) ?? [255, 255, 255, 1], bg);
    const fg = blend(parse(t.foreground) ?? [0, 0, 0, 1], bg);
    const muted = blend(parse(t["muted-foreground"]) ?? [0, 0, 0, 1], bg);
    const primary = blend(parse(t.primary) ?? [0, 0, 0, 1], bg);
    const cardFg = blend(parse(t["card-foreground"]) ?? [0, 0, 0, 1], card);
    const search = blend(parse(t.default) ?? [255, 255, 255, 1], bg);

    const checks = [
      ["正文/背景", fg && ratio(fg, bg), 4.5],
      ["次要/背景", muted && ratio(muted, bg), 3],
      ["主色/背景", primary && ratio(primary, bg), 3],
      ["主色/卡片", primary && ratio(primary, card), 3],
      ["卡片文字/卡片", cardFg && ratio(cardFg, card), 4.5],
      ["搜索底/背景", ratio(search, bg), 1.04],
    ];
    for (const [label, value, min] of checks) {
      if (value == null) continue;
      if (value < min) problems.push(`${key}: ${label} ${value.toFixed(2)} < ${min}`);
    }
    rows.push({ key, fg: fg && ratio(fg, bg), muted: muted && ratio(muted, bg), primary: primary && ratio(primary, bg) });
  }
}

for (const r of rows) {
  console.log(`${r.key.padEnd(16)} fg ${r.fg?.toFixed(1)}  muted ${r.muted?.toFixed(1)}  primary ${r.primary?.toFixed(1)}`);
}

if (problems.length) {
  console.error(`\n${problems.length} 个问题：`);
  for (const p of problems) console.error("  - " + p);
  process.exit(1);
}
console.log(`\nOK：${activeIds.length} 套风格 × 2 种明暗，token 与对比度全部达标`);
