import { DEMO_HISTORY, HISTORY_KEY } from "./defaults";
import { isTauri } from "./platform";
import type { HistoryEntry, OcrResult } from "./types";

export function loadHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed as HistoryEntry[];
    }
  } catch {
    /* corrupted history is not worth recovering */
  }
  // The browser preview should not look like a brand-new install.
  return isTauri ? [] : DEMO_HISTORY;
}

export function saveHistory(entries: HistoryEntry[]): void {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(entries));
  } catch {
    /* quota is not fatal: history is a convenience, not the product */
  }
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
