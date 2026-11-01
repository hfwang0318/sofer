import { useEffect, useMemo, useRef, useState } from "react";

import { Icon } from "../components/Icon";
import { Keycaps } from "../components/Controls";
import {
  duration,
  engineLabel,
  exportEntryJson,
  exportEntryMarkdown,
  exportHistoryJson,
  filterHistory,
  formatLabel,
  relativeTime,
} from "../lib/history";
import type { HistoryEntry } from "../lib/types";

interface HistoryViewProps {
  entries: HistoryEntry[];
  shortcut: string;
  onCopy: (text: string, id: string) => void;
  onDelete: (id: string) => void;
  onClear: () => void;
  onCapture: () => void;
  onTogglePin: (id: string) => void;
}

function preview(text: string): string {
  const trimmed = text.trim();
  return trimmed.length > 260 ? `${trimmed.slice(0, 260)}…` : trimmed;
}

export function HistoryView({
  entries,
  shortcut,
  onCopy,
  onDelete,
  onClear,
  onCapture,
  onTogglePin,
}: HistoryViewProps) {
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement | null>(null);

  const visible = useMemo(() => filterHistory(entries, query), [entries, query]);
  const trimmedQuery = query.trim();

  useEffect(() => {
    if (!copiedId) return;
    const timer = window.setTimeout(() => setCopiedId(null), 1800);
    return () => window.clearTimeout(timer);
  }, [copiedId]);

  useEffect(() => {
    if (entries.length > 0) return;
    setConfirmClear(false);
    setQuery("");
  }, [entries.length]);

  useEffect(() => {
    if (!confirmClear) return;
    const timer = window.setTimeout(() => setConfirmClear(false), 4000);
    return () => window.clearTimeout(timer);
  }, [confirmClear]);

  const resetSearch = () => {
    setQuery("");
    searchRef.current?.focus();
  };

  return (
    <div className="view history-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">工作区</p>
            <h1>历史</h1>
            <p className="lede">
              {entries.length === 0
                ? "截图后，结果会出现在这里。"
                : trimmedQuery
                  ? `找到 ${visible.length} 条结果，共 ${entries.length} 条。`
                  : `最近的 ${entries.length} 条结果。点复制可再次使用。`}
            </p>
          </div>
          <div className="head-actions">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => exportHistoryJson(entries)}
              disabled={entries.length === 0}
            >
              导出全部
            </button>
            <button
              type="button"
              className={`btn btn-ghost btn-danger ${confirmClear ? "is-confirming" : ""}`}
              onClick={() => {
                if (confirmClear) {
                  onClear();
                  setConfirmClear(false);
                } else {
                  setConfirmClear(true);
                }
              }}
              disabled={entries.length === 0}
            >
              <Icon name="trash" size={14} />
              {confirmClear ? "再点一次清空" : "清空记录"}
            </button>
            <button type="button" className="btn btn-primary" onClick={onCapture}>
              <Icon name="capture" size={15} />
              截图
            </button>
          </div>
        </header>

        {entries.length === 0 ? (
          <section className="panel empty-state">
            <div className="empty-icon">
              <Icon name="history" size={24} />
            </div>
            <h2>还没有记录</h2>
            <p>截图后，结果会出现在这里。</p>
            <button type="button" className="btn btn-primary" onClick={onCapture}>
              截第一张图
            </button>
            <span className="empty-shortcut">
              或在任意界面按 <Keycaps accelerator={shortcut} size="sm" />
            </span>
          </section>
        ) : (
          <>
            <div className="history-toolbar">
              <div className="history-search">
                <input
                  ref={searchRef}
                  className="input"
                  type="search"
                  value={query}
                  placeholder="搜索文本、引擎或格式"
                  aria-label="搜索历史"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape" && query) {
                      event.preventDefault();
                      setQuery("");
                    }
                  }}
                />
                {query && (
                  <button
                    type="button"
                    className="icon-btn history-search-clear"
                    aria-label="清空搜索"
                    title="清空搜索"
                    onClick={resetSearch}
                  >
                    <Icon name="close" size={13} />
                  </button>
                )}
              </div>
              <span className="history-count" role="status" aria-live="polite">
                {trimmedQuery
                  ? `${visible.length} / ${entries.length} 条`
                  : `${entries.length} 条`}
              </span>
            </div>

            {visible.length === 0 ? (
              <section className="panel empty-state">
                <div className="empty-icon">
                  <Icon name="history" size={24} />
                </div>
                <h2>没有匹配的记录</h2>
                <p>换个关键词，或清空搜索查看全部 {entries.length} 条。</p>
                <button type="button" className="btn btn-primary" onClick={resetSearch}>
                  清空搜索
                </button>
              </section>
            ) : (
              <>
                <div className="history-list panel">
                  {visible.map((entry) => {
                    const copied = copiedId === entry.id;
                    return (
                      <article
                        className={`history-row ${entry.pinned ? "is-pinned" : ""}`}
                        key={entry.id}
                      >
                        {entry.thumbnail ? (
                          <img
                            className="history-thumb"
                            src={entry.thumbnail}
                            alt=""
                            loading="lazy"
                            draggable={false}
                          />
                        ) : (
                          <span
                            className="history-thumb history-thumb-empty"
                            aria-hidden="true"
                          >
                            <Icon name="image" size={15} />
                          </span>
                        )}
                        <div className="history-main">
                          <div className="history-top">
                            <time dateTime={new Date(entry.capturedAt).toISOString()}>
                              {relativeTime(entry.capturedAt)}
                            </time>
                            {entry.pinned && <span className="badge badge-pin">置顶</span>}
                            <span className="badge">{formatLabel(entry.format)}</span>
                            <span className="history-engine">
                              <span className="meta-dot tone-ok" />
                              {engineLabel(entry.engine)}
                            </span>
                            {entry.language && (
                              <span className="badge">{entry.language}</span>
                            )}
                          </div>
                          <pre className="history-preview">{preview(entry.text)}</pre>
                        </div>
                        <div className="history-side">
                          <span className="history-meta">
                            {entry.lines} 行 · {duration(entry.ms)}
                          </span>
                          <div className="history-actions">
                            <button
                              type="button"
                              className={`icon-btn ${entry.pinned ? "is-active" : ""}`}
                              aria-label={entry.pinned ? "取消置顶" : "置顶这条记录"}
                              aria-pressed={Boolean(entry.pinned)}
                              title={entry.pinned ? "取消置顶" : "置顶"}
                              onClick={() => onTogglePin(entry.id)}
                            >
                              <Icon name="spark" size={15} />
                            </button>
                            <button
                              type="button"
                              className={`icon-btn ${copied ? "is-active" : ""}`}
                              aria-label={copied ? "已复制到剪贴板" : "复制这条记录"}
                              title="复制"
                              onClick={() => {
                                onCopy(entry.text, entry.id);
                                setCopiedId(entry.id);
                              }}
                            >
                              <Icon name={copied ? "check" : "copy"} size={15} />
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn-label"
                              aria-label="导出 Markdown"
                              title="导出 Markdown"
                              onClick={() => exportEntryMarkdown(entry)}
                            >
                              MD
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn-label"
                              aria-label="导出 JSON"
                              title="导出 JSON"
                              onClick={() => exportEntryJson(entry)}
                            >
                              JSON
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn-danger"
                              aria-label="删除这条记录"
                              title="删除"
                              onClick={() => onDelete(entry.id)}
                            >
                              <Icon name="close" size={15} />
                            </button>
                          </div>
                        </div>
                      </article>
                    );
                  })}
                </div>
                <p className="view-footnote">
                  <Icon name="info" size={13} />
                  记录只保存在本机。
                </p>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
