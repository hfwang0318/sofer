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
          ? "Waiting for your selection — drag a region, Esc cancels."
          : phase === "starting"
            ? "Starting the local engine and loading the model…"
            : `Reading the screenshot with ${engineName}…`}
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
        <p className="eyebrow">Local OCR</p>
        <h2>One-time setup: download the vision model</h2>
        <p>
          The llama.cpp runtime is already bundled. Add GLM-OCR once (1.43 GB,
          SHA-256 verified) and every capture stays on this machine.
        </p>
      </div>
      <button type="button" className="btn btn-primary" onClick={onSetup}>
        <Icon name="arrow-right" size={15} />
        Set up local OCR
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
            ? "Clipboard ready"
            : phase === "selecting"
              ? "Selecting a region"
              : "Recognising"}
        </div>
        <h2>
          From screen to clipboard,
          <br />
          with the layout intact.
        </h2>
        <p className="hero-text">
          Press the shortcut and drag over a region. Indentation, line breaks and
          columns come back with the text — no upload, no cloud round-trip.
        </p>
        <div className="hero-actions">
          <button
            type="button"
            className="btn btn-primary"
            onClick={onCapture}
            disabled={busy}
          >
            <Icon name="spark" size={15} />
            {busy ? "Working…" : "Capture now"}
          </button>
          <span className="hero-shortcut">
            or press <Keycaps accelerator={shortcut} size="sm" />
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
                ? "Waiting for your selection"
                : "Reading the screenshot"}
            </strong>
            <span>
              {phase === "selecting"
                ? "Drag over any region. Press Esc to cancel."
                : phase === "starting"
                  ? "Loading the local model. The first run takes 10–20 seconds."
                  : `${engineName} · reconstructing indentation`}
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
          <h2>Capture didn’t come through</h2>
          <span className="badge badge-danger">engine error</span>
        </div>
        <p className="error-message">{error.message}</p>
        <p className="error-hint">{error.hint}</p>
        {isMac && (
          <p className="error-footnote">
            <Icon name="info" size={13} />
            Screen Recording permission is required for the native selection.
          </p>
        )}
      </div>
      <div className="error-actions">
        <button type="button" className="btn btn-secondary btn-sm" onClick={onOpenSettings}>
          Open settings
        </button>
        {needsSetup ? (
          <button type="button" className="btn btn-primary btn-sm" onClick={onSetup}>
            <Icon name="arrow-right" size={14} />
            Set up engine
          </button>
        ) : (
          <button type="button" className="btn btn-primary btn-sm" onClick={onCapture}>
            <Icon name="refresh" size={14} />
            Try again
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
          <span className="eyebrow">Last capture</span>
          <span className="head-sep" />
          <span className="muted">{relativeTime(result.capturedAt)}</span>
          {result.copied || copied ? (
            <span className="copied-chip">
              <Icon name="check" size={12} />
              Copied
            </span>
          ) : (
            <span className="badge badge-warn">not copied</span>
          )}
        </div>
        <div className="panel-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={onOpenHistory}>
            History
            <Icon name="arrow-right" size={13} />
          </button>
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={() => onCopy(result.text)}
          >
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? "Copied" : "Copy text"}
          </button>
        </div>
      </header>

      <div className="result-body">
        {result.thumbnail ? (
          <img
            className="result-thumb"
            src={result.thumbnail}
            alt="Screenshot sent to the OCR engine"
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
        <MetaChip>{result.format}</MetaChip>
        <MetaChip>{result.lines} lines</MetaChip>
        <MetaChip>{duration(result.ms)}</MetaChip>
        {result.language && <MetaChip>{result.language}</MetaChip>}
        <MetaChip>
          {result.width} × {result.height}
        </MetaChip>
        <span className="meta-spacer" />
        <span className="meta-note">
          <Icon name="clipboard" size={13} />
          Paste anywhere with {isMac ? "⌘V" : "Ctrl+V"}
        </span>
      </footer>
    </section>
  );
}

function StepsPanel({ shortcut, engineName }: { shortcut: string; engineName: string }) {
  return (
    <section className="panel steps-panel" aria-label="How a capture works">
      <div className="step">
        <span className="step-number">01</span>
        <div>
          <h3>Select</h3>
          <p>
            Hit <span className="inline-shortcut">{shortcutText(shortcut)}</span> and
            drag over the region. Native selection, every display.
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">02</span>
        <div>
          <h3>Recognise</h3>
          <p>
            {engineName === "llama.cpp"
              ? "Your local vision model reads the text."
              : "The configured engine reads the text."}{" "}
            Geometry restores the layout.
          </p>
        </div>
      </div>
      <div className="step">
        <span className="step-number">03</span>
        <div>
          <h3>Paste</h3>
          <p>
            The result is already on the clipboard. Switch back and paste — no
            intermediate window.
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
            <p className="eyebrow">Workspace</p>
            <h1>Capture</h1>
            <p className="lede">
              Screenshot code, terminals or stack traces. The layout is rebuilt from
              pixels and the text lands on your clipboard.
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={setupBlocking ? onSetup : onCapture}
            disabled={busy && !setupBlocking}
          >
            <Icon name={setupBlocking ? "spark" : "capture"} size={15} />
            {setupBlocking ? "Set up engine" : busy ? "Working…" : "Capture region"}
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
