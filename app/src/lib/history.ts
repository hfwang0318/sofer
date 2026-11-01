import { DEMO_HISTORY, HISTORY_KEY, MAX_HISTORY_ENTRIES } from "./defaults";
import { isTauri } from "./platform";
import type { HistoryEntry, OcrResult } from "./types";

const MIN_FENCE = "```";
const FENCE_RUN = /`+/g;
const DOWNLOAD_REVOKE_MS = 1000;

function persist(entries: HistoryEntry[]): boolean {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(entries));
    return true;
  } catch {
    return false;
  }
}

function asNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

/** Rebuild an entry from localStorage, tolerating data saved before the
 *  `thumbnail` and `pinned` fields existed. */
function normalizeEntry(raw: unknown): HistoryEntry | null {
  if (typeof raw !== "object" || raw === null) return null;
  const record = raw as Record<string, unknown>;
  if (typeof record.id !== "string" || typeof record.text !== "string") return null;
  return {
    id: record.id,
    text: record.text,
    engine: typeof record.engine === "string" ? record.engine : "",
    ms: asNumber(record.ms, 0),
    width: asNumber(record.width, 0),
    height: asNumber(record.height, 0),
    language: typeof record.language === "string" ? record.language : null,
    format: typeof record.format === "string" ? record.format : "text",
    chars: asNumber(record.chars, record.text.length),
    lines: asNumber(record.lines, record.text.split("\n").length),
    capturedAt: asNumber(record.capturedAt, 0),
    thumbnail: typeof record.thumbnail === "string" ? record.thumbnail : undefined,
    pinned: record.pinned === true,
  };
}

/** Pinned entries first, then newest first. Never mutates the input. */
export function sortHistory(entries: HistoryEntry[]): HistoryEntry[] {
  return [...entries].sort((a, b) => {
    const pinnedA = Boolean(a.pinned);
    const pinnedB = Boolean(b.pinned);
    if (pinnedA !== pinnedB) return pinnedA ? -1 : 1;
    return b.capturedAt - a.capturedAt;
  });
}

/** Sort, then keep at most `limit` entries (also bounded by the storage cap). */
export function trimHistory(entries: HistoryEntry[], limit: number): HistoryEntry[] {
  const cap = Math.max(0, Math.min(limit, MAX_HISTORY_ENTRIES));
  return sortHistory(entries).slice(0, cap);
}

export function insertHistoryEntry(
  entries: HistoryEntry[],
  entry: HistoryEntry,
  limit: number,
): HistoryEntry[] {
  return trimHistory(
    [entry, ...entries.filter((item) => item.id !== entry.id)],
    limit,
  );
}

export function toggleHistoryPin(entries: HistoryEntry[], id: string): HistoryEntry[] {
  return sortHistory(
    entries.map((entry) =>
      entry.id === id ? { ...entry, pinned: !entry.pinned } : entry,
    ),
  );
}

export function loadHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        const entries = parsed
          .map(normalizeEntry)
          .filter((entry): entry is HistoryEntry => entry !== null);
        return trimHistory(entries, MAX_HISTORY_ENTRIES);
      }
    }
  } catch {
    /* corrupted history is not worth recovering */
  }
  // The browser preview should not look like a brand-new install.
  return isTauri ? [] : trimHistory(DEMO_HISTORY, MAX_HISTORY_ENTRIES);
}

export function saveHistory(entries: HistoryEntry[]): void {
  const bounded = trimHistory(entries, MAX_HISTORY_ENTRIES);
  if (persist(bounded)) return;
  // Thumbnails dominate the payload. Drop them once and keep the text and pin
  // state; a failed second write only costs history persistence, never the
  // current result.
  persist(bounded.map((entry) => ({ ...entry, thumbnail: undefined })));
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
    thumbnail: result.thumbnail ?? undefined,
    pinned: false,
  };
}

export function engineLabel(engine: string): string {
  if (engine.startsWith("llamacpp")) return "本地模型";
  if (engine.startsWith("tesseract")) return "tesseract";
  if (engine.startsWith("mock")) return "预览";
  return engine;
}

export function formatLabel(format: string): string {
  if (format === "text") return "纯文本";
  if (format === "markdown") return "Markdown";
  if (format === "json") return "JSON";
  return format;
}

/** Case-insensitive incremental filter over text, engine and format. The
 *  human labels are included so "本地模型" / "纯文本" also match. */
export function filterHistory(entries: HistoryEntry[], query: string): HistoryEntry[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return entries;
  return entries.filter((entry) =>
    [
      entry.text,
      entry.engine,
      engineLabel(entry.engine),
      entry.format,
      formatLabel(entry.format),
      entry.language ?? "",
    ].some((value) => value.toLocaleLowerCase().includes(needle)),
  );
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

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function fileStamp(timestamp: number): string {
  const date = new Date(timestamp);
  const day = `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}`;
  const clock = `${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`;
  return `${day}-${clock}`;
}

function readableStamp(timestamp: number): string {
  const date = new Date(timestamp);
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}`,
  ].join(" ");
}

/** A fence longer than any backtick run in the text, so pasted code cannot
 *  terminate the block early. */
function fenceFor(text: string): string {
  let longest = 0;
  for (const match of text.matchAll(FENCE_RUN)) {
    longest = Math.max(longest, match[0].length);
  }
  return "`".repeat(Math.max(MIN_FENCE.length, longest + 1));
}

export function entryMarkdown(entry: HistoryEntry): string {
  const fence = fenceFor(entry.text);
  const language = entry.language?.trim() ?? "";
  return [
    `# 截图 ${readableStamp(entry.capturedAt)}`,
    "",
    `- 引擎：${entry.engine}`,
    `- 格式：${formatLabel(entry.format)}`,
    `- 识别：${entry.lines} 行 · ${duration(entry.ms)}`,
    "",
    `${fence}${language}`,
    entry.text.replace(/\n+$/, ""),
    fence,
    "",
  ].join("\n");
}

export function downloadText(filename: string, content: string, mime: string): void {
  const blob = new Blob([content], { type: `${mime};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noopener";
  anchor.hidden = true;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  // Give WebKit a moment to start the download before dropping the URL.
  window.setTimeout(() => URL.revokeObjectURL(url), DOWNLOAD_REVOKE_MS);
}

export function exportEntryMarkdown(entry: HistoryEntry): void {
  downloadText(
    `ocr4code-${fileStamp(entry.capturedAt)}.md`,
    entryMarkdown(entry),
    "text/markdown",
  );
}

export function exportEntryJson(entry: HistoryEntry): void {
  downloadText(
    `ocr4code-${fileStamp(entry.capturedAt)}.json`,
    JSON.stringify(
      { version: 1, exportedAt: new Date().toISOString(), entry },
      null,
      2,
    ),
    "application/json",
  );
}

export function exportHistoryJson(entries: HistoryEntry[]): void {
  downloadText(
    `ocr4code-history-${fileStamp(Date.now())}.json`,
    JSON.stringify(
      { version: 1, exportedAt: new Date().toISOString(), entries },
      null,
      2,
    ),
    "application/json",
  );
}
