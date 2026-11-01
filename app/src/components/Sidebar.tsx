import { Icon } from "./Icon";
import { Keycaps } from "./Controls";
import { engineSummary } from "../lib/status";
import { formatBytes } from "../lib/format";
import { isMac } from "../lib/platform";
import type {
  DownloadProgress,
  EngineMode,
  EngineStatus,
  SettingsSection,
  View,
} from "../lib/types";

interface SidebarProps {
  view: View;
  shortcut: string;
  status: EngineStatus | null;
  engineMode: EngineMode;
  download: DownloadProgress | null;
  historyCount: number;
  settingsSection: SettingsSection;
  version: string;
  onNavigate: (view: View) => void;
  onSettingsSection: (section: SettingsSection) => void;
}

const NAV = [
  { view: "capture" as const, label: "截图", icon: "capture" as const, hint: "1" },
  { view: "history" as const, label: "历史", icon: "history" as const, hint: "2" },
];

const SETTINGS_NAV: Array<{ id: SettingsSection; label: string }> = [
  { id: "general", label: "通用" },
  { id: "recognition", label: "OCR" },
  { id: "output", label: "输出" },
  { id: "advanced", label: "高级" },
  { id: "about", label: "关于" },
];

function downloadSummary(download: DownloadProgress): {
  label: string;
  detail: string;
  tone: "ok" | "warn";
} {
  const percent =
    download.total > 0
      ? Math.round((download.downloaded / download.total) * 100)
      : 0;
  if (download.phase === "starting") {
    return { label: "正在加载模型", detail: "通常 10–20 秒", tone: "ok" };
  }
  if (download.phase === "verifying") {
    return { label: "正在校验文件", detail: "下载已完成", tone: "ok" };
  }
  return {
    label: "正在下载模型",
    detail: `${percent}% · ${formatBytes(download.downloaded)} / ${formatBytes(download.total)}`,
    tone: "ok",
  };
}

export function Sidebar({
  view,
  shortcut,
  status,
  engineMode,
  download,
  historyCount,
  settingsSection,
  version,
  onNavigate,
  onSettingsSection,
}: SidebarProps) {
  const downloading = Boolean(
    download && ["downloading", "verifying", "starting"].includes(download.phase),
  );
  const engine =
    downloading && download
      ? downloadSummary(download)
      : engineMode === "tesseract"
        ? status?.tesseract
          ? { label: "tesseract 已就绪", detail: "tesseract · 不需要模型", tone: "ok" as const }
          : { label: "tesseract 不可用", detail: "请安装 tesseract", tone: "danger" as const }
        : engineSummary(status);

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
            {item.view === "capture" ? (
              <span className="nav-shortcut">
                <Keycaps accelerator={shortcut} size="sm" />
              </span>
            ) : historyCount > 0 ? (
              <span className="nav-count">{historyCount}</span>
            ) : null}
          </button>
        ))}

        <p className="nav-label nav-label-spaced">设置</p>
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
        {view === "settings" && (
          <div className="settings-subnav" role="group" aria-label="设置分类">
            {SETTINGS_NAV.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`subnav-item ${settingsSection === item.id ? "is-active" : ""}`}
                aria-current={settingsSection === item.id ? "true" : undefined}
                onClick={() => onSettingsSection(item.id)}
              >
                <span className="subnav-dot" aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        )}
      </nav>

      <div className="sidebar-spacer" />

      <button
        type="button"
        className="engine-card"
        onClick={() => onSettingsSection("recognition")}
        title="OCR 设置"
      >
        <span className={`status-dot tone-${engine.tone}`} />
        <span className="engine-copy">
          <span className="engine-label">{engine.label}</span>
          <span className="engine-detail">{engine.detail}</span>
        </span>
        <Icon name="arrow-right" size={13} className="engine-chevron" />
      </button>

      <p className="sidebar-footnote">{version || "—"} · 本地处理</p>
    </aside>
  );
}
