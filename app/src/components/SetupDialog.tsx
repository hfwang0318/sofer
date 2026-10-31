import { useEffect, useRef, type ReactNode } from "react";

import { Icon } from "./Icon";
import { formatBytes, formatEta, formatPlatform, formatSpeed } from "../lib/format";
import { MODEL_LICENSE } from "../lib/defaults";
import type { DownloadProgress, EngineStatus } from "../lib/types";

interface SetupDialogProps {
  open: boolean;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  onStart: () => void;
  onCancel: () => void;
  onDismiss: () => void;
  onCapture: () => void;
  onOpenSettings: () => void;
}

type Mode = "idle" | "busy" | "ready" | "error";

function modeFor(download: DownloadProgress | null, status: EngineStatus | null): Mode {
  if (download?.phase === "error") return "error";
  if (download && ["downloading", "verifying", "starting"].includes(download.phase)) {
    return "busy";
  }
  if (download?.phase === "ready") return "ready";
  if (status?.model.installed && status.llama) return "ready";
  return "idle";
}

function ProgressBar({ percent }: { percent: number }) {
  return (
    <div
      className="setup-progress"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(percent)}
    >
      <span style={{ width: `${Math.max(0, Math.min(100, percent))}%` }} />
    </div>
  );
}

function CheckRow({
  children,
  title,
  detail,
  badge,
  tone = "ok",
}: {
  children?: ReactNode;
  title: string;
  detail: string;
  badge: string;
  tone?: "ok" | "warn" | "danger";
}) {
  return (
    <div className="setup-row">
      <span className={`setup-row-icon tone-${tone}`}>
        <Icon name={tone === "ok" ? "check" : tone === "warn" ? "info" : "alert"} size={15} />
      </span>
      <div className="setup-row-copy">
        <span className="setup-row-title">{title}</span>
        <span className="setup-row-detail">{detail}</span>
      </div>
      <span className={`setup-row-badge ${tone === "danger" ? "badge-danger" : ""}`}>
        {badge}
      </span>
      {children}
    </div>
  );
}

export function SetupDialog({
  open,
  status,
  download,
  onStart,
  onCancel,
  onDismiss,
  onCapture,
  onOpenSettings,
}: SetupDialogProps) {
  const primaryRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const mode = modeFor(download, status);
  const busy = mode === "busy";
  const runtimeMissing =
    status !== null && !status.runtime.bundled && !status.runtime.system;
  const percent =
    download && download.total > 0
      ? (download.downloaded / download.total) * 100
      : 0;

  useEffect(() => {
    if (!open) return;
    (primaryRef.current ?? dialogRef.current)?.focus();
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !busy) onDismiss();
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusables = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.offsetParent !== null);
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, busy, onDismiss]);

  if (!open || !status) return null;

  const progressLabel =
    download?.phase === "verifying"
      ? "Verifying the download"
      : download?.phase === "starting"
        ? "Starting the local engine"
        : "Downloading the OCR model";

  const eta = download ? formatEta(download.etaSecs) : "";

  return (
    <div className="setup-backdrop" role="presentation">
      <section
        ref={dialogRef}
        className="setup-dialog"
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="setup-title"
      >
        <button
          type="button"
          className="setup-close"
          aria-label="Close setup"
          onClick={onDismiss}
          disabled={busy}
        >
          <Icon name="close" size={15} />
        </button>

        <div className="setup-mark" aria-hidden="true">
          <Icon name="spark" size={20} />
        </div>

        {mode === "ready" ? (
          <>
            <p className="eyebrow">One-time setup</p>
            <h2 id="setup-title">The local engine is ready.</h2>
            <p className="setup-lede">
              {status.model.installed
                ? "The model is installed and verified. Captures now run entirely on this machine."
                : "Everything is in place. Captures now run entirely on this machine."}
            </p>
            <div className="setup-ready-card">
              <span className="setup-ready-icon">
                <Icon name="check" size={18} />
              </span>
              <div>
                <strong>{status.setup.modelName}</strong>
                <span>
                  {formatBytes(status.model.bytes)} · SHA-256 verified ·{" "}
                  {status.runtime.version ?? "bundled llama.cpp"}
                </span>
                <span>{formatPlatform(status.runtime.platform)}</span>
              </div>
            </div>
            <div className="setup-actions">
              <button type="button" className="btn btn-primary" onClick={onCapture}>
                <Icon name="capture" size={15} />
                Capture region
              </button>
              <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                Done
              </button>
            </div>
          </>
        ) : mode === "busy" ? (
          <>
            <p className="eyebrow">One-time setup</p>
            <h2 id="setup-title">{progressLabel}</h2>
            <p className="setup-lede">
              {download?.phase === "starting"
                ? "The first load reads 1.4 GB from disk. This usually takes 10–20 seconds."
                : "Keep the app open. You can cancel and resume later — partial files are kept."}
            </p>

            <div className="setup-progress-head">
              <span>
                {download?.phase === "downloading" && download.file
                  ? download.file
                  : download?.phase === "verifying"
                    ? "Checking file integrity"
                    : "llama-server · load weights"}
              </span>
              <span className="mono">
                {download?.phase === "downloading"
                  ? `${Math.round(percent)}%`
                  : "…"}
              </span>
            </div>
            <ProgressBar percent={download?.phase === "starting" ? 100 : percent} />
            <div className="setup-progress-meta">
              <span>
                {download
                  ? `${formatBytes(download.downloaded)} of ${formatBytes(download.total)}`
                  : ""}
              </span>
              <span>
                {download?.phase === "downloading"
                  ? `${formatSpeed(download.bytesPerSec)}${eta ? ` · ${eta}` : ""}`
                  : download?.phase === "verifying"
                    ? "SHA-256"
                    : "Loading…"}
              </span>
            </div>

            <div className="setup-actions">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={onCancel}
                disabled={download?.phase !== "downloading"}
              >
                Cancel download
              </button>
            </div>
            <p className="setup-footnote">
              Downloads from huggingface.co/ggml-org · {MODEL_LICENSE} licensed ·
              resumable
            </p>
          </>
        ) : mode === "error" ? (
          <>
            <p className="eyebrow">Setup failed</p>
            <h2 id="setup-title">The model didn’t finish downloading.</h2>
            <p className="setup-lede">
              Your partial download is kept, so retrying resumes instead of starting
              from zero.
            </p>
            <div className="setup-error" role="alert">
              <Icon name="alert" size={16} />
              <span>{download?.message ?? "Unknown setup error."}</span>
            </div>
            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onStart}
              >
                <Icon name="refresh" size={15} />
                Retry setup
              </button>
              <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="eyebrow">One-time setup</p>
            <h2 id="setup-title">
              {status.model.installed
                ? "Start the local engine."
                : "Give ocr4code its local eyes."}
            </h2>
            <p className="setup-lede">
              {status.model.installed
                ? "The model is already installed. Start llama-server to finish setup — it starts automatically on the next capture too."
                : "The app already ships the llama.cpp runtime. It only needs the GLM-OCR model once — after that every capture is fully offline."}
            </p>

            <div className="setup-checklist">
              <CheckRow
                title="llama.cpp runtime"
                detail={
                  runtimeMissing
                    ? "Not found in this build"
                    : `Bundled with the app · ${status.runtime.version ?? "b12018"}`
                }
                badge={runtimeMissing ? "Missing" : "Included"}
                tone={runtimeMissing ? "danger" : "ok"}
              />
              <CheckRow
                title={status.setup.modelName}
                detail={
                  status.model.installed
                    ? `${formatBytes(status.model.bytes)} · SHA-256 verified`
                    : `${formatBytes(status.setup.downloadBytes)} download · SHA-256 verified · one-time`
                }
                badge={status.model.installed ? "Installed" : "Required"}
                tone={status.model.installed ? "ok" : "warn"}
              />
              <CheckRow
                title="Screen capture"
                detail="macOS screencapture / Windows Snipping Tool"
                badge="System"
                tone="ok"
              />
            </div>

            {download?.phase === "cancelled" && (
              <p className="setup-note">
                <Icon name="info" size={13} />
                Download cancelled. Partial files are kept, so you can resume.
              </p>
            )}

            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onStart}
                disabled={runtimeMissing}
              >
                <Icon name="arrow-right" size={15} />
                {status.model.installed ? "Start engine" : "Download & set up"}
              </button>
              <button type="button" className="btn btn-ghost" onClick={onDismiss}>
                Not now
              </button>
            </div>
            <p className="setup-footnote">
              {status.setup.modelName} · {MODEL_LICENSE} · from huggingface.co/ggml-org
              <button type="button" className="link-btn" onClick={onOpenSettings}>
                Engine settings
              </button>
            </p>
          </>
        )}
      </section>
    </div>
  );
}
