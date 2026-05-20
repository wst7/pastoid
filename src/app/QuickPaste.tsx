import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { Search, Clipboard, Pin, Trash2, X } from "lucide-react";
import { Kbd, Button } from "@heroui/react";
import type { ClipboardItem } from "@/types/clipboard";

export default function QuickPaste() {
  const [search, setSearch] = useState("");
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const blurEnabled = useRef(false);

  const filteredItems = useMemo(
    () =>
      items
        .filter((item) => {
          if (!search.trim()) return true;
          return item.content.toLowerCase().includes(search.toLowerCase());
        })
        .sort((a, b) => Number(b.isPinned) - Number(a.isPinned))
        .slice(0, 30),
    [items, search]
  );

  const loadItems = useCallback(async () => {
    try {
      const data: ClipboardItem[] = await invoke("get_clipboard_items");
      setItems(data);
    } catch (e) {
      console.error("Failed to load items:", e);
    }
  }, []);

  const paste = useCallback(async (item: ClipboardItem) => {
    try {
      await invoke("paste_clipboard", { content: item.content });
    } catch (e) {
      console.error("Paste failed:", e);
    }
  }, []);

  const activeIndexRef = useRef(activeIndex);
  activeIndexRef.current = activeIndex;

  const filteredItemsRef = useRef(filteredItems);
  filteredItemsRef.current = filteredItems;

  const togglePin = useCallback(async (item: ClipboardItem) => {
    try {
      const updated = { ...item, isPinned: !item.isPinned, updatedAt: Date.now() };
      await invoke("update_clipboard_item", { item: updated });
      setItems((prev) => prev.map((i) => (i.id === item.id ? updated : i)));
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

  const togglePinRef = useRef(togglePin);
  togglePinRef.current = togglePin;

  const deleteItemRef = useRef(deleteItem);
  deleteItemRef.current = deleteItem;

  const pasteRef = useRef(paste);
  pasteRef.current = paste;

  useEffect(() => {
    loadItems();

    const unlisten = listen("panel-opened", () => {
      loadItems();
      setSearch("");
      setActiveIndex(0);
      inputRef.current?.focus();
      blurEnabled.current = false;
      setTimeout(() => { blurEnabled.current = true; }, 200);
    });

    const unlistenBlur = getCurrentWindow().listen("tauri://blur", () => {
      if (blurEnabled.current) {
        getCurrentWindow().hide();
      }
    });

    const handleKey = (e: KeyboardEvent) => {
      const items = filteredItemsRef.current;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(prev + 1, items.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const item = items[activeIndexRef.current];
        if (item) pasteRef.current(item);
      } else if (e.key === "Escape") {
        e.preventDefault();
        getCurrentWindow().hide();
      } else if ((e.metaKey || e.ctrlKey) && e.key === "p") {
        e.preventDefault();
        const item = items[activeIndexRef.current];
        if (item) togglePinRef.current(item);
      } else if ((e.metaKey || e.ctrlKey) && (e.key === "Backspace" || e.key === "Delete")) {
        e.preventDefault();
        const item = items[activeIndexRef.current];
        if (item) {
          deleteItemRef.current(item);
          setActiveIndex((prev) => Math.max(0, Math.min(prev, items.length - 2)));
        }
      }
    };

    window.addEventListener("keydown", handleKey);
    return () => {
      window.removeEventListener("keydown", handleKey);
      unlisten.then((fn) => fn());
      unlistenBlur.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const el = listRef.current?.querySelector(
      `[data-index="${activeIndex}"]`
    ) as HTMLElement;
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  useEffect(() => {
    setActiveIndex(0);
  }, [search]);

  const preview = (text: string, max = 80) =>
    text.length > max ? text.slice(0, max) + "…" : text;

  return (
    <>
      <div className="h-screen flex flex-col justify-center overflow-hidden rounded-2xl bg-white/85 dark:bg-zinc-900/85 border-2 border-zinc-300 dark:border-zinc-600">
      {/* header */}
      <div className="shrink-0 px-4 pt-3 pb-2">
        <div className="flex items-center gap-2.5 rounded-xl bg-zinc-100 dark:bg-zinc-800 px-3 py-2 ring-1 ring-zinc-200 dark:ring-zinc-700 focus-within:ring-2 focus-within:ring-blue-400/60 transition-shadow">
          <Search size={16} className="text-zinc-400 shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索剪贴板内容…"
            className="flex-1 bg-transparent text-sm text-zinc-800 dark:text-zinc-100 outline-none placeholder:text-zinc-400"
            autoFocus
            spellCheck={false}
          />
          {search && (
            <Button
              isIconOnly
              variant="ghost"
              size="sm"
              onPress={() => setSearch("")}
              className="text-zinc-400 shrink-0 min-w-6 w-6 h-6"
            >
              <X size={14} />
            </Button>
          )}
        </div>
      </div>

      {/* list */}
      <div ref={listRef} className="overflow-y-auto px-2 pb-1 max-h-[320px]">
        {filteredItems.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-zinc-400">
            <Clipboard size={28} strokeWidth={1.5} />
            <span className="text-sm">
              {search ? "没有匹配的内容" : "剪贴板为空"}
            </span>
          </div>
        ) : (
          <div className="space-y-0.5">
            {filteredItems.map((item, idx) => {
              const isActive = idx === activeIndex;
              return (
                <div
                  key={item.id}
                  data-index={idx}
                  onClick={() => paste(item)}
                  className={`group flex items-center gap-2.5 px-3 py-2 rounded-lg cursor-pointer select-none transition-all duration-300 ${
                    isActive
                      ? "bg-blue-50 dark:bg-blue-500/10 text-blue-700 dark:text-blue-300"
                      : "text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800/60"
                  }`}
                >
                  {/* pin / type icon */}
                  <span className="shrink-0 w-5 flex justify-center">
                    {item.isPinned ? (
                      <Pin
                        size={13}
                        className={`transition-colors duration-300 ${
                          isActive
                            ? "text-amber-500"
                            : "text-amber-400/70"
                        }`}
                        fill={isActive ? "currentColor" : "none"}
                      />
                    ) : (
                      <span
                        className={`text-[10px] font-medium transition-colors duration-300 ${
                          isActive
                            ? "text-blue-400"
                            : "text-zinc-300 dark:text-zinc-600"
                        }`}
                      >
                        T
                      </span>
                    )}
                  </span>

                  {/* content */}
                  <span className="flex-1 truncate text-sm leading-5">
                    {preview(item.content)}
                  </span>

                  {/* actions — appear on hover */}
                  <span className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity duration-300">
                    <div
                      onClick={(e) => { e.stopPropagation(); togglePin(item); }}
                      className={`min-w-6 w-6 h-6 flex items-center justify-center rounded-md hover:bg-zinc-200 dark:hover:bg-zinc-700 cursor-pointer transition-colors duration-300 ${item.isPinned ? "text-amber-400" : "text-zinc-400 opacity-40"}`}
                    >
                      <Pin size={11} fill={item.isPinned ? "currentColor" : "none"} />
                    </div>
                    <div
                      onClick={(e) => { e.stopPropagation(); deleteItem(item); }}
                      className="min-w-6 w-6 h-6 flex items-center justify-center rounded-md hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-400 hover:text-red-500 cursor-pointer transition-colors duration-300"
                    >
                      <Trash2 size={11} />
                    </div>
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* footer */}
      <div className="shrink-0 flex items-center justify-between px-4 py-2 text-[11px] text-zinc-400 border-t border-zinc-100 dark:border-zinc-800">
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1">
            <Kbd className="bg-zinc-200 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 text-[10px] px-1.5 py-0.5">↑↓</Kbd> 导航
          </span>
          <span className="flex items-center gap-1">
            <Kbd className="bg-zinc-200 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 text-[10px] px-1.5 py-0.5">↵</Kbd> 粘贴
          </span>
          <span className="flex items-center gap-1">
            <Kbd className="bg-zinc-200 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 text-[10px] px-1.5 py-0.5">⌘P</Kbd> 固定
          </span>
          <span className="flex items-center gap-1">
            <Kbd className="bg-zinc-200 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 text-[10px] px-1.5 py-0.5">⌘⌫</Kbd> 删除
          </span>
        </div>
        <span className="flex items-center gap-1">
          <Kbd className="bg-zinc-200 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 text-[10px] px-1.5 py-0.5">Esc</Kbd> 关闭
        </span>
      </div>
    </div>
    </>
  );
}
