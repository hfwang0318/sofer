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
            <p className="eyebrow">Workspace</p>
            <h1>History</h1>
            <p className="lede">
              The last {entries.length || "few"} recognitions, kept on this device. Click
              copy to put an older result back on the clipboard.
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
              Clear all
            </button>
            <button type="button" className="btn btn-primary" onClick={onCapture}>
              <Icon name="capture" size={15} />
              Capture
            </button>
          </div>
        </header>

        {entries.length === 0 ? (
          <section className="panel empty-state">
            <div className="empty-icon">
              <Icon name="history" size={24} />
            </div>
            <h2>No captures yet</h2>
            <p>
              Your recognised text will collect here — nothing leaves this machine, and
              history is capped to keep the list useful.
            </p>
            <button type="button" className="btn btn-primary" onClick={onCapture}>
              Capture your first region
            </button>
            <span className="empty-shortcut">
              or press <Keycaps accelerator={shortcut} size="sm" /> anywhere
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
                        <span className="badge">{entry.format}</span>
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
                        {entry.lines} lines · {duration(entry.ms)}
                      </span>
                      <div className="history-actions">
                        <button
                          type="button"
                          className={`icon-btn ${copied ? "is-active" : ""}`}
                          aria-label={copied ? "Copied to clipboard" : "Copy this capture"}
                          title="Copy"
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
                          aria-label="Delete this capture"
                          title="Delete"
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
              History is stored locally and limited to the most recent entries. Clear it
              any time; the OCR engine is not affected.
            </p>
          </>
        )}
      </div>
    </div>
  );
}
