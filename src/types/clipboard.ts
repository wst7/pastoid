/**
 * 剪切板类型定义
 */

export type ClipboardItemType = "text" | "image" | "file";

export interface ClipboardItem {
  id: string;
  /** 文本条目是原文；图片条目是相对数据目录的路径，如 `images/<hash>.png` */
  content: string;
  type: ClipboardItemType;
  createdAt: number;
  updatedAt: number;
  isPinned: boolean;
  tags?: string[];
  source?: string;
  imageHash?: string;
}

export type ClipboardFilter = "all" | "text" | "image" | "link";
