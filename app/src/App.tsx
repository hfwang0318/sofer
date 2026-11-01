import { useEffect, useRef } from "react";

import { Icon } from "./components/Icon";
import { PermissionDialog } from "./components/PermissionDialog";
import { PreviewDesktop } from "./components/PreviewDesktop";
import { SetupDialog } from "./components/SetupDialog";
import { Sidebar } from "./components/Sidebar";
import { Toast } from "./components/Toast";
import { WindowChrome } from "./components/WindowChrome";
import { probeEngine, quitApp } from "./lib/bridge";
import { IMAGE_FORMAT_LABEL } from "./lib/defaults";
import {
  INITIAL_ERROR,
  INITIAL_FIRST_CAPTURE_TIP,
  INITIAL_RESULT,
  IMAGE_FORMATS,
  demo,
  initialScreenAccess,
  initialView,
} from "./lib/demo";
import { isPreview } from "./lib/platform";
import { captureReadiness } from "./lib/status";
import { useCaptureActions } from "./hooks/useCaptureActions";
import { useCaptureEvents } from "./hooks/useCaptureEvents";
import { useClipboard } from "./hooks/useClipboard";
import { useDialogs } from "./hooks/useDialogs";
import { useEngineLifecycle } from "./hooks/useEngineLifecycle";
import { useHistory } from "./hooks/useHistory";
import { useNavigation } from "./hooks/useNavigation";
import { useSaveSettings } from "./hooks/useSaveSettings";
import { useScreenPermission } from "./hooks/useScreenPermission";
import {
  useSettingsBootstrap,
  useSettingsDraft,
} from "./hooks/useSettingsDraft";
import { useToast } from "./hooks/useToast";
import { useUpdates } from "./hooks/useUpdates";
import { CaptureView } from "./views/CaptureView";
import { HistoryView } from "./views/HistoryView";
import { SettingsView } from "./views/SettingsView";

export default function App() {
  const { toast, pushToast, dismissToast } = useToast();
  const dialogs = useDialogs(demo === "permission");
  const navigation = useNavigation({ dialogs, initialView });
  const settingsDraft = useSettingsDraft();
  const engine = useEngineLifecycle({
    settings: settingsDraft.settings,
    settingsRef: settingsDraft.settingsRef,
    permissionOpen: dialogs.permissionOpen,
    setupDismissed: dialogs.setupDismissed,
    setSetupOpen: dialogs.setSetupOpen,
    setSetupDismissed: dialogs.setSetupDismissed,
    pushToast,
  });
  const permission = useScreenPermission({
    initialAccess: initialScreenAccess(),
    setSetupOpen: dialogs.setSetupOpen,
    setPermissionOpen: dialogs.setPermissionOpen,
    pushToast,
  });
  const history = useHistory({
    settingsRef: settingsDraft.settingsRef,
    historyLimit: settingsDraft.settings.historyLimit,
    pushToast,
  });
  const updates = useUpdates({
    settingsRef: settingsDraft.settingsRef,
    appVersion: settingsDraft.appVersion,
    autoCheckUpdates: settingsDraft.settings.autoCheckUpdates,
    pushToast,
  });

  const { settings } = settingsDraft;
  const remoteEnabled = settings.remoteEnabled && settings.engine !== "tesseract";
  const remoteNeedsDeploy =
    remoteEnabled && Boolean(engine.status) && !engine.status?.remote.deployed;
  const localSetupBlocking =
    !remoteEnabled &&
    Boolean(engine.status?.setup.needed) &&
    settings.engine !== "tesseract" &&
    (settings.engine === "llamacpp" || !engine.status?.tesseract);
  const setupBlocking = remoteEnabled ? remoteNeedsDeploy : localSetupBlocking;

  const setupBlockingRef = useRef(false);
  useEffect(() => {
    setupBlockingRef.current = setupBlocking;
  }, [setupBlocking]);

  const capture = useCaptureEvents({
    initialResult: INITIAL_RESULT,
    initialError: INITIAL_ERROR,
    initialFirstCaptureTip: INITIAL_FIRST_CAPTURE_TIP,
    settingsRef: settingsDraft.settingsRef,
    setupBlockingRef,
    setView: navigation.setView,
    openSettings: navigation.openSettings,
    addHistory: history.addHistory,
    setStatus: engine.setStatus,
    setDownload: engine.setDownload,
    refreshStatus: engine.refreshStatus,
    refreshScreenAccess: permission.refreshScreenAccess,
    setSetupOpen: dialogs.setSetupOpen,
    setPermissionOpen: dialogs.setPermissionOpen,
    setSetupDismissed: dialogs.setSetupDismissed,
    pushToast,
  });

  const captureActions = useCaptureActions({
    phaseRef: capture.phaseRef,
    setError: capture.setError,
    remoteNeedsDeploy,
    localSetupBlocking,
    screenAccessState: permission.screenAccessState,
    openSettings: navigation.openSettings,
    setView: navigation.setView,
    setSetupOpen: dialogs.setSetupOpen,
    setPermissionOpen: dialogs.setPermissionOpen,
    setSetupDismissed: dialogs.setSetupDismissed,
    resetCaptureState: capture.resetCaptureState,
    pushToast,
  });

  const { handleCopy, handleCopyError } = useClipboard({
    result: capture.result,
    setCopied: capture.setCopied,
    pushToast,
  });

  useSettingsBootstrap({
    setSettings: settingsDraft.setSettings,
    setStatus: engine.setStatus,
    setShortcutActive: settingsDraft.setShortcutActive,
    setAppVersion: settingsDraft.setAppVersion,
    setScreenAccess: permission.setScreenAccessState,
    pushToast,
  });

  const handleSaveSettings = useSaveSettings({
    settings,
    setSettings: settingsDraft.setSettings,
    setShortcutActive: settingsDraft.setShortcutActive,
    setSetupOpen: dialogs.setSetupOpen,
    setSetupDismissed: dialogs.setSetupDismissed,
    setStatus: engine.setStatus,
    setEngineBusy: engine.setEngineBusy,
    refreshStatus: engine.refreshStatus,
    pushToast,
  });

  const readiness = captureReadiness({
    status: engine.status,
    engine: settings.engine,
    phase: capture.phase,
    shortcutActive: settingsDraft.shortcutActive,
    screenAccess: permission.screenAccessState,
  });
  const permissionDenied = permission.screenAccessState === "denied";
  const showSteps =
    !capture.firstCaptureDone &&
    !capture.result &&
    !capture.error &&
    !setupBlocking &&
    !permissionDenied;

  return (
    <>
      {isPreview && <PreviewDesktop />}
      <div
        className={`app view-${navigation.view}`}
        data-preview={isPreview ? "true" : undefined}
      >
        <WindowChrome
          shortcut={settings.shortcut}
          readiness={readiness}
          version={settingsDraft.appVersion}
          onOpenSettings={() => navigation.openSettings("general")}
        />
        <Sidebar
          view={navigation.view}
          shortcut={settings.shortcut}
          status={engine.status}
          engineMode={settings.engine}
          download={engine.download}
          historyCount={history.history.length}
          settingsSection={navigation.settingsSection}
          version={settingsDraft.appVersion}
          onNavigate={navigation.navigate}
          onSettingsSection={navigation.handleSettingsSection}
        />
        <main className="content" id="main-content">
          <div className="view-host" key={navigation.view}>
            {navigation.view === "capture" && (
              <CaptureView
                phase={capture.phase}
                partial={capture.partial}
                shortcut={settings.shortcut}
                result={capture.result}
                error={capture.error}
                copied={capture.copied}
                setupBlocking={setupBlocking}
                remote={remoteEnabled}
                permissionDenied={permissionDenied}
                showSteps={showSteps}
                firstCaptureTip={capture.firstCaptureTip}
                onCapture={() => void captureActions.handleCapture()}
                onClipboard={() => void captureActions.handleClipboardImage()}
                onPickImage={captureActions.handlePickedImage}
                onCopy={(text) => void handleCopy(text)}
                onOpenHistory={() => navigation.setView("history")}
                onOpenSettings={navigation.openSettings}
                onSetup={() => {
                  if (remoteEnabled) navigation.openSettings("recognition");
                  else dialogs.setSetupOpen(true);
                }}
                onPermission={permission.requestPermission}
                onDismissTip={() => capture.setFirstCaptureTip(false)}
              />
            )}
            {navigation.view === "history" && (
              <HistoryView
                entries={history.history}
                shortcut={settings.shortcut}
                onCopy={(text, id) => void handleCopy(text, id)}
                onDelete={history.handleDeleteHistory}
                onClear={history.handleClearHistory}
                onCapture={() => void captureActions.handleCapture()}
                onTogglePin={history.handleTogglePin}
              />
            )}
            {navigation.view === "settings" && (
              <SettingsView
                settings={settings}
                status={engine.status}
                download={engine.download}
                engineBusy={engine.engineBusy}
                appVersion={settingsDraft.appVersion}
                screenAccess={permission.screenAccessState}
                updateInfo={updates.updateInfo}
                updatePhase={updates.updatePhase}
                updateProgress={updates.updateProgress}
                updateError={updates.updateError}
                onCheckUpdates={() => void updates.checkUpdates(true)}
                onDownloadUpdate={() => void updates.downloadNow()}
                onInstallUpdate={() => void updates.installNow()}
                onSave={handleSaveSettings}
                onTestEngine={engine.handleTestEngine}
                onProbeEngine={probeEngine}
                onNotify={pushToast}
                onQuit={() => void quitApp()}
                onSetup={() => dialogs.setSetupOpen(true)}
                onDownloadModel={() => void engine.handleSetupStart()}
                onStartEngine={() => void engine.handleStartEngine()}
                onStopEngine={() => void engine.handleStopEngine()}
                onRemoveModel={() => void engine.handleRemoveModel()}
                onCancelSetup={() => void engine.handleSetupCancel()}
                onRequestPermission={permission.requestPermission}
                onOpenPermissionSettings={() =>
                  void permission.handleOpenPermissionSettings()
                }
                onResetOnboarding={captureActions.handleResetOnboarding}
                section={navigation.settingsSection}
                sectionNav={navigation.settingsNav}
                onSectionChange={navigation.handleSettingsActive}
              />
            )}
          </div>
        </main>
        {/* Dropping works on every view, so the target belongs to the window
            rather than the capture page. */}
        {captureActions.drag?.active && (
          <div
            className={`drop-overlay${captureActions.drag.accepted ? "" : " is-rejected"}`}
            role="status"
            aria-live="polite"
          >
            <div className="drop-card">
              <span className="drop-icon">
                <Icon
                  name={captureActions.drag.accepted ? "image" : "alert"}
                  size={22}
                />
              </span>
              <strong>
                {captureActions.drag.accepted ? "松开，识别这张图片" : "只支持图片文件"}
              </strong>
              <p>
                {captureActions.drag.accepted
                  ? `${IMAGE_FORMATS}。`
                  : `换成 ${IMAGE_FORMAT_LABEL} 再拖一次。`}
              </p>
            </div>
          </div>
        )}
        {toast && <Toast toast={toast} onDismiss={dismissToast} />}
      </div>
      <SetupDialog
        open={dialogs.setupOpen}
        status={engine.status}
        download={engine.download}
        screenAccess={permission.screenAccessState}
        firstRun={!dialogs.setupDismissed && !capture.firstCaptureDone}
        shortcut={settings.shortcut}
        onStart={() => void engine.handleSetupStart()}
        onCancel={() => void engine.handleSetupCancel()}
        onDismiss={engine.handleSetupDismiss}
        onCapture={() => {
          dialogs.setSetupOpen(false);
          engine.setDownload(null);
          void captureActions.handleCapture();
        }}
        onPermission={permission.requestPermission}
        onOpenSettings={navigation.openSettings}
        onCopyError={(text) => void handleCopyError(text)}
      />
      <PermissionDialog
        open={dialogs.permissionOpen}
        access={permission.screenAccessState}
        busy={permission.permissionBusy}
        onRequest={() => void permission.handleRequestPermission()}
        onOpenSettings={() => void permission.handleOpenPermissionSettings()}
        onRecheck={() => void permission.handleRecheckPermission()}
        onRestart={() => void permission.handleRestart()}
        onCapture={() => {
          dialogs.setPermissionOpen(false);
          void captureActions.handleCapture();
        }}
        onClose={() => dialogs.setPermissionOpen(false)}
      />
    </>
  );
}
