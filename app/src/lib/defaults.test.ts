import { describe, expect, it } from "vitest";
import {
  DEFAULT_PROMPT,
  DEFAULT_SETTINGS,
  FIRST_CAPTURE_KEY,
  HISTORY_KEY,
  MODEL_LICENSE,
  MODEL_NAME_HIGH,
  MODEL_NAME_LOW,
  MODEL_TOTAL_BYTES_HIGH,
  MODEL_TOTAL_BYTES_LOW,
  modelName,
  modelTotalBytes,
  SETTINGS_KEY,
  SETUP_DISMISSED_KEY,
} from "./defaults";
import { isWindows } from "./platform";
import type { Settings } from "./types";

/** Keep this list in sync with `interface Settings` in types.ts. */
const SETTINGS_KEYS: Array<keyof Settings> = [
  "shortcut",
  "autoCopy",
  "notifyOnResult",
  "fastPreview",
  "historyLimit",
  "useMirror",
  "proxyUrl",
  "proxyUsername",
  "proxyPassword",
  "modelQuality",
  "idleSleepSecs",
  "kvCacheQuant",
  "uiScale",
  "autoCheckUpdates",
  "autoDownloadUpdates",
  "remoteEnabled",
  "remoteHost",
  "remoteUser",
  "remoteSshPort",
  "remoteKey",
  "remoteDir",
  "remoteServerPort",
  "remoteLocalPort",
  "remoteAccel",
  "remoteGpuLayers",
  "remoteSource",
  "remoteAutoStart",
  "engine",
  "llamaUrl",
  "promptPreset",
  "customPrompt",
  "maxTokens",
  "tessLang",
  "psm",
  "tile",
  "geometry",
  "joinWrap",
  "keepGutter",
  "format",
  "language",
  "jobs",
];

describe("DEFAULT_SETTINGS", () => {
  it("defines every field of the Settings type", () => {
    expect(Object.keys(DEFAULT_SETTINGS).sort()).toStrictEqual(
      [...SETTINGS_KEYS].sort(),
    );
    for (const key of SETTINGS_KEYS) {
      expect(DEFAULT_SETTINGS[key]).toBeDefined();
    }
  });

  it("matches the pin values shared with settings.rs", () => {
    expect(DEFAULT_SETTINGS.shortcut).toBe("CommandOrControl+Shift+2");
    expect(DEFAULT_SETTINGS.engine).toBe("auto");
    expect(DEFAULT_SETTINGS.modelQuality).toBe("low");
    expect(DEFAULT_SETTINGS.format).toBe("text");
    expect(DEFAULT_SETTINGS.historyLimit).toBe(20);
    expect(DEFAULT_SETTINGS.maxTokens).toBe(3072);
    expect(DEFAULT_SETTINGS.jobs).toBe(3);
    expect(DEFAULT_SETTINGS.idleSleepSecs).toBe(isWindows ? 1 : 300);
    expect(DEFAULT_SETTINGS.uiScale).toBe(1.15);
    expect(DEFAULT_SETTINGS.psm).toBe(6);
  });

  it("defaults KV-cache quantization on for every platform", () => {
    expect(DEFAULT_SETTINGS.kvCacheQuant).toBe(true);
  });

  it("keeps remote deployment off with an empty host", () => {
    expect(DEFAULT_SETTINGS.remoteEnabled).toBe(false);
    expect(DEFAULT_SETTINGS.remoteHost).toBe("");
    expect(DEFAULT_SETTINGS.remoteSshPort).toBe(22);
    expect(DEFAULT_SETTINGS.remoteDir).toBe("~/.sofer");
    expect(DEFAULT_SETTINGS.remoteLocalPort).toBe(0);
    expect(DEFAULT_SETTINGS.llamaUrl).toBe("http://127.0.0.1:8099");
  });

  it("ships a non-empty default prompt preset", () => {
    expect(DEFAULT_SETTINGS.promptPreset).toBe("code");
    expect(DEFAULT_PROMPT.length).toBeGreaterThan(0);
  });
});

describe("model pack helpers", () => {
  it("maps the quality setting to the pinned byte totals", () => {
    expect(modelTotalBytes("low")).toBe(MODEL_TOTAL_BYTES_LOW);
    expect(modelTotalBytes("high")).toBe(MODEL_TOTAL_BYTES_HIGH);
    expect(MODEL_TOTAL_BYTES_LOW).toBe(1_032_918_784);
    expect(MODEL_TOTAL_BYTES_HIGH).toBe(1_434_837_056);
    expect(MODEL_TOTAL_BYTES_HIGH).toBeGreaterThan(MODEL_TOTAL_BYTES_LOW);
  });

  it("maps the quality setting to the display name", () => {
    expect(modelName("low")).toBe(MODEL_NAME_LOW);
    expect(modelName("high")).toBe(MODEL_NAME_HIGH);
    expect(MODEL_NAME_LOW).toBe("GLM-OCR Q4_K_M");
    expect(MODEL_NAME_HIGH).toBe("GLM-OCR Q8_0");
  });

  it("keeps the license aligned with engine.rs", () => {
    expect(MODEL_LICENSE).toBe("MIT");
  });
});

describe("storage keys", () => {
  it("uses a versioned key for settings and history", () => {
    expect(SETTINGS_KEY).toBe("sofer.settings.v1");
    expect(HISTORY_KEY).toBe("sofer.history.v1");
    expect(SETUP_DISMISSED_KEY).toBe("sofer.setup.dismissed");
    expect(FIRST_CAPTURE_KEY).toBe("sofer.onboarding.firstCapture");
  });
});
