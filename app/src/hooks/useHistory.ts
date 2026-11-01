import { useCallback, useEffect, useState, type RefObject } from "react";

import {
  fromResult,
  hydrateHistory,
  insertHistoryEntry,
  loadHistory,
  saveHistory,
  toggleHistoryPin,
  trimHistory,
} from "../lib/history";
import { isTauri } from "../lib/platform";
import type { HistoryEntry, OcrResult, Settings } from "../lib/types";
import type { PushToast } from "./useToast";

interface UseHistoryOptions {
  settingsRef: RefObject<Settings>;
  historyLimit: number;
  pushToast: PushToast;
}

export interface HistoryController {
  history: HistoryEntry[];
  addHistory: (next: OcrResult) => void;
  handleDeleteHistory: (id: string) => void;
  handleTogglePin: (id: string) => void;
  handleClearHistory: () => void;
}

/** History lives in app-data on desktop and in localStorage in the preview. */
export function useHistory({
  settingsRef,
  historyLimit,
  pushToast,
}: UseHistoryOptions): HistoryController {
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);

  const addHistory = useCallback(
    (next: OcrResult) => {
      setHistory((current) => {
        const limit = settingsRef.current.historyLimit || 20;
        const merged = insertHistoryEntry(current, fromResult(next), limit);
        saveHistory(merged);
        return merged;
      });
    },
    [settingsRef],
  );

  // Tauri keeps history in app-data; the first frame starts empty and this
  // hydrates it once the disk read (and the one-time localStorage import)
  // finishes. The browser preview already has its data synchronously.
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    void hydrateHistory()
      .then((entries) => {
        if (!alive || entries.length === 0) return;
        setHistory((current) => {
          const limit = settingsRef.current.historyLimit || 20;
          const known = new Set(current.map((entry) => entry.id));
          return [
            ...current,
            ...entries.filter((entry) => !known.has(entry.id)),
          ].slice(0, limit);
        });
      })
      .catch((error: unknown) => {
        console.warn(`读取历史失败：${String(error)}`);
      });
    return () => {
      alive = false;
    };
  }, [settingsRef]);

  // Enforce the configured limit when it shrinks.
  useEffect(() => {
    if (!historyLimit) return;
    setHistory((current) => {
      if (current.length <= historyLimit) return current;
      const next = trimHistory(current, historyLimit);
      saveHistory(next);
      return next;
    });
  }, [historyLimit]);

  const handleDeleteHistory = useCallback((id: string) => {
    setHistory((current) => {
      const next = current.filter((entry) => entry.id !== id);
      saveHistory(next);
      return next;
    });
  }, []);

  const handleTogglePin = useCallback((id: string) => {
    setHistory((current) => {
      const next = toggleHistoryPin(current, id);
      saveHistory(next);
      return next;
    });
  }, []);

  const handleClearHistory = useCallback(() => {
    setHistory([]);
    saveHistory([]);
    pushToast("info", "历史记录已清空");
  }, [pushToast]);

  return {
    history,
    addHistory,
    handleDeleteHistory,
    handleTogglePin,
    handleClearHistory,
  };
}
