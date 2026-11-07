import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Pin, Copy, Trash2 } from "lucide-react";
import type { ClipboardItem } from "@/types/clipboard";

const ICON_SIZE = 14;

interface ClipboardItemCardProps {
  item: ClipboardItem;
  onCopy: (content: string) => Promise<boolean | void> | boolean | void;
  onDelete: (id: string) => void;
  onTogglePin: (id: string) => void;
}

export function ClipboardItemCard({
  item,
  onCopy,
  onDelete,
  onTogglePin,
}: ClipboardItemCardProps) {
  const { t } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    const result = await onCopy(item.content);
    if (result !== false) {
      setShowCopied(true);
      setTimeout(() => setShowCopied(false), 1500);
    }
  }, [item.content, onCopy]);

  const handleDelete = useCallback(() => {
    onDelete(item.id);
  }, [item.id, onDelete]);

  const handleTogglePin = useCallback(() => {
    onTogglePin(item.id);
  }, [item.id, onTogglePin]);

  return (
    <div
      className={`group flex items-center gap-2 p-2 rounded-xl bg-card border hover:shadow-md transition-all cursor-pointer ${item.isPinned ? "border-warning/50 bg-warning/10" : "border-border hover:border-ring"}`}
      onClick={handleCopy}
      role="button"
      aria-label={t('copy')}
    >
      {item.isPinned && (
        <Pin size={12} className="text-warning flex-shrink-0" />
      )}
      <div className="flex-1 min-w-0">
        
        <div className="text-sm leading-tight truncate overflow-hidden whitespace-nowrap" title={item.content}>
          {item.content}
        </div>
      </div>

      <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity" onClick={(e) => e.stopPropagation()}>
        {showCopied ? (
          <span className="text-xs text-success font-medium">{t('copied')}</span>
        ) : (
          <button onClick={handleCopy} className="p-1.5 rounded-lg hover:bg-muted" aria-label={t('copy')}>
            <Copy size={ICON_SIZE} className="text-muted-foreground" />
          </button>
        )}
        <button onClick={handleTogglePin} className="p-1.5 rounded-lg hover:bg-muted" aria-label={t('pin')}>
          <Pin size={ICON_SIZE} className={item.isPinned ? "text-warning" : "text-muted-foreground"} />
        </button>
        <button onClick={handleDelete} className="p-1.5 rounded-lg hover:bg-destructive/10 text-muted-foreground hover:text-destructive" aria-label={t('delete')}>
          <Trash2 size={ICON_SIZE} />
        </button>
      </div>
    </div>
  );
}