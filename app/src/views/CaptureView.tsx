import type { ReactNode } from "react";

import { Icon } from "../components/Icon";
import { Keycaps } from "../components/Controls";
import { duration, relativeTime } from "../lib/history";
import { isMac, shortcutText } from "../lib/platform";
import type { OcrError, OcrResult, Phase } from "../lib/types";

interface CaptureViewProps {
  phase: Phase;
  shortcut: string;
  result: OcrResult | null;
  error: OcrError | null;
  copied: boolean;
  engineName: string;
  onCapture: () => void;
  onCopy: (text: string) => void;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
  onSetup: () => void;
  setupNeeded: boolean;
  setupBlocking: boolean;
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

function BusyStrip({ phase, engineName }: { phase: Phase; engineName: string }) {
  return (
    <div className="busy-strip" role="status" aria-live="polite">
      <span className="busy-ring busy-ring-sm" />
      <span className="busy-strip-text">
        {phase === "selecting"
          ? "等待选择区域——拖选一块区域，按 Esc 取消。"
          : phase === "starting"
            ? "正在启动本地引擎并加载模型…"
            : `正在用 ${engineName} 识别截图…`}
      </span>
    </div>
  );
}

function SetupBanner({ onSetup }: { onSetup: () => void }) {
  return (
    <section className="panel setup-banner">
      <div className="setup-banner-icon">
        <Icon name="spark" size={20} />
      </div>
      <div className="setup-banner-copy">
        <p className="eyebrow">本地 OCR</p>
        <h2>首次设置：下载视觉模型</h2>
        <p>
          llama.cpp 运行时已内置。下载一次 GLM-OCR（高精度约 1.4 GB，低精度约 1.0 GB，SHA-256 校验），之后完全离线。
        </p>
      </div>
      <button type="button" className="btn btn-primary" onClick={onSetup}>
        <Icon name="arrow-right" size={15} />
        配置本地 OCR
      </button>
    </section>
  );
}

function CaptureHero({
  phase,
  shortcut,
  engineName,
  onCapture,
}: {
  phase: Phase;
  shortcut: string;
  engineName: string;
  onCapture: () => void;
}) {
  const busy = phase !== "idle";
  return (
    <section className={`hero ${busy ? "is-busy" : ""}`}>
      <div className="hero-copy">
        <div className="hero-state">
          <span className={`status-dot ${busy ? "is-pulsing" : ""}`} />
          {phase === "idle"
            ? "剪贴板就绪"
            : phase === "selecting"
              ? "正在选择区域"
              : "正在识别"}
        </div>
        <h2>
          从屏幕到剪贴板，
          <br />
          保留原始排版。
        </h2>
        <p className="hero-text">
          按下快捷键并拖选区域。缩进、换行和列对齐都会保留，完全在本机处理。
        </p>
        <div className="hero-actions">
          <button
            type="button"
            className="btn btn-primary"
            onClick={onCapture}
            disabled={busy}
          >
            <Icon name="spark" size={15} />
            {busy ? "处理中…" : "立即截图"}
          </button>
          <span className="hero-shortcut">
            或按下 <Keycaps accelerator={shortcut} size="sm" />
          </span>
        </div>
      </div>

      <HeroVisual />

      {busy && (
        <div className="busy-veil" role="status" aria-live="polite">
          <span className="busy-ring" />
          <div className="busy-copy">
            <strong>
              {phase === "selecting"
                ? "等待选择区域"
                : "正在识别截图"}
            </strong>
            <span>
              {phase === "selecting"
                ? "拖选任意区域，按 Esc 取消。"
                : phase === "starting"
                  ? "正在加载本地模型；首次启动约需 10–20 秒。"
                  : `${engineName} · 正在重建缩进`}
            </span>
          </div>
        </div>
      )}
    </section>
  );
}

function ErrorPanel({
  error,
  onOpenSettings,
  onCapture,
  onSetup,
}: {
  error: OcrError;
  onOpenSettings: () => void;
  onCapture: () => void;
  onSetup: () => void;
}) {
  const needsSetup = error.code === "setup-required";
  return (
    <section className="panel error-panel" role="alert">
      <div className="error-icon">
        <Icon name="alert" size={19} />
      </div>
      <div className="error-body">
        <div className="error-title-row">
          <h2>截图未能完成</h2>
          <span className="badge badge-danger">引擎错误</span>
        </div>
        <p className="error-message">{error.message}</p>
        <p className="error-hint">{error.hint}</p>
        {isMac && (
          <p className="error-footnote">
            <Icon name="info" size={13} />
            原生选区需要屏幕录制权限。
          </p>
        )}
      </div>
      <div className="error-actions">
        <button type="button" className="btn btn-secondary btn-sm" onClick={onOpenSettings}>
          打开设置
        </button>
        {needsSetup ? (
          <button type="button" className="btn btn-primary btn-sm" onClick={onSetup}>
            <Icon name="arrow-right" size={14} />
            配置引擎
          </button>
        ) : (
          <button type="button" className="btn btn-primary btn-sm" onClick={onCapture}>
            <Icon name="refresh" size={14} />
            重试
          </button>
        )}
      </div>
    </section>
  );
}

function ResultPanel({
  result,
  copied,
  onCopy,
  onOpenHistory,
}: {
  result: OcrResult;
  copied: boolean;
  onCopy: (text: string) => void;
  onOpenHistory: () => void;
}) {
  return (
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
            {copied ? "已复制" : "复制文本"}
          </button>
        </div>
      </header>

      <div className="result-body">
        {result.thumbnail ? (
          <img
            className="result-thumb"
            src={result.thumbnail}
            alt="发送给 OCR 引擎的截图"
          />
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

      <footer className="result-meta">
        <MetaChip>
          <span className="meta-dot tone-ok" />
          {engineLabel(result.engine)}
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
          粘贴到任意位置：{isMac ? "⌘V" : "Ctrl+V"}
        </span>
      </footer>
    </section>
  );
}

function StepsPanel({ shortcut, engineName }: { shortcut: string; engineName: string }) {
  return (
    <section className="panel steps-panel" aria-label="截图流程">
      <div className="step">
        <span className="step-number">01</span>
        <div>
          <h3>选择</h3>
          <p>
            按 <span className="inline-shortcut">{shortcutText(shortcut)}</span> 并拖选区域。系统原生选择，支持所有显示器。
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">02</span>
        <div>
          <h3>识别</h3>
          <p>
            {engineName === "llama.cpp"
              ? "本地视觉模型读取文字。"
              : "当前引擎读取文字。"}{" "}
            几何层恢复原有排版。
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">03</span>
        <div>
          <h3>粘贴</h3>
          <p>
            结果已经在剪贴板中。切回原应用直接粘贴，不需要中转窗口。
          </p>
        </div>
      </div>
    </section>
  );
}

export function CaptureView({
  phase,
  shortcut,
  result,
  error,
  copied,
  engineName,
  onCapture,
  onCopy,
  onOpenHistory,
  onOpenSettings,
  onSetup,
  setupNeeded,
  setupBlocking,
}: CaptureViewProps) {
  const busy = phase !== "idle";
  const showResult = !error && Boolean(result);

  return (
    <div className="view capture-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">工作区</p>
            <h1>截图</h1>
            <p className="lede">
              截取代码、终端或报错堆栈。文字会从像素中重建版面，然后直接放进剪贴板。
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={setupBlocking ? onSetup : onCapture}
            disabled={busy && !setupBlocking}
          >
            <Icon name={setupBlocking ? "spark" : "capture"} size={15} />
            {setupBlocking ? "配置引擎" : busy ? "处理中…" : "截取区域"}
          </button>
        </header>

        {busy && showResult && <BusyStrip phase={phase} engineName={engineName} />}

        {error && (
          <ErrorPanel
            error={error}
            onOpenSettings={onOpenSettings}
            onCapture={onCapture}
            onSetup={onSetup}
          />
        )}

        {showResult && result && (
          <ResultPanel
            result={result}
            copied={copied}
            onCopy={onCopy}
            onOpenHistory={onOpenHistory}
          />
        )}

        {!error && !result && setupNeeded && <SetupBanner onSetup={onSetup} />}

        {!error && !result && (!setupNeeded || !setupBlocking) && (
          <CaptureHero
            phase={phase}
            shortcut={shortcut}
            engineName={engineName}
            onCapture={onCapture}
          />
        )}

        {!error && <StepsPanel shortcut={shortcut} engineName={engineName} />}
      </div>
    </div>
  );
}
