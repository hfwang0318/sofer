import { useEffect, useRef, useState, type ReactNode } from "react";

import { Icon } from "./Icon";
import { Keycaps } from "./Controls";
import { formatBytes, formatEta, formatPlatform, formatSpeed } from "../lib/format";
import { MODEL_LICENSE } from "../lib/defaults";
import { isMac } from "../lib/platform";
import type {
  DownloadProgress,
  EngineStatus,
  ScreenAccess,
  SettingsSection,
} from "../lib/types";

interface SetupDialogProps {
  open: boolean;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  screenAccess: ScreenAccess;
  firstRun: boolean;
  shortcut: string;
  onStart: () => void;
  onCancel: () => void;
  onDismiss: () => void;
  onCapture: () => void;
  onPermission: () => void;
  onOpenSettings: (section?: SettingsSection) => void;
  onCopyError: (text: string) => void;
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
  title,
  detail,
  badge,
  tone = "ok",
  children,
}: {
  title: string;
  detail: string;
  badge: string;
  tone?: "ok" | "warn" | "danger";
  children?: ReactNode;
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

function Steps({ shortcut }: { shortcut: string }) {
  return (
    <div className="setup-steps" aria-label="使用步骤">
      <div>
        <span className="setup-step-number">1</span>
        <span>按</span>
        <Keycaps accelerator={shortcut} size="sm" />
      </div>
      <div>
        <span className="setup-step-number">2</span>
        <span>框选屏幕上的代码</span>
      </div>
      <div>
        <span className="setup-step-number">3</span>
        <span>回到编辑器粘贴</span>
      </div>
    </div>
  );
}

function permissionDetail(access: ScreenAccess): string {
  if (access === "granted") return "已授权，可以正常截图";
  if (access === "not-needed") return "当前平台不需要额外授权";
  return "未授权时截图会是空白";
}

export function SetupDialog({
  open,
  status,
  download,
  screenAccess,
  firstRun,
  shortcut,
  onStart,
  onCancel,
  onDismiss,
  onCapture,
  onPermission,
  onOpenSettings,
  onCopyError,
}: SetupDialogProps) {
  const primaryRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const dismissRef = useRef(onDismiss);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const mode = modeFor(download, status);
  const runtimeMissing =
    status !== null && !status.runtime.bundled && !status.runtime.system;
  const percent =
    download && download.total > 0
      ? (download.downloaded / download.total) * 100
      : 0;
  const permissionGranted = screenAccess === "granted" || screenAccess === "not-needed";

  useEffect(() => {
    dismissRef.current = onDismiss;
  }, [onDismiss]);

  useEffect(() => {
    if (!open) return;
    const primary = primaryRef.current;
    if (primary && !primary.disabled) primary.focus();
    else dialogRef.current?.focus();
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") dismissRef.current();
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusables = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.offsetParent !== null);
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (!dialogRef.current.contains(document.activeElement)) {
        // The active button was unmounted by a mode change; do not let Tab
        // escape into the window behind the dialog.
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, mode]);

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [open]);

  if (!open || !status) return null;

  const progressTitle =
    download?.phase === "verifying"
      ? "正在校验文件"
      : download?.phase === "starting"
        ? "正在加载模型"
        : "正在下载模型";
  const eta = download ? formatEta(download.etaSecs) : "";
  const errorText = download?.message ?? "没有更多信息。";

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
          aria-label="关闭"
          onClick={onDismiss}
        >
          <Icon name="close" size={15} />
        </button>

        <div className="setup-mark" aria-hidden="true">
          <Icon name={mode === "ready" ? "check" : mode === "error" ? "alert" : "spark"} size={20} />
        </div>

        {mode === "ready" ? (
          <>
            <p className="eyebrow">{firstRun ? "首次使用" : "OCR 模型"}</p>
            <h2 id="setup-title">可以开始截图了</h2>
            <p className="setup-lede">
              按 <Keycaps accelerator={shortcut} size="sm" /> 框选屏幕上的代码，结果会直接进剪贴板。
            </p>
            <div className="setup-ready-card">
              <span className="setup-ready-icon">
                <Icon name="check" size={18} />
              </span>
              <div>
                <strong>{status.setup.modelName}</strong>
                <span>{formatBytes(status.model.bytes)} · 本地处理</span>
                <span>{formatPlatform(status.runtime.platform)}</span>
              </div>
            </div>
            {!permissionGranted && (
              <p className="setup-note">
                <Icon name="alert" size={13} />
                还需要授予屏幕录制权限，否则截图会是空白。
                <button type="button" className="link-btn" onClick={onPermission}>
                  去授权
                </button>
              </p>
            )}
            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={permissionGranted ? onCapture : onPermission}
              >
                <Icon name={permissionGranted ? "capture" : "alert"} size={15} />
                {permissionGranted
                  ? firstRun
                    ? "截第一张图"
                    : "截一张图"
                  : "授权屏幕录制"}
              </button>
              <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                完成
              </button>
            </div>
          </>
        ) : mode === "busy" ? (
          <>
            <p className="eyebrow">{firstRun ? "首次使用" : "OCR 模型"}</p>
            <h2 id="setup-title">{progressTitle}</h2>
            <p className="setup-lede">
              {download?.phase === "starting"
                ? "通常需要 10–20 秒。"
                : "下载在后台进行，可以关掉这个窗口。"}
            </p>

            <div className="setup-progress-head">
              <span>
                {download?.phase === "downloading" && download.file
                  ? download.file
                  : download?.phase === "verifying"
                    ? "校验文件完整性"
                    : "加载模型"}
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
                    ? "校验中"
                    : "加载中"}
              </span>
            </div>

            <div className="setup-actions">
              {download?.phase === "downloading" ? (
                <button type="button" className="btn btn-secondary" onClick={onCancel}>
                  取消下载
                </button>
              ) : (
                <button type="button" className="btn btn-secondary" onClick={onDismiss}>
                  后台继续
                </button>
              )}
              {download?.phase === "downloading" && (
                <button type="button" className="btn btn-ghost" onClick={onDismiss}>
                  后台继续
                </button>
              )}
            </div>
            <p className="setup-footnote">已下载的部分会保留，下次接着下。</p>
          </>
        ) : mode === "error" ? (
          <>
            <p className="eyebrow">没有完成</p>
            <h2 id="setup-title">设置没有完成</h2>
            <p className="setup-lede">已下载的部分会保留，重试会接着下。</p>
            <div className="setup-error" role="alert">
              <Icon name="alert" size={16} />
              <span>{errorText}</span>
            </div>
            <div className="setup-error-actions">
              <button
                type="button"
                className="link-btn"
                onClick={() => onCopyError(errorText)}
              >
                复制错误
              </button>
            </div>
            <p className="setup-note">
              <Icon name="info" size={13} />
              连不上？可以在下载设置里试试镜像或代理。
            </p>
            {download?.message &&
              /tls|certificate|unknown issuer/i.test(download.message) && (
                <p className="setup-note">
                  <Icon name="info" size={13} />
                  证书校验失败。请把代理或安全软件的根证书装进系统信任库。
                </p>
              )}
            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onStart}
              >
                <Icon name="refresh" size={15} />
                重试
              </button>
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => onOpenSettings("recognition")}
              >
                下载设置
              </button>
              <button type="button" className="btn btn-ghost" onClick={onDismiss}>
                稍后再说
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="eyebrow">{firstRun ? "首次使用" : "OCR 模型"}</p>
            <h2 id="setup-title">
              {status.model.installed ? "启动 OCR 引擎" : "启用本地 OCR 引擎"}
            </h2>
            <p className="setup-lede">
              {status.model.installed
                ? "模型已经装好，启动后就能截图。"
                : "下载一次 OCR 模型，之后截图都在本机处理。"}
            </p>

            {firstRun && <Steps shortcut={shortcut} />}

            <div className="setup-checklist">
              <CheckRow
                title="OCR 模型"
                detail={
                  status.model.installed
                    ? `${formatBytes(status.model.bytes)} · 已安装`
                    : `${formatBytes(status.setup.downloadBytes)} · 只需下载一次`
                }
                badge={status.model.installed ? "已安装" : "未下载"}
                tone={status.model.installed ? "ok" : "warn"}
              />
              {isMac && (
                <CheckRow
                  title="屏幕录制权限"
                  detail={permissionDetail(screenAccess)}
                  badge={permissionGranted ? "已授权" : "待授权"}
                  tone={permissionGranted ? "ok" : "warn"}
                >
                  {!permissionGranted && (
                    <button type="button" className="link-btn" onClick={onPermission}>
                      授权
                    </button>
                  )}
                </CheckRow>
              )}
              <CheckRow
                title="本地处理"
                detail="截图和文字不会离开这台电脑"
                badge="离线"
                tone="ok"
              />
            </div>

            <details
              className="setup-details"
              open={detailsOpen}
              onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
            >
              <summary>技术详情</summary>
              <div>
                <span>推理运行时：llama.cpp · {runtimeMissing ? "未找到" : "已内置"}</span>
                <span>
                  模型：{status.setup.modelName} · {MODEL_LICENSE} · 来自 Hugging Face
                </span>
                <span>下载后使用 SHA-256 校验文件完整性</span>
                <span>{formatPlatform(status.runtime.platform)}</span>
              </div>
            </details>

            {download?.phase === "cancelled" && (
              <p className="setup-note">
                <Icon name="info" size={13} />
                已取消，下次会接着下。
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
                <Icon name={status.model.installed ? "power" : "arrow-right"} size={15} />
                {status.model.installed
                  ? "启动服务"
                  : `下载模型 · ${formatBytes(status.setup.downloadBytes)}`}
              </button>
              <button type="button" className="btn btn-ghost" onClick={onDismiss}>
                {firstRun ? "稍后设置" : "关闭"}
              </button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
