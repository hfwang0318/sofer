import { useCallback, useEffect, useRef, useState } from "react";

import { Icon } from "./components/Icon";
import { PermissionDialog } from "./components/PermissionDialog";
import { PreviewDesktop } from "./components/PreviewDesktop";
import { SetupDialog } from "./components/SetupDialog";
import { Sidebar } from "./components/Sidebar";
import { Toast } from "./components/Toast";
import { WindowChrome } from "./components/WindowChrome";
import {
  appInfo,
  cancelEngineSetup,
  captureNow,
  copyText,
  engineStatus,
  loadSettings,
  ocrClipboardImage,
  ocrImageFile,
  onDragState,
  onEngineDownload,
  onEngineState,
  onError,
  onImageDrop,
  onPartial,
  onPhase,
  onRemoteProgress,
  onRemoteState,
  onResult,
  openScreenSettings,
  prepareUpdate,
  probeEngine,
  quitApp,
  removeModel,
  requestScreenAccess,
  restartApp,
  saveSettings,
  screenAccess,
  setupEngine,
  startEngine,
  stopEngine,
} from "./lib/bridge";
import {
  DEFAULT_SETTINGS,
  DEMO_RESULT,
  FIRST_CAPTURE_KEY,
  IMAGE_FORMAT_LABEL,
  MAX_IMAGE_BYTES,
  modelName,
  modelTotalBytes,
  SETUP_DISMISSED_KEY,
} from "./lib/defaults";
import {
  fromResult,
  hydrateHistory,
  insertHistoryEntry,
  loadHistory,
  saveHistory,
  toggleHistoryPin,
  trimHistory,
} from "./lib/history";
import {
  checkForUpdate,
  downloadUpdate,
  installUpdate,
  type AvailableUpdate,
  type UpdatePhase,
} from "./lib/updater";
import { isMac, isPreview, isTauri } from "./lib/platform";
import { captureReadiness } from "./lib/status";
import type {
  DownloadProgress,
  DragPayload,
  DroppedImage,
  EngineStatus,
  HistoryEntry,
  InputError,
  OcrError,
  OcrPartial,
  OcrResult,
  Phase,
  ScreenAccess,
  Settings,
  SettingsSection,
  ToastMessage,
  ToastTone,
  View,
} from "./lib/types";
import { CaptureView } from "./views/CaptureView";
import { HistoryView } from "./views/HistoryView";
import { SettingsView } from "./views/SettingsView";

const params = new URLSearchParams(window.location.search);
const demo = isTauri ? null : params.get("demo");

function initialView(): View {
  if (demo === "settings") return "settings";
  if (demo === "history") return "history";
  return "capture";
}

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, value: boolean): void {
  try {
    if (value) localStorage.setItem(key, "1");
    else localStorage.removeItem(key);
  } catch {
    /* storage is optional */
  }
}

function initialScreenAccess(): ScreenAccess {
  if (!isMac) return "not-needed";
  return demo === "permission" ? "denied" : "unknown";
}

const DEMO_ERROR: OcrError = {
  message: "本地 OCR 引擎没有响应。",
  hint: "可能是模型还没加载完。稍等几秒再试，或到设置里检查 OCR 引擎状态。",
  code: "engine",
};

const IMAGE_FORMATS = `${IMAGE_FORMAT_LABEL}，最大 ${MAX_IMAGE_BYTES / (1024 * 1024)} MB`;

export default function App() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [view, setView] = useState<View>(initialView);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("general");
  const [settingsNav, setSettingsNav] = useState(0);
  const [phase, setPhase] = useState<Phase>("idle");
  // Accumulated preview while the model is generating; always replaced by the
  // final result, never added to history or copied.
  const [partial, setPartial] = useState<OcrPartial | null>(null);
  const [result, setResult] = useState<OcrResult | null>(
    demo === "result" ? DEMO_RESULT : null,
  );
  const [drag, setDrag] = useState<DragPayload | null>(null);
  const [error, setError] = useState<OcrError | null>(demo === "error" ? DEMO_ERROR : null);
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);
  const [shortcutActive, setShortcutActive] = useState(true);
  const [screenAccessState, setScreenAccessState] = useState<ScreenAccess>(initialScreenAccess);
  const [permissionOpen, setPermissionOpen] = useState(demo === "permission");
  const [permissionBusy, setPermissionBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [toast, setToast] = useState<ToastMessage | null>(null);
  const [download, setDownload] = useState<DownloadProgress | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [setupDismissed, setSetupDismissed] = useState(() =>
    readFlag(SETUP_DISMISSED_KEY),
  );
  const [firstCaptureDone, setFirstCaptureDone] = useState(() =>
    readFlag(FIRST_CAPTURE_KEY),
  );
  const [firstCaptureTip, setFirstCaptureTip] = useState(demo === "result");
  const [engineBusy, setEngineBusy] = useState<"start" | "stop" | "remove" | null>(null);
  const [appVersion, setAppVersion] = useState("");
  const [updateInfo, setUpdateInfo] = useState<AvailableUpdate | null>(null);
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>("idle");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const updateBusy = useRef(false);

  const toastId = useRef(0);
  const copiedTimer = useRef<number | null>(null);
  const settingsRef = useRef(settings);
  const phaseRef = useRef(phase);
  const firstCaptureDoneRef = useRef(firstCaptureDone);
  const setupBlockingRef = useRef(false);
  const refreshing = useRef(false);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  const pushToast = useCallback(
    (tone: ToastTone, title: string, detail?: string) => {
      toastId.current += 1;
      setToast({ id: toastId.current, tone, title, detail });
    },
    [],
  );

  const dismissToast = useCallback(() => setToast(null), []);

  const openSettings = useCallback((section: SettingsSection = "general") => {
    setSettingsSection(section);
    setSettingsNav((value) => value + 1);
    setView("settings");
    setSetupOpen(false);
    setPermissionOpen(false);
  }, []);

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

  const refreshScreenAccess = useCallback(async (): Promise<ScreenAccess | null> => {
    try {
      const access = await screenAccess();
      setScreenAccessState(access);
      return access;
    } catch {
      return null;
    }
  }, []);

  const beginDownloadFor = useCallback(
    async (next: AvailableUpdate, automatic: boolean) => {
      setUpdatePhase("downloading");
      setUpdateProgress(0);
      setUpdateError(null);
      try {
        await downloadUpdate(next.update, setUpdateProgress);
        setUpdatePhase("ready");
        pushToast(
          "ok",
          `v${next.version} 已下载`,
          automatic ? "到 设置 → 关于 重启并安装。" : "重启应用后完成安装。",
        );
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setUpdatePhase("error");
        setUpdateError(message);
        if (!automatic) pushToast("error", "更新下载失败", message);
      }
    },
    [pushToast],
  );

  const checkUpdates = useCallback(
    async (manual: boolean) => {
      if (!isTauri || import.meta.env.DEV) {
        if (manual) {
          pushToast(
            "info",
            "当前不检查更新",
            isTauri ? "开发模式请使用安装版测试更新。" : "浏览器预览不会检查版本。",
          );
        }
        return;
      }
      if (updateBusy.current) return;
      updateBusy.current = true;
      setUpdatePhase("checking");
      setUpdateError(null);
      try {
        const next = await checkForUpdate();
        if (!next) {
          setUpdatePhase("current");
          if (manual) {
            pushToast("ok", "已是最新版本", appVersion ? `当前 v${appVersion}` : undefined);
          }
          return;
        }
        setUpdateInfo(next);
        setUpdatePhase("available");
        setUpdateProgress(0);
        pushToast("info", `发现新版本 v${next.version}`, "可在 设置 → 关于 下载。");
        if (settingsRef.current.autoDownloadUpdates) {
          await beginDownloadFor(next, true);
        }
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setUpdatePhase("error");
        setUpdateError(message);
        if (manual) pushToast("error", "检查更新失败", message);
      } finally {
        updateBusy.current = false;
      }
    },
    [appVersion, beginDownloadFor, pushToast],
  );

  const downloadNow = useCallback(async () => {
    if (!updateInfo || updateBusy.current) return;
    updateBusy.current = true;
    try {
      await beginDownloadFor(updateInfo, false);
    } finally {
      updateBusy.current = false;
    }
  }, [beginDownloadFor, updateInfo]);

  const installNow = useCallback(async () => {
    if (!updateInfo || updateBusy.current) return;
    updateBusy.current = true;
    setUpdatePhase("installing");
    setUpdateError(null);
    try {
      // Release the runtime directory before NSIS replaces the bundled
      // llama-server/DLLs. Otherwise Windows reports the files as in use.
      await prepareUpdate();
      await installUpdate(updateInfo.update);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setUpdatePhase("error");
      setUpdateError(message);
      pushToast("error", "更新安装失败", message);
    } finally {
      updateBusy.current = false;
    }
  }, [pushToast, updateInfo]);

  useEffect(() => {
    if (!isTauri || !settings.autoCheckUpdates) return;
    const timer = window.setTimeout(() => {
      void checkUpdates(false);
    }, 4000);
    return () => window.clearTimeout(timer);
  }, [checkUpdates, settings.autoCheckUpdates]);

  const addHistory = useCallback((next: OcrResult) => {
    setHistory((current) => {
      const limit = settingsRef.current.historyLimit || 20;
      const merged = insertHistoryEntry(current, fromResult(next), limit);
      saveHistory(merged);
      return merged;
    });
  }, []);

  // Tauri keeps history in app-data; the first frame starts empty and this
  // hydrates it once the disk read (and the one-time localStorage import)
  // finishes. The browser preview already has its data synchronously.
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    void hydrateHistory()
      .then((entries) => {
        if (!alive || entries.length === 0) return;
        setHistory((current) => {
          const limit = settingsRef.current.historyLimit || 20;
          const known = new Set(current.map((entry) => entry.id));
          return [
            ...current,
            ...entries.filter((entry) => !known.has(entry.id)),
          ].slice(0, limit);
        });
      })
      .catch((error: unknown) => {
        console.warn(`读取历史失败：${String(error)}`);
      });
    return () => {
      alive = false;
    };
  }, []);

  // Load persisted settings and check the engine once.
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [loaded, engine, info, access] = await Promise.all([
          loadSettings(),
          engineStatus(),
          appInfo(),
          screenAccess().catch(() => null),
        ]);
        if (!alive) return;
        setSettings(loaded);
        setStatus(engine);
        setShortcutActive(info.shortcutActive);
        setAppVersion(info.version);
        if (access) setScreenAccessState(access);
        if (!info.shortcutActive) {
          pushToast(
            "error",
            "全局快捷键不可用",
            `${info.shortcut} 可能被其他应用占用，请到设置里换一个组合。`,
          );
        }
      } catch (cause) {
        if (!alive) return;
        pushToast(
          "error",
          "无法加载设置",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    })();
    return () => {
      alive = false;
    };
  }, [pushToast]);

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
  }, [status, settings.engine, settings.remoteEnabled, setupDismissed, permissionOpen]);

  // ?demo=streaming starts a capture on its own so the preview can be reviewed
  // without pressing anything. The mock bus supplies the partial events.
  useEffect(() => {
    if (demo !== "streaming") return;
    const timer = window.setTimeout(() => void captureNow(), 400);
    return () => window.clearTimeout(timer);
  }, []);

  // Re-check after the user comes back from System Settings.
  useEffect(() => {
    if (!isMac) return;
    function onFocus() {
      void refreshScreenAccess();
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshScreenAccess]);

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
  }, [addHistory, openSettings, pushToast, refreshScreenAccess, refreshStatus]);

  const handleSettingsSection = useCallback((section: SettingsSection) => {
    setSettingsSection(section);
    setSettingsNav((value) => value + 1);
    setView("settings");
  }, []);

  const handleSettingsActive = useCallback((section: SettingsSection) => {
    setSettingsSection(section);
  }, []);

  const remoteEnabled = settings.remoteEnabled && settings.engine !== "tesseract";
  const remoteNeedsDeploy =
    remoteEnabled && Boolean(status) && !status?.remote.deployed;
  const localSetupBlocking =
    !remoteEnabled &&
    Boolean(status?.setup.needed) &&
    settings.engine !== "tesseract" &&
    (settings.engine === "llamacpp" || !status?.tesseract);
  const setupBlocking = remoteEnabled ? remoteNeedsDeploy : localSetupBlocking;

  useEffect(() => {
    setupBlockingRef.current = setupBlocking;
  }, [setupBlocking]);

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
    pushToast,
    remoteNeedsDeploy,
    screenAccessState,
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
    [pushToast, showInputError],
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
  }, [pushToast, showInputError]);

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

  const requestPermission = useCallback(() => {
    setSetupOpen(false);
    setPermissionOpen(true);
  }, []);

  const handleRequestPermission = useCallback(async () => {
    if (permissionBusy) return;
    setPermissionBusy(true);
    try {
      const granted = await requestScreenAccess();
      const access = await screenAccess().catch(() => null);
      if (access) setScreenAccessState(access);
      if (granted || access === "granted") {
        pushToast("ok", "屏幕录制权限已开启", "现在可以截图了。");
      } else {
        pushToast(
          "info",
          "还需要在系统设置里勾选",
          "勾选 ocr4code 后回来点“重新检查”，或重启应用。",
        );
      }
    } catch (cause) {
      pushToast(
        "error",
        "无法请求权限",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setPermissionBusy(false);
    }
  }, [permissionBusy, pushToast]);

  const handleOpenPermissionSettings = useCallback(async () => {
    try {
      await openScreenSettings();
    } catch {
      pushToast(
        "error",
        "无法打开系统设置",
        "请手动打开 系统设置 → 隐私与安全性 → 屏幕录制。",
      );
    }
  }, [pushToast]);

  const handleRecheckPermission = useCallback(async () => {
    setPermissionBusy(true);
    try {
      const access = await screenAccess();
      setScreenAccessState(access);
      if (access === "granted" || access === "not-needed") {
        pushToast("ok", "权限已开启", "可以开始截图了。");
      } else {
        pushToast(
          "info",
          "还没有授权",
          "在系统设置里勾选 ocr4code；如果已经勾选，请重启应用。",
        );
      }
    } catch (cause) {
      pushToast(
        "error",
        "无法检查权限",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setPermissionBusy(false);
    }
  }, [pushToast]);

  const handleRestart = useCallback(async () => {
    try {
      await restartApp();
    } catch (cause) {
      pushToast(
        "error",
        "无法重启应用",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [pushToast]);

  const handleCopy = useCallback(
    async (text: string, _id?: string) => {
      try {
        await copyText(text);
        if (result && text === result.text) {
          setCopied(true);
          if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
          copiedTimer.current = window.setTimeout(() => setCopied(false), 1800);
        } else {
          pushToast("ok", "已复制到剪贴板");
        }
      } catch (cause) {
        pushToast(
          "error",
          "写入剪贴板失败",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    },
    [pushToast, result],
  );

  const handleCopyError = useCallback(
    async (text: string) => {
      try {
        await copyText(text);
        pushToast("ok", "错误信息已复制", "可以贴到反馈里，方便排查。");
      } catch (cause) {
        pushToast(
          "error",
          "写入剪贴板失败",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    },
    [pushToast],
  );

  const handleSaveSettings = useCallback(
    async (draft: Settings) => {
      const qualityChanged = draft.modelQuality !== settings.modelQuality;
      const serverChanged =
        qualityChanged ||
        draft.engine !== settings.engine ||
        draft.jobs !== settings.jobs ||
        draft.maxTokens !== settings.maxTokens ||
        draft.kvCacheQuant !== settings.kvCacheQuant ||
        draft.idleSleepSecs !== settings.idleSleepSecs;
      const saved = await saveSettings(draft);
      setSettings(saved);
      setShortcutActive(true);
      if (qualityChanged) {
        setSetupDismissed(false);
        writeFlag(SETUP_DISMISSED_KEY, false);
      }
      const next = await engineStatus().catch(() => null);
      if (next) {
        setStatus(next);
        if (
          next.setup.needed &&
          saved.engine !== "tesseract" &&
          !saved.remoteEnabled
        ) {
          setSetupOpen(true);
        } else if (
          !saved.remoteEnabled &&
          serverChanged &&
          next.model.installed &&
          saved.engine !== "tesseract"
        ) {
          // Save already stopped the old managed server; bring the newly
          // selected weight pack up so the next screenshot is instant.
          setEngineBusy("start");
          try {
            const started = await startEngine();
            setStatus(started);
          } catch (cause) {
            pushToast(
              "error",
              "模型切换后启动失败",
              cause instanceof Error ? cause.message : String(cause),
            );
          } finally {
            setEngineBusy(null);
          }
        }
      } else {
        void refreshStatus();
      }
    },
    [
      pushToast,
      refreshStatus,
      settings.engine,
      settings.modelQuality,
      settings.jobs,
      settings.maxTokens,
      settings.kvCacheQuant,
      settings.idleSleepSecs,
    ],
  );

  const handleTestEngine = useCallback(async () => {
    const next = await engineStatus();
    setStatus(next);
    return next;
  }, []);

  const handleDeleteHistory = useCallback((id: string) => {
    setHistory((current) => {
      const next = current.filter((entry) => entry.id !== id);
      saveHistory(next);
      return next;
    });
  }, []);

  const handleTogglePin = useCallback((id: string) => {
    setHistory((current) => {
      const next = toggleHistoryPin(current, id);
      saveHistory(next);
      return next;
    });
  }, []);

  const handleClearHistory = useCallback(() => {
    setHistory([]);
    saveHistory([]);
    pushToast("info", "历史记录已清空");
  }, [pushToast]);

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
  }, [download, status]);

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
  }, [download, status]);

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
  }, [pushToast, refreshStatus]);

  const handleResetOnboarding = useCallback(() => {
    writeFlag(FIRST_CAPTURE_KEY, false);
    writeFlag(SETUP_DISMISSED_KEY, false);
    firstCaptureDoneRef.current = false;
    setFirstCaptureDone(false);
    setFirstCaptureTip(false);
    setSetupDismissed(false);
    // Clear the old result/error so the first-run steps have room to appear.
    setResult(null);
    setError(null);
    setPartial(null);
    setPhase("idle");
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
    screenAccessState,
  ]);

  // Enforce the configured limit when it shrinks.
  useEffect(() => {
    if (!settings.historyLimit) return;
    setHistory((current) => {
      if (current.length <= settings.historyLimit) return current;
      const next = trimHistory(current, settings.historyLimit);
      saveHistory(next);
      return next;
    });
  }, [settings.historyLimit]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const typing = Boolean(
        target?.closest("input, textarea, select, [contenteditable='true']"),
      );
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        openSettings("general");
        return;
      }
      if (typing) return;
      if ((event.metaKey || event.ctrlKey) && (event.key === "1" || event.key === "2")) {
        event.preventDefault();
        setView(event.key === "1" ? "capture" : "history");
        setSetupOpen(false);
        setPermissionOpen(false);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openSettings]);

  useEffect(() => {
    return () => {
      if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    };
  }, []);

  const readiness = captureReadiness({
    status,
    engine: settings.engine,
    phase,
    shortcutActive,
    screenAccess: screenAccessState,
  });
  const permissionDenied = screenAccessState === "denied";
  const showSteps =
    !firstCaptureDone && !result && !error && !setupBlocking && !permissionDenied;

  return (
    <>
      {isPreview && <PreviewDesktop />}
      <div
        className={`app view-${view}`}
        data-preview={isPreview ? "true" : undefined}
      >
        <WindowChrome
          shortcut={settings.shortcut}
          readiness={readiness}
          version={appVersion}
          onOpenSettings={() => openSettings("general")}
        />
        <Sidebar
          view={view}
          shortcut={settings.shortcut}
          status={status}
          engineMode={settings.engine}
          download={download}
          historyCount={history.length}
          settingsSection={settingsSection}
          version={appVersion}
          onNavigate={(next) => {
            setView(next);
            setSetupOpen(false);
            setPermissionOpen(false);
          }}
          onSettingsSection={handleSettingsSection}
        />
        <main className="content" id="main-content">
          <div className="view-host" key={view}>
            {view === "capture" && (
              <CaptureView
                phase={phase}
                partial={partial}
                shortcut={settings.shortcut}
                result={result}
                error={error}
                copied={copied}
                setupBlocking={setupBlocking}
                remote={remoteEnabled}
                permissionDenied={permissionDenied}
                showSteps={showSteps}
                firstCaptureTip={firstCaptureTip}
                onCapture={() => void handleCapture()}
                onClipboard={() => void handleClipboardImage()}
                onPickImage={handlePickedImage}
                onCopy={(text) => void handleCopy(text)}
                onOpenHistory={() => setView("history")}
                onOpenSettings={openSettings}
                onSetup={() => {
                  if (remoteEnabled) openSettings("recognition");
                  else setSetupOpen(true);
                }}
                onPermission={requestPermission}
                onDismissTip={() => setFirstCaptureTip(false)}
              />
            )}
            {view === "history" && (
              <HistoryView
                entries={history}
                shortcut={settings.shortcut}
                onCopy={(text, id) => void handleCopy(text, id)}
                onDelete={handleDeleteHistory}
                onClear={handleClearHistory}
                onCapture={() => void handleCapture()}
                onTogglePin={handleTogglePin}
              />
            )}
            {view === "settings" && (
              <SettingsView
                settings={settings}
                status={status}
                download={download}
                engineBusy={engineBusy}
                appVersion={appVersion}
                screenAccess={screenAccessState}
                updateInfo={updateInfo}
                updatePhase={updatePhase}
                updateProgress={updateProgress}
                updateError={updateError}
                onCheckUpdates={() => void checkUpdates(true)}
                onDownloadUpdate={() => void downloadNow()}
                onInstallUpdate={() => void installNow()}
                onSave={handleSaveSettings}
                onTestEngine={handleTestEngine}
                onProbeEngine={probeEngine}
                onNotify={pushToast}
                onQuit={() => void quitApp()}
                onSetup={() => setSetupOpen(true)}
                onDownloadModel={() => void handleSetupStart()}
                onStartEngine={() => void handleStartEngine()}
                onStopEngine={() => void handleStopEngine()}
                onRemoveModel={() => void handleRemoveModel()}
                onCancelSetup={() => void handleSetupCancel()}
                onRequestPermission={requestPermission}
                onOpenPermissionSettings={() => void handleOpenPermissionSettings()}
                onResetOnboarding={handleResetOnboarding}
                section={settingsSection}
                sectionNav={settingsNav}
                onSectionChange={handleSettingsActive}
              />
            )}
          </div>
        </main>
        {/* Dropping works on every view, so the target belongs to the window
            rather than the capture page. */}
        {drag?.active && (
          <div
            className={`drop-overlay${drag.accepted ? "" : " is-rejected"}`}
            role="status"
            aria-live="polite"
          >
            <div className="drop-card">
              <span className="drop-icon">
                <Icon name={drag.accepted ? "image" : "alert"} size={22} />
              </span>
              <strong>
                {drag.accepted ? "松开，识别这张图片" : "只支持图片文件"}
              </strong>
              <p>
                {drag.accepted
                  ? `${IMAGE_FORMATS}。`
                  : `换成 ${IMAGE_FORMAT_LABEL} 再拖一次。`}
              </p>
            </div>
          </div>
        )}
        {toast && <Toast toast={toast} onDismiss={dismissToast} />}
      </div>
      <SetupDialog
        open={setupOpen}
        status={status}
        download={download}
        screenAccess={screenAccessState}
        firstRun={!setupDismissed && !firstCaptureDone}
        shortcut={settings.shortcut}
        onStart={() => void handleSetupStart()}
        onCancel={() => void handleSetupCancel()}
        onDismiss={handleSetupDismiss}
        onCapture={() => {
          setSetupOpen(false);
          setDownload(null);
          void handleCapture();
        }}
        onPermission={requestPermission}
        onOpenSettings={openSettings}
        onCopyError={(text) => void handleCopyError(text)}
      />
      <PermissionDialog
        open={permissionOpen}
        access={screenAccessState}
        busy={permissionBusy}
        onRequest={() => void handleRequestPermission()}
        onOpenSettings={() => void handleOpenPermissionSettings()}
        onRecheck={() => void handleRecheckPermission()}
        onRestart={() => void handleRestart()}
        onCapture={() => {
          setPermissionOpen(false);
          void handleCapture();
        }}
        onClose={() => setPermissionOpen(false)}
      />
    </>
  );
}
