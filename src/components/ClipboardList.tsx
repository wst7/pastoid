import { ClipboardItemCard } from "./ClipboardItem";
import type { ClipboardItem } from "@/types/clipboard";
import { Clipboard } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ClipboardListProps {
  pinnedItems: ClipboardItem[];
  unpinnedItems: ClipboardItem[];
  onCopy: (content: string) => void;
  onDelete: (id: string) => void;
  onTogglePin: (id: string) => void;
}

export function ClipboardList({
  pinnedItems,
  unpinnedItems,
  onCopy,
  onDelete,
  onTogglePin,
}: ClipboardListProps) {
  const { t } = useTranslation();
  const hasPinned = pinnedItems.length > 0;
  const hasUnpinned = unpinnedItems.length > 0;
  const isEmpty = !hasPinned && !hasUnpinned;

  if (isEmpty) {
    return (
      <div className="flex-1 flex items-center justify-center p-6">
        <div className="flex flex-col items-center gap-2 text-center text-gray-500">
          <Clipboard size={18} />
          <span>{t("noItems")}</span>
          <span className="text-xs">{t("noItemsHint")}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto p-4">
      {hasPinned && (
        <section className="mb-4">
          <div className="flex flex-col gap-2">
            {pinnedItems.map((item) => (
              <ClipboardItemCard
                key={item.id}
                item={item}
                onCopy={onCopy}
                onDelete={onDelete}
                onTogglePin={onTogglePin}
              />
            ))}
          </div>
        </section>
      )}

      {hasUnpinned && (
        <section>
          <div className="flex flex-col gap-2">
            {unpinnedItems.map((item) => (
              <ClipboardItemCard
                key={item.id}
                item={item}
                onCopy={onCopy}
                onDelete={onDelete}
                onTogglePin={onTogglePin}
              />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}