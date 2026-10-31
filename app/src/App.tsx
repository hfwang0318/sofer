import { useCallback, useEffect, useRef, useState } from "react";

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
  onEngineDownload,
  onEngineState,
  onError,
  onPhase,
  onResult,
  quitApp,
  removeModel,
  saveSettings,
  setupEngine,
  startEngine,
  stopEngine,
} from "./lib/bridge";
import {
  DEFAULT_SETTINGS,
  DEMO_RESULT,
  MODEL_NAME,
  MODEL_TOTAL_BYTES,
  SETUP_DISMISSED_KEY,
} from "./lib/defaults";
import { fromResult, loadHistory, saveHistory } from "./lib/history";
import { isPreview, isTauri } from "./lib/platform";
import type {
  DownloadProgress,
  EngineStatus,
  HistoryEntry,
  OcrError,
  OcrResult,
  Phase,
  Settings,
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

function initialDismissed(): boolean {
  try {
    return localStorage.getItem(SETUP_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

const DEMO_ERROR: OcrError = {
  message:
    "无法连接本地 OCR 引擎。内置 llama-server 没有运行，也没有外部服务响应。",
  hint: "请到设置中检查本地引擎，或重试初始化。模型只需下载一次。",
  code: "engine-unavailable",
};

export default function App() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [view, setView] = useState<View>(initialView);
  const [phase, setPhase] = useState<Phase>("idle");
  const [result, setResult] = useState<OcrResult | null>(
    demo === "result" ? DEMO_RESULT : null,
  );
  const [error, setError] = useState<OcrError | null>(demo === "error" ? DEMO_ERROR : null);
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);
  const [shortcutActive, setShortcutActive] = useState(true);
  const [copied, setCopied] = useState(false);
  const [toast, setToast] = useState<ToastMessage | null>(null);
  const [download, setDownload] = useState<DownloadProgress | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [setupDismissed, setSetupDismissed] = useState(initialDismissed);
  const [engineBusy, setEngineBusy] = useState<"start" | "stop" | "remove" | null>(null);

  const toastId = useRef(0);
  const copiedTimer = useRef<number | null>(null);
  const settingsRef = useRef(settings);
  const phaseRef = useRef(phase);
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

  const addHistory = useCallback((next: OcrResult) => {
    setHistory((current) => {
      const limit = settingsRef.current.historyLimit || 20;
      const entry = fromResult(next);
      const merged = [entry, ...current.filter((item) => item.id !== entry.id)].slice(
        0,
        limit,
      );
      saveHistory(merged);
      return merged;
    });
  }, []);

  // Load persisted settings and check the engine once.
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [loaded, engine, info] = await Promise.all([
          loadSettings(),
          engineStatus(),
          appInfo(),
        ]);
        if (!alive) return;
        setSettings(loaded);
        setStatus(engine);
        setShortcutActive(info.shortcutActive);
        if (!info.shortcutActive) {
          pushToast(
            "error",
            "全局快捷键不可用",
            `${info.shortcut} 可能被其他应用占用，请到设置中换一个组合。`,
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
    if (settings.engine === "tesseract") return;
    if (!status.setup.needed) return;
    // If tesseract is available, the banner is enough; do not interrupt a
    // working fallback with a modal on every launch.
    if (status.tesseract && settings.engine === "auto") return;
    if (!setupDismissed) setSetupOpen(true);
  }, [status, settings.engine, setupDismissed]);

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

    track(onPhase((payload) => setPhase(payload.phase)));
    track(
      onResult((next) => {
        setPhase("idle");
        setResult(next);
        setError(null);
        setView("capture");
        setCopied(false);
        addHistory(next);
        if (next.copied) {
          pushToast(
            "ok",
            `已复制 ${next.lines} 行`,
            `${next.chars.toLocaleString()} 个字符 · 可以直接粘贴`,
          );
        } else {
          pushToast("info", "已识别，未自动复制", "自动复制已在设置中关闭。");
        }
      }),
    );
    track(
      onError((payload) => {
        setPhase("idle");
        setError(payload);
        setView("capture");
        if (payload.code === "setup-required") setSetupOpen(true);
        pushToast("error", "截图失败", payload.message);
      }),
    );

    track(
      onEngineDownload((progress) => {
        setDownload(progress);
        if (progress.phase === "ready") {
          setError(null);
          setSetupDismissed(false);
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
          try {
            localStorage.removeItem(SETUP_DISMISSED_KEY);
          } catch {
            /* ignore */
          }
          pushToast(
            "ok",
            "本地引擎已就绪",
            `${MODEL_NAME} 已安装，服务正在运行。`,
          );
          void refreshStatus();
        } else if (progress.phase === "error") {
          setSetupOpen(true);
          pushToast("error", "引擎设置失败", progress.message ?? undefined);
        } else if (progress.phase === "cancelled") {
          pushToast("info", "下载已取消", "已下载的部分会保留，可继续。");
          void refreshStatus();
        }
      }),
    );

    track(onEngineState(() => void refreshStatus()));

    return () => {
      disposed = true;
      unsubscribers.forEach((unsubscribe) => unsubscribe());
    };
  }, [addHistory, pushToast, refreshStatus]);

  const handleCapture = useCallback(async () => {
    if (phaseRef.current !== "idle") return;
    if (
      status?.setup.needed &&
      settingsRef.current.engine !== "tesseract" &&
      (settingsRef.current.engine === "llamacpp" || !status.tesseract)
    ) {
      setSetupOpen(true);
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
  }, [pushToast, status]);

  const handleCopy = useCallback(
    async (text: string, _id?: string) => {
      try {
        await copyText(text);
        pushToast("ok", "已复制到剪贴板");
        if (result && text === result.text) {
          setCopied(true);
          if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
          copiedTimer.current = window.setTimeout(() => setCopied(false), 1800);
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

  const handleSaveSettings = useCallback(
    async (draft: Settings) => {
      const saved = await saveSettings(draft);
      setSettings(saved);
      setShortcutActive(true);
      void refreshStatus();
      pushToast("ok", "设置已保存", "快捷键和内核参数已生效。");
    },
    [pushToast, refreshStatus],
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

  const handleClearHistory = useCallback(() => {
    setHistory([]);
    saveHistory([]);
    pushToast("info", "历史记录已清空", "引擎和设置不受影响。");
  }, [pushToast]);

  const handleSetupStart = useCallback(async () => {
    setDownload({
      phase: "downloading",
      file: null,
      downloaded: 0,
      total: status?.setup.downloadBytes ?? MODEL_TOTAL_BYTES,
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
        total: status?.setup.downloadBytes ?? MODEL_TOTAL_BYTES,
        bytesPerSec: 0,
        etaSecs: null,
        message,
      });
    }
  }, [status]);

  const handleSetupCancel = useCallback(async () => {
    try {
      await cancelEngineSetup();
    } catch (cause) {
      pushToast(
        "error",
        "无法取消设置",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [pushToast]);

  const handleSetupDismiss = useCallback(() => {
    if (download && ["downloading", "verifying", "starting"].includes(download.phase)) {
      return;
    }
    setSetupOpen(false);
    setDownload(null);
    if (status && !status.model.installed) {
      setSetupDismissed(true);
      try {
        localStorage.setItem(SETUP_DISMISSED_KEY, "1");
      } catch {
        /* ignore */
      }
    }
  }, [download, status]);

  const handleStartEngine = useCallback(async () => {
    setEngineBusy("start");
    try {
      const next = await startEngine();
      setStatus(next);
      pushToast("ok", "本地引擎已启动", next.llamaUrl);
    } catch (cause) {
      pushToast(
        "error",
        "无法启动本地引擎",
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
      pushToast("info", "本地引擎已停止", "下次截图会自动启动。");
    } catch (cause) {
      pushToast(
        "error",
        "无法停止本地引擎",
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
      try {
        localStorage.removeItem(SETUP_DISMISSED_KEY);
      } catch {
        /* ignore */
      }
      await refreshStatus();
      setSetupOpen(true);
      pushToast("info", "模型已移除", "需要本地 OCR 时重新下载即可。");
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

  // Enforce the configured limit when it shrinks.
  useEffect(() => {
    if (!settings.historyLimit) return;
    setHistory((current) => {
      if (current.length <= settings.historyLimit) return current;
      const next = current.slice(0, settings.historyLimit);
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
        setView("settings");
        return;
      }
      if (typing) return;
      if ((event.metaKey || event.ctrlKey) && (event.key === "1" || event.key === "2")) {
        event.preventDefault();
        setView(event.key === "1" ? "capture" : "history");
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    return () => {
      if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    };
  }, []);

  const engineName = status?.llama
    ? "llama.cpp"
    : status?.tesseract
      ? "tesseract"
      : settings.engine === "tesseract"
        ? "tesseract"
        : "llama.cpp";
  const setupNeeded = Boolean(status?.setup.needed) && settings.engine !== "tesseract";
  const setupBlocking =
    setupNeeded &&
    (settings.engine === "llamacpp" || !status?.tesseract);

  return (
    <>
      {isPreview && <PreviewDesktop />}
      <div
        className={`app view-${view}`}
        data-preview={isPreview ? "true" : undefined}
      >
        <WindowChrome
          phase={phase}
          shortcut={settings.shortcut}
          status={status}
          shortcutActive={shortcutActive}
          setupNeeded={setupBlocking}
          onOpenSettings={() => setView("settings")}
        />
        <Sidebar
          view={view}
          shortcut={settings.shortcut}
          status={status}
          historyCount={history.length}
          onNavigate={setView}
          onCapture={() => void handleCapture()}
        />
        <main className="content" id="main-content">
          <div className="view-host" key={view}>
            {view === "capture" && (
              <CaptureView
                phase={phase}
                shortcut={settings.shortcut}
                result={result}
                error={error}
                copied={copied}
                engineName={engineName}
                setupNeeded={setupNeeded}
                setupBlocking={setupBlocking}
                onSetup={() => setSetupOpen(true)}
                onCapture={() => void handleCapture()}
                onCopy={(text) => void handleCopy(text)}
                onOpenHistory={() => setView("history")}
                onOpenSettings={() => setView("settings")}
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
              />
            )}
            {view === "settings" && (
              <SettingsView
                settings={settings}
                status={status}
                download={download}
                engineBusy={engineBusy}
                onSave={handleSaveSettings}
                onTestEngine={handleTestEngine}
                onNotify={pushToast}
                onQuit={() => void quitApp()}
                onSetup={() => setSetupOpen(true)}
                onDownloadModel={() => void handleSetupStart()}
                onStartEngine={() => void handleStartEngine()}
                onStopEngine={() => void handleStopEngine()}
                onRemoveModel={() => void handleRemoveModel()}
                onCancelSetup={() => void handleSetupCancel()}
              />
            )}
          </div>
        </main>
        {toast && <Toast toast={toast} onDismiss={dismissToast} />}
      </div>
      <SetupDialog
        open={setupOpen}
        status={status}
        download={download}
        onStart={() => void handleSetupStart()}
        onCancel={() => void handleSetupCancel()}
        onDismiss={handleSetupDismiss}
        onCapture={() => {
          setSetupOpen(false);
          setDownload(null);
          void handleCapture();
        }}
        onOpenSettings={() => {
          setSetupOpen(false);
          setView("settings");
        }}
      />
    </>
  );
}
