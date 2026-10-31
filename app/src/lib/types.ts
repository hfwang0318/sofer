export type EngineMode = "auto" | "llamacpp" | "tesseract";
export type ModelQuality = "high" | "low";
export type SettingsSection =
  | "general"
  | "recognition"
  | "output"
  | "advanced"
  | "about";
export type OutputFormat = "text" | "markdown" | "json";
export type View = "capture" | "history" | "settings";
export type Phase = "idle" | "selecting" | "starting" | "recognizing";
export type ScreenAccess = "granted" | "denied" | "not-needed" | "unknown";
export type ReadinessState =
  | "checking"
  | "needs-setup"
  | "needs-permission"
  | "starting"
  | "busy"
  | "error"
  | "shortcut-unavailable"
  | "ready";

/** Mirrors `Settings` in src-tauri/src/settings.rs. */
export interface Settings {
  shortcut: string;
  autoCopy: boolean;
  historyLimit: number;
  useMirror: boolean;
  proxyUrl: string;
  proxyUsername: string;
  proxyPassword: string;
  modelQuality: ModelQuality;
  idleSleepSecs: number;
  kvCacheQuant: boolean;
  uiScale: number;
  autoCheckUpdates: boolean;
  autoDownloadUpdates: boolean;
  engine: EngineMode;
  llamaUrl: string;
  prompt: string;
  maxTokens: number;
  tessLang: string;
  psm: number;
  tile: boolean;
  geometry: boolean;
  joinWrap: boolean;
  keepGutter: boolean;
  format: OutputFormat;
  language: string;
  jobs: number;
}

export type ServerState = "stopped" | "starting" | "online" | "failed";
export type SetupPhase =
  | "downloading"
  | "verifying"
  | "starting"
  | "ready"
  | "error"
  | "cancelled";

export interface RuntimeInfo {
  bundled: boolean;
  system: boolean;
  version: string | null;
  platform: string;
  path: string | null;
}

export interface ModelInfo {
  installed: boolean;
  verified: boolean;
  bytes: number;
  total: number;
  dir: string;
}

export interface ServerInfo {
  state: ServerState;
  url: string | null;
  managed: boolean;
  error: string | null;
  quality: ModelQuality | null;
}

export interface SetupInfo {
  needed: boolean;
  downloadBytes: number;
  modelName: string;
  license: string;
  runtimeReady: boolean;
}

export interface EngineStatus {
  llama: boolean;
  tesseract: boolean;
  llamaUrl: string;
  runtime: RuntimeInfo;
  model: ModelInfo;
  server: ServerInfo;
  setup: SetupInfo;
}

export interface DownloadProgress {
  phase: SetupPhase;
  file: string | null;
  downloaded: number;
  total: number;
  bytesPerSec: number;
  etaSecs: number | null;
  message: string | null;
}

export interface AppInfo {
  shortcutActive: boolean;
  shortcut: string;
  version: string;
}

/** Mirrors `OcrResult` in src-tauri/src/capture.rs. */
export interface OcrResult {
  id: string;
  text: string;
  engine: string;
  ms: number;
  width: number;
  height: number;
  language: string | null;
  format: string;
  chars: number;
  lines: number;
  thumbnail: string | null;
  capturedAt: number;
  copied: boolean;
}

export interface OcrError {
  message: string;
  hint: string;
  code?: string | null;
}

export interface HistoryEntry {
  id: string;
  text: string;
  engine: string;
  ms: number;
  width: number;
  height: number;
  language: string | null;
  format: string;
  chars: number;
  lines: number;
  capturedAt: number;
}

export interface PhasePayload {
  phase: Phase;
}

export type ToastTone = "ok" | "error" | "info";

export interface ToastMessage {
  id: number;
  tone: ToastTone;
  title: string;
  detail?: string;
}
