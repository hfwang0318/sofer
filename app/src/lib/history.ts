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
  if (delta < 45_000) return "just now";
  if (delta < 3_600_000) return `${Math.max(1, Math.round(delta / 60_000))} min ago`;
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  if (sameDay) {
    return `Today, ${date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" }) +
    `, ${date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

export function duration(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`;
}

export function compactNumber(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact" }).format(value);
}
