import type { EngineMode, EngineStatus } from "./types";

export type StatusTone = "ok" | "warn" | "danger" | "muted";

export interface EngineSummary {
  label: string;
  detail: string;
  tone: StatusTone;
}

export function engineSummary(status: EngineStatus | null): EngineSummary {
  if (!status) {
    return { label: "Checking engines…", detail: "llama.cpp · tesseract", tone: "muted" };
  }
  if (status.llama) {
    return {
      label: status.server.managed && status.server.state === "online"
        ? "llama.cpp ready"
        : "llama.cpp online",
      detail: status.server.managed ? "Bundled local engine" : "External server",
      tone: "ok",
    };
  }
  if (status.tesseract) {
    return {
      label: "tesseract fallback",
      detail: status.model.installed ? "llama.cpp unavailable" : "local model not installed",
      tone: "warn",
    };
  }
  if (status.setup.needed) {
    return {
      label: "Setup needed",
      detail: "Download the local OCR model",
      tone: "danger",
    };
  }
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "Runtime missing", detail: "Rebuild with the bundled runtime", tone: "danger" };
  }
  if (status.server.state === "starting") {
    return { label: "Starting engine", detail: "Loading the local model", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "Engine failed", detail: "Open Settings for the log hint", tone: "danger" };
  }
  return { label: "Engine stopped", detail: "Starts on the next capture", tone: "warn" };
}

/** Status strip in Settings follows the engine the user selected, not just health. */
export function engineForMode(
  status: EngineStatus | null,
  mode: EngineMode,
): EngineSummary {
  if (mode === "tesseract") {
    return status?.tesseract
      ? { label: "tesseract ready", detail: "Local CPU engine", tone: "ok" }
      : { label: "tesseract missing", detail: "Install it or pick another engine", tone: "danger" };
  }

  if (!status) return engineSummary(null);
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "Runtime missing", detail: "This build has no llama.cpp runtime", tone: "danger" };
  }
  if (status.llama) {
    return { label: "llama.cpp online", detail: status.llamaUrl, tone: "ok" };
  }
  if (status.server.state === "starting") {
    return { label: "Starting llama.cpp", detail: "Loading the local model", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "llama.cpp failed", detail: "Restart it or re-run setup", tone: "danger" };
  }
  if (!status.model.installed) {
    return { label: "Model not installed", detail: "One-time 1.43 GB setup", tone: "warn" };
  }
  return { label: "llama.cpp stopped", detail: "Start it or capture to wake it", tone: "warn" };
}

export function phaseLabel(
  phase: "idle" | "selecting" | "starting" | "recognizing",
  shortcutActive = true,
): { label: string; tone: StatusTone } {
  if (phase === "selecting") return { label: "Select a region", tone: "ok" };
  if (phase === "starting") return { label: "Starting engine", tone: "ok" };
  if (phase === "recognizing") return { label: "Reading pixels", tone: "ok" };
  return shortcutActive
    ? { label: "Ready", tone: "ok" }
    : { label: "Shortcut unavailable", tone: "warn" };
}
