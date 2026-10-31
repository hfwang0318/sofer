import { Icon } from "./Icon";
import { Keycaps } from "./Controls";
import { engineSummary } from "../lib/status";
import { isMac, shortcutText } from "../lib/platform";
import type { EngineStatus, View } from "../lib/types";

interface SidebarProps {
  view: View;
  shortcut: string;
  status: EngineStatus | null;
  historyCount: number;
  onNavigate: (view: View) => void;
  onCapture: () => void;
}

const NAV = [
  { view: "capture" as const, label: "Capture", icon: "capture" as const, hint: "1" },
  { view: "history" as const, label: "History", icon: "history" as const, hint: "2" },
];

export function Sidebar({
  view,
  shortcut,
  status,
  historyCount,
  onNavigate,
  onCapture,
}: SidebarProps) {
  const engine = engineSummary(status);

  return (
    <aside className="sidebar">
      <nav className="sidebar-nav" aria-label="Primary">
        <p className="nav-label">Workspace</p>
        {NAV.map((item) => (
          <button
            key={item.view}
            type="button"
            className={`nav-item ${view === item.view ? "is-active" : ""}`}
            aria-current={view === item.view ? "page" : undefined}
            title={`${item.label} (${isMac ? "⌘" : "Ctrl"}+${item.hint})`}
            onClick={() => onNavigate(item.view)}
          >
            <Icon name={item.icon} size={17} />
            <span className="nav-label-text">{item.label}</span>
            {item.view === "history" && historyCount > 0 && (
              <span className="nav-count">{historyCount}</span>
            )}
          </button>
        ))}

        <p className="nav-label nav-label-spaced">Preferences</p>
        <button
          type="button"
          className={`nav-item ${view === "settings" ? "is-active" : ""}`}
          aria-current={view === "settings" ? "page" : undefined}
          title="Settings (⌘,)"
          onClick={() => onNavigate("settings")}
        >
          <Icon name="settings" size={17} />
          <span className="nav-label-text">Settings</span>
        </button>
      </nav>

      <div className="sidebar-spacer" />

      <button
        type="button"
        className="quick-capture"
        onClick={onCapture}
        aria-label={`Capture a region (${shortcutText(shortcut)})`}
      >
        <span className="quick-capture-head">
          <span className="quick-capture-title">Quick capture</span>
          <span className="quick-capture-icon">
            <Icon name="capture" size={14} />
          </span>
        </span>
        <Keycaps accelerator={shortcut} size="sm" />
        <span className="quick-capture-hint">Screenshot → clipboard</span>
      </button>

      <button
        type="button"
        className="engine-card"
        onClick={() => onNavigate("settings")}
        title="Engine settings"
      >
        <span className={`status-dot tone-${engine.tone}`} />
        <span className="engine-copy">
          <span className="engine-label">{engine.label}</span>
          <span className="engine-detail">{engine.detail}</span>
        </span>
        <Icon name="chevron-down" size={13} className="engine-chevron" />
      </button>

      <p className="sidebar-footnote">v0.1.0 · runs offline</p>
    </aside>
  );
}
