import { afterEach, describe, expect, it, vi } from "vitest";
import { DEMO_HISTORY, HISTORY_KEY } from "./defaults";
import {
  compactNumber,
  duration,
  fromResult,
  loadHistory,
  relativeTime,
  saveHistory,
} from "./history";
import type { HistoryEntry, OcrResult } from "./types";

const RESULT: OcrResult = {
  id: "shot-1",
  text: "fn main() {}",
  engine: "llamacpp(http://127.0.0.1:8099)",
  ms: 864,
  width: 1284,
  height: 742,
  language: "rust",
  format: "text",
  chars: 12,
  lines: 1,
  thumbnail: "data:image/png;base64,AAAA",
  capturedAt: 1_760_000_000_000,
  copied: true,
  truncated: false,
};

const ENTRY: HistoryEntry = {
  id: "shot-1",
  text: "fn main() {}",
  engine: "llamacpp(http://127.0.0.1:8099)",
  ms: 864,
  width: 1284,
  height: 742,
  language: "rust",
  format: "text",
  chars: 12,
  lines: 1,
  capturedAt: 1_760_000_000_000,
  thumbnail: undefined,
  pinned: false,
};

const RESULT_ENTRY: HistoryEntry = {
  ...ENTRY,
  thumbnail: "data:image/png;base64,AAAA",
};

describe("fromResult", () => {
  it("keeps the history fields and the thumbnail, and drops the clipboard flag", () => {
    const entry = fromResult(RESULT);

    expect(entry).toStrictEqual(RESULT_ENTRY);
    expect(entry).not.toHaveProperty("copied");
  });

  it("preserves a null language instead of coercing it to a string", () => {
    const entry = fromResult({ ...RESULT, language: null });

    expect(entry.language).toBeNull();
  });
});

describe("loadHistory / saveHistory", () => {
  it("round-trips entries through localStorage", () => {
    saveHistory([ENTRY]);

    expect(loadHistory()).toStrictEqual([ENTRY]);
  });

  it("falls back to the demo history in the browser preview when storage is empty", () => {
    expect(loadHistory().map((entry) => entry.id)).toStrictEqual(
      DEMO_HISTORY.map((entry) => entry.id),
    );
  });

  it("ignores corrupted JSON and shows the demo history instead", () => {
    localStorage.setItem(HISTORY_KEY, "{not json");

    expect(loadHistory().map((entry) => entry.id)).toStrictEqual(
      DEMO_HISTORY.map((entry) => entry.id),
    );
  });

  it("ignores valid JSON that is not an array", () => {
    localStorage.setItem(HISTORY_KEY, JSON.stringify({ entries: [] }));

    expect(loadHistory().map((entry) => entry.id)).toStrictEqual(
      DEMO_HISTORY.map((entry) => entry.id),
    );
  });
});

describe("relativeTime", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  function freezeAtLocalNoon(): number {
    vi.useFakeTimers();
    const now = new Date(2026, 5, 15, 12, 0, 0);
    vi.setSystemTime(now);
    return now.getTime();
  }

  it("shows 刚刚 for anything under 45 seconds", () => {
    const now = freezeAtLocalNoon();

    expect(relativeTime(now)).toBe("刚刚");
    expect(relativeTime(now - 44_999)).toBe("刚刚");
  });

  it("switches to minutes at the 45 second boundary", () => {
    const now = freezeAtLocalNoon();

    expect(relativeTime(now - 45_000)).toBe("1 分钟前");
    expect(relativeTime(now - 90_000)).toBe("2 分钟前");
  });

  it("keeps counting minutes up to the one hour boundary", () => {
    const now = freezeAtLocalNoon();

    expect(relativeTime(now - 3_540_000)).toBe("59 分钟前");
  });

  it("shows the clock time for earlier entries from today", () => {
    const now = freezeAtLocalNoon();

    expect(relativeTime(now - 3_600_000)).toBe("今天 11:00");
  });

  it("shows month and day for entries from earlier days", () => {
    const now = freezeAtLocalNoon();

    expect(relativeTime(now - 2 * 24 * 3_600_000)).toBe("6月13日 12:00");
  });
});

describe("duration", () => {
  it("uses milliseconds below one second and seconds above", () => {
    expect(duration(0)).toBe("0 毫秒");
    expect(duration(999)).toBe("999 毫秒");
    expect(duration(1000)).toBe("1.0 秒");
    expect(duration(1234)).toBe("1.2 秒");
    expect(duration(60_000)).toBe("60.0 秒");
  });
});

describe("compactNumber", () => {
  it("keeps plain digits under ten thousand", () => {
    expect(compactNumber(0)).toBe("0");
    expect(compactNumber(9999)).toBe("9999");
  });

  it("uses the 万 unit from ten thousand upwards", () => {
    expect(compactNumber(10_000)).toBe("1万");
    expect(compactNumber(12_345)).toBe("1.2万");
    expect(compactNumber(1_000_000)).toBe("100万");
  });
});
