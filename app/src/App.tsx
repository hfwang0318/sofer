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
    "The local OCR engine could not be reached. The managed llama-server is not running and no external server answered.",
  hint: "Open Settings to check the local engine, or retry setup. The model is a one-time download.",
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
            "Global shortcut unavailable",
            `${info.shortcut} is probably owned by another app. Pick another one in Settings.`,
          );
        }
      } catch (cause) {
        if (!alive) return;
        pushToast(
          "error",
          "Could not load settings",
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
            `Copied ${next.lines} ${next.lines === 1 ? "line" : "lines"}`,
            `${next.chars.toLocaleString()} characters · ready to paste`,
          );
        } else {
          pushToast("info", "Recognised — not copied", "Automatic copy is off in Settings.");
        }
      }),
    );
    track(
      onError((payload) => {
        setPhase("idle");
        setError(payload);
        setView("capture");
        if (payload.code === "setup-required") setSetupOpen(true);
        pushToast("error", "Capture failed", payload.message);
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
            "Local engine ready",
            `${MODEL_NAME} is installed and the server is running.`,
          );
          void refreshStatus();
        } else if (progress.phase === "error") {
          setSetupOpen(true);
          pushToast("error", "Engine setup failed", progress.message ?? undefined);
        } else if (progress.phase === "cancelled") {
          pushToast("info", "Download cancelled", "Partial files are kept for resume.");
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
        "Could not start capture",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [pushToast, status]);

  const handleCopy = useCallback(
    async (text: string, _id?: string) => {
      try {
        await copyText(text);
        pushToast("ok", "Copied to clipboard");
        if (result && text === result.text) {
          setCopied(true);
          if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
          copiedTimer.current = window.setTimeout(() => setCopied(false), 1800);
        }
      } catch (cause) {
        pushToast(
          "error",
          "Clipboard write failed",
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
      pushToast("ok", "Settings saved", "The shortcut and kernel options are live.");
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
    pushToast("info", "History cleared", "The engine and your settings are untouched.");
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
        "Could not cancel setup",
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
      pushToast("ok", "Local engine running", next.llamaUrl);
    } catch (cause) {
      pushToast(
        "error",
        "Could not start the local engine",
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
      pushToast("info", "Local engine stopped", "It starts again on the next capture.");
    } catch (cause) {
      pushToast(
        "error",
        "Could not stop the local engine",
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
      pushToast("info", "Model removed", "Download it again whenever you need local OCR.");
    } catch (cause) {
      pushToast(
        "error",
        "Could not remove the model",
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
