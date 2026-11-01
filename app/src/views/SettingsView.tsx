import { Icon } from "../components/Icon";
import { engineForMode } from "../lib/status";
import type { AvailableUpdate, UpdatePhase } from "../lib/updater";
import type {
  DownloadProgress,
  EngineBusy,
  EngineStatus,
  ScreenAccess,
  Settings,
  SettingsSection,
  ToastTone,
} from "../lib/types";
import { AboutSection } from "./settings/AboutSection";
import { AdvancedSection } from "./settings/AdvancedSection";
import { GeneralSection } from "./settings/GeneralSection";
import { OcrSection } from "./settings/OcrSection";
import { OutputSection } from "./settings/OutputSection";
import { useSettingsForm } from "./settings/useSettingsForm";
import { useSettingsScroll } from "./settings/useSettingsScroll";

interface SettingsViewProps {
  settings: Settings;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  engineBusy: EngineBusy;
  appVersion: string;
  screenAccess: ScreenAccess;
  updateInfo: AvailableUpdate | null;
  updatePhase: UpdatePhase;
  updateProgress: number;
  updateError: string | null;
  onCheckUpdates: () => void;
  onDownloadUpdate: () => void;
  onInstallUpdate: () => void;
  onSave: (draft: Settings) => Promise<void>;
  onTestEngine: () => Promise<EngineStatus>;
  onProbeEngine: (url: string) => Promise<boolean>;
  onNotify: (tone: ToastTone, title: string, detail?: string) => void;
  onQuit: () => void;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
  onRequestPermission: () => void;
  onOpenPermissionSettings: () => void;
  onResetOnboarding: () => void;
  section: SettingsSection;
  sectionNav: number;
  onSectionChange: (section: SettingsSection) => void;
}

/**
 * Settings page composition: the draft form and the save bar live here, each
 * section owns its own rows and receives only the values it renders.
 */
export function SettingsView({
  settings,
  status,
  download,
  engineBusy,
  appVersion,
  screenAccess,
  updateInfo,
  updatePhase,
  updateProgress,
  updateError,
  onCheckUpdates,
  onDownloadUpdate,
  onInstallUpdate,
  onSave,
  onTestEngine,
  onProbeEngine,
  onNotify,
  onQuit,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
  onRequestPermission,
  onOpenPermissionSettings,
  onResetOnboarding,
  section,
  sectionNav,
  onSectionChange,
}: SettingsViewProps) {
  const form = useSettingsForm({
    settings,
    onSave,
    onTestEngine,
    onProbeEngine,
    onNotify,
  });
  const scrollRef = useSettingsScroll({ section, sectionNav, onSectionChange });
  const engine = engineForMode(status, form.draft.engine);

  return (
    <div className="view settings-view">
      <div className="view-scroll" ref={scrollRef}>
        <header className="view-head">
          <div>
            <p className="eyebrow">设置</p>
            <h1>设置</h1>
            <p className="lede">
              调整截图、识别和输出的方式。修改后点保存；模型的启动和下载会立即生效。
            </p>
          </div>
          <span className="settings-version">ocr4code {appVersion || "—"}</span>
        </header>

        <GeneralSection
          draft={form.draft}
          update={form.update}
          onRecordingChange={form.handleRecordingChange}
          screenAccess={screenAccess}
          onRequestPermission={onRequestPermission}
          onOpenPermissionSettings={onOpenPermissionSettings}
        />

        <OcrSection
          draft={form.draft}
          update={form.update}
          status={status}
          download={download}
          engineBusy={engineBusy}
          engine={engine}
          testing={form.testing}
          onTest={() => void form.handleTest()}
          onSetup={onSetup}
          onDownloadModel={onDownloadModel}
          onStartEngine={onStartEngine}
          onStopEngine={onStopEngine}
          onRemoveModel={onRemoveModel}
          onCancelSetup={onCancelSetup}
          onSave={onSave}
          onNotify={onNotify}
          proxyError={form.proxyError}
          proxyAuthError={form.proxyAuthError}
        />

        <OutputSection
          draft={form.draft}
          update={form.update}
          fenceError={form.fenceError}
        />

        <AdvancedSection
          draft={form.draft}
          update={form.update}
          urlError={form.urlError}
          promptError={form.promptError}
          testingUrl={form.testingUrl}
          onTestUrl={() => void form.handleTestUrl()}
          langError={form.langError}
        />

        <AboutSection
          draft={form.draft}
          update={form.update}
          engine={engine}
          appVersion={appVersion}
          updateInfo={updateInfo}
          updatePhase={updatePhase}
          updateProgress={updateProgress}
          updateError={updateError}
          onCheckUpdates={onCheckUpdates}
          onDownloadUpdate={onDownloadUpdate}
          onInstallUpdate={onInstallUpdate}
          onResetOnboarding={onResetOnboarding}
          onQuit={onQuit}
        />
      </div>

      <footer className="settings-bar">
        <div className="save-state">
          {form.saveError ? (
            <span className="save-error" role="alert">
              <Icon name="alert" size={14} />
              {form.saveError}
            </span>
          ) : form.dirty && !form.valid ? (
            <span className="save-error">
              <Icon name="alert" size={14} />
              有字段需要修正后才能保存
            </span>
          ) : form.dirty ? (
            <span className="save-dirty">
              <span className="dirty-dot" />
              有未保存的更改
            </span>
          ) : (
            <span className="save-clean">
              <Icon name="check" size={14} />
              已保存
            </span>
          )}
        </div>
        <div className="save-actions">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={form.resetDefaults}
            disabled={form.saving}
          >
            恢复默认
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void form.handleSave()}
            disabled={!form.dirty || form.saving || !form.valid}
          >
            {form.saving ? <span className="btn-spinner" /> : <Icon name="check" size={15} />}
            {form.saving ? "保存中…" : "保存更改"}
          </button>
        </div>
      </footer>
    </div>
  );
}
