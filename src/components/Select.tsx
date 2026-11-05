import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption {
  id: string;
  name: string;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (id: string) => void;
  disabled?: boolean;
  /** 弹层最小宽度 */
  minWidth?: number;
}

/**
 * 原生 <select> 的弹层由系统绘制：macOS 还算协调，Windows 上就是 Win32 的灰白下拉，
 * 和界面完全不是一个风格，深色模式下尤其突兀。这里改成自绘弹层。
 * 用 portal 挂到 body 上，避免被卡片容器的 overflow-hidden 裁掉。
 */
export function Select({ value, options, onChange, disabled, minWidth = 132 }: SelectProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState({ top: 0, left: 0, width: minWidth });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const activeRef = useRef(0);
  activeRef.current = active;
  const menuRef = useRef<HTMLDivElement>(null);

  const current = options.find((o) => o.id === value) ?? options[0];

  const place = useCallback(() => {
    const el = triggerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const width = Math.max(rect.width, minWidth);
    const left = Math.min(
      Math.max(8, rect.right - width),
      window.innerWidth - width - 8,
    );
    const below = rect.bottom + 4;
    const estimated = options.length * 30 + 12;
    const top = below + estimated > window.innerHeight - 8 ? rect.top - estimated - 4 : below;
    setPos({ top, left, width });
  }, [minWidth, options.length]);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place]);

  // 打开时把高亮定位到当前选中项。这个 effect 绝不能依赖 active：
  // 否则每次 hover 改 active 都会重新跑一遍，把高亮又打回选中项。
  useEffect(() => {
    if (!open) return;
    const index = options.findIndex((o) => o.id === value);
    setActive(index < 0 ? 0 : index);
  }, [open, options, value]);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (triggerRef.current?.contains(t) || menuRef.current?.contains(t)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((i) => (i + 1) % options.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (i - 1 + options.length) % options.length);
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        const picked = options[activeRef.current];
        if (picked) {
          onChange(picked.id);
          setOpen(false);
          triggerRef.current?.focus();
        }
      }
    };
    const onScroll = () => setOpen(false);

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onScroll);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onScroll);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open, options, onChange]);

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[13px] text-muted-foreground transition-colors hover:bg-default disabled:opacity-50"
      >
        <span className="truncate">{current?.name}</span>
        <ChevronDown
          size={13}
          className={`shrink-0 transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>

      {open &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            style={{ top: pos.top, left: pos.left, width: pos.width }}
            className="fixed z-[60] rounded-xl border border-zinc-200/80 bg-card p-1 shadow-xl ring-1 ring-black/5 dark:border-white/10 dark:ring-white/10"
          >
            {options.map((option, index) => {
              const selected = option.id === value;
              return (
                <button
                  key={option.id}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => {
                    onChange(option.id);
                    setOpen(false);
                    triggerRef.current?.focus();
                  }}
                  className={`flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left text-[13px] transition-colors ${
                    index === active
                      ? "bg-default text-foreground"
                      : "text-muted-foreground"
                  }`}
                >
                  <span className="truncate">{option.name}</span>
                  {selected && <Check size={13} className="shrink-0 text-blue-500" />}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </>
  );
}
