import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption {
  id: string;
  name: string;
  /** 分组标题：同组选项共用一个标题，用于「明暗 / 风格」这类二维列表 */
  group?: string;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (id: string) => void;
  disabled?: boolean;
  /** 弹层最小宽度 */
  minWidth?: number;
  /** 触发按钮上显示的文字；不传则用当前选项名 */
  label?: string;
}

const GAP = 4;
const EDGE = 8;

/**
 * 原生 <select> 的弹层由系统绘制：macOS 还算协调，Windows 上就是 Win32 的灰白下拉，
 * 和界面完全不是一个风格，深色模式下尤其突兀。这里改成自绘弹层。
 * 用 portal 挂到 body 上，避免被卡片容器的 overflow-hidden 裁掉。
 *
 * 定位要点（之前踩过的坑）：
 * - 菜单高度要**实测**，不能按行数估：分组标题和换行都会让估算偏小，
 *   偏小就会顶出窗口底部被裁掉
 * - 下方放不下就翻到上方；上下都放不下时按可用空间给 maxHeight + 滚动
 * - 菜单自己滚动时不能触发"滚动即关闭"，否则一滚就没了
 */
export function Select({
  value,
  options,
  onChange,
  disabled,
  minWidth = 132,
  label,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState({ top: 0, left: 0, width: minWidth, maxHeight: 360 });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const activeRef = useRef(0);
  activeRef.current = active;
  const menuRef = useRef<HTMLDivElement>(null);

  const current = options.find((o) => o.id === value) ?? options[0];
  const triggerLabel = label ?? current?.name ?? "";

  const place = useCallback(() => {
    const el = triggerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const width = Math.max(rect.width, minWidth);
    const left = Math.min(
      Math.max(EDGE, rect.right - width),
      Math.max(EDGE, window.innerWidth - width - EDGE),
    );

    // 先量真实高度；菜单还没挂载时按行数估一个够用的值
    const measured = menuRef.current?.offsetHeight ?? 0;
    const desired = measured > 0 ? measured : options.length * 32 + 16;

    const spaceBelow = window.innerHeight - rect.bottom - GAP - EDGE;
    const spaceAbove = rect.top - GAP - EDGE;
    const openUp = spaceBelow < desired && spaceAbove > spaceBelow;

    // 不让菜单吃掉整个窗口：最多占 62%，剩下的滚动看
    const cap = Math.max(180, Math.round(window.innerHeight * 0.62));
    const maxHeight = Math.max(120, Math.min(cap, Math.floor(openUp ? spaceAbove : spaceBelow)));
    const height = Math.min(desired, maxHeight);
    const top = openUp
      ? Math.max(EDGE, rect.top - height - GAP)
      : Math.min(rect.bottom + GAP, window.innerHeight - height - EDGE);

    setPos({ top, left, width, maxHeight });
  }, [minWidth, options.length]);

  useLayoutEffect(() => {
    if (!open) return;
    // 第一次渲染时菜单可能还没量到高度，place 内部会用估算值兜底；
    // 下一帧再量一次，把位置按真实高度收紧
    place();
    const raf = requestAnimationFrame(place);
    return () => cancelAnimationFrame(raf);
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
    // 页面滚动时不关闭，只重新定位。
    // 之前的写法是"一滚动就关"——结果点击触发的聚焦滚动会把刚打开的下拉立刻关掉，
    // 表现就是"点了没反应 / 闪一下没了"。
    let raf = 0;
    const onScroll = (e: Event) => {
      if (menuRef.current?.contains(e.target as Node)) return;
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        place();
      });
    };
    const onResize = () => place();

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      if (raf) cancelAnimationFrame(raf);
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
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
        <span className="truncate">{triggerLabel}</span>
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
            style={{
              top: pos.top,
              left: pos.left,
              width: pos.width,
              maxHeight: pos.maxHeight,
            }}
            className="fixed z-[60] overflow-y-auto overscroll-contain rounded-xl border border-border bg-card p-1 shadow-xl ring-1 ring-black/5 dark:ring-white/10"
          >
            {options.map((option, index) => {
              const selected = option.id === value;
              const showGroup =
                Boolean(option.group) &&
                (index === 0 || options[index - 1]?.group !== option.group);
              return (
                <Fragment key={option.id}>
                  {showGroup && (
                    <div className="px-2 pb-0.5 pt-1 text-[9.5px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/70">
                      {option.group}
                    </div>
                  )}
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    onMouseEnter={() => setActive(index)}
                    onClick={() => {
                      onChange(option.id);
                      setOpen(false);
                      triggerRef.current?.focus();
                    }}
                    className={`flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1 text-left text-[12.5px] transition-colors ${
                      index === active
                        ? "bg-default text-foreground"
                        : "text-muted-foreground"
                    }`}
                  >
                    <span className="truncate">{option.name}</span>
                    {selected && <Check size={13} className="shrink-0 text-primary" />}
                  </button>
                </Fragment>
              );
            })}
          </div>,
          document.body,
        )}
    </>
  );
}
