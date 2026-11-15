import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { paint, watchSystemTheme } from "@/lib/theme";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  Search,
  Clipboard,
  Pin,
  Trash2,
  X,
  Image as ImageIcon,
  Link2,
  Type,
  AlertCircle,
  Tag,
  Pencil,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import i18n from "@/i18n";
import { isMac, modKeyLabel } from "@/lib/platform";
import type { ClipboardCounts, ClipboardItem, ClipboardFilter, TagCount } from "@/types/clipboard";
import { formatRelativeTime, itemKind, linkDomain } from "@/lib/clipboard";

interface SettingsData {
  theme: string;
  theme_style: string;
  language: string;
}

/** 平台相关的按键显示：macOS 用符号，Windows 用文字 */
const modKey = modKeyLabel;
const shiftLabel = isMac ? "⇧" : "Shift";
const backspaceLabel = isMac ? "⌫" : "Backspace";
const enterLabel = isMac ? "↵" : "Enter";
const escLabel = isMac ? "esc" : "Esc";

/** 面板退场动画时长，必须和 index.css 里的 .panel-anim-out 一致 */
const PANEL_EXIT_MS = 70;

/** 后端一次返回的窗口大小：历史可以无限增长，前端只拿面板用得上的那一截 */
const PANEL_WINDOW = 200;
/** 面板上最多摆多少张卡，更多靠搜索找 */
const PANEL_VISIBLE = 40;
/** 搜索防抖：每敲一个字符就打一次后端没有意义 */
const SEARCH_DEBOUNCE_MS = 120;

/** 图片内容只拉一次，后续复用 data URL */
const imageCache = new Map<string, string>();

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function ImageThumb({ filename }: { filename: string }) {
  const [src, setSrc] = useState<string | null>(() => imageCache.get(filename) ?? null);

  useEffect(() => {
    if (imageCache.has(filename)) {
      setSrc(imageCache.get(filename)!);
      return;
    }
    let cancelled = false;
    invoke<string>("get_image_data", { filename })
      .then((data) => {
        imageCache.set(filename, data);
        if (!cancelled) setSrc(data);
      })
      .catch(() => {
        /* 图片读不出来就不显示，卡片其余部分照常 */
      });
    return () => {
      cancelled = true;
    };
  }, [filename]);

  if (!src) {
    return (
      <div className="w-full h-full flex items-center justify-center text-muted-foreground">
        <ImageIcon size={20} strokeWidth={1.5} />
      </div>
    );
  }

  return (
    <img
      src={src}
      alt=""
      className="w-full h-full object-cover rounded-lg"
      draggable={false}
    />
  );
}

function KindIcon({ kind }: { kind: "image" | "link" | "text" }) {
  if (kind === "image") return <ImageIcon size={12} className="text-violet-400" />;
  if (kind === "link") return <Link2 size={12} className="text-sky-400" />;
  return <Type size={12} className="text-muted-foreground" />;
}

export default function QuickPaste() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ClipboardFilter>("all");
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  // 筛选 chip 的计数是全量统计（后端 SQL 出），不是当前窗口的条数
  const [counts, setCounts] = useState<ClipboardCounts>({ all: 0, text: 0, image: 0, link: 0 });
  // 选中态用 id 而不是下标：卡片重排（置顶、删除、新条目插入）时，
  // 选中的应该始终是「同一条内容」，否则回车会粘错东西。
  const [activeId, setActiveId] = useState<string | null>(null);
  // 面板打开期间冻结排序，避免卡片在指针下方跳动；关闭后再重排
  const [orderFrozen, setOrderFrozen] = useState(false);
  // 标签编辑条 / 正文编辑弹层（同一时间只开一个）
  const [tagEditor, setTagEditor] = useState<{ id: string; value: string } | null>(null);
  // 所有标签 + 使用次数，给 `#` 搜索做建议
  const [allTags, setAllTags] = useState<TagCount[]>([]);
  const [tagIndex, setTagIndex] = useState(0);
  const [editor, setEditor] = useState<{ id: string; draft: string; error?: string } | null>(
    null
  );
  const [language, setLanguage] = useState("zh");
  // 双模式风格跟随系统明暗，系统切暗时要重画
  const styleRef = useRef<string>("default");

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const blurEnabled = useRef(false);
  const closingRef = useRef(false);
  const closeTimerRef = useRef<number | null>(null);
  // 下一次 activeId 变化要不要滚动到可视区：键盘要，hover 不要
  const scrollNext = useRef(true);
  // 横向滚动的补间句柄（同一时间只允许一个）
  const scrollAnimRef = useRef<number | null>(null);
  // 滚动补间期间及其后一小段时间内忽略 hover 选中：
  // 卡片从静止的鼠标指针下经过会触发 mouseover，否则会把键盘选中的卡抢走
  const hoverLockUntil = useRef(0);

  /**
   * 横向滚动的补间：键盘换卡时让卡片条滑过去，而不是瞬移。
   *
   * 不用 `scrollIntoView({behavior:"smooth"})`：WKWebView / WebView2 的
   * 原生平滑滚动时长不可控（往往 300ms+），连续按键会拖沓；
   * 这里自己补间，时长按距离给，封顶 220ms。
   */
  function animateScrollLeft(strip: HTMLDivElement, target: number) {
    if (scrollAnimRef.current !== null) {
      cancelAnimationFrame(scrollAnimRef.current);
      scrollAnimRef.current = null;
    }
    const start = strip.scrollLeft;
    const delta = target - start;
    if (Math.abs(delta) < 1) return;

    if (prefersReducedMotion()) {
      strip.scrollLeft = target;
      return;
    }

    const duration = Math.min(220, 110 + Math.abs(delta) * 0.35);
    // 补间结束再宽限 80ms：滚动停下时可能还会补发一次 mouseover
    hoverLockUntil.current = performance.now() + duration + 80;
    const started = performance.now();
    const step = (now: number) => {
      const p = Math.min(1, (now - started) / duration);
      // easeOutCubic：起步快、收尾稳，和面板入场的 decelerate 一致
      const eased = 1 - Math.pow(1 - p, 3);
      strip.scrollLeft = start + delta * eased;
      scrollAnimRef.current = p < 1 ? requestAnimationFrame(step) : null;
    };
    scrollAnimRef.current = requestAnimationFrame(step);
  }

  // 键盘监听只注册一次，闭包里拿不到最新的 search，用 ref 兜住。
  // 之前 Esc 用闭包里的 search，永远是 ""，所以「先清搜索再关窗」那段是死代码。
  const searchRef = useRef(search);
  searchRef.current = search;

  const filterRef = useRef(filter);
  filterRef.current = filter;

  // `#` 前缀进入标签模式：只按标签查，并给出已有标签的建议
  const tagQuery = search.startsWith("#") ? search.slice(1).trim().toLowerCase() : null;
  const suggestions =
    tagQuery === null
      ? []
      : allTags
          .filter((t) => t.tag.toLowerCase().includes(tagQuery))
          .slice(0, 6);
  // 已经完整打出某个标签时不再弹建议，否则会把第一张卡挡住
  const exactTag =
    tagQuery !== null && allTags.some((t) => t.tag.toLowerCase() === tagQuery);
  const showSuggestions = suggestions.length > 0 && !exactTag;


  /**
   * 拉一次面板数据：窗口、筛选、搜索全在后端（SQL）完成。
   * `reset` 时选中落到第一张（搜索/筛选/重开面板）；否则尽量保持选中同一条内容。
   */
  const refresh = useCallback(
    async (opts?: { reset?: boolean; query?: string; filter?: ClipboardFilter }) => {
      const query = opts?.query ?? searchRef.current;
      const flt = opts?.filter ?? filterRef.current;
      try {
        const [data, countsData] = await Promise.all([
          invoke<ClipboardItem[]>("list_clipboard_items", {
            limit: PANEL_WINDOW,
            filter: flt,
            query,
          }),
          invoke<ClipboardCounts>("get_clipboard_counts"),
        ]);
        setItems(data);
        setCounts(countsData);
        setActiveId((prev) => {
          if (!opts?.reset && prev && data.some((i) => i.id === prev)) return prev;
          return data[0]?.id ?? null;
        });
        setLoadError(null);
      } catch (e) {
        console.error("Failed to load items:", e);
        // 出错和「剪贴板是空的」长得一样，必须区分，否则用户以为记录丢了
        setLoadError(String(e));
      }
    },
    []
  );

  // 前端只负责展示窗口里的前 40 张，其余靠搜索
  const filteredItems = useMemo(() => items.slice(0, PANEL_VISIBLE), [items]);

  const paste = useCallback(async (item: ClipboardItem) => {
    try {
      // 按 id 粘贴：图片条目要走图片通道，不能把文件路径当文本写进剪贴板。
      // 粘贴路径不做退场动画——Rust 侧隐藏窗口后还有 Cmd+Tab 的等待。
      await invoke("paste_item", { id: item.id });
    } catch (e) {
      console.error("Paste failed:", e);
    }
  }, []);

  const togglePin = useCallback(async (item: ClipboardItem) => {
    try {
      const updated = { ...item, isPinned: !item.isPinned, updatedAt: Date.now() };
      await invoke("update_clipboard_item", { item: updated });
      // 只换图钉，不重排：面板开着的时候卡片不能从指针下面跑掉。
      // 下次打开时 refresh() 会按置顶/时间重新排序。
      setItems((prev) => prev.map((i) => (i.id === item.id ? updated : i)));
      setOrderFrozen(true);
    } catch (e) {
      console.error("Toggle pin failed:", e);
    }
  }, []);

  const loadTags = useCallback(async () => {
    try {
      setAllTags(await invoke<TagCount[]>("get_clipboard_tags"));
    } catch (e) {
      console.error("Load tags failed:", e);
    }
  }, []);

  /** 把搜索切到「只看这个标签」 */
  const applyTagQuery = useCallback((tag: string) => {
    setSearch(`#${tag}`);
    inputRef.current?.focus();
  }, []);

  /** 把一条修改写回后端并同步本地列表 */
  const persistItem = useCallback(async (next: ClipboardItem) => {
    await invoke("update_clipboard_item", { item: next });
    setItems((prev) => prev.map((i) => (i.id === next.id ? next : i)));
  }, []);

  /** 加标签：去掉前导 #、限长、去重；标签是用户投入，后端不会滚动删除这类条目 */
  const addTag = useCallback(
    async (item: ClipboardItem, raw: string) => {
      const tag = raw.trim().replace(/^#+/, "").slice(0, 24);
      if (!tag) return;
      const existing = item.tags ?? [];
      if (existing.includes(tag)) return;
      try {
        await persistItem({ ...item, tags: [...existing, tag], updatedAt: Date.now() });
        void loadTags();
      } catch (e) {
        console.error("Add tag failed:", e);
      }
    },
    [persistItem, loadTags]
  );

  const removeTag = useCallback(
    async (item: ClipboardItem, tag: string) => {
      const next = (item.tags ?? []).filter((t) => t !== tag);
      try {
        await persistItem({ ...item, tags: next, updatedAt: Date.now() });
        void loadTags();
      } catch (e) {
        console.error("Remove tag failed:", e);
      }
    },
    [persistItem, loadTags]
  );

  const openTagEditor = useCallback((item: ClipboardItem) => {
    setEditor(null);
    setTagEditor({ id: item.id, value: "" });
  }, []);

  const openEditor = useCallback((item: ClipboardItem) => {
    if (item.type === "image") return;
    setTagEditor(null);
    setEditor({ id: item.id, draft: item.content });
  }, []);

  const saveEditor = useCallback(async () => {
    if (!editor) return;
    const item = items.find((i) => i.id === editor.id);
    if (!item) {
      setEditor(null);
      return;
    }
    if (editor.draft === item.content) {
      setEditor(null);
      return;
    }
    try {
      await persistItem({ ...item, content: editor.draft, updatedAt: Date.now() });
      setEditor(null);
    } catch (e) {
      // 内容哈希有唯一索引：改成了另一条已有的内容会撞约束
      const message = String(e).includes("UNIQUE") ? t("duplicateContent") : String(e);
      setEditor((prev) => (prev ? { ...prev, error: message } : prev));
    }
  }, [editor, items, persistItem, t]);

  const deleteItem = useCallback(async (item: ClipboardItem) => {
    try {
      const list = filteredItemsRef.current;
      const idx = list.findIndex((i) => i.id === item.id);
      await invoke("delete_clipboard_item", { id: item.id });
      setItems((prev) => prev.filter((i) => i.id !== item.id));
      setActiveId((prev) => {
        if (prev !== item.id) return prev;
        // 优先落到后一张，删的是最后一张就往前一张
        const next = list[idx + 1] ?? list[idx - 1];
        return next?.id ?? null;
      });
    } catch (e) {
      console.error("Delete failed:", e);
    }
  }, []);

  const clearAll = useCallback(async () => {
    try {
      await invoke("clear_clipboard_items");
      setItems([]);
      setActiveId(null);
      imageCache.clear();
    } catch (e) {
      console.error("Clear all failed:", e);
    }
  }, []);

  /** 按方向键移动选中：先算下标，再落到 id */
  const selectIndex = useCallback((index: number, scroll = true) => {
    const list = filteredItemsRef.current;
    if (!list.length) return;
    const clamped = Math.max(0, Math.min(index, list.length - 1));
    scrollNext.current = scroll;
    setActiveId(list[clamped].id);
  }, []);

  const hoverItem = useCallback((id: string) => {
    // 键盘滚动刚把卡片挪到指针下面，这次 mouseover 不是用户移动鼠标造成的，忽略
    if (performance.now() < hoverLockUntil.current) return;
    // hover 只改选中，不滚动：否则鼠标下的卡片会自己跑，感觉"追不上"
    scrollNext.current = false;
    setActiveId(id);
  }, []);

  // 事件回调里要用到最新值，用 ref 兜住，避免每次重建监听
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  const filteredItemsRef = useRef(filteredItems);
  filteredItemsRef.current = filteredItems;

  const pasteRef = useRef(paste);
  pasteRef.current = paste;
  const togglePinRef = useRef(togglePin);
  togglePinRef.current = togglePin;
  const deleteItemRef = useRef(deleteItem);
  deleteItemRef.current = deleteItem;
  const clearAllRef = useRef(clearAll);
  clearAllRef.current = clearAll;
  const openEditorRef = useRef(openEditor);
  openEditorRef.current = openEditor;
  const openTagEditorRef = useRef(openTagEditor);
  openTagEditorRef.current = openTagEditor;
  // 建议列表要在键盘回调里读到最新值
  const suggestionsRef = useRef<{ tag: string }[]>([]);
  suggestionsRef.current = suggestions;
  const showSuggestionsRef = useRef(false);
  showSuggestionsRef.current = showSuggestions;
  const tagIndexRef = useRef(0);
  tagIndexRef.current = tagIndex;
  const applyTagQueryRef = useRef(applyTagQuery);
  applyTagQueryRef.current = applyTagQuery;

  /** 重放入场动画（窗口本身是 show() 出来的，CSS 动画得手动触发） */
  const playEnter = useCallback(() => {
    const el = panelRef.current;
    if (!el || prefersReducedMotion()) return;
    el.classList.remove("panel-anim-in", "panel-anim-out");
    void el.offsetWidth;
    el.classList.add("panel-anim-in");
  }, []);

  /** 退场动画结束后再真正隐藏；粘贴不走这里，由 Rust 直接 hide */
  const requestClose = useCallback(() => {
    if (closingRef.current) return;
    closingRef.current = true;
    const win = getCurrentWindow();
    const el = panelRef.current;

    if (!el || prefersReducedMotion()) {
      void win.hide();
      closingRef.current = false;
      return;
    }

    el.classList.remove("panel-anim-in");
    el.classList.add("panel-anim-out");
    closeTimerRef.current = window.setTimeout(() => {
      void win.hide();
      el.classList.remove("panel-anim-out");
      closingRef.current = false;
      closeTimerRef.current = null;
    }, PANEL_EXIT_MS);
  }, []);

  useEffect(() => {
    return watchSystemTheme(() => {
      paint(styleRef.current);
    });
  }, []);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((data) => {
        styleRef.current = data.theme_style || "default";
        paint(styleRef.current);
        if (data.language) {
          setLanguage(data.language);
          // quick-paste 是独立窗口，i18n 初始化时用的是系统语言。
          // 不同步的话，卡片上的相对时间按设置走、其余文案按系统走，两边会打架。
          void i18n.changeLanguage(data.language);
        }
      })
      .catch(() => paint("default"));

    // 首帧也拉一次标签，别等面板第一次打开才有建议
    void loadTags();

    const unlistenPanel = listen("panel-opened", () => {
      if (closeTimerRef.current !== null) {
        clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
      closingRef.current = false;
      setSearch("");
      setFilter("all");
      // 状态是异步的，这里显式带上空查询/全筛选，别依赖 ref 已经更新
      void refresh({ reset: true, query: "", filter: "all" });
      void loadTags();
      setOrderFrozen(false);
      // 上次关窗时可能停在列表中间，重开必须回到最左边，
      // 否则第一张卡在视口外，回车粘的是看不见的内容。
      // 这里要瞬移，不能走补间（否则开面板时列表会自己滑一下）。
      if (scrollAnimRef.current !== null) {
        cancelAnimationFrame(scrollAnimRef.current);
        scrollAnimRef.current = null;
      }
      if (listRef.current) listRef.current.scrollLeft = 0;
      // 列表瞬间回到最左，指针下的卡片会换人，别让 hover 抢选中
      hoverLockUntil.current = performance.now() + 120;
      playEnter();
      inputRef.current?.focus();
      blurEnabled.current = false;
      setTimeout(() => {
        blurEnabled.current = true;
      }, 200);
    });

    const unlistenBlur = getCurrentWindow().listen("tauri://blur", () => {
      if (blurEnabled.current) {
        requestClose();
      }
    });

    const unlistenTheme = listen<{ theme: string; style: string }>(
      "theme-changed",
      (event) => {
        // 明暗不再由用户设置，只关心风格
        if (event.payload?.style) {
          styleRef.current = event.payload.style;
        }
        paint(styleRef.current);
      }
    );

    const unlistenChanged = listen("clipboard_changed", () => {
      // 不 reset：面板开着时新复制进来，选中不该被拽走
      void refresh();
    });

    const unlistenCleared = listen("clipboard-cleared", () => {
      setItems([]);
      setActiveId(null);
      imageCache.clear();
    });

    const handleKey = (e: KeyboardEvent) => {
      // 标签建议打开时，方向键/回车先交给建议列表，别去移动卡片
      if (showSuggestionsRef.current) {
        const count = suggestionsRef.current.length;
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          setTagIndex((i) =>
            e.key === "ArrowDown" ? (i + 1) % count : (i - 1 + count) % count
          );
          return;
        }
        if (e.key === "Enter" || e.key === "Tab") {
          e.preventDefault();
          const picked = suggestionsRef.current[tagIndexRef.current];
          if (picked) applyTagQueryRef.current(picked.tag);
          return;
        }
      }

      const list = filteredItemsRef.current;
      const current = Math.max(
        0,
        list.findIndex((i) => i.id === activeIdRef.current)
      );

      // 搜索框里有内容（或按着修饰键）时，方向键/Home/End 留给光标编辑，
      // 否则会把 ⌘← / ⌥← 这种移动光标、跳到行首的快捷键一起吞掉。
      const editingSearch =
        document.activeElement === inputRef.current &&
        (searchRef.current.length > 0 || e.metaKey || e.ctrlKey || e.altKey);
      if (
        editingSearch &&
        ["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)
      ) {
        return;
      }

      if (e.key === "ArrowRight") {
        e.preventDefault();
        selectIndex(current + 1);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        selectIndex(current - 1);
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        selectIndex(current + 4);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        selectIndex(current - 4);
      } else if (e.key === "Home") {
        e.preventDefault();
        selectIndex(0);
      } else if (e.key === "End") {
        e.preventDefault();
        selectIndex(list.length - 1);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const item = list[current];
        if (item) pasteRef.current(item);
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (searchRef.current.length > 0) {
          setSearch("");
        } else {
          requestClose();
        }
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        const item = list[current];
        if (item) togglePinRef.current(item);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "e") {
        e.preventDefault();
        const item = list[current];
        if (item) openEditorRef.current(item);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "t") {
        e.preventDefault();
        const item = list[current];
        if (item) openTagEditorRef.current(item);
      } else if (
        (e.metaKey || e.ctrlKey) &&
        (e.key === "Backspace" || e.key === "Delete")
      ) {
        e.preventDefault();
        const item = list[current];
        if (item) deleteItemRef.current(item);
      } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "x") {
        e.preventDefault();
        clearAllRef.current();
      } else if ((e.metaKey || e.ctrlKey) && ["1", "2", "3", "4"].includes(e.key)) {
        e.preventDefault();
        const map: Record<string, ClipboardFilter> = {
          "1": "all",
          "2": "text",
          "3": "image",
          "4": "link",
        };
        setFilter(map[e.key]);
      }
    };

    window.addEventListener("keydown", handleKey);
    return () => {
      window.removeEventListener("keydown", handleKey);
      unlistenPanel.then((fn) => fn());
      unlistenBlur.then((fn) => fn());
      unlistenTheme.then((fn) => fn());
      unlistenChanged.then((fn) => fn());
      unlistenCleared.then((fn) => fn());
    };
    // search 只用于 Esc 判断，不希望在打字时重建监听
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh, playEnter, requestClose, selectIndex]);

  // 选中项跟着滚动，别跑出可视区。hover 触发的选中不滚（scrollNext=false）。
  useEffect(() => {
    const el = listRef.current?.querySelector(
      `[data-id="${activeId}"]`
    ) as HTMLElement | null;
    const strip = listRef.current;
    if (el && strip && scrollNext.current) {
      const left = el.offsetLeft;
      const right = left + el.offsetWidth;
      const viewLeft = strip.scrollLeft;
      const viewRight = viewLeft + strip.clientWidth;

      let target: number | null = null;
      if (left < viewLeft) {
        target = left - 8;
      } else if (right > viewRight) {
        target = right - strip.clientWidth + 8;
      }

      if (target !== null) {
        // 别滚过内容末尾（末张卡时目标会大于可滚距离）
        const max = Math.max(0, strip.scrollWidth - strip.clientWidth);
        animateScrollLeft(strip, Math.max(0, Math.min(target, max)));
      }
    }
    scrollNext.current = true;
  }, [activeId]);

  // 卸载时别把 rAF 留在队列里
  useEffect(() => {
    return () => {
      if (scrollAnimRef.current !== null) {
        cancelAnimationFrame(scrollAnimRef.current);
        scrollAnimRef.current = null;
      }
    };
  }, []);

  // 搜索防抖、筛选立即刷新，结果都由后端出（首帧挂载时也会跑一次，即初始加载）
  useEffect(() => {
    setTagIndex(0);
  }, [search]);

  useEffect(() => {
    const timer = window.setTimeout(
      () => void refresh({ reset: true }),
      search ? SEARCH_DEBOUNCE_MS : 0
    );
    return () => window.clearTimeout(timer);
  }, [search, filter, refresh]);

  // 鼠标滚轮上下 = 横向滚动。React 的 onWheel 是 passive 的，拦不住页面滚动，
  // 所以这里用原生监听 + preventDefault。
  useEffect(() => {
    const strip = listRef.current;
    if (!strip) return;
    const onWheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      e.preventDefault();
      // 用户自己滚了，就别让键盘的补间继续抢滚动位置
      if (scrollAnimRef.current !== null) {
        cancelAnimationFrame(scrollAnimRef.current);
        scrollAnimRef.current = null;
      }
      strip.scrollLeft += e.deltaY;
    };
    strip.addEventListener("wheel", onWheel, { passive: false });
    return () => strip.removeEventListener("wheel", onWheel);
  }, []);

  const filters: { key: ClipboardFilter; label: string; count: number }[] = [
    { key: "all", label: t("filterAll"), count: counts.all },
    { key: "text", label: t("filterText"), count: counts.text },
    { key: "image", label: t("filterImage"), count: counts.image },
    { key: "link", label: t("filterLink"), count: counts.link },
  ];

  const renderPreview = (item: ClipboardItem) => {
    if (item.type === "image") {
      return (
        <div className="w-full h-full overflow-hidden rounded-lg bg-default">
          <ImageThumb filename={item.content} />
        </div>
      );
    }

    if (itemKind(item) === "link") {
      return (
        <div className="flex flex-col gap-1">
          <span className="text-[11px] font-medium text-sky-600 dark:text-sky-400 truncate">
            {linkDomain(item.content)}
          </span>
          <span className="text-[12px] leading-snug text-muted-foreground line-clamp-4 break-all">
            {item.content}
          </span>
        </div>
      );
    }

    return (
      <p className="text-[12.5px] leading-[1.45] text-card-foreground line-clamp-5 whitespace-pre-wrap break-words">
        {item.content}
      </p>
    );
  };

  // 标签编辑条要展示当前条目的已有标签
  const tagTarget = tagEditor ? items.find((i) => i.id === tagEditor.id) ?? null : null;

  return (
    <div className="h-screen w-screen p-2">
      <div
        ref={panelRef}
        className="pastoid-panel relative h-full flex flex-col overflow-hidden"
      >
        {/* 顶部：搜索 + 分类 */}
        <div className="shrink-0 flex items-center gap-2 px-3 pt-3 pb-2">
          <div className="flex-1 flex items-center gap-2 h-9 px-3 rounded-xl bg-default ring-1 ring-transparent focus-within:ring-ring/50 transition-shadow">
            <Search size={15} className="text-muted-foreground shrink-0" />
            <input
              ref={inputRef}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("search")}
              className="flex-1 min-w-0 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
              autoFocus
              spellCheck={false}
            />
            {search ? (
              <button
                onClick={() => setSearch("")}
                className="shrink-0 text-muted-foreground hover:text-foreground"
                aria-label={t("clearSearch")}
              >
                <X size={13} />
              </button>
            ) : null}
          </div>

          <div className="flex items-center gap-1 shrink-0">
            {filters.map((f) => {
              const isActive = filter === f.key;
              return (
                <button
                  key={f.key}
                  onClick={() => setFilter(f.key)}
                  className={`px-2.5 h-7 rounded-lg text-[11.5px] transition-colors ${
                    isActive
                      ? "bg-default text-foreground font-semibold"
                      : "text-muted-foreground hover:bg-default"
                  }`}
                >
                  {f.label}
                  <span className={`ml-1 tabular-nums ${isActive ? "opacity-60" : "opacity-45"}`}>
                    {f.count}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        {/* 标签建议：输入 # 时出现 */}
        {showSuggestions && (
          <div className="pastoid-menu absolute left-3 top-[50px] z-30 w-[340px] overflow-hidden rounded-xl p-1">
            {suggestions.map((s, index) => (
              <button
                key={s.tag}
                type="button"
                onMouseEnter={() => setTagIndex(index)}
                onClick={() => applyTagQuery(s.tag)}
                className={`flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] ${
                  index === tagIndex ? "bg-default text-foreground" : "text-muted-foreground"
                }`}
              >
                <span className="truncate">#{s.tag}</span>
                <span className="shrink-0 text-[10.5px] text-muted-foreground">{s.count}</span>
              </button>
            ))}
          </div>
        )}

        {/* 横向卡片条 */}
        <div
          ref={listRef}
          className="flex-1 min-h-0 overflow-x-auto overflow-y-hidden px-3 pb-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {filteredItems.length === 0 ? (
            loadError ? (
              <div className="h-full flex flex-col items-center justify-center gap-2 text-destructive/80">
                <AlertCircle size={24} strokeWidth={1.5} />
                <span className="max-w-[420px] px-4 text-center text-[12.5px]">
                  {t("loadFailed")}: {loadError}
                </span>
                <button
                  onClick={() => void refresh({ reset: true })}
                  className="rounded-md border border-current/30 px-2.5 py-1 text-[11.5px] hover:bg-destructive/10"
                >
                  {t("retry")}
                </button>
              </div>
            ) : (
              <div className="h-full flex flex-col items-center justify-center gap-2 text-muted-foreground">
                <Clipboard size={26} strokeWidth={1.5} />
                <span className="text-[12.5px]">
                  {search ? t("noMatches") : t("emptyClipboard")}
                </span>
              </div>
            )
          ) : (
            <div className="flex gap-2.5 h-full items-stretch py-0.5">
              {filteredItems.map((item) => {
                const isActive = item.id === activeId;
                const kind = itemKind(item);
                return (
                  <div
                    key={item.id}
                    data-id={item.id}
                    role="button"
                    tabIndex={-1}
                    onClick={() => paste(item)}
                    onMouseEnter={() => hoverItem(item.id)}
                    className={`pastoid-card group relative shrink-0 w-[190px] h-full flex flex-col text-left overflow-hidden cursor-pointer select-none transition-[background-color,border-color,box-shadow,transform] duration-150 active:scale-[0.985] ${
                      isActive
                        ? "ring-2 ring-primary/40 shadow-sm"
                        : item.isPinned
                          ? "hover:brightness-[0.98]"
                          : "hover:brightness-[0.98]"
                    } ${item.isPinned ? "pinned" : ""}`}
                  >
                    <div className="shrink-0 flex items-center gap-1.5 px-2.5 pt-2 pb-1">
                      <KindIcon kind={kind} />
                      <span className="text-[10.5px] text-muted-foreground truncate">
                        {formatRelativeTime(item.createdAt, language)}
                      </span>
                      {item.isPinned && (
                        // 悬浮按钮就覆盖在右上角，两个图层会叠在一起，
                        // 所以悬浮时把这枚图钉淡出，交给按钮接管这块空间
                        <Pin
                          size={10}
                          className="ml-auto text-warning shrink-0 transition-opacity group-hover:opacity-0"
                          fill="currentColor"
                        />
                      )}
                    </div>

                    <div className="flex-1 min-h-0 px-2.5 pb-2 overflow-hidden">
                      {renderPreview(item)}
                    </div>

                    {/* 用户打的标签：最多显示 3 个，其余折成 +N */}
                    {item.tags?.length ? (
                      <div className="shrink-0 flex items-center gap-1 px-2.5 pb-1.5 overflow-hidden">
                        {item.tags.slice(0, 3).map((tag) => (
                          <button
                            key={tag}
                            onClick={(e) => {
                              e.stopPropagation();
                              applyTagQuery(tag);
                            }}
                            className="text-[9px] leading-[14px] px-1 rounded bg-primary/10 text-primary truncate max-w-[80px] hover:bg-primary/20"
                            title={`#${tag}`}
                          >
                            #{tag}
                          </button>
                        ))}
                        {item.tags.length > 3 && (
                          <span className="text-[9px] text-muted-foreground shrink-0">
                            +{item.tags.length - 3}
                          </span>
                        )}
                      </div>
                    ) : null}

                    {/* 悬浮操作 */}
                    <div className="absolute right-1 top-1 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          openTagEditor(item);
                        }}
                        className={`w-6 h-6 flex items-center justify-center rounded-md bg-card/90 hover:bg-default ${
                          item.tags?.length ? "text-primary" : "text-muted-foreground hover:text-foreground"
                        }`}
                        aria-label={t("tags")}
                      >
                        <Tag size={11} />
                      </button>
                      {item.type !== "image" && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            openEditor(item);
                          }}
                          className="w-6 h-6 flex items-center justify-center rounded-md bg-card/90 hover:bg-default text-muted-foreground hover:text-foreground"
                          aria-label={t("edit")}
                        >
                          <Pencil size={11} />
                        </button>
                      )}
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          togglePin(item);
                        }}
                        className={`w-6 h-6 flex items-center justify-center rounded-md bg-card/90 hover:bg-default ${
                          item.isPinned ? "text-warning" : "text-muted-foreground hover:text-foreground"
                        }`}
                        aria-label={item.isPinned ? t("unpin") : t("pin")}
                      >
                        <Pin size={11} fill={item.isPinned ? "currentColor" : "none"} />
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          deleteItem(item);
                        }}
                        className="w-6 h-6 flex items-center justify-center rounded-md bg-card/90 hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                        aria-label={t("delete")}
                      >
                        <Trash2 size={11} />
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* 标签编辑条：贴着底部快捷键栏，不遮挡卡片 */}
        {tagEditor && tagTarget && (
          <div className="absolute inset-x-0 bottom-8 z-20 flex items-center gap-1.5 px-3 py-2 border-t border-border/60 bg-background">
            <Tag size={12} className="text-muted-foreground shrink-0" />
            {(tagTarget.tags ?? []).map((tag) => (
              <button
                key={tag}
                onClick={() => void removeTag(tagTarget, tag)}
                className="shrink-0 flex items-center gap-1 text-[10.5px] px-1.5 py-[2px] rounded bg-primary/10 text-primary"
                aria-label={t("delete")}
              >
                #{tag}
                <X size={9} />
              </button>
            ))}
            <input
              autoFocus
              value={tagEditor.value}
              onChange={(e) => setTagEditor({ ...tagEditor, value: e.target.value })}
              onKeyDown={(e) => {
                // 别让 Esc / 方向键漏到面板的全局快捷键
                e.stopPropagation();
                if (e.key === "Enter") {
                  e.preventDefault();
                  void addTag(tagTarget, tagEditor.value);
                  setTagEditor({ ...tagEditor, value: "" });
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  setTagEditor(null);
                } else if (
                  e.key === "Backspace" &&
                  !tagEditor.value &&
                  (tagTarget.tags ?? []).length > 0
                ) {
                  e.preventDefault();
                  const tags = tagTarget.tags ?? [];
                  void removeTag(tagTarget, tags[tags.length - 1]);
                }
              }}
              placeholder={t("addTag")}
              spellCheck={false}
              className="flex-1 min-w-0 bg-transparent text-[11.5px] text-foreground outline-none placeholder:text-muted-foreground"
            />
            <span className="shrink-0 text-[10px] text-muted-foreground">{t("tagHint")}</span>
          </div>
        )}

        {/* 正文编辑弹层：图片不支持改内容 */}
        {editor && (
          <div className="absolute inset-0 z-30 flex flex-col gap-2 p-3 bg-background">
            <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
              <Pencil size={12} />
              <span>{t("edit")}</span>
            </div>
            <textarea
              autoFocus
              value={editor.draft}
              onChange={(e) =>
                setEditor({ ...editor, draft: e.target.value, error: undefined })
              }
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Escape") {
                  e.preventDefault();
                  setEditor(null);
                } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                  e.preventDefault();
                  void saveEditor();
                }
              }}
              spellCheck={false}
              className="flex-1 min-h-0 w-full resize-none rounded-lg border border-border bg-card p-3 text-[12.5px] leading-relaxed text-card-foreground outline-none focus:ring-2 focus:ring-ring/40"
            />
            {editor.error && (
              <div className="text-[11px] text-destructive">{editor.error}</div>
            )}
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-muted-foreground">{t("editHint")}</span>
              <div className="ml-auto flex items-center gap-2">
                <button
                  onClick={() => setEditor(null)}
                  className="rounded-md border border-border px-2.5 py-1 text-[11.5px] text-muted-foreground hover:bg-default"
                >
                  {t("cancel")}
                </button>
                <button
                  onClick={() => void saveEditor()}
                  className="rounded-md bg-primary px-2.5 py-1 text-[11.5px] text-primary-foreground hover:opacity-90"
                >
                  {t("save")}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* 底部：快捷键提示（按平台显示 ⌘ 还是 Ctrl） */}
        <div className="shrink-0 flex items-center gap-3 px-3 h-8 border-t border-border/60 text-[10.5px] text-muted-foreground">
          <Hint keys={["←", "→"]} label={t("nav")} />
          <Hint keys={[enterLabel]} label={t("paste")} />
          <Hint keys={[modKey, "P"]} label={t("pin")} />
          <Hint keys={[modKey, "E"]} label={t("edit")} />
          <Hint keys={[modKey, "T"]} label={t("tags")} />
          <Hint keys={[modKey, backspaceLabel]} label={t("delete")} />
          <Hint keys={[modKey, shiftLabel, "X"]} label={t("clearAll")} />
          <span className="ml-auto flex items-center gap-2">
            {orderFrozen && (
              <span className="text-warning/90">{t("orderFrozen")}</span>
            )}
            <Hint keys={[escLabel]} label={t("close")} />
          </span>
        </div>
      </div>
    </div>
  );
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <span className="flex items-center gap-1 whitespace-nowrap">
      {keys.map((k) => (
        <kbd
          key={k}
          className="px-1 py-[1px] rounded border border-border bg-default text-[9.5px] leading-none text-muted-foreground"
        >
          {k}
        </kbd>
      ))}
      <span>{label}</span>
    </span>
  );
}
