import type { ClipboardItem } from "@/types/clipboard";

/** 文本里是否是一个 http(s) 链接 */
export function isLink(content: string): boolean {
  return /^https?:\/\//i.test(content.trim());
}

/** 取出链接的域名，取不到就退回原文 */
export function linkDomain(content: string): string {
  try {
    return new URL(content.trim()).hostname.replace(/^www\./, "");
  } catch {
    return content.trim();
  }
}

/** 图片条目的磁盘相对路径 */
export function imagePath(item: ClipboardItem): string {
  return item.content;
}

/** 列表里用来区分展示类型的标签 */
export function itemKind(item: ClipboardItem): "image" | "link" | "text" {
  if (item.type === "image") return "image";
  if (isLink(item.content)) return "link";
  return "text";
}

/**
 * 相对时间：刚刚 / 12 分钟前 / 3 小时前 / 昨天 14:32 / 3/12 14:32
 * `lang` 只影响“刚刚/分钟前”这类词。
 */
export function formatRelativeTime(timestamp: number, lang: string): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const zh = lang.startsWith("zh");

  const pad = (n: number) => n.toString().padStart(2, "0");
  const hhmm = `${pad(date.getHours())}:${pad(date.getMinutes())}`;

  if (diffMs < 0) return hhmm;
  if (diffMs < 60_000) return zh ? "刚刚" : "just now";

  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return zh ? `${minutes} 分钟前` : `${minutes}m ago`;

  const sameDay = date.toDateString() === now.toDateString();
  if (sameDay) return hhmm;

  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) {
    return zh ? `昨天 ${hhmm}` : `Yesterday ${hhmm}`;
  }

  const md = `${date.getMonth() + 1}/${date.getDate()}`;
  return `${md} ${hhmm}`;
}

/** 列表排序：置顶优先，其次按时间倒序 */
export function sortItems(items: ClipboardItem[]): ClipboardItem[] {
  return [...items].sort((a, b) => {
    if (a.isPinned !== b.isPinned) return Number(b.isPinned) - Number(a.isPinned);
    return b.createdAt - a.createdAt;
  });
}
