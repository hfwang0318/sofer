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
  onEngineDownload,
  onEngineState,
  onError,
  onPartial,
  onPhase,
  onRemoteProgress,
  onRemoteState,
  onResult,
} from "../lib/bridge";
import {
  FIRST_CAPTURE_KEY,
  SETUP_DISMISSED_KEY,
  modelName,
} from "../lib/defaults";
import { readFlag, writeFlag } from "../lib/flags";
import type {
  DownloadProgress,
  EngineStatus,
  OcrError,
  OcrPartial,
  OcrResult,
  Phase,
  ScreenAccess,
  Settings,
  SettingsSection,
  View,
} from "../lib/types";
import type { PushToast } from "./useToast";

interface UseCaptureEventsOptions {
  initialResult: OcrResult | null;
  initialError: OcrError | null;
  initialFirstCaptureTip: boolean;
  settingsRef: RefObject<Settings>;
  setupBlockingRef: RefObject<boolean>;
  setView: (view: View) => void;
  openSettings: (section?: SettingsSection) => void;
  addHistory: (result: OcrResult) => void;
  setStatus: Dispatch<SetStateAction<EngineStatus | null>>;
  setDownload: (progress: DownloadProgress) => void;
  refreshStatus: () => Promise<void>;
  refreshScreenAccess: () => Promise<ScreenAccess | null>;
  setSetupOpen: (open: boolean) => void;
  setPermissionOpen: (open: boolean) => void;
  setSetupDismissed: (dismissed: boolean) => void;
  pushToast: PushToast;
}

export interface CaptureEvents {
  phase: Phase;
  phaseRef: RefObject<Phase>;
  setPhase: Dispatch<SetStateAction<Phase>>;
  partial: OcrPartial | null;
  setPartial: Dispatch<SetStateAction<OcrPartial | null>>;
  result: OcrResult | null;
  setResult: Dispatch<SetStateAction<OcrResult | null>>;
  error: OcrError | null;
  setError: Dispatch<SetStateAction<OcrError | null>>;
  copied: boolean;
  setCopied: Dispatch<SetStateAction<boolean>>;
  firstCaptureDone: boolean;
  firstCaptureTip: boolean;
  setFirstCaptureTip: (tip: boolean) => void;
  /** Clear the onboarding state so the capture page shows the steps again. */
  resetCaptureState: () => void;
}

/**
 * Turns the backend event channels into app state: OCR phases/partials/results
 * and errors, engine download health and remote-server phases. Drag/drop events
 * live with the capture actions because they share the same entry points.
 */
export function useCaptureEvents({
  initialResult,
  initialError,
  initialFirstCaptureTip,
  settingsRef,
  setupBlockingRef,
  setView,
  openSettings,
  addHistory,
  setStatus,
  setDownload,
  refreshStatus,
  refreshScreenAccess,
  setSetupOpen,
  setPermissionOpen,
  setSetupDismissed,
  pushToast,
}: UseCaptureEventsOptions): CaptureEvents {
  const [phase, setPhase] = useState<Phase>("idle");
  // Accumulated preview while the model is generating; always replaced by the
  // final result, never added to history or copied.
  const [partial, setPartial] = useState<OcrPartial | null>(null);
  const [result, setResult] = useState<OcrResult | null>(initialResult);
  const [error, setError] = useState<OcrError | null>(initialError);
  const [copied, setCopied] = useState(false);
  const [firstCaptureDone, setFirstCaptureDone] = useState(() =>
    readFlag(FIRST_CAPTURE_KEY),
  );
  const [firstCaptureTip, setFirstCaptureTip] = useState(initialFirstCaptureTip);

  const phaseRef = useRef(phase);
  const firstCaptureDoneRef = useRef(firstCaptureDone);

  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  // Backend events: the same channels are emitted by Rust and by the browser
  // preview bus.
  useEffect(() => {
    let disposed = false;
    const unsubscribers: Array<() => void> = [];
    const track = (subscription: Promise<() => void>) => {
      subscription
        .then((unsubscribe) => {
          if (disposed) unsubscribe();
          else unsubscribers.push(unsubscribe);
        })
        .catch(() => undefined);
    };

    track(
      onPhase((payload) => {
        setPhase(payload.phase);
        // A preview only lives between `recognizing` and its final result.
        if (payload.phase === "selecting" || payload.phase === "idle") {
          setPartial(null);
        }
      }),
    );
    track(onPartial((next) => setPartial(next)));
    track(
      onResult((next) => {
        setPhase("idle");
        setPartial(null);
        setResult(next);
        setError(null);
        setView("capture");
        setSetupOpen(false);
        setPermissionOpen(false);
        setCopied(false);
        setFirstCaptureTip(false);
        addHistory(next);
        if (!firstCaptureDoneRef.current) {
          firstCaptureDoneRef.current = true;
          setFirstCaptureDone(true);
          setFirstCaptureTip(true);
          writeFlag(FIRST_CAPTURE_KEY, true);
        }
      }),
    );
    track(
      onError((payload) => {
        setPhase("idle");
        setPartial(null);
        setError(payload);
        setView("capture");
        setSetupOpen(false);
        setPermissionOpen(false);
        if (payload.code === "setup-required") {
          if (settingsRef.current.remoteEnabled) openSettings("recognition");
          else setSetupOpen(true);
        }
        if (payload.code === "permission") {
          // The global shortcut bypasses handleCapture; keep the same order:
          // finish model setup before asking for screen recording.
          if (setupBlockingRef.current) {
            if (settingsRef.current.remoteEnabled) openSettings("recognition");
            else setSetupOpen(true);
          } else setPermissionOpen(true);
          void refreshScreenAccess();
        }
        void refreshStatus();
      }),
    );

    track(
      onEngineDownload((progress) => {
        setDownload(progress);
        if (progress.phase === "ready") {
          setError(null);
          setSetupDismissed(false);
          writeFlag(SETUP_DISMISSED_KEY, false);
          setStatus((current) =>
            current
              ? {
                  ...current,
                  llama: true,
                  model: {
                    ...current.model,
                    installed: true,
                    verified: true,
                    bytes: current.model.total,
                  },
                  setup: { ...current.setup, needed: false },
                  server: { ...current.server, state: "online" },
                }
              : current,
          );
          pushToast(
            "ok",
            "OCR 模型已就绪",
            `${modelName(settingsRef.current.modelQuality)} 已安装，可以截图了。`,
          );
          void refreshStatus();
        } else if (progress.phase === "error") {
          setPermissionOpen(false);
          setSetupOpen(true);
        } else if (progress.phase === "cancelled") {
          pushToast("info", "下载已取消", "已下载的部分会保留，可以继续。");
          void refreshStatus();
        }
      }),
    );

    track(onEngineState(() => void refreshStatus()));

    track(
      onRemoteProgress((progress) => {
        setStatus((current) =>
          current
            ? { ...current, remote: { ...current.remote, progress } }
            : current,
        );
      }),
    );

    track(
      onRemoteState((remote) => {
        setStatus((current) =>
          current ? { ...current, remote } : current,
        );
        if (
          remote.phase === "online" ||
          remote.phase === "failed" ||
          remote.phase === "stopped" ||
          remote.phase === "cancelled"
        ) {
          void refreshStatus();
        }
      }),
    );

    return () => {
      disposed = true;
      unsubscribers.forEach((unsubscribe) => unsubscribe());
    };
  }, [
    addHistory,
    openSettings,
    pushToast,
    refreshScreenAccess,
    refreshStatus,
    setDownload,
    setPermissionOpen,
    setSetupDismissed,
    setSetupOpen,
    setStatus,
    setView,
    settingsRef,
    setupBlockingRef,
  ]);

  const resetCaptureState = useCallback(() => {
    writeFlag(FIRST_CAPTURE_KEY, false);
    firstCaptureDoneRef.current = false;
    setFirstCaptureDone(false);
    setFirstCaptureTip(false);
    setResult(null);
    setError(null);
    setPartial(null);
    setPhase("idle");
  }, []);

  return {
    phase,
    phaseRef,
    setPhase,
    partial,
    setPartial,
    result,
    setResult,
    error,
    setError,
    copied,
    setCopied,
    firstCaptureDone,
    firstCaptureTip,
    setFirstCaptureTip,
    resetCaptureState,
  };
}
