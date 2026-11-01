import {
  useCallback,
  useEffect,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";

import {
  captureNow,
  ocrClipboardImage,
  ocrImageFile,
  onDragState,
  onImageDrop,
} from "../lib/bridge";
import { SETUP_DISMISSED_KEY } from "../lib/defaults";
import { demo } from "../lib/demo";
import { writeFlag } from "../lib/flags";
import type {
  DragPayload,
  DroppedImage,
  InputError,
  OcrError,
  Phase,
  ScreenAccess,
  SettingsSection,
  View,
} from "../lib/types";
import type { PushToast } from "./useToast";

interface UseCaptureActionsOptions {
  phaseRef: RefObject<Phase>;
  setError: Dispatch<SetStateAction<OcrError | null>>;
  remoteNeedsDeploy: boolean;
  localSetupBlocking: boolean;
  screenAccessState: ScreenAccess;
  openSettings: (section?: SettingsSection) => void;
  setView: (view: View) => void;
  setSetupOpen: (open: boolean) => void;
  setPermissionOpen: (open: boolean) => void;
  setSetupDismissed: (dismissed: boolean) => void;
  resetCaptureState: () => void;
  pushToast: PushToast;
}

export interface CaptureActions {
  drag: DragPayload | null;
  handleCapture: () => Promise<void>;
  handleDroppedFiles: (files: DroppedImage[]) => Promise<void>;
  handleClipboardImage: () => Promise<void>;
  handlePickedImage: (file: { name: string; size: number }) => void;
  handleResetOnboarding: () => void;
}

/**
 * The user-triggered entry points into the same pipeline: shortcut capture,
 * dropped files and clipboard images, plus the onboarding reset. Every one of
 * them checks readiness in the same order (model, then permission).
 */
export function useCaptureActions({
  phaseRef,
  setError,
  remoteNeedsDeploy,
  localSetupBlocking,
  screenAccessState,
  openSettings,
  setView,
  setSetupOpen,
  setPermissionOpen,
  setSetupDismissed,
  resetCaptureState,
  pushToast,
}: UseCaptureActionsOptions): CaptureActions {
  const [drag, setDrag] = useState<DragPayload | null>(null);

  const handleCapture = useCallback(async () => {
    if (phaseRef.current !== "idle") return;
    // Same order as the readiness state: model first, then screen permission.
    if (remoteNeedsDeploy) {
      openSettings("recognition");
      pushToast(
        "info",
        "先把模型部署到服务器",
        "部署一次后，截图会发到你的服务器识别。",
      );
      return;
    }
    if (localSetupBlocking) {
      setSetupOpen(true);
      return;
    }
    if (screenAccessState === "denied") {
      setPermissionOpen(true);
      return;
    }
    setError(null);
    try {
      await captureNow();
    } catch (cause) {
      pushToast(
        "error",
        "无法开始截图",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [
    localSetupBlocking,
    openSettings,
    phaseRef,
    pushToast,
    remoteNeedsDeploy,
    screenAccessState,
    setError,
    setPermissionOpen,
    setSetupOpen,
  ]);

  /** A rejected file is the command's `InputError`; anything else is a bug. */
  const showInputError = useCallback(
    (cause: unknown) => {
      if (cause && typeof cause === "object") {
        const input = cause as Partial<InputError>;
        if (typeof input.message === "string" && typeof input.hint === "string") {
          pushToast("error", input.message, input.hint);
          return;
        }
      }
      pushToast(
        "error",
        "这张图片没有识别",
        cause instanceof Error ? cause.message : String(cause),
      );
    },
    [pushToast],
  );

  // Dropped files and clipboard images enter the same pipeline as a capture:
  // the shell only hands over the pixels, everything else is shared.
  const handleDroppedFiles = useCallback(
    async (files: DroppedImage[]) => {
      const first = files[0];
      if (!first) return;
      if (phaseRef.current !== "idle") {
        pushToast("info", "正在识别上一张图", "等这次识别结束再试。");
        return;
      }
      if (files.length > 1) {
        pushToast("info", "一次识别一张图", "先处理第一张，完成后再拖下一张。");
      }
      setError(null);
      try {
        await ocrImageFile(first);
      } catch (cause) {
        showInputError(cause);
      }
    },
    [phaseRef, pushToast, setError, showInputError],
  );

  const handleClipboardImage = useCallback(async () => {
    if (phaseRef.current !== "idle") {
      pushToast("info", "正在识别上一张图", "等这次识别结束再试。");
      return;
    }
    setError(null);
    try {
      await ocrClipboardImage();
    } catch (cause) {
      showInputError(cause);
    }
  }, [phaseRef, pushToast, setError, showInputError]);

  const handlePickedImage = useCallback(
    (file: { name: string; size: number }) => {
      void handleDroppedFiles([
        { path: file.name, name: file.name, size: file.size },
      ]);
    },
    [handleDroppedFiles],
  );

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
      onDragState((payload) => {
        setDrag(payload.active ? payload : null);
      }),
    );
    track(
      onImageDrop((files) => {
        setDrag(null);
        void handleDroppedFiles(files);
      }),
    );

    return () => {
      disposed = true;
      unsubscribers.forEach((unsubscribe) => unsubscribe());
    };
  }, [handleDroppedFiles]);

  // ?demo=streaming starts a capture on its own so the preview can be reviewed
  // without pressing anything. The mock bus supplies the partial events.
  useEffect(() => {
    if (demo !== "streaming") return;
    const timer = window.setTimeout(() => void captureNow(), 400);
    return () => window.clearTimeout(timer);
  }, []);

  const handleResetOnboarding = useCallback(() => {
    writeFlag(SETUP_DISMISSED_KEY, false);
    resetCaptureState();
    setSetupDismissed(false);
    setView("capture");
    if (remoteNeedsDeploy) {
      openSettings("recognition");
    } else if (localSetupBlocking) {
      setSetupOpen(true);
    } else if (screenAccessState === "denied") {
      setSetupOpen(false);
      setPermissionOpen(true);
    }
    pushToast("info", "引导已重新开启", "截图页会重新显示使用步骤。");
  }, [
    localSetupBlocking,
    openSettings,
    pushToast,
    remoteNeedsDeploy,
    resetCaptureState,
    screenAccessState,
    setPermissionOpen,
    setSetupDismissed,
    setSetupOpen,
    setView,
  ]);

  return {
    drag,
    handleCapture,
    handleDroppedFiles,
    handleClipboardImage,
    handlePickedImage,
    handleResetOnboarding,
  };
}
