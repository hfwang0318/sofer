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
        aria-label="Hide ocr4code"
        title="Hide"
        onClick={() => void windowAction("hide")}
      />
      <button
        type="button"
        className="traffic traffic-min"
        aria-label="Minimize"
        title="Minimize"
        onClick={() => void windowAction("minimize")}
      />
      <button
        type="button"
        className="traffic traffic-max"
        aria-label="Zoom"
        title="Zoom"
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
        aria-label="Minimize"
        title="Minimize"
        onClick={() => void windowAction("minimize")}
      >
        <Icon name="minus" size={15} />
      </button>
      <button
        type="button"
        className="window-control"
        aria-label="Maximize"
        title="Maximize"
        onClick={() => void windowAction("toggle-maximize")}
      >
        <Icon name="maximize" size={13} />
      </button>
      <button
        type="button"
        className="window-control window-control-close"
        aria-label="Hide to menu bar"
        title="Hide to menu bar"
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
    ? { label: "Setup needed", tone: "warn" as const }
    : starting
      ? { label: "Starting engine", tone: "ok" as const }
      : offline
        ? { label: "Engine offline", tone: "danger" as const }
        : idle;

  return (
    <header className="titlebar">
      <div className="titlebar-drag" data-tauri-drag-region aria-hidden="true" />
      {isMac && <TrafficLights />}
      <div className="brand">
        <Logo size={23} />
        <span className="brand-name">ocr4code</span>
        <span className="brand-version">0.1</span>
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
        title="Change the global shortcut"
        aria-label={`Global shortcut ${shortcutText(shortcut)} — change in settings`}
      >
        <Icon name="keyboard" size={14} />
        <Keycaps accelerator={shortcut} size="sm" />
      </button>

      {!isMac && <WindowControls />}
    </header>
  );
}
