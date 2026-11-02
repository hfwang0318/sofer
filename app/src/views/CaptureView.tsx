import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { Icon } from "../components/Icon";
import { retryCapture } from "../lib/bridge";
import { duration, relativeTime } from "../lib/history";
import { isMac, isPreview, shortcutText } from "../lib/platform";
import type {
  OcrError,
  OcrPartial,
  OcrResult,
  Phase,
  SettingsSection,
} from "../lib/types";

interface CaptureViewProps {
  phase: Phase;
  partial: OcrPartial | null;
  shortcut: string;
  result: OcrResult | null;
  error: OcrError | null;
  copied: boolean;
  /** Outline recognised regions on the result preview (settings → 通用). */
  showBoxes: boolean;
  setupBlocking: boolean;
  remote: boolean;
  permissionDenied: boolean;
  showSteps: boolean;
  firstCaptureTip: boolean;
  onCapture: () => void;
  onClipboard: () => void;
  onPickImage: (file: { name: string; size: number }) => void;
  onCopy: (text: string) => void;
  onOpenHistory: () => void;
  onOpenSettings: (section?: SettingsSection) => void;
  onSetup: () => void;
  onPermission: () => void;
  onDismissTip: () => void;
}

function engineLabel(engine: string, remote: boolean): string {
  if (engine.startsWith("llamacpp")) return remote ? "服务器模型" : "本地模型";
  if (engine.startsWith("tesseract")) return "tesseract";
  if (engine.startsWith("mock")) return "预览";
  return engine;
}

function formatLabel(format: string): string {
  if (format === "text") return "纯文本";
  if (format === "markdown") return "Markdown";
  if (format === "json") return "JSON";
  return format;
}

function MetaChip({ children }: { children: ReactNode }) {
  return <span className="badge">{children}</span>;
}

function HeroVisual() {
  return (
    <div className="hero-visual" aria-hidden="true">
      <div className="mini-window">
        <div className="mini-titlebar">
          <span className="mini-dot" />
          <span className="mini-dot" />
          <span className="mini-dot" />
          <span className="mini-tab">src/loader.rs</span>
        </div>
        <div className="mini-code">
          <div className="mini-line">
            <span className="ln">1</span>
            <span className="kw">async fn</span> <span className="fn">load_user</span>
            <span className="punct">(</span>
          </div>
          <div className="mini-line">
            <span className="ln">2</span>
            <span className="indent" />
            <span className="kw">let</span> <span className="plain">user =</span>{" "}
            <span className="fn">sqlx</span>
            <span className="punct">::query_as</span>
          </div>
          <div className="mini-line is-selected">
            <span className="ln">3</span>
            <span className="indent" />
            <span className="str">"SELECT id, email"</span>
          </div>
          <div className="mini-line is-selected">
            <span className="ln">4</span>
            <span className="str">"FROM users WHERE id = $1"</span>
          </div>
          <div className="mini-line">
            <span className="ln">5</span>
            <span className="punct">)</span>
            <span className="plain">.bind(id)</span>
          </div>
          <div className="mini-line">
            <span className="ln">6</span>
            <span className="plain">.fetch_one(pool).await?;</span>
          </div>
          <div className="mini-line">
            <span className="ln">7</span>
          </div>
          <div className="mini-line">
            <span className="ln">8</span>
            <span className="kw">Ok</span>
            <span className="punct">(</span>
            <span className="plain">user</span>
            <span className="punct">)</span>
          </div>
        </div>
        <div className="selection-frame">
          <span className="corner corner-tl" />
          <span className="corner corner-tr" />
          <span className="corner corner-bl" />
          <span className="corner corner-br" />
          <span className="selection-tag">1284 × 742</span>
          <span className="scanline" />
        </div>
      </div>
      <div className="visual-glow" />
    </div>
  );
}

function BusyStrip({ phase }: { phase: Phase }) {
  return (
    <div className="busy-strip" role="status" aria-live="polite">
      <span className="busy-ring busy-ring-sm" />
      <span className="busy-strip-text">
        {phase === "selecting"
          ? "拖选要识别的区域，按 Esc 取消。"
          : phase === "starting"
            ? "正在加载模型。"
            : "正在识别…"}
      </span>
    </div>
  );
}

/**
 * Read-only accumulating preview shown while the model is generating. It uses
 * the same status word as the titlebar (`正在识别`); the final result replaces
 * it and remains the only copyable text.
 */
function PartialPanel({ partial }: { partial: OcrPartial }) {
  // The model often opens with a blank line; the finished result drops it.
  const text = partial.text.replace(/^\n+/, "");
  return (
    <section className="panel partial-panel" aria-busy="true" aria-label="识别预览">
      <header className="panel-head">
        <div className="panel-heading">
          <span className="busy-ring busy-ring-sm" aria-hidden="true" />
          <span className="eyebrow">
            {partial.preview ? "快速预览" : "正在识别"}
          </span>
          <span className="head-sep" />
          <span className="muted">{duration(partial.ms)}</span>
        </div>
        <span className="badge">预览</span>
      </header>
      <pre className="partial-text">{text}</pre>
      <footer className="partial-foot">
        {partial.preview
          ? "正在用本地模型精修，完成后会替换。"
          : "识别完成后会替换为可复制的结果。"}
      </footer>
    </section>
  );
}

function SetupBanner({ onSetup, remote }: { onSetup: () => void; remote: boolean }) {
  return (
    <section className="panel setup-banner">
      <div className="setup-banner-icon">
        <Icon name="spark" size={20} />
      </div>
      <div className="setup-banner-copy">
        <p className="eyebrow">{remote ? "远程 OCR 引擎" : "OCR 模型"}</p>
        <h2>{remote ? "先把模型部署到服务器" : "先下载 OCR 模型"}</h2>
        <p>
          {remote
            ? "部署一次，截图会发到你的服务器识别。"
            : "下载一次，之后截图都在本机处理。"}
        </p>
      </div>
      <button type="button" className="btn btn-primary" onClick={onSetup}>
        <Icon name="arrow-right" size={15} />
        {remote ? "去部署" : "下载模型"}
      </button>
    </section>
  );
}

function PermissionBanner({ onPermission }: { onPermission: () => void }) {
  return (
    <section className="panel setup-banner permission-banner">
      <div className="setup-banner-icon tone-warn-icon">
        <Icon name="alert" size={20} />
      </div>
      <div className="setup-banner-copy">
        <p className="eyebrow">屏幕录制权限</p>
        <h2>先允许录制屏幕</h2>
        <p>没有这项权限，截图会是空白。</p>
      </div>
      <button type="button" className="btn btn-primary" onClick={onPermission}>
        <Icon name="alert" size={15} />
        去授权
      </button>
    </section>
  );
}

function CaptureHero({ shortcut, remote }: { shortcut: string; remote: boolean }) {
  return (
    <section className="hero hero-compact">
      <div className="hero-copy">
        <div className="hero-state">
          <span className="status-dot" />
          {remote ? "截图发到你的服务器" : "本地处理，不上传"}
        </div>
        <h2>截图，然后粘贴。</h2>
        <p className="hero-text">
          框选代码、终端输出或报错。缩进和换行会保留。
        </p>
        <div className="hero-shortcut-line">
          <Icon name="capture" size={14} />
          按 {shortcutText(shortcut)} 框选
        </div>
      </div>
      <HeroVisual />
    </section>
  );
}

interface ErrorCopy {
  title: string;
  badge: string;
  tone: "danger" | "warn";
}

function errorCopy(error: OcrError): ErrorCopy {
  switch (error.code) {
    case "permission":
      return { title: "需要屏幕录制权限", badge: "需要授权", tone: "warn" };
    case "setup-required":
      return { title: "先下载 OCR 模型", badge: "需要设置", tone: "warn" };
    case "blank":
      return { title: "截图区域是空白的", badge: "截图失败", tone: "danger" };
    case "engine":
      return { title: "识别没有完成", badge: "OCR 引擎问题", tone: "danger" };
    default:
      return { title: "这次截图没有成功", badge: "截图失败", tone: "danger" };
  }
}

function ErrorPanel({
  error,
  remote,
  onOpenSettings,
  onCapture,
  onSetup,
  onPermission,
}: {
  error: OcrError;
  remote: boolean;
  onOpenSettings: (section?: SettingsSection) => void;
  onCapture: () => void;
  onSetup: () => void;
  onPermission: () => void;
}) {
  const copy = errorCopy(error);
  const permission = error.code === "permission";
  const setup = error.code === "setup-required";

  return (
    <section className="panel error-panel" role="alert">
      <div className="error-icon">
        <Icon name="alert" size={19} />
      </div>
      <div className="error-body">
        <div className="error-title-row">
          <h2>
            {setup && remote ? "先把模型部署到服务器" : copy.title}
          </h2>
          <span className={`badge ${copy.tone === "danger" ? "badge-danger" : "badge-warn"}`}>
            {copy.badge}
          </span>
        </div>
        <p className="error-message">{error.message}</p>
        <p className="error-hint">{error.hint}</p>
        {(permission || error.code === "blank") && isMac && (
          <p className="error-footnote">
            <Icon name="info" size={13} />
            位置：系统设置 → 隐私与安全性 → 屏幕录制。
          </p>
        )}
      </div>
      <div className="error-actions">
        {permission ? (
          <>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => onOpenSettings()}
            >
              打开设置
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={onPermission}>
              <Icon name="alert" size={14} />
              去授权
            </button>
          </>
        ) : setup ? (
          <>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => onOpenSettings("recognition")}
            >
              打开设置
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={onSetup}>
              <Icon name="arrow-right" size={14} />
              {remote ? "去部署" : "下载模型"}
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => onOpenSettings("recognition")}
            >
              检查设置
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={onCapture}>
              <Icon name="refresh" size={14} />
              重试截图
            </button>
          </>
        )}
      </div>
    </section>
  );
}

/** Outline the ink regions the model actually recognised. */
function PreviewBoxes({ result }: { result: OcrResult }) {
  if (result.previewBoxes.length === 0) return null;
  return (
    <svg
      className="preview-boxes"
      viewBox={`0 0 ${result.width} ${result.height}`}
      preserveAspectRatio="xMidYMid meet"
      aria-hidden="true"
    >
      {result.previewBoxes.map((box) => (
        <rect
          key={`${box.line}-${box.x0}-${box.y0}`}
          x={box.x0}
          y={box.y0}
          width={Math.max(1, box.x1 - box.x0)}
          height={Math.max(1, box.y1 - box.y0)}
        />
      ))}
    </svg>
  );
}

/** Enlarged preview. Backdrop click, Esc and the close button all dismiss it. */
function PreviewDialog({
  result,
  showBoxes,
  onClose,
}: {
  result: OcrResult;
  showBoxes: boolean;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const focusables = dialogRef.current?.querySelectorAll<HTMLElement>(
        "button, [href], [tabindex]:not([tabindex='-1'])",
      );
      if (!focusables || focusables.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      if (previous?.isConnected) previous.focus();
    };
  }, [onClose]);

  return (
    <div
      className="preview-backdrop"
      onMouseDown={(event) => {
        // Only a press on the backdrop itself closes: selecting text inside
        // the image and releasing over the backdrop must not dismiss it.
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="preview-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="截图预览"
        ref={dialogRef}
      >
        <div className="preview-dialog-bar">
          <span className="muted">
            {result.width} × {result.height}
          </span>
          <button
            ref={closeRef}
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={onClose}
          >
            <Icon name="close" size={14} />
            关闭
          </button>
        </div>
        <div className="preview-dialog-media">
          <img src={result.thumbnail ?? ""} alt="截图预览" draggable={false} />
          {showBoxes && <PreviewBoxes result={result} />}
        </div>
      </div>
    </div>
  );
}

function ResultPanel({
  result,
  copied,
  busy,
  remote,
  showBoxes,
  onCopy,
  onOpenHistory,
}: {
  result: OcrResult;
  copied: boolean;
  busy: boolean;
  remote: boolean;
  showBoxes: boolean;
  onCopy: (text: string) => void;
  onOpenHistory: () => void;
}) {
  const [previewOpen, setPreviewOpen] = useState(false);
  const closePreview = useCallback(() => setPreviewOpen(false), []);

  return (
    <>
    <section className="panel result-panel">
      <header className="panel-head">
        <div className="panel-heading">
          <span className="eyebrow">最近截图</span>
          <span className="head-sep" />
          <span className="muted">{relativeTime(result.capturedAt)}</span>
          {result.copied || copied ? (
            <span className="copied-chip">
              <Icon name="check" size={12} />
              已复制
            </span>
          ) : (
            <span className="badge badge-warn">未复制</span>
          )}
        </div>
        <div className="panel-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={onOpenHistory}>
            历史
            <Icon name="arrow-right" size={13} />
          </button>
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={() => onCopy(result.text)}
          >
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? "已复制" : "复制"}
          </button>
        </div>
      </header>

      <div className="result-body">
        {result.thumbnail ? (
          <button
            type="button"
            className="result-thumb-wrap"
            onClick={() => setPreviewOpen(true)}
            aria-label="放大查看截图预览"
            title="放大查看"
          >
            <img
              className="result-thumb"
              src={result.thumbnail}
              alt="截图预览"
              draggable={false}
            />
            {showBoxes && <PreviewBoxes result={result} />}
          </button>
        ) : (
          <div className="result-thumb result-thumb-empty" aria-hidden="true">
            <Icon name="image" size={22} />
            <span>
              {result.width} × {result.height}
            </span>
          </div>
        )}
        <pre className="result-text" tabIndex={0}>
          {result.text}
        </pre>
      </div>

      {result.truncated && (
        <div className="setup-note" role="status" aria-live="polite">
          <Icon name="alert" size={13} />
          <span>识别结果可能不完整。</span>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => {
              // Failures of the re-run arrive on `ocr:error`, like a capture.
              void retryCapture().catch(() => undefined);
            }}
            disabled={busy}
          >
            <Icon name="refresh" size={13} />
            {busy ? "正在重新识别…" : "重新识别（分块）"}
          </button>
        </div>
      )}

      <footer className="result-meta">
        <MetaChip>
          <span className="meta-dot tone-ok" />
          {engineLabel(result.engine, remote)}
        </MetaChip>
        <MetaChip>{formatLabel(result.format)}</MetaChip>
        <MetaChip>{result.lines} 行</MetaChip>
        <MetaChip>{duration(result.ms)}</MetaChip>
        {result.language && <MetaChip>{result.language}</MetaChip>}
        <MetaChip>
          {result.width} × {result.height}
        </MetaChip>
        <span className="meta-spacer" />
        <span className="meta-note">
          <Icon name="clipboard" size={13} />
          回编辑器按 {isMac ? "⌘V" : "Ctrl+V"}
        </span>
      </footer>
    </section>

    {previewOpen && result.thumbnail && (
      <PreviewDialog
        result={result}
        showBoxes={showBoxes}
        onClose={closePreview}
      />
    )}
    </>
  );
}

function FirstCaptureTip({
  copied,
  shortcut,
  onDismiss,
}: {
  copied: boolean;
  shortcut: string;
  onDismiss: () => void;
}) {
  return (
    <section className="first-capture-tip" role="status">
      <span className="first-capture-icon">
        <Icon name="check" size={16} />
      </span>
      <div>
        <strong>{copied ? "已复制，可以粘贴了。" : "识别完成，点复制后粘贴。"}</strong>
        <p>
          回到编辑器按 {isMac ? "⌘V" : "Ctrl+V"}。关闭窗口不会退出，
          {shortcutText(shortcut)} 随时可以再截。
        </p>
      </div>
      <button type="button" className="btn btn-ghost btn-sm" onClick={onDismiss}>
        知道了
      </button>
    </section>
  );
}

function StepsPanel({ shortcut, remote }: { shortcut: string; remote: boolean }) {
  return (
    <section className="panel steps-panel" aria-label="截图流程">
      <div className="step">
        <span className="step-number">01</span>
        <div>
          <h3>框选</h3>
          <p>
            按 <span className="inline-shortcut">{shortcutText(shortcut)}</span> 拖选代码。
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">02</span>
        <div>
          <h3>识别</h3>
          <p>
            {remote ? "服务器上的模型" : "本地模型"}读取文字，缩进和换行会保留。
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">03</span>
        <div>
          <h3>粘贴</h3>
          <p>结果已在剪贴板。回到编辑器按 {isMac ? "⌘V" : "Ctrl+V"}。</p>
        </div>
      </div>
    </section>
  );
}

export function CaptureView({
  phase,
  partial,
  shortcut,
  result,
  error,
  copied,
  showBoxes,
  setupBlocking,
  remote,
  permissionDenied,
  showSteps,
  firstCaptureTip,
  onCapture,
  onClipboard,
  onPickImage,
  onCopy,
  onOpenHistory,
  onOpenSettings,
  onSetup,
  onPermission,
  onDismissTip,
}: CaptureViewProps) {
  const fileInput = useRef<HTMLInputElement>(null);
  const busy = phase !== "idle";
  const showResult = !error && Boolean(result);
  const blocking = setupBlocking || permissionDenied;
  const showBlockingBanner = !error && blocking;
  const streaming = partial !== null && phase === "recognizing";
  const headerLabel = busy ? "处理中…" : "截图";

  return (
    <div className="view capture-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">工作区</p>
            <h1>截图</h1>
            <p className="lede">
              截代码、终端输出或报错。也可以把图片文件拖进窗口，结果直接进剪贴板。
            </p>
          </div>
          {!error &&
            (blocking ? (
              <button
                type="button"
                className="btn btn-primary"
                onClick={setupBlocking ? onSetup : onPermission}
              >
                <Icon name={setupBlocking ? "arrow-right" : "alert"} size={15} />
                {setupBlocking ? (remote ? "去部署" : "下载模型") : "去授权"}
              </button>
            ) : (
              <div className="head-actions">
                {/* A browser drop never reveals a path, so the preview also
                    offers a picker; the desktop app uses the native drop. */}
                {isPreview && (
                  <>
                    <input
                      ref={fileInput}
                      type="file"
                      hidden
                      accept="image/png,image/jpeg,image/webp,image/bmp,image/tiff"
                      onChange={(event) => {
                        const file = event.target.files?.[0];
                        if (file) onPickImage({ name: file.name, size: file.size });
                        // Allow picking the same file twice in a row.
                        event.target.value = "";
                      }}
                    />
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => fileInput.current?.click()}
                      disabled={busy}
                    >
                      <Icon name="image" size={14} />
                      选择图片
                    </button>
                  </>
                )}
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={onClipboard}
                  disabled={busy}
                >
                  <Icon name="clipboard" size={14} />
                  从剪贴板识别
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={onCapture}
                  disabled={busy}
                >
                  <Icon name={busy ? "spark" : "capture"} size={15} />
                  {headerLabel}
                </button>
              </div>
            ))}
        </header>

        {busy && showResult && !streaming && <BusyStrip phase={phase} />}

        {streaming && partial && <PartialPanel partial={partial} />}

        {error && (
          <ErrorPanel
            error={error}
            remote={remote}
            onOpenSettings={onOpenSettings}
            onCapture={onCapture}
            onSetup={onSetup}
            onPermission={onPermission}
          />
        )}

        {showResult && result && (
          <>
            {firstCaptureTip && (
              <FirstCaptureTip
                copied={result.copied || copied}
                shortcut={shortcut}
                onDismiss={onDismissTip}
              />
            )}
            <ResultPanel
              result={result}
              copied={result.copied || copied}
              busy={busy}
              remote={remote}
              showBoxes={showBoxes}
              onCopy={onCopy}
              onOpenHistory={onOpenHistory}
            />
          </>
        )}

        {showBlockingBanner && setupBlocking && (
          <SetupBanner onSetup={onSetup} remote={remote} />
        )}
        {showBlockingBanner && !setupBlocking && permissionDenied && (
          <PermissionBanner onPermission={onPermission} />
        )}

        {!error && !result && !showBlockingBanner && !streaming && (
          <CaptureHero shortcut={shortcut} remote={remote} />
        )}

        {!error && showSteps && !streaming && (
          <StepsPanel shortcut={shortcut} remote={remote} />
        )}
      </div>
    </div>
  );
}
