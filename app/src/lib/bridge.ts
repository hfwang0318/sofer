import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  DEFAULT_SETTINGS,
  DEMO_RESULT,
  MODEL_LICENSE,
  MODEL_NAME,
  MODEL_TOTAL_BYTES,
  SETTINGS_KEY,
} from "./defaults";
import { isTauri } from "./platform";
import type {
  AppInfo,
  DownloadProgress,
  EngineStatus,
  OcrError,
  OcrResult,
  PhasePayload,
  ServerInfo,
  ServerState,
  Settings,
} from "./types";

// ---------------------------------------------------------------------------
// A tiny event bus for the browser preview. Inside Tauri the same events come
// from Rust, so the React tree does not know which world it is in.
// ---------------------------------------------------------------------------

type Handler = (payload: unknown) => void;
const bus = new Map<string, Set<Handler>>();

function emitPreview(event: string, payload: unknown) {
  bus.get(event)?.forEach((handler) => handler(payload));
}

function subscribe<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (isTauri) {
    return listen<T>(event, (message) => handler(message.payload));
  }
  const wrapped: Handler = (payload) => handler(payload as T);
  if (!bus.has(event)) bus.set(event, new Set());
  bus.get(event)!.add(wrapped);
  return Promise.resolve(() => {
    bus.get(event)?.delete(wrapped);
  });
}

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

const demo = isTauri
  ? null
  : new URLSearchParams(window.location.search).get("demo");

let mockInstalled = demo !== "setup";
let mockServer: ServerState = mockInstalled ? "online" : "stopped";
let mockTesseract = demo !== "setup" && demo !== "error";
let mockSetupRunning = false;
let mockSetupCancelled = false;
let mockSetupInterval: number | null = null;
let mockSetupTimers: number[] = [];

function clearMockSetup() {
  if (mockSetupInterval !== null) window.clearInterval(mockSetupInterval);
  mockSetupInterval = null;
  mockSetupTimers.forEach((timer) => window.clearTimeout(timer));
  mockSetupTimers = [];
}

function mockLlamaUrl(): string {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return JSON.parse(raw).llamaUrl ?? DEFAULT_SETTINGS.llamaUrl;
  } catch {
    /* ignore */
  }
  return DEFAULT_SETTINGS.llamaUrl;
}

function mockEngineStatus(): EngineStatus {
  const offlineDemo = demo === "error";
  const online = mockInstalled && mockServer === "online" && !offlineDemo;
  const llamaUrl = online ? "http://127.0.0.1:18099" : mockLlamaUrl();
  return {
    llama: online,
    tesseract: mockTesseract,
    llamaUrl,
    runtime: {
      bundled: true,
      system: false,
      version: "llama.cpp b12018 (v0.8.0-dev)",
      platform: "macos-aarch64",
      path: "…/Resources/runtime/llama/llama-server",
    },
    model: {
      installed: mockInstalled,
      verified: mockInstalled,
      bytes: mockInstalled ? MODEL_TOTAL_BYTES : 0,
      total: MODEL_TOTAL_BYTES,
      dir: "…/models/glm-ocr",
    },
    server: {
      state: offlineDemo ? "stopped" : mockServer,
      url: online ? "http://127.0.0.1:18099" : null,
      managed: true,
      error: null,
    },
    setup: {
      needed: !mockInstalled && !online,
      downloadBytes: MODEL_TOTAL_BYTES,
      modelName: MODEL_NAME,
      license: MODEL_LICENSE,
      runtimeReady: true,
    },
  };
}

function mockDownload(progress: Partial<DownloadProgress> & { phase: DownloadProgress["phase"] }): DownloadProgress {
  return {
    file: null,
    downloaded: 0,
    total: MODEL_TOTAL_BYTES,
    bytesPerSec: 0,
    etaSecs: null,
    message: null,
    ...progress,
  };
}

function startMockSetup() {
  if (mockSetupRunning) return;
  if (mockInstalled && mockServer === "online") {
    emitPreview("engine:download", mockDownload({ phase: "ready", downloaded: MODEL_TOTAL_BYTES }));
    return;
  }
  mockSetupRunning = true;
  mockSetupCancelled = false;
  clearMockSetup();

  let downloaded = 0;
  const speed = 78_000_000;
  // Roughly 1.43 GB at a simulated 78 MB/s, compressed into a short preview.
  mockSetupInterval = window.setInterval(() => {
    if (mockSetupCancelled) return;
    downloaded = Math.min(MODEL_TOTAL_BYTES, downloaded + MODEL_TOTAL_BYTES / 34);
    const file =
      downloaded > MODEL_TOTAL_BYTES * 0.66
        ? "mmproj-GLM-OCR-Q8_0.gguf"
        : "GLM-OCR-Q8_0.gguf";
    emitPreview(
      "engine:download",
      mockDownload({
        phase: "downloading",
        file,
        downloaded,
        bytesPerSec: speed,
        etaSecs: Math.max(0, Math.round((MODEL_TOTAL_BYTES - downloaded) / speed)),
      }),
    );

    if (downloaded >= MODEL_TOTAL_BYTES) {
      clearMockSetup();
      emitPreview("engine:download", mockDownload({ phase: "verifying", downloaded }));
      mockSetupTimers.push(
        window.setTimeout(() => {
          if (mockSetupCancelled) return;
          emitPreview("engine:download", mockDownload({ phase: "starting", downloaded }));
          mockSetupTimers.push(
            window.setTimeout(() => {
              if (mockSetupCancelled) return;
              mockInstalled = true;
              mockServer = "online";
              mockTesseract = true;
              mockSetupRunning = false;
              const state: ServerInfo = {
                state: "online",
                url: "http://127.0.0.1:18099",
                managed: true,
                error: null,
              };
              emitPreview("engine:state", state);
              emitPreview(
                "engine:download",
                mockDownload({ phase: "ready", downloaded: MODEL_TOTAL_BYTES }),
              );
            }, 1000),
          );
        }, 550),
      );
    }
  }, 120);
}

async function previewCapture() {
  emitPreview("ocr:phase", { phase: "selecting" } satisfies PhasePayload);
  await sleep(650);
  if (!mockInstalled || mockServer !== "online") {
    emitPreview("ocr:phase", { phase: "idle" } satisfies PhasePayload);
    emitPreview("ocr:error", {
      message: "本地 OCR 模型尚未安装。",
      hint: "打开引导设置，一次下载即可，无需手动安装。",
      code: "setup-required",
    } satisfies OcrError);
    return;
  }
  emitPreview("ocr:phase", { phase: "recognizing" } satisfies PhasePayload);
  await sleep(1300);

  let settings = DEFAULT_SETTINGS;
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) settings = { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch {
    /* defaults are fine */
  }

  const result: OcrResult = {
    ...DEMO_RESULT,
    id: `preview-${Date.now()}`,
    capturedAt: Date.now(),
    copied: settings.autoCopy,
  };
  if (settings.autoCopy) await copyText(result.text).catch(() => undefined);
  emitPreview("ocr:result", result);
  emitPreview("ocr:phase", { phase: "idle" } satisfies PhasePayload);
}

// ---------------------------------------------------------------------------
// Public bridge
// ---------------------------------------------------------------------------

function applyBrowserZoom(settings: Settings): void {
  if (!isTauri) {
    document.documentElement.style.setProperty("zoom", String(settings.uiScale));
  }
}

export async function loadSettings(): Promise<Settings> {
  if (isTauri) return invoke<Settings>("load_settings");
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    const settings = raw
      ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) }
      : { ...DEFAULT_SETTINGS };
    applyBrowserZoom(settings);
    return settings;
  } catch {
    applyBrowserZoom(DEFAULT_SETTINGS);
    return { ...DEFAULT_SETTINGS };
  }
}

export async function saveSettings(settings: Settings): Promise<Settings> {
  if (isTauri) return invoke<Settings>("save_settings", { settings });
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  applyBrowserZoom(settings);
  return settings;
}

export async function appInfo(): Promise<AppInfo> {
  if (isTauri) return invoke<AppInfo>("app_info");
  return { shortcutActive: true, shortcut: DEFAULT_SETTINGS.shortcut, version: "0.1.9" };
}

export async function engineStatus(): Promise<EngineStatus> {
  if (isTauri) return invoke<EngineStatus>("engine_status");
  return mockEngineStatus();
}

export async function setupEngine(): Promise<void> {
  if (isTauri) {
    await invoke("setup_engine");
    return;
  }
  startMockSetup();
}

export async function cancelEngineSetup(): Promise<void> {
  if (isTauri) {
    await invoke("cancel_engine_setup");
    return;
  }
  mockSetupCancelled = true;
  mockSetupRunning = false;
  clearMockSetup();
  emitPreview(
    "engine:download",
    mockDownload({
      phase: "cancelled",
      message: "下载已取消，已下载的部分会保留，可继续。",
    }),
  );
}

export async function startEngine(): Promise<EngineStatus> {
  if (isTauri) return invoke<EngineStatus>("start_engine");
  mockServer = "online";
  const state: ServerInfo = {
    state: "online",
    url: "http://127.0.0.1:18099",
    managed: true,
    error: null,
  };
  emitPreview("engine:state", state);
  return mockEngineStatus();
}

export async function stopEngine(): Promise<void> {
  if (isTauri) {
    await invoke("stop_engine");
    return;
  }
  mockServer = "stopped";
  emitPreview("engine:state", {
    state: "stopped",
    url: null,
    managed: false,
    error: null,
  } satisfies ServerInfo);
}

export async function removeModel(): Promise<void> {
  if (isTauri) {
    await invoke("remove_model");
    return;
  }
  mockInstalled = false;
  mockServer = "stopped";
  emitPreview("engine:state", {
    state: "stopped",
    url: null,
    managed: false,
    error: null,
  } satisfies ServerInfo);
}

export async function setShortcutEnabled(enabled: boolean): Promise<boolean> {
  if (isTauri) return invoke<boolean>("set_shortcut_enabled", { enabled });
  return enabled;
}

export async function captureNow(): Promise<void> {
  if (isTauri) {
    await invoke("capture_now");
    return;
  }
  void previewCapture();
}

export async function copyText(text: string): Promise<void> {
  if (isTauri) {
    await invoke("copy_text", { text });
    return;
  }
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
  }
}

export async function quitApp(): Promise<void> {
  if (isTauri) await invoke("quit_app");
}

export type WindowAction = "minimize" | "hide" | "toggle-maximize";

export async function windowAction(action: WindowAction): Promise<void> {
  if (!isTauri) return;
  const window = getCurrentWindow();
  if (action === "minimize") await window.minimize();
  else if (action === "hide") await window.hide();
  else if (action === "toggle-maximize") await window.toggleMaximize();
}

export const onPhase = (handler: (payload: PhasePayload) => void) =>
  subscribe<PhasePayload>("ocr:phase", handler);
export const onResult = (handler: (payload: OcrResult) => void) =>
  subscribe<OcrResult>("ocr:result", handler);
export const onError = (handler: (payload: OcrError) => void) =>
  subscribe<OcrError>("ocr:error", handler);
export const onEngineDownload = (handler: (payload: DownloadProgress) => void) =>
  subscribe<DownloadProgress>("engine:download", handler);
export const onEngineState = (handler: (payload: ServerInfo) => void) =>
  subscribe<ServerInfo>("engine:state", handler);
