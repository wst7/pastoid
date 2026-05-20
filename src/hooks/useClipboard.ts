/**
 * 剪切板 Hook
 */
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ClipboardItem } from '@/types/clipboard';

export function useClipboard() {
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [search, setSearch] = useState('');
  const [isLoading, setIsLoading] = useState(false);

  const filteredItems = items.filter((item) => {
    if (search.trim()) {
      const s = search.toLowerCase();
      if (!item.content.toLowerCase().includes(s)) {
        return false;
      }
    }
    return true;
  });

  const displayedPinned = filteredItems.filter((item) => item.isPinned);
  const displayedUnpinned = filteredItems.filter((item) => !item.isPinned);

  const copyToClipboard = useCallback(async (content: string): Promise<boolean> => {
    try {
      await navigator.clipboard.writeText(content);
      return true;
    } catch (err) {
      console.error('复制失败:', err);
      return false;
    }
  }, []);

  const deleteItem = useCallback(async (id: string) => {
    setIsLoading(true);
    try {
      await invoke('delete_clipboard_item', { id });
      setItems((prev) => prev.filter((item) => item.id !== id));
    } catch (err) {
      console.error('删除失败:', err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const togglePin = useCallback(async (id: string) => {
    setIsLoading(true);
    try {
      const item = items.find((i) => i.id === id);
      if (!item) throw new Error('Item not found');

      const updatedItem = { ...item, isPinned: !item.isPinned, updatedAt: Date.now() };
      await invoke('update_clipboard_item', { item: updatedItem });

      setItems((prev) => prev.map((i) => (i.id === id ? updatedItem : i)));
    } catch (err) {
      console.error('置顶失败:', err);
    } finally {
      setIsLoading(false);
    }
  }, [items]);

  const reload = useCallback(async () => {
    setIsLoading(true);
    try {
      const data: ClipboardItem[] = await invoke('get_clipboard_items');
      setItems(data);
    } catch (err) {
      console.error('同步失败:', err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();

    const unlistenClipboardChanged = listen<ClipboardItem>('clipboard_changed', async () => {
      try {
        const data: ClipboardItem[] = await invoke('get_clipboard_items');
        setItems(data);
      } catch (err) {
        console.error('同步失败:', err);
      }
    });

    const unlistenClipboardCleared = listen('clipboard-cleared', () => {
      setItems([]);
      setIsLoading(false);
    });

    return () => {
      unlistenClipboardChanged.then((fn) => fn());
      unlistenClipboardCleared.then((fn) => fn());
    };
  }, [reload]);

  return {
    items,
    filteredItems,
    displayedPinned,
    displayedUnpinned,
    isLoading,
    search,
    setSearch,
    copyToClipboard,
    deleteItem,
    togglePin,
    reload,
  };
}