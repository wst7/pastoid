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

/** 标签及使用次数（搜索框的标签建议用） */
export interface TagCount {
  tag: string;
  count: number;
}

/** 各筛选维度的全量计数（后端 SQL 统计，筛选 chip 用） */
export interface ClipboardCounts {
  all: number;
  text: number;
  image: number;
  link: number;
}
