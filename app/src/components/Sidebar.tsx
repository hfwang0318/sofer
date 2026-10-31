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
  { view: "capture" as const, label: "截图", icon: "capture" as const, hint: "1" },
  { view: "history" as const, label: "历史", icon: "history" as const, hint: "2" },
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
      <nav className="sidebar-nav" aria-label="主导航">
        <p className="nav-label">工作区</p>
        {NAV.map((item) => (
          <button
            key={item.view}
            type="button"
            className={`nav-item ${view === item.view ? "is-active" : ""}`}
            aria-current={view === item.view ? "page" : undefined}
            title={`${item.label}（${isMac ? "⌘" : "Ctrl"}+${item.hint}）`}
            onClick={() => onNavigate(item.view)}
          >
            <Icon name={item.icon} size={17} />
            <span className="nav-label-text">{item.label}</span>
            {item.view === "history" && historyCount > 0 && (
              <span className="nav-count">{historyCount}</span>
            )}
          </button>
        ))}

        <p className="nav-label nav-label-spaced">偏好设置</p>
        <button
          type="button"
          className={`nav-item ${view === "settings" ? "is-active" : ""}`}
          aria-current={view === "settings" ? "page" : undefined}
          title="设置（⌘,）"
          onClick={() => onNavigate("settings")}
        >
          <Icon name="settings" size={17} />
          <span className="nav-label-text">设置</span>
        </button>
      </nav>

      <div className="sidebar-spacer" />

      <button
        type="button"
        className="quick-capture"
        onClick={onCapture}
        aria-label={`截取区域（${shortcutText(shortcut)}）`}
      >
        <span className="quick-capture-head">
          <span className="quick-capture-title">快速截图</span>
          <span className="quick-capture-icon">
            <Icon name="capture" size={14} />
          </span>
        </span>
        <Keycaps accelerator={shortcut} size="sm" />
        <span className="quick-capture-hint">截图 → 剪贴板</span>
      </button>

      <button
        type="button"
        className="engine-card"
        onClick={() => onNavigate("settings")}
        title="引擎设置"
      >
        <span className={`status-dot tone-${engine.tone}`} />
        <span className="engine-copy">
          <span className="engine-label">{engine.label}</span>
          <span className="engine-detail">{engine.detail}</span>
        </span>
        <Icon name="chevron-down" size={13} className="engine-chevron" />
      </button>

      <p className="sidebar-footnote">v0.1.1 · 完全离线</p>
    </aside>
  );
}
