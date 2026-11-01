import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";

import {
  cancelEngineSetup,
  engineStatus,
  removeModel,
  setupEngine,
  startEngine,
  stopEngine,
} from "../lib/bridge";
import { SETUP_DISMISSED_KEY, modelTotalBytes } from "../lib/defaults";
import { writeFlag } from "../lib/flags";
import type {
  DownloadProgress,
  EngineBusy,
  EngineStatus,
  Settings,
} from "../lib/types";
import type { PushToast } from "./useToast";

interface UseEngineLifecycleOptions {
  settings: Settings;
  settingsRef: RefObject<Settings>;
  permissionOpen: boolean;
  setupDismissed: boolean;
  setSetupOpen: (open: boolean) => void;
  setSetupDismissed: (dismissed: boolean) => void;
  pushToast: PushToast;
}

export interface EngineLifecycle {
  status: EngineStatus | null;
  setStatus: Dispatch<SetStateAction<EngineStatus | null>>;
  download: DownloadProgress | null;
  setDownload: Dispatch<SetStateAction<DownloadProgress | null>>;
  engineBusy: EngineBusy;
  setEngineBusy: (busy: EngineBusy) => void;
  refreshStatus: () => Promise<void>;
  handleSetupStart: () => Promise<void>;
  handleSetupCancel: () => Promise<void>;
  handleSetupDismiss: () => void;
  handleStartEngine: () => Promise<void>;
  handleStopEngine: () => Promise<void>;
  handleRemoveModel: () => Promise<void>;
  handleTestEngine: () => Promise<EngineStatus>;
}

/**
 * Owns engine health, the model download progress and the immediate engine
 * actions (start / stop / remove / setup). Backend events that also touch
 * capture state are subscribed in `useCaptureEvents`.
 */
export function useEngineLifecycle({
  settings,
  settingsRef,
  permissionOpen,
  setupDismissed,
  setSetupOpen,
  setSetupDismissed,
  pushToast,
}: UseEngineLifecycleOptions): EngineLifecycle {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [download, setDownload] = useState<DownloadProgress | null>(null);
  const [engineBusy, setEngineBusy] = useState<EngineBusy>(null);
  const refreshing = useRef(false);

  const refreshStatus = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      setStatus(await engineStatus());
    } catch {
      /* transient health failures are surfaced through the event channel */
    } finally {
      refreshing.current = false;
    }
  }, []);

  // Auto-open setup once per install, or whenever the user asks for it.
  useEffect(() => {
    if (!status) return;
    if (settings.remoteEnabled) return;
    if (settings.engine === "tesseract") return;
    if (!status.setup.needed) return;
    // If tesseract is available, the banner is enough; do not interrupt a
    // working fallback with a modal on every launch.
    if (status.tesseract && settings.engine === "auto") return;
    if (!setupDismissed && !permissionOpen) setSetupOpen(true);
  }, [
    permissionOpen,
    setSetupOpen,
    setupDismissed,
    settings.engine,
    settings.remoteEnabled,
    status,
  ]);

  const handleSetupStart = useCallback(async () => {
    const active =
      download &&
      ["downloading", "verifying", "starting"].includes(download.phase);
    if (active) {
      // The Rust thread is already running; never reset the visible progress.
      setSetupOpen(true);
      return;
    }
    setDownload({
      phase: "downloading",
      file: null,
      downloaded: 0,
      total: status?.setup.downloadBytes ?? modelTotalBytes(settingsRef.current.modelQuality),
      bytesPerSec: 0,
      etaSecs: null,
      message: null,
    });
    setSetupOpen(true);
    try {
      await setupEngine();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setDownload({
        phase: "error",
        file: null,
        downloaded: 0,
        total: status?.setup.downloadBytes ?? modelTotalBytes(settingsRef.current.modelQuality),
        bytesPerSec: 0,
        etaSecs: null,
        message,
      });
    }
  }, [download, setSetupOpen, settingsRef, status]);

  const handleSetupCancel = useCallback(async () => {
    try {
      await cancelEngineSetup();
    } catch (cause) {
      pushToast(
        "error",
        "无法取消下载",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [pushToast]);

  const handleSetupDismiss = useCallback(() => {
    setSetupOpen(false);
    const active =
      download && ["downloading", "verifying", "starting"].includes(download.phase);
    if (active) {
      // The Rust thread keeps downloading; the sidebar keeps showing progress.
      return;
    }
    setDownload(null);
    if (status && !status.model.installed) {
      setSetupDismissed(true);
      writeFlag(SETUP_DISMISSED_KEY, true);
    }
  }, [download, setDownload, setSetupDismissed, setSetupOpen, status]);

  const handleStartEngine = useCallback(async () => {
    setEngineBusy("start");
    try {
      const next = await startEngine();
      setStatus(next);
      pushToast("ok", "OCR 引擎已启动");
    } catch (cause) {
      pushToast(
        "error",
        "无法启动 OCR 引擎",
        cause instanceof Error ? cause.message : String(cause),
      );
      void refreshStatus();
    } finally {
      setEngineBusy(null);
    }
  }, [pushToast, refreshStatus]);

  const handleStopEngine = useCallback(async () => {
    setEngineBusy("stop");
    try {
      await stopEngine();
      await refreshStatus();
      pushToast("info", "OCR 引擎已停止", "下次截图会自动启动。");
    } catch (cause) {
      pushToast(
        "error",
        "无法停止 OCR 引擎",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setEngineBusy(null);
    }
  }, [pushToast, refreshStatus]);

  const handleRemoveModel = useCallback(async () => {
    setEngineBusy("remove");
    try {
      await removeModel();
      setDownload(null);
      setSetupDismissed(false);
      writeFlag(SETUP_DISMISSED_KEY, false);
      await refreshStatus();
      setSetupOpen(true);
      pushToast("info", "模型已移除", "需要本地 OCR 引擎时重新下载即可。");
    } catch (cause) {
      pushToast(
        "error",
        "无法移除模型",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setEngineBusy(null);
    }
  }, [pushToast, refreshStatus, setDownload, setSetupDismissed, setSetupOpen]);

  const handleTestEngine = useCallback(async () => {
    const next = await engineStatus();
    setStatus(next);
    return next;
  }, []);

  return {
    status,
    setStatus,
    download,
    setDownload,
    engineBusy,
    setEngineBusy,
    refreshStatus,
    handleSetupStart,
    handleSetupCancel,
    handleSetupDismiss,
    handleStartEngine,
    handleStopEngine,
    handleRemoveModel,
    handleTestEngine,
  };
}
