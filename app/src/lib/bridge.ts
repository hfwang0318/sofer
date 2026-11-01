import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  DEFAULT_SETTINGS,
  DEMO_RESULT,
  MODEL_LICENSE,
  modelName,
  modelTotalBytes,
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
  RemotePhase,
  RemoteProbe,
  RemoteProgress,
  RemoteSource,
  RemoteState,
  ScreenAccess,
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
let mockScreenAccess: ScreenAccess =
  demo === "permission" ? "denied" : "granted";
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

function mockModelQuality(): "high" | "low" {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return JSON.parse(raw).modelQuality === "low" ? "low" : "high";
  } catch {
    /* ignore */
  }
  return "high";
}

let mockRemoteDeployed = false;
let mockRemoteOnline = false;
let mockRemotePhase: RemotePhase = "stopped";
let mockRemoteProgress: RemoteProgress | null = null;
let mockDeployGeneration = 0;

function mockRemoteEnabled(): boolean {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return Boolean(JSON.parse(raw).remoteEnabled);
  } catch {
    /* ignore */
  }
  return false;
}

function mockRemoteState(): RemoteState {
  const enabled = mockRemoteEnabled();
  const online = enabled && mockRemoteOnline && mockRemotePhase === "online";
  return {
    enabled,
    phase: enabled ? mockRemotePhase : "stopped",
    url: online ? "http://127.0.0.1:28099" : null,
    remoteUrl: online ? "http://127.0.0.1:8099" : null,
    tunnel: online,
    error: null,
    quality: mockModelQuality(),
    deployed: mockRemoteDeployed,
    progress: mockRemoteProgress,
  };
}

function mockEngineStatus(): EngineStatus {
  const quality = mockModelQuality();
  const total = modelTotalBytes(quality);
  const offlineDemo = demo === "error";
  const remote = mockRemoteState();
  const online =
    (mockInstalled && mockServer === "online" && !offlineDemo) ||
    remote.phase === "online";
  const llamaUrl =
    remote.phase === "online"
      ? remote.url ?? "http://127.0.0.1:28099"
      : online
        ? "http://127.0.0.1:18099"
        : mockLlamaUrl();
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
      bytes: mockInstalled ? total : 0,
      total,
      dir: "…/models/glm-ocr",
    },
    server: {
      state: offlineDemo ? "stopped" : mockServer,
      url: online ? "http://127.0.0.1:18099" : null,
      managed: true,
      error: null,
      quality,
    },
    setup: {
      needed: !mockInstalled && !online && !remote.deployed,
      downloadBytes: total,
      modelName: modelName(quality),
      license: MODEL_LICENSE,
      runtimeReady: true,
    },
    remote,
  };
}

function mockDownload(progress: Partial<DownloadProgress> & { phase: DownloadProgress["phase"] }): DownloadProgress {
  return {
    file: null,
    downloaded: 0,
    total: modelTotalBytes(mockModelQuality()),
    bytesPerSec: 0,
    etaSecs: null,
    message: null,
    ...progress,
  };
}

function startMockSetup() {
  if (mockSetupRunning) return;
  const total = modelTotalBytes(mockModelQuality());
  const quality = mockModelQuality();
  if (mockInstalled && mockServer === "online") {
    emitPreview("engine:download", mockDownload({ phase: "ready", downloaded: total, total }));
    return;
  }
  mockSetupRunning = true;
  mockSetupCancelled = false;
  clearMockSetup();

  let downloaded = 0;
  const speed = 78_000_000;
  // Compressed into a short preview; the real total depends on the quality.
  mockSetupInterval = window.setInterval(() => {
    if (mockSetupCancelled) return;
    downloaded = Math.min(total, downloaded + total / 34);
    const file =
      downloaded > total * 0.66
        ? "mmproj-GLM-OCR-Q8_0.gguf"
        : quality === "low"
          ? "GLM-OCR.Q4_K_M.gguf"
          : "GLM-OCR-Q8_0.gguf";
    emitPreview(
      "engine:download",
      mockDownload({
        phase: "downloading",
        file,
        downloaded,
        total,
        bytesPerSec: speed,
        etaSecs: Math.max(0, Math.round((total - downloaded) / speed)),
      }),
    );

    if (downloaded >= total) {
      clearMockSetup();
      emitPreview("engine:download", mockDownload({ phase: "verifying", downloaded, total }));
      mockSetupTimers.push(
        window.setTimeout(() => {
          if (mockSetupCancelled) return;
          emitPreview("engine:download", mockDownload({ phase: "starting", downloaded, total }));
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
                quality,
              };
              emitPreview("engine:state", state);
              emitPreview(
                "engine:download",
                mockDownload({ phase: "ready", downloaded: total, total }),
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
      message: "OCR 模型还没有下载。",
      hint: "下载一次即可，之后截图都在本机处理。",
      code: "setup-required",
    } satisfies OcrError);
    return;
  }
  emitPreview("ocr:phase", { phase: "recognizing" } satisfies PhasePayload);
  await sleep(1300);

  const settings = previewSettings();
  const result: OcrResult = {
    ...DEMO_RESULT,
    id: `preview-${Date.now()}`,
    capturedAt: Date.now(),
    copied: settings.autoCopy,
    truncated: false,
  };
  if (settings.autoCopy) await copyText(result.text).catch(() => undefined);
  emitPreview("ocr:result", result);
  emitPreview("ocr:phase", { phase: "idle" } satisfies PhasePayload);
}

function previewSettings(): Settings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch {
    /* defaults are fine */
  }
  return DEFAULT_SETTINGS;
}

/** The chunked re-run of a retained screenshot. The real command reads the PNG
 * Rust kept for this; the preview replays a complete result. */
async function previewRetry() {
  emitPreview("ocr:phase", { phase: "recognizing" } satisfies PhasePayload);
  await sleep(900);
  const settings = previewSettings();
  const text = `${DEMO_RESULT.text}\n\nfn main() {\n    println!("done");\n}`;
  const result: OcrResult = {
    ...DEMO_RESULT,
    id: `preview-retry-${Date.now()}`,
    text,
    chars: text.length,
    lines: text.split("\n").length,
    capturedAt: Date.now(),
    copied: settings.autoCopy,
    truncated: false,
  };
  if (settings.autoCopy) await copyText(result.text).catch(() => undefined);
  emitPreview("ocr:result", result);
  emitPreview("ocr:phase", { phase: "idle" } satisfies PhasePayload);
}

// ---------------------------------------------------------------------------
// Public bridge
// ---------------------------------------------------------------------------

function applyBrowserZoom(settings: Settings): void {
  if (isTauri) return;
  // Zooming the root element makes 100vw overflow the viewport. Scale the body
  // instead and give it the inverse size, so the preview still fills the window.
  const root = document.documentElement;
  root.style.setProperty("--preview-scale", String(settings.uiScale));
  root.classList.add("preview-zoom");
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
  return { shortcutActive: true, shortcut: DEFAULT_SETTINGS.shortcut, version: "0.1.10" };
}

export async function engineStatus(): Promise<EngineStatus> {
  if (isTauri) return invoke<EngineStatus>("engine_status");
  return mockEngineStatus();
}

export async function screenAccess(): Promise<ScreenAccess> {
  if (isTauri) return invoke<ScreenAccess>("screen_access");
  return mockScreenAccess;
}

export async function requestScreenAccess(): Promise<boolean> {
  if (isTauri) return invoke<boolean>("request_screen_access");
  mockScreenAccess = "granted";
  return true;
}

export async function openScreenSettings(): Promise<void> {
  if (isTauri) await invoke("open_screen_settings");
}

export async function restartApp(): Promise<void> {
  if (isTauri) await invoke("restart_app");
}

export async function probeEngine(url: string): Promise<boolean> {
  if (isTauri) return invoke<boolean>("probe_engine", { url });
  return url.trim().length > 0;
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
    quality: mockModelQuality(),
  };
  emitPreview("engine:state", state);
  return mockEngineStatus();
}

export async function prepareUpdate(): Promise<void> {
  if (isTauri) await invoke("prepare_update");
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
    quality: null,
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
    quality: null,
  } satisfies ServerInfo);
}

// ---------------------------------------------------------------------------
// Remote model server
// ---------------------------------------------------------------------------

function emitRemoteState(progress?: RemoteProgress | null) {
  if (progress !== undefined) mockRemoteProgress = progress;
  emitPreview("remote:state", mockRemoteState());
  if (progress) emitPreview("remote:progress", progress);
}

function startMockDeploy(source: RemoteSource) {
  const generation = ++mockDeployGeneration;
  mockRemotePhase = "deploying";
  const phases: RemoteProgress[] = [
    {
      phase: "probing",
      message: "正在检查服务器",
      file: null,
      downloaded: 0,
      total: 0,
      bytesPerSec: 0,
      etaSecs: null,
    },
    {
      phase: source === "upload" ? "uploading" : "downloading",
      message: source === "upload" ? "正在上传模型" : "正在服务器下载模型",
      file: source === "upload" ? "GLM-OCR.Q4_K_M.gguf" : "GLM-OCR.Q4_K_M.gguf",
      downloaded: modelTotalBytes(mockModelQuality()) / 2,
      total: modelTotalBytes(mockModelQuality()),
      bytesPerSec: 24_000_000,
      etaSecs: 22,
    },
    {
      phase: "verifying",
      message: "正在校验模型",
      file: null,
      downloaded: 0,
      total: 0,
      bytesPerSec: 0,
      etaSecs: null,
    },
    {
      phase: "starting",
      message: "正在启动 OCR 引擎",
      file: null,
      downloaded: 0,
      total: 0,
      bytesPerSec: 0,
      etaSecs: null,
    },
  ];
  phases.forEach((progress, index) => {
    window.setTimeout(() => {
      if (generation !== mockDeployGeneration) return;
      if (index === phases.length - 1) {
        mockRemoteDeployed = true;
        mockRemoteOnline = true;
        mockRemotePhase = "online";
        mockRemoteProgress = null;
        emitRemoteState(null);
      } else {
        emitRemoteState(progress);
      }
    }, 600 + index * 900);
  });
}

export async function remoteProbe(): Promise<RemoteProbe> {
  if (isTauri) return invoke<RemoteProbe>("remote_probe");
  return {
    reachable: true,
    os: "Linux",
    arch: "x86_64",
    home: "/home/dev",
    dir: "/home/dev/.ocr4code",
    runtime: mockRemoteDeployed ? "/home/dev/.ocr4code/runtime/llama-server" : null,
    modelInstalled: mockRemoteDeployed,
    modelVerified: mockRemoteDeployed,
    files: [],
    diskFree: 48 * 1024 ** 3,
    curl: true,
    wget: true,
    tar: true,
    sha256: "sha256sum",
    error: null,
  };
}

export async function remoteDeploy(source: RemoteSource = "auto"): Promise<void> {
  if (isTauri) {
    await invoke("remote_deploy", { source });
    return;
  }
  mockRemoteProgress = {
    phase: "probing",
    message: "正在检查服务器",
    file: null,
    downloaded: 0,
    total: 0,
    bytesPerSec: 0,
    etaSecs: null,
  };
  emitRemoteState(mockRemoteProgress);
  startMockDeploy(source);
}

export async function remoteStart(): Promise<EngineStatus> {
  if (isTauri) return invoke<EngineStatus>("remote_start");
  mockRemoteDeployed = true;
  mockRemoteOnline = true;
  mockRemotePhase = "online";
  mockRemoteProgress = null;
  emitRemoteState(null);
  return mockEngineStatus();
}

export async function remoteStop(): Promise<EngineStatus> {
  if (isTauri) return invoke<EngineStatus>("remote_stop");
  mockRemoteOnline = false;
  mockRemotePhase = "stopped";
  mockRemoteProgress = null;
  emitRemoteState(null);
  return mockEngineStatus();
}

export async function remoteCancel(): Promise<void> {
  if (isTauri) await invoke("remote_cancel");
  mockDeployGeneration += 1;
  mockRemotePhase = "cancelled";
  mockRemoteOnline = false;
  mockRemoteProgress = null;
  emitRemoteState(null);
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

/** Re-run the retained screenshot with a chunked pass (truncated results). */
export async function retryCapture(): Promise<void> {
  if (isTauri) {
    await invoke("retry_capture");
    return;
  }
  void previewRetry();
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
export const onRemoteProgress = (handler: (payload: RemoteProgress) => void) =>
  subscribe<RemoteProgress>("remote:progress", handler);
export const onRemoteState = (handler: (payload: RemoteState) => void) =>
  subscribe<RemoteState>("remote:state", handler);
