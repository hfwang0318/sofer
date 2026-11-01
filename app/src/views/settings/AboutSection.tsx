import { Icon } from "../../components/Icon";
import { Keycaps, SettingRow, Switch } from "../../components/Controls";
import { isMac } from "../../lib/platform";
import type { EngineSummary } from "../../lib/status";
import type { AvailableUpdate, UpdatePhase } from "../../lib/updater";
import type { Settings } from "../../lib/types";
import { Section, type UpdateSetting } from "./shared";

interface AboutSectionProps {
  draft: Settings;
  update: UpdateSetting;
  engine: EngineSummary;
  appVersion: string;
  updateInfo: AvailableUpdate | null;
  updatePhase: UpdatePhase;
  updateProgress: number;
  updateError: string | null;
  onCheckUpdates: () => void;
  onDownloadUpdate: () => void;
  onInstallUpdate: () => void;
  onResetOnboarding: () => void;
  onQuit: () => void;
}

function updateDescription(
  updatePhase: UpdatePhase,
  updateInfo: AvailableUpdate | null,
): string {
  if (updatePhase === "checking") return "正在检查更新…";
  if (updatePhase === "current") return "已是最新版本。";
  if (updatePhase === "available" && updateInfo) {
    return `有新版 v${updateInfo.version} 可以下载。`;
  }
  if (updatePhase === "downloading" && updateInfo) {
    return `正在下载 v${updateInfo.version}…`;
  }
  if (updatePhase === "ready" && updateInfo) {
    return `v${updateInfo.version} 已下载，重启后安装。`;
  }
  if (updatePhase === "installing") return "正在启动安装程序…";
  if (updatePhase === "error") return "更新失败，请重试。";
  return "尚未检查。";
}

/** Version, privacy, update controls, onboarding reset and quit. */
export function AboutSection({
  draft,
  update,
  engine,
  appVersion,
  updateInfo,
  updatePhase,
  updateProgress,
  updateError,
  onCheckUpdates,
  onDownloadUpdate,
  onInstallUpdate,
  onResetOnboarding,
  onQuit,
}: AboutSectionProps) {
  const description = updateDescription(updatePhase, updateInfo);

  return (
    <Section
      id="about"
      title="关于"
      description="版本、更新和退出。"
      meta={<span className="about-version">v{appVersion || "—"}</span>}
    >
      <div className="about-grid">
        <div>
          <span className="about-label">OCR 引擎</span>
          <span className={`about-value tone-${engine.tone}`}>
            <span className="status-dot" />
            {engine.label}
          </span>
        </div>
        <div>
          <span className="about-label">隐私</span>
          <span className="about-value">
            {draft.remoteEnabled && draft.engine !== "tesseract"
              ? "截图发到你的服务器识别"
              : "本地处理，不上传"}
          </span>
        </div>
        <div>
          <span className="about-label">截图</span>
          <span className="about-value">
            {isMac ? "macOS 系统选区" : "Windows 自绘选区"}
          </span>
        </div>
        <div>
          <span className="about-label">快捷键</span>
          <span className="about-value">
            <Keycaps accelerator={draft.shortcut} size="sm" />
          </span>
        </div>
      </div>

      <div className="setting-group-label">更新</div>
      <SettingRow
        title="启动时检查更新"
        description="后台检查新版本，不打断截图。"
      >
        <Switch
          checked={draft.autoCheckUpdates}
          onChange={(autoCheckUpdates) => update("autoCheckUpdates", autoCheckUpdates)}
          label="启动时检查更新"
        />
      </SettingRow>
      <SettingRow
        title="发现有更新时自动下载"
        description="安装前仍会等你确认重启。"
      >
        <Switch
          checked={draft.autoDownloadUpdates}
          onChange={(autoDownloadUpdates) =>
            update("autoDownloadUpdates", autoDownloadUpdates)
          }
          label="发现有更新时自动下载"
        />
      </SettingRow>
      <SettingRow title="检查更新" description={description}>
        <div className="update-actions">
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={onCheckUpdates}
            disabled={
              updatePhase === "checking" ||
              updatePhase === "downloading" ||
              updatePhase === "installing" ||
              updatePhase === "ready"
            }
          >
            {updatePhase === "checking" ? "检查中…" : "检查更新"}
          </button>
          {updatePhase === "available" && (
            <button type="button" className="btn btn-primary btn-sm" onClick={onDownloadUpdate}>
              下载更新
            </button>
          )}
          {updatePhase === "ready" && (
            <button type="button" className="btn btn-primary btn-sm" onClick={onInstallUpdate}>
              重启并安装
            </button>
          )}
        </div>
      </SettingRow>
      {updatePhase === "downloading" && (
        <div className="update-progress" role="status">
          <div className="update-bar">
            <span style={{ width: `${Math.max(0, Math.min(100, updateProgress))}%` }} />
          </div>
          <span>{Math.round(updateProgress)}%</span>
        </div>
      )}
      {updatePhase === "error" && updateError && (
        <p className="field-error update-error">{updateError}</p>
      )}

      <div className="about-foot">
        <div className="about-actions">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={onResetOnboarding}
          >
            <Icon name="refresh" size={14} />
            重新显示引导
          </button>
          <button type="button" className="btn btn-ghost btn-danger btn-sm" onClick={onQuit}>
            <Icon name="power" size={14} />
            退出 sofer
          </button>
        </div>
        <p>
          {isMac
            ? "截图需要屏幕录制权限。授权后如果状态没变，请重启应用。"
            : "Windows 选区由应用自己绘制，不调用系统截屏工具。"}
        </p>
      </div>
    </Section>
  );
}
