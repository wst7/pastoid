import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { paint, watchSystemTheme, type ThemePref } from "@/lib/theme";
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
} from "lucide-react";
import { useTranslation } from "react-i18next";
import i18n from "@/i18n";
import { isMac, modKeyLabel } from "@/lib/platform";
import type { ClipboardItem, ClipboardFilter } from "@/types/clipboard";
import {
  formatRelativeTime,
  itemKind,
  linkDomain,
  sortItems,
} from "@/lib/clipboard";

interface SettingsData {
  theme: string;
  language: string;
}

/** 平台相关的按键显示：macOS 用符号，Windows 用文字 */
const modKey = modKeyLabel;
const shiftLabel = isMac ? "⇧" : "Shift";
const backspaceLabel = isMac ? "⌫" : "Backspace";
const enterLabel = isMac ? "↵" : "Enter";
const escLabel = isMac ? "esc" : "Esc";

/** 图片内容只拉一次，后续复用 data URL */
const imageCache = new Map<string, string>();

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
  const [activeIndex, setActiveIndex] = useState(0);
  const [language, setLanguage] = useState("zh");
  // 跟随系统时，系统夜间切暗也要跟着变
  const themeRef = useRef<ThemePref>("system");

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const blurEnabled = useRef(false);

  // 键盘监听只注册一次，闭包里拿不到最新的 search，用 ref 兜住。
  // 之前 Esc 用闭包里的 search，永远是 ""，所以「先清搜索再关窗」那段是死代码。
  const searchRef = useRef(search);
  searchRef.current = search;

  const loadItems = useCallback(async () => {
    try {
      const data: ClipboardItem[] = await invoke("get_clipboard_items");
      setItems(sortItems(data));
      setLoadError(null);
    } catch (e) {
      console.error("Failed to load items:", e);
      // 出错和「剪贴板是空的」长得一样，必须区分，否则用户以为记录丢了
      setLoadError(String(e));
    }
  }, []);

  const searchLower = search.trim().toLowerCase();

  const filteredItems = useMemo(() => {
    const bySearch = searchLower
      ? items.filter((item) => item.content.toLowerCase().includes(searchLower))
      : items;

    const byKind = bySearch.filter((item) => {
      switch (filter) {
        case "image":
          return item.type === "image";
        case "link":
          return itemKind(item) === "link";
        case "text":
          return itemKind(item) === "text";
        default:
          return true;
      }
    });

    return byKind.slice(0, 40);
  }, [items, searchLower, filter]);

  const counts = useMemo(
    () => ({
      all: items.length,
      text: items.filter((i) => itemKind(i) === "text").length,
      image: items.filter((i) => i.type === "image").length,
      link: items.filter((i) => itemKind(i) === "link").length,
    }),
    [items]
  );

  const paste = useCallback(async (item: ClipboardItem) => {
    try {
      // 按 id 粘贴：图片条目要走图片通道，不能把文件路径当文本写进剪贴板
      await invoke("paste_item", { id: item.id });
    } catch (e) {
      console.error("Paste failed:", e);
    }
  }, []);

  const togglePin = useCallback(async (item: ClipboardItem) => {
    try {
      const updated = { ...item, isPinned: !item.isPinned, updatedAt: Date.now() };
      await invoke("update_clipboard_item", { item: updated });
      setItems((prev) =>
        sortItems(prev.map((i) => (i.id === item.id ? updated : i)))
      );
    } catch (e) {
      console.error("Toggle pin failed:", e);
    }
  }, []);

  const deleteItem = useCallback(async (item: ClipboardItem) => {
    try {
      await invoke("delete_clipboard_item", { id: item.id });
      setItems((prev) => prev.filter((i) => i.id !== item.id));
    } catch (e) {
      console.error("Delete failed:", e);
    }
  }, []);

  const clearAll = useCallback(async () => {
    try {
      await invoke("clear_clipboard_items");
      setItems([]);
      imageCache.clear();
    } catch (e) {
      console.error("Clear all failed:", e);
    }
  }, []);

  // 事件回调里要用到最新值，用 ref 兜住，避免每次重建监听
  const activeIndexRef = useRef(activeIndex);
  activeIndexRef.current = activeIndex;

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

  useEffect(() => {
    return watchSystemTheme(() => {
      if (themeRef.current === "system") paint("system");
    });
  }, []);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((data) => {
        themeRef.current = (data.theme as ThemePref) || "system";
        paint(themeRef.current);
        if (data.language) {
          setLanguage(data.language);
          // quick-paste 是独立窗口，i18n 初始化时用的是系统语言。
          // 不同步的话，卡片上的相对时间按设置走、其余文案按系统走，两边会打架。
          void i18n.changeLanguage(data.language);
        }
      })
      .catch(() => paint("light"));

    loadItems();

    const unlistenPanel = listen("panel-opened", () => {
      loadItems();
      setSearch("");
      setFilter("all");
      setActiveIndex(0);
      inputRef.current?.focus();
      blurEnabled.current = false;
      setTimeout(() => {
        blurEnabled.current = true;
      }, 200);
    });

    const unlistenBlur = getCurrentWindow().listen("tauri://blur", () => {
      if (blurEnabled.current) {
        getCurrentWindow().hide();
      }
    });

    const unlistenTheme = listen<string>("theme-changed", (event) => {
      themeRef.current = (event.payload as ThemePref) || "system";
      paint(themeRef.current);
    });

    const unlistenChanged = listen("clipboard_changed", () => {
      loadItems();
    });

    const unlistenCleared = listen("clipboard-cleared", () => {
      setItems([]);
      imageCache.clear();
    });

    const handleKey = (e: KeyboardEvent) => {
      const list = filteredItemsRef.current;

      // 搜索框里有内容（或按着修饰键）时，左右键留给光标编辑，
      // 否则会把 ⌘← / ⌥← 这种移动光标、跳到行首的快捷键一起吞掉。
      const editingSearch =
        document.activeElement === inputRef.current &&
        (searchRef.current.length > 0 || e.metaKey || e.ctrlKey || e.altKey);
      if (editingSearch && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        return;
      }

      const lastIndex = Math.max(list.length - 1, 0);

      if (e.key === "ArrowRight") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(Math.max(prev, 0) + 1, lastIndex));
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(prev + 4, lastIndex));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 4, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const item = list[activeIndexRef.current];
        if (item) pasteRef.current(item);
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (searchRef.current.length > 0) {
          setSearch("");
          setActiveIndex(0);
        } else {
          getCurrentWindow().hide();
        }
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        const item = list[activeIndexRef.current];
        if (item) togglePinRef.current(item);
      } else if (
        (e.metaKey || e.ctrlKey) &&
        (e.key === "Backspace" || e.key === "Delete")
      ) {
        e.preventDefault();
        const item = list[activeIndexRef.current];
        if (item) {
          deleteItemRef.current(item);
          setActiveIndex((prev) => Math.max(0, Math.min(prev, list.length - 2)));
        }
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
        setActiveIndex(0);
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
  }, [loadItems]);

  // 选中项跟着滚动，别跑出可视区
  useEffect(() => {
    const el = listRef.current?.querySelector(
      `[data-index="${activeIndex}"]`
    ) as HTMLElement | null;
    el?.scrollIntoView({ inline: "nearest", block: "nearest", behavior: "smooth" });
  }, [activeIndex]);

  useEffect(() => {
    setActiveIndex(0);
  }, [search, filter]);

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

  return (
    <div className="h-screen w-screen p-2">
      <div className="h-full flex flex-col overflow-hidden rounded-2xl border border-border/80 bg-background/90 backdrop-blur-2xl shadow-xl shadow-black/10">
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
                  onClick={() => {
                    setFilter(f.key);
                    setActiveIndex(0);
                  }}
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
                  onClick={() => void loadItems()}
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
              {filteredItems.map((item, idx) => {
                const isActive = idx === activeIndex;
                const kind = itemKind(item);
                return (
                  <div
                    key={item.id}
                    data-index={idx}
                    role="button"
                    tabIndex={-1}
                    onClick={() => paste(item)}
                    onMouseEnter={() => setActiveIndex(idx)}
                    className={`group relative shrink-0 w-[190px] h-full flex flex-col text-left rounded-xl border overflow-hidden cursor-pointer select-none transition-all duration-150 ${
                      isActive
                        ? item.isPinned
                          ? "border-warning/60 ring-2 ring-primary/30 bg-warning/[0.10] shadow-sm"
                          : "border-primary/80 ring-2 ring-primary/30 bg-card shadow-sm"
                        : item.isPinned
                          ? "border-warning/30 bg-warning/[0.06] hover:border-warning/50"
                          : "border-border bg-card/70 hover:border-border/80"
                    }`}
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

                    {/* 悬浮操作 */}
                    <div className="absolute right-1 top-1 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          togglePin(item);
                        }}
                        className={`w-5 h-5 flex items-center justify-center rounded-md bg-card/90 hover:bg-default ${
                          item.isPinned ? "text-warning" : "text-muted-foreground hover:text-foreground"
                        }`}
                        aria-label={item.isPinned ? t("unpin") : t("pin")}
                      >
                        <Pin size={10} fill={item.isPinned ? "currentColor" : "none"} />
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          deleteItem(item);
                          // 鼠标删除也要收敛选中下标，否则删掉最后一张后 Enter 会静默失效
                          setActiveIndex((prev) =>
                            Math.max(0, Math.min(prev, filteredItems.length - 2))
                          );
                        }}
                        className="w-5 h-5 flex items-center justify-center rounded-md bg-card/90 hover:bg-destructive/10 text-muted-foreground hover:text-destructive"
                        aria-label={t("delete")}
                      >
                        <Trash2 size={10} />
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* 底部：快捷键提示（按平台显示 ⌘ 还是 Ctrl） */}
        <div className="shrink-0 flex items-center gap-3 px-3 h-8 border-t border-border/60 text-[10.5px] text-muted-foreground">
          <Hint keys={["←", "→"]} label={t("nav")} />
          <Hint keys={[enterLabel]} label={t("paste")} />
          <Hint keys={[modKey, "P"]} label={t("pin")} />
          <Hint keys={[modKey, backspaceLabel]} label={t("delete")} />
          <Hint keys={[modKey, shiftLabel, "X"]} label={t("clearAll")} />
          <span className="ml-auto flex items-center gap-1">
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
