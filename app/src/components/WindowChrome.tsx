import { Icon } from "./Icon";
import { Keycaps } from "./Controls";
import { Logo } from "./Logo";
import { isMac, shortcutText } from "../lib/platform";
import { phaseLabel } from "../lib/status";
import type { EngineStatus, Phase } from "../lib/types";
import { windowAction } from "../lib/bridge";

interface WindowChromeProps {
  phase: Phase;
  shortcut: string;
  status: EngineStatus | null;
  shortcutActive: boolean;
  setupNeeded: boolean;
  onOpenSettings: () => void;
}

function TrafficLights() {
  return (
    <div className="traffic-lights">
      <button
        type="button"
        className="traffic traffic-close"
        aria-label="隐藏 ocr4code"
        title="隐藏"
        onClick={() => void windowAction("hide")}
      />
      <button
        type="button"
        className="traffic traffic-min"
        aria-label="最小化"
        title="最小化"
        onClick={() => void windowAction("minimize")}
      />
      <button
        type="button"
        className="traffic traffic-max"
        aria-label="缩放"
        title="缩放"
        onClick={() => void windowAction("toggle-maximize")}
      />
    </div>
  );
}

function WindowControls() {
  return (
    <div className="window-controls">
      <button
        type="button"
        className="window-control"
        aria-label="最小化"
        title="最小化"
        onClick={() => void windowAction("minimize")}
      >
        <Icon name="minus" size={15} />
      </button>
      <button
        type="button"
        className="window-control"
        aria-label="最大化"
        title="最大化"
        onClick={() => void windowAction("toggle-maximize")}
      >
        <Icon name="maximize" size={13} />
      </button>
      <button
        type="button"
        className="window-control window-control-close"
        aria-label="隐藏到菜单栏"
        title="隐藏到菜单栏"
        onClick={() => void windowAction("hide")}
      >
        <Icon name="close" size={15} />
      </button>
    </div>
  );
}

export function WindowChrome({
  phase,
  shortcut,
  status: engine,
  shortcutActive,
  setupNeeded: setupRequired,
  onOpenSettings,
}: WindowChromeProps) {
  const idle = phaseLabel(phase, shortcutActive);
  const setupNeeded =
    phase === "idle" && shortcutActive && setupRequired;
  const starting =
    phase === "idle" && engine?.server.state === "starting" && !setupNeeded;
  const offline =
    phase === "idle" &&
    shortcutActive &&
    engine &&
    !engine.llama &&
    !engine.tesseract &&
    !setupNeeded;
  const status = setupNeeded
    ? { label: "需要设置", tone: "warn" as const }
    : starting
      ? { label: "正在启动引擎", tone: "ok" as const }
      : offline
        ? { label: "引擎离线", tone: "danger" as const }
        : idle;

  return (
    <header className="titlebar">
      <div className="titlebar-drag" data-tauri-drag-region aria-hidden="true" />
      {isMac && <TrafficLights />}
      <div className="brand">
        <Logo size={23} />
        <span className="brand-name">ocr4code</span>
        <span className="brand-version">0.1.6</span>
      </div>

      <div className="titlebar-spacer" />

      <div className={`phase-pill tone-${status.tone}`} aria-live="polite">
        <span className="status-dot" />
        <span>{status.label}</span>
      </div>

      <button
        type="button"
        className="shortcut-pill"
        onClick={onOpenSettings}
        title="修改全局快捷键"
        aria-label={`全局快捷键 ${shortcutText(shortcut)}，点击修改`}
      >
        <Icon name="keyboard" size={14} />
        <Keycaps accelerator={shortcut} size="sm" />
      </button>

      {!isMac && <WindowControls />}
    </header>
  );
}
