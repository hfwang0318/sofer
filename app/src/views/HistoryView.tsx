import { useEffect, useState } from "react";

import { Icon } from "../components/Icon";
import { Keycaps } from "../components/Controls";
import { duration, relativeTime } from "../lib/history";
import type { HistoryEntry } from "../lib/types";

interface HistoryViewProps {
  entries: HistoryEntry[];
  shortcut: string;
  onCopy: (text: string, id: string) => void;
  onDelete: (id: string) => void;
  onClear: () => void;
  onCapture: () => void;
}

function engineLabel(engine: string): string {
  if (engine.startsWith("llamacpp")) return "llama.cpp";
  if (engine.startsWith("tesseract")) return "tesseract";
  if (engine.startsWith("mock")) return "mock";
  return engine;
}

function formatLabel(format: string): string {
  if (format === "text") return "纯文本";
  if (format === "markdown") return "Markdown";
  if (format === "json") return "JSON";
  return format;
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
}: HistoryViewProps) {
  const [copiedId, setCopiedId] = useState<string | null>(null);

  useEffect(() => {
    if (!copiedId) return;
    const timer = window.setTimeout(() => setCopiedId(null), 1800);
    return () => window.clearTimeout(timer);
  }, [copiedId]);

  return (
    <div className="view history-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">工作区</p>
            <h1>历史</h1>
            <p className="lede">
              最近 {entries.length || "几"} 条识别记录，保存在本机。点击复制可把旧结果重新放回剪贴板。
            </p>
          </div>
          <div className="head-actions">
            <button
              type="button"
              className="btn btn-ghost btn-danger"
              onClick={onClear}
              disabled={entries.length === 0}
            >
              <Icon name="trash" size={14} />
              清空全部
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
            <h2>还没有截图记录</h2>
            <p>
              识别结果会收集在这里。不会离开本机，并且会自动限制条数。
            </p>
            <button type="button" className="btn btn-primary" onClick={onCapture}>
              截取第一张截图
            </button>
            <span className="empty-shortcut">
              或在任意界面按 <Keycaps accelerator={shortcut} size="sm" />
            </span>
          </section>
        ) : (
          <>
            <div className="history-list panel">
              {entries.map((entry) => {
                const copied = copiedId === entry.id;
                return (
                  <article className="history-row" key={entry.id}>
                    <div className="history-main">
                      <div className="history-top">
                        <time dateTime={new Date(entry.capturedAt).toISOString()}>
                          {relativeTime(entry.capturedAt)}
                        </time>
                        <span className="badge">{formatLabel(entry.format)}</span>
                        <span className="history-engine">
                          <span className="meta-dot tone-ok" />
                          {engineLabel(entry.engine)}
                        </span>
                        {entry.language && <span className="badge">{entry.language}</span>}
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
              历史记录保存在本机，只保留最近若干条。随时可以清空，不会影响 OCR 引擎。
            </p>
          </>
        )}
      </div>
    </div>
  );
}
