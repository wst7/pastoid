import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { applyTheme, paint, watchSystemTheme, type ThemePref } from "@/lib/theme";
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
  AlertTriangle,
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
      className="w-full h-full object-cover rounded-[var(--card-radius)]"
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
  const [needAccessibility, setNeedAccessibility] = useState(false);
  const [editor, setEditor] = useState<{ id: string; draft: string; error?: string } | null>(
    null
  );
  const [language, setLanguage] = useState("zh");
  // 双模式风格跟随系统明暗，系统切暗时要重画
  const styleRef = useRef<string>("default");
  const themeRef = useRef<ThemePref>("system");

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
  // 指数逼近的当前目标与上一帧时间
  const scrollTargetRef = useRef(0);
  const scrollLastFrameRef = useRef(0);
  // 显式 hover 状态：键盘操作后要关掉 hover 反馈，
  // 否则鼠标停留的那张卡会一直保留悬浮样式（按钮、变暗），看起来像"还选中着"。
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  // 操作按钮的说明气泡：卡片有 overflow-hidden，不能靠 CSS 挂在按钮上
  const [tip, setTip] = useState<{ text: string; x: number; y: number } | null>(null);
  const hoverEnabledRef = useRef(true);
  // 键盘操作时记下指针位置：之后鼠标要有 4px 以上位移才算"用户真的在动鼠标"，
  // 避免手放在触控板上轻轻一碰就把选中抢回去
  const pointerRef = useRef({ x: 0, y: 0 });
  const pointerGuardRef = useRef<{ x: number; y: number } | null>(null);

  /**
   * 横向滚动的补间：键盘换卡时让卡片条滑过去，而不是瞬移。
   *
   * 不用 `scrollIntoView({behavior:"smooth"})`：WKWebView / WebView2 的
   * 原生平滑滚动时长不可控（往往 300ms+），连续按键会拖沓；
   * 这里自己补间，时长按距离给，封顶 220ms。
   */
  function animateScrollLeft(strip: HTMLDivElement, target: number) {
    if (prefersReducedMotion()) {
      strip.scrollLeft = target;
      return;
    }

    // 指数逼近而不是「起点→终点」的固定补间：
    // 连续按键时只更新目标，正在跑的动画会顺着新目标继续走，不会每按一次就顿一下。
    scrollTargetRef.current = target;
    if (scrollAnimRef.current !== null) return;

    scrollLastFrameRef.current = performance.now();
    const step = (now: number) => {
      const dt = Math.min(32, now - scrollLastFrameRef.current);
      scrollLastFrameRef.current = now;

      const current = strip.scrollLeft;
      const goal = scrollTargetRef.current;
      const diff = goal - current;

      if (Math.abs(diff) < 1.2) {
        strip.scrollLeft = goal;
        scrollAnimRef.current = null;
        return;
      }

      // 时间常数 60ms：约 3 帧走完 90%，收尾再快一点，不留长尾
      const k = 1 - Math.exp(-dt / 60);
      strip.scrollLeft = current + diff * k;
      scrollAnimRef.current = requestAnimationFrame(step);
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
      // 没有辅助功能权限：内容已经写进剪贴板，别静默失败，提示用户去授权
      if (String(e).includes("NEED_ACCESSIBILITY")) {
        setNeedAccessibility(true);
        return;
      }
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

  /** 按方向键移动选中：先算下标，再落到 id */
  const selectIndex = useCallback((index: number, scroll = true) => {
    const list = filteredItemsRef.current;
    if (!list.length) return;
    const clamped = Math.max(0, Math.min(index, list.length - 1));
    scrollNext.current = scroll;
    // 键盘接管：先把 hover 反馈收掉，等鼠标真的动了再恢复
    hoverEnabledRef.current = false;
    pointerGuardRef.current = { ...pointerRef.current };
    setHoveredId(null);
    setActiveId(list[clamped].id);
  }, []);

  const showTip = useCallback((e: React.MouseEvent<HTMLElement>, text: string) => {
    const r = e.currentTarget.getBoundingClientRect();
    // 面板有 backdrop-filter，会为 fixed 子元素创建 containing block，
    // 所以坐标要换算成「相对面板」而不是相对视口
    const panel = panelRef.current?.getBoundingClientRect();
    const originX = panel?.left ?? 0;
    const originY = panel?.top ?? 0;
    const width = panel?.width ?? window.innerWidth;
    const x = Math.min(Math.max(r.left + r.width / 2 - originX, 56), width - 56);
    setTip({ text, x, y: r.top - originY - 6 });
  }, []);
  const hideTip = useCallback(() => setTip(null), []);

  const hoverItem = useCallback((id: string) => {
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
      paint(styleRef.current, themeRef.current);
    });
  }, []);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((data) => {
        styleRef.current = data.theme_style || "default";
        themeRef.current = (data.theme as ThemePref) || "system";
        // 面板也是独立窗口：除了换 class，还要把原生窗口外观一起设上，
        // 否则用户选「深色」而系统是浅色时，原生滚动条/输入控件仍是浅色
        applyTheme(styleRef.current, themeRef.current);
        if (data.language) {
          setLanguage(data.language);
          // quick-paste 是独立窗口，i18n 初始化时用的是系统语言。
          // 不同步的话，卡片上的相对时间按设置走、其余文案按系统走，两边会打架。
          void i18n.changeLanguage(data.language);
        }
      })
      .catch(() => applyTheme("default", "system"));

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
      scrollTargetRef.current = 0;
      if (listRef.current) listRef.current.scrollLeft = 0;
      // 列表瞬间回到最左，指针下的卡片会换人，别让 hover 抢选中
      hoverEnabledRef.current = false;
      setHoveredId(null);
      playEnter();
      void invoke<boolean>("check_accessibility")
        .then((ok) => setNeedAccessibility(!ok))
        .catch(() => {});
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
        if (event.payload?.style) {
          styleRef.current = event.payload.style;
        }
        if (event.payload?.theme) {
          themeRef.current = event.payload.theme as ThemePref;
        }
        // 同步原生窗口外观，避免只有 CSS 变了、系统控件还是旧明暗
        applyTheme(styleRef.current, themeRef.current);
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

      // 右侧留 28px 的 peek：滑到最右时还能看到下一张的边，
      // 既提示"后面还有"，也让每按一次都有可见的滑动
      const PEEK = 28;
      let target: number | null = null;
      if (left < viewLeft) {
        target = left - 8;
      } else if (right > viewRight - PEEK) {
        target = right - strip.clientWidth + PEEK;
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
      // 用户自己滚了，补间目标跟着当前值，别在松手后弹回去
      scrollTargetRef.current = strip.scrollLeft;
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
        <div className="w-full h-full overflow-hidden rounded-[var(--card-radius)] bg-default">
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
    <div className="h-screen w-screen">
      <div
        ref={panelRef}
        className="pastoid-panel relative h-full flex flex-col overflow-hidden"
      >
        {/* 顶部：搜索 + 分类 */}
        <div className="shrink-0 flex items-center gap-2 px-3 pt-3 pb-2">
          <div className="flex-1 flex items-center gap-2 h-9 px-3 rounded-[var(--card-radius)] bg-default border border-border focus-within:ring-2 focus-within:ring-ring/50 transition-shadow">
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
                  className={`px-2.5 h-7 rounded-[var(--control-radius)] text-[11.5px] transition-colors ${
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
          <div className="pastoid-menu absolute left-3 top-[50px] z-30 w-[340px] overflow-hidden rounded-[var(--card-radius)] p-1">
            {suggestions.map((s, index) => (
              <button
                key={s.tag}
                type="button"
                onMouseEnter={() => setTagIndex(index)}
                onClick={() => applyTagQuery(s.tag)}
                className={`flex w-full items-center justify-between gap-2 rounded-[var(--control-radius)] px-2 py-1.5 text-left text-[12px] ${
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
          onMouseMove={(e) => {
            pointerRef.current = { x: e.clientX, y: e.clientY };
            if (hoverEnabledRef.current) return;
            const guard = pointerGuardRef.current;
            if (guard) {
              const moved =
                Math.abs(e.clientX - guard.x) + Math.abs(e.clientY - guard.y);
              if (moved < 4) return;
              pointerGuardRef.current = null;
            }
            hoverEnabledRef.current = true;
            const card = (e.target as HTMLElement).closest("[data-id]") as HTMLElement | null;
            const id = card?.dataset.id;
            if (id) {
              setHoveredId(id);
              hoverItem(id);
            }
          }}
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
                  className="rounded-[var(--control-radius)] border border-current/30 px-2.5 py-1 text-[11.5px] hover:bg-destructive/10"
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
                // 键盘选中的卡片同样把操作按钮亮出来，不用先动鼠标
                const showActions = isActive || hoveredId === item.id;
                const canEdit = item.type !== "image";
                const actionBtn = (visible: boolean, extra = "") =>
                  `w-5 h-5 flex items-center justify-center rounded-[var(--control-radius)] transition-opacity ${
                    visible ? "opacity-100" : "opacity-0 pointer-events-none"
                  } ${extra}`;
                return (
                  <div
                    key={item.id}
                    data-id={item.id}
                    role="button"
                    tabIndex={-1}
                    onClick={() => paste(item)}
                    data-hovered={hoveredId === item.id ? "true" : undefined}
                    onMouseEnter={() => {
                      if (!hoverEnabledRef.current) return;
                      setHoveredId(item.id);
                      hoverItem(item.id);
                    }}
                    onMouseLeave={() =>
                      setHoveredId((prev) => (prev === item.id ? null : prev))
                    }
                    className={`pastoid-card group relative shrink-0 w-[190px] h-full flex flex-col text-left overflow-hidden cursor-pointer select-none transition-[background-color,border-color,box-shadow,transform] duration-150 active:scale-[0.985] ${
                      isActive ? "ring-2 ring-primary shadow-sm" : ""
                    } ${item.isPinned ? "pinned" : ""}`}
                  >
                    <div
                      className={`shrink-0 flex items-center gap-1.5 pl-2.5 pt-2 pb-1 ${
                        showActions || item.isPinned
                          ? canEdit
                            ? "pr-[96px]"
                            : "pr-[76px]"
                          : "pr-2.5"
                      }`}
                    >
                      <KindIcon kind={kind} />
                      <span className="text-[10.5px] text-muted-foreground truncate">
                        {formatRelativeTime(item.createdAt, language)}
                      </span>

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
                            className="text-[9px] leading-[14px] px-1 rounded-[var(--chip-radius)] bg-primary/10 text-primary truncate max-w-[80px] hover:bg-primary/20"
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

                    {/* 操作按钮：悬浮或键盘选中时出现。
                        四个槽位始终占位（隐藏用 opacity），图钉按钮的位置在
                        「仅显示固定状态」和「四个按钮全显示」时完全一致，不会点错。 */}
                    <div className="absolute right-1.5 top-1.5 flex items-center gap-0.5">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          openTagEditor(item);
                        }}
                        onMouseEnter={(e) => showTip(e, `${t("tags")} (${modKey}T)`)}
                        onMouseLeave={hideTip}
                        className={actionBtn(
                          showActions,
                          `bg-card/90 hover:bg-default ${
                            item.tags?.length
                              ? "text-primary"
                              : "text-muted-foreground hover:text-foreground"
                          }`,
                        )}
                        aria-label={t("tags")}
                      >
                        <Tag size={10} />
                      </button>
                      {canEdit && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            openEditor(item);
                          }}
                          onMouseEnter={(e) => showTip(e, `${t("edit")} (${modKey}E)`)}
                          onMouseLeave={hideTip}
                          className={actionBtn(
                            showActions,
                            "bg-card/90 hover:bg-default text-muted-foreground hover:text-foreground",
                          )}
                          aria-label={t("edit")}
                        >
                          <Pencil size={10} />
                        </button>
                      )}
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          deleteItem(item);
                        }}
                        onMouseEnter={(e) =>
                          showTip(e, `${t("delete")} (${modKey}${backspaceLabel})`)
                        }
                        onMouseLeave={hideTip}
                        className={actionBtn(
                          showActions,
                          "bg-card/90 hover:bg-destructive/10 text-muted-foreground hover:text-destructive",
                        )}
                        aria-label={t("delete")}
                      >
                        <Trash2 size={10} />
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          togglePin(item);
                        }}
                        onMouseEnter={(e) =>
                          showTip(e, `${item.isPinned ? t("unpin") : t("pin")} (${modKey}P)`)
                        }
                        onMouseLeave={hideTip}
                        className={actionBtn(
                          showActions || item.isPinned,
                          `${showActions ? "bg-card/90 hover:bg-default" : "bg-transparent"} ${
                            item.isPinned
                              ? "text-warning"
                              : "text-muted-foreground hover:text-foreground"
                          }`,
                        )}
                        aria-label={item.isPinned ? t("unpin") : t("pin")}
                      >
                        <Pin size={10} fill={item.isPinned ? "currentColor" : "none"} />
                      </button>
                    </div>
                  </div>
                );
              })}
              {/* 右边距垫片：滚动容器的 padding-right 不计入 scrollWidth，
                  没有它滑到最右时最后一张会贴边。负 margin 抵消 flex gap。 */}
              <div aria-hidden="true" className="shrink-0 w-3 -ml-2.5" />
            </div>
          )}
        </div>

        {tip && (
          <div
            className="pointer-events-none fixed z-50 whitespace-nowrap rounded-[var(--control-radius)] border border-border bg-background px-1.5 py-0.5 text-[10px] text-foreground shadow-lg"
            style={{ left: tip.x, top: tip.y, transform: "translate(-50%, -100%)" }}
            role="tooltip"
          >
            {tip.text}
          </div>
        )}

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
              className="flex-1 min-h-0 w-full resize-none rounded-[var(--card-radius)] border border-border bg-card p-3 text-[12.5px] leading-relaxed text-card-foreground outline-none focus:ring-2 focus:ring-ring/40"
            />
            {editor.error && (
              <div className="text-[11px] text-destructive">{editor.error}</div>
            )}
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-muted-foreground">{t("editHint")}</span>
              <div className="ml-auto flex items-center gap-2">
                <button
                  onClick={() => setEditor(null)}
                  className="rounded-[var(--control-radius)] border border-border px-2.5 py-1 text-[11.5px] text-muted-foreground hover:bg-default"
                >
                  {t("cancel")}
                </button>
                <button
                  onClick={() => void saveEditor()}
                  className="rounded-[var(--control-radius)] bg-primary px-2.5 py-1 text-[11.5px] text-primary-foreground hover:opacity-90"
                >
                  {t("save")}
                </button>
              </div>
            </div>
          </div>
        )}

        {needAccessibility && (
          <div className="shrink-0 mx-3 mb-1.5 flex items-center gap-2 rounded-[var(--control-radius)] border border-warning/40 bg-warning/10 px-2.5 py-1.5 text-[11px] text-warning">
            <AlertTriangle size={12} className="shrink-0" />
            <span className="flex-1 leading-snug">{t("needAccessibility")}</span>
            <button
              onClick={() => void invoke("open_accessibility_settings")}
              className="shrink-0 rounded-[var(--control-radius)] bg-warning/20 px-2 py-0.5 text-[10.5px] hover:bg-warning/30"
            >
              {t("openSettings")}
            </button>
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
