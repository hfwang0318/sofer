import {
  loadHistoryFromDisk,
  migrateHistoryEntries,
  previewHistory,
  saveHistoryToDisk,
} from "./bridge";
import { HISTORY_KEY, HISTORY_MIGRATED_KEY } from "./defaults";
import { isTauri } from "./platform";
import type { HistoryEntry, OcrResult } from "./types";

/**
 * The synchronous first frame. Tauri reads `<app-data>/history.json`
 * asynchronously, so it starts empty and `hydrateHistory` fills it after mount;
 * the browser preview still reads localStorage synchronously.
 */
export function loadHistory(): HistoryEntry[] {
  return isTauri ? [] : previewHistory();
}

/**
 * Read the persisted history once, importing the legacy `localStorage` copy the
 * first time the disk store is empty. The import runs at most once: the marker
 * is kept in localStorage and the store records `migrated` in the file, so
 * neither a cache wipe nor clearing history can bring the old copy back.
 */
export async function hydrateHistory(): Promise<HistoryEntry[]> {
  if (!isTauri) return previewHistory();
  const disk = await loadHistoryFromDisk();
  if (disk.length > 0) return disk;
  if (migratedAlready()) return disk;

  const legacy = readLegacyHistory();
  if (legacy.length > 0) {
    try {
      const imported = await migrateHistoryEntries(legacy);
      markMigrated();
      return imported;
    } catch (error: unknown) {
      // Keep the legacy copy so the next launch can retry the import.
      console.warn(`历史迁移失败：${errorMessage(error)}`);
      return disk;
    }
  }
  markMigrated();
  return disk;
}

/**
 * Persist history and resolve with a readable message when the write fails.
 * Callers may ignore the promise; the in-memory list stays authoritative, so a
 * failed write never clears what the user can still copy.
 */
export function saveHistory(entries: HistoryEntry[]): Promise<string | null> {
  return saveHistoryToDisk(entries).then(
    () => null,
    (error: unknown) => {
      const message = errorMessage(error);
      console.warn(`保存历史失败：${message}`);
      return message;
    },
  );
}

function readLegacyHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed as HistoryEntry[];
    }
  } catch {
    /* corrupted history is not worth recovering */
  }
  return [];
}

function migratedAlready(): boolean {
  try {
    return localStorage.getItem(HISTORY_MIGRATED_KEY) !== null;
  } catch {
    return false;
  }
}

function markMigrated(): void {
  try {
    localStorage.setItem(HISTORY_MIGRATED_KEY, "1");
  } catch {
    /* an unavailable localStorage only costs a repeat check on next launch */
  }
}

/** Tauri rejects commands with the plain string returned from Rust. */
function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function fromResult(result: OcrResult): HistoryEntry {
  return {
    id: result.id,
    text: result.text,
    engine: result.engine,
    ms: result.ms,
    width: result.width,
    height: result.height,
    language: result.language,
    format: result.format,
    chars: result.chars,
    lines: result.lines,
    thumbnail: result.thumbnail,
    capturedAt: result.capturedAt,
  };
}

export function relativeTime(timestamp: number): string {
  const delta = Date.now() - timestamp;
  if (delta < 45_000) return "刚刚";
  if (delta < 3_600_000) return `${Math.max(1, Math.round(delta / 60_000))} 分钟前`;
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  const clock = date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return `今天 ${clock}`;
  const day = date.toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
  return `${day} ${clock}`;
}

export function duration(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)} 毫秒` : `${(ms / 1000).toFixed(1)} 秒`;
}

export function compactNumber(value: number): string {
  return new Intl.NumberFormat("zh-CN", { notation: "compact" }).format(value);
}
