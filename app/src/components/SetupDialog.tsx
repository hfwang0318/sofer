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
      ? "正在校验下载文件"
      : download?.phase === "starting"
        ? "正在启动本地引擎"
        : "正在下载 OCR 模型";

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
          aria-label="关闭设置"
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
            <p className="eyebrow">首次设置</p>
            <h2 id="setup-title">本地引擎已就绪。</h2>
            <p className="setup-lede">
              {status.model.installed
                ? "模型已安装并通过校验。之后的截图完全在本机处理。"
                : "一切就绪。之后的截图完全在本机处理。"}
            </p>
            <div className="setup-ready-card">
              <span className="setup-ready-icon">
                <Icon name="check" size={18} />
              </span>
              <div>
                <strong>{status.setup.modelName}</strong>
                <span>
                  {formatBytes(status.model.bytes)} · SHA-256 已校验 ·{" "}
                  {status.runtime.version ?? "内置 llama.cpp"}
                </span>
                <span>{formatPlatform(status.runtime.platform)}</span>
              </div>
            </div>
            <div className="setup-actions">
              <button type="button" className="btn btn-primary" onClick={onCapture}>
                <Icon name="capture" size={15} />
                截取区域
              </button>
              <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                完成
              </button>
            </div>
          </>
        ) : mode === "busy" ? (
          <>
            <p className="eyebrow">首次设置</p>
            <h2 id="setup-title">{progressLabel}</h2>
            <p className="setup-lede">
              {download?.phase === "starting"
                ? "首次启动会从磁盘加载约 1.4 GB 模型，通常需要 10–20 秒。"
                : "请保持应用开启。可以取消，之后继续下载——已下载的部分会保留。"}
            </p>

            <div className="setup-progress-head">
              <span>
                {download?.phase === "downloading" && download.file
                  ? download.file
                  : download?.phase === "verifying"
                    ? "正在校验文件完整性"
                    : "llama-server · 加载权重"}
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
                  ? `${formatBytes(download.downloaded)} / ${formatBytes(download.total)}`
                  : ""}
              </span>
              <span>
                {download?.phase === "downloading"
                  ? `${formatSpeed(download.bytesPerSec)}${eta ? ` · ${eta}` : ""}`
                  : download?.phase === "verifying"
                    ? "SHA-256"
                    : "加载中…"}
              </span>
            </div>

            <div className="setup-actions">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={onCancel}
                disabled={download?.phase !== "downloading"}
              >
                取消下载
              </button>
            </div>
            <p className="setup-footnote">
              下载自 huggingface.co/ggml-org · {MODEL_LICENSE} 许可 · 支持断点续传
            </p>
          </>
        ) : mode === "error" ? (
          <>
            <p className="eyebrow">设置失败</p>
            <h2 id="setup-title">模型下载未完成。</h2>
            <p className="setup-lede">
              已下载的部分会保留，重试会从断点继续。
            </p>
            <div className="setup-error" role="alert">
              <Icon name="alert" size={16} />
              <span>{download?.message ?? "未知的设置错误。"}</span>
            </div>
            <p className="setup-note">
              <Icon name="info" size={13} />
              连接失败？可在设置 → 引擎 → 模型下载中填写镜像或代理。
              <button type="button" className="link-btn" onClick={onOpenSettings}>
                配置下载
              </button>
            </p>
            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onStart}
              >
                <Icon name="refresh" size={15} />
                重试设置
              </button>
              <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                关闭
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="eyebrow">首次设置</p>
            <h2 id="setup-title">
              {status.model.installed
                ? "启动本地引擎。"
                : "让 ocr4code 拥有本地识别能力。"}
            </h2>
            <p className="setup-lede">
              {status.model.installed
                ? "模型已经安装。启动本地服务即可完成设置；下次截图也会自动启动。"
                : "已内置 llama.cpp 运行时；模型只需下载一次，之后完全离线。"}
            </p>

            <div className="setup-checklist">
              <CheckRow
                title="llama.cpp 运行时"
                detail={
                  runtimeMissing
                    ? "此构建中未找到"
                    : `随应用内置 · ${status.runtime.version ?? "b12018"}`
                }
                badge={runtimeMissing ? "缺失" : "已内置"}
                tone={runtimeMissing ? "danger" : "ok"}
              />
              <CheckRow
                title={status.setup.modelName}
                detail={
                  status.model.installed
                    ? `${formatBytes(status.model.bytes)} · SHA-256 已校验`
                    : `${formatBytes(status.setup.downloadBytes)} 下载 · SHA-256 已校验 · 仅需一次`
                }
                badge={status.model.installed ? "已安装" : "需要下载"}
                tone={status.model.installed ? "ok" : "warn"}
              />
              <CheckRow
                title="系统截图"
                detail="macOS screencapture / Windows 截屏工具"
                badge="系统"
                tone="ok"
              />
            </div>

            {download?.phase === "cancelled" && (
              <p className="setup-note">
                <Icon name="info" size={13} />
                下载已取消。已下载的部分会保留，可以继续。
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
                {status.model.installed ? "启动引擎" : "下载并配置"}
              </button>
              <button type="button" className="btn btn-ghost" onClick={onDismiss}>
                以后再说
              </button>
            </div>
            <p className="setup-footnote">
              {status.setup.modelName} · {MODEL_LICENSE} · 来自 huggingface.co/ggml-org
              <button type="button" className="link-btn" onClick={onOpenSettings}>
                引擎设置
              </button>
            </p>
          </>
        )}
      </section>
    </div>
  );
}
