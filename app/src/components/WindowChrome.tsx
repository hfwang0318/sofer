import { Icon } from "./Icon";
import { Keycaps } from "./Controls";
import { Logo } from "./Logo";
import { isMac, shortcutText } from "../lib/platform";
import type { Readiness } from "../lib/status";
import { windowAction } from "../lib/bridge";

interface WindowChromeProps {
  shortcut: string;
  readiness: Readiness;
  version: string;
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
        aria-label="最小化到托盘"
        title="最小化到托盘"
        onClick={() => void windowAction("hide")}
      >
        <Icon name="close" size={15} />
      </button>
    </div>
  );
}

export function WindowChrome({
  shortcut,
  readiness,
  version,
  onOpenSettings,
}: WindowChromeProps) {
  return (
    <header className="titlebar">
      <div className="titlebar-drag" data-tauri-drag-region aria-hidden="true" />
      {isMac && <TrafficLights />}
      <div className="brand">
        <Logo size={23} />
        <span className="brand-name">ocr4code</span>
        <span className="brand-version">{version || "—"}</span>
      </div>

      <div className="titlebar-spacer" />

      <div className={`phase-pill tone-${readiness.tone}`} aria-live="polite">
        <span className="status-dot" />
        <span>{readiness.label}</span>
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
