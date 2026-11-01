import { describe, expect, it } from "vitest";
import {
  captureReadiness,
  engineForMode,
  engineSummary,
} from "./status";
import type {
  EngineMode,
  EngineStatus,
  Phase,
  ScreenAccess,
} from "./types";

const BASE_STATUS: EngineStatus = {
  llama: true,
  tesseract: false,
  llamaUrl: "http://127.0.0.1:8099",
  runtime: {
    bundled: true,
    system: false,
    version: "b6000",
    platform: "macos-aarch64",
    path: "/runtime/llama-server",
  },
  model: {
    installed: true,
    verified: true,
    bytes: 1_032_918_784,
    total: 1_032_918_784,
    dir: "/models/glm-ocr",
  },
  server: {
    state: "online",
    url: "http://127.0.0.1:8099",
    managed: true,
    error: null,
    quality: "low",
  },
  setup: {
    needed: false,
    downloadBytes: 1_032_918_784,
    modelName: "GLM-OCR Q4_K_M",
    license: "MIT",
    runtimeReady: true,
  },
  remote: {
    enabled: false,
    phase: "stopped",
    url: null,
    remoteUrl: null,
    tunnel: false,
    error: null,
    quality: null,
    deployed: false,
    progress: null,
  },
};

function makeStatus(mutate: (status: EngineStatus) => void = () => {}): EngineStatus {
  const status = structuredClone(BASE_STATUS);
  mutate(status);
  return status;
}

interface ReadinessCase {
  status: EngineStatus | null;
  engine: EngineMode;
  phase: Phase;
  shortcutActive: boolean;
  screenAccess: ScreenAccess;
}

const READY_CASE: ReadinessCase = {
  status: BASE_STATUS,
  engine: "auto",
  phase: "idle",
  shortcutActive: true,
  screenAccess: "granted",
};

function readiness(overrides: Partial<ReadinessCase> = {}) {
  return captureReadiness({ ...READY_CASE, ...overrides });
}

describe("captureReadiness — idle and phase states", () => {
  it("reports a ready engine", () => {
    expect(readiness()).toStrictEqual({ state: "ready", label: "就绪", tone: "ok" });
  });

  it("reports checking while the first status request is in flight", () => {
    expect(readiness({ status: null })).toStrictEqual({
      state: "checking",
      label: "检查中",
      tone: "muted",
    });
  });

  it("reports an unavailable shortcut before anything else", () => {
    expect(readiness({ shortcutActive: false, status: null })).toStrictEqual({
      state: "shortcut-unavailable",
      label: "快捷键不可用",
      tone: "warn",
    });
  });

  it("lets an active capture phase win over blocking setup and permission states", () => {
    const blocked = makeStatus((status) => {
      status.setup.needed = true;
      status.model.installed = false;
    });
    const blocking = {
      status: blocked,
      engine: "llamacpp" as EngineMode,
      screenAccess: "denied" as ScreenAccess,
    };

    expect(readiness({ ...blocking, phase: "selecting" })).toStrictEqual({
      state: "busy",
      label: "选择区域中",
      tone: "ok",
    });
    expect(readiness({ ...blocking, phase: "starting" })).toStrictEqual({
      state: "starting",
      label: "正在启动",
      tone: "ok",
    });
    expect(readiness({ ...blocking, phase: "recognizing" })).toStrictEqual({
      state: "busy",
      label: "正在识别",
      tone: "ok",
    });
  });
});

describe("captureReadiness — model setup comes before screen permission", () => {
  const needsSetup = makeStatus((status) => {
    status.setup.needed = true;
    status.model.installed = false;
    status.server.state = "stopped";
    status.llama = false;
  });

  it("asks for the model first when the local engine is selected", () => {
    expect(
      readiness({
        status: needsSetup,
        engine: "llamacpp",
        screenAccess: "denied",
      }),
    ).toStrictEqual({ state: "needs-setup", label: "需要下载模型", tone: "warn" });
  });

  it("asks for the model first in auto mode when tesseract is unavailable", () => {
    expect(
      readiness({
        status: needsSetup,
        engine: "auto",
        screenAccess: "denied",
      }),
    ).toStrictEqual({ state: "needs-setup", label: "需要下载模型", tone: "warn" });
  });

  it("asks for permission first in auto mode when tesseract can cover the capture", () => {
    const withTesseract = structuredClone(needsSetup);
    withTesseract.tesseract = true;

    expect(
      readiness({
        status: withTesseract,
        engine: "auto",
        screenAccess: "denied",
      }),
    ).toStrictEqual({ state: "needs-permission", label: "需要授权", tone: "warn" });
  });

  it("asks for permission once the model is installed, even if the server is stopped", () => {
    expect(
      readiness({ screenAccess: "denied" }),
    ).toStrictEqual({ state: "needs-permission", label: "需要授权", tone: "warn" });
  });
});

describe("captureReadiness — local engines", () => {
  it("is ready for tesseract when it is installed", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = true;
      value.server.state = "stopped";
      value.model.installed = false;
    });

    expect(readiness({ status, engine: "tesseract" })).toStrictEqual({
      state: "ready",
      label: "就绪",
      tone: "ok",
    });
  });

  it("errors when tesseract is selected but missing", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = false;
      value.server.state = "stopped";
    });

    expect(readiness({ status, engine: "tesseract" })).toStrictEqual({
      state: "error",
      label: "tesseract 不可用",
      tone: "danger",
    });
  });

  it("reports the server starting and failing", () => {
    const starting = makeStatus((value) => {
      value.llama = false;
      value.server.state = "starting";
    });
    const failed = makeStatus((value) => {
      value.llama = false;
      value.server.state = "failed";
    });

    expect(readiness({ status: starting })).toStrictEqual({
      state: "starting",
      label: "正在启动",
      tone: "ok",
    });
    expect(readiness({ status: failed })).toStrictEqual({
      state: "error",
      label: "OCR 引擎出错",
      tone: "danger",
    });
  });

  it("errors when no runtime is bundled or installed", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.server.state = "stopped";
      value.runtime.bundled = false;
      value.runtime.system = false;
    });

    expect(readiness({ status })).toStrictEqual({
      state: "error",
      label: "运行时缺失",
      tone: "danger",
    });
  });

  it("stays ready through the tesseract fallback without the local model", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = true;
      value.server.state = "stopped";
    });

    expect(readiness({ status })).toStrictEqual({
      state: "ready",
      label: "就绪",
      tone: "ok",
    });
  });

  it("reports 待启动 when the model is installed but the server sleeps", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = false;
      value.server.state = "stopped";
    });

    expect(readiness({ status })).toStrictEqual({
      state: "ready",
      label: "待启动",
      tone: "muted",
    });
  });
});

describe("captureReadiness — remote deployment", () => {
  function remoteStatus(phase: EngineStatus["remote"]["phase"], deployed: boolean) {
    return makeStatus((status) => {
      status.remote.enabled = true;
      status.remote.phase = phase;
      status.remote.deployed = deployed;
    });
  }

  it("tracks probing, deploying and starting as transient states", () => {
    expect(readiness({ status: remoteStatus("probing", true) })).toStrictEqual({
      state: "starting",
      label: "正在检查服务器",
      tone: "ok",
    });
    expect(readiness({ status: remoteStatus("deploying", false) })).toStrictEqual({
      state: "starting",
      label: "正在部署模型",
      tone: "ok",
    });
    expect(readiness({ status: remoteStatus("starting", true) })).toStrictEqual({
      state: "starting",
      label: "正在启动",
      tone: "ok",
    });
  });

  it("surfaces a failed remote deployment as an engine error", () => {
    expect(readiness({ status: remoteStatus("failed", true) })).toStrictEqual({
      state: "error",
      label: "远程 OCR 引擎出错",
      tone: "danger",
    });
  });

  it("asks for a deployment when the model is missing on the server", () => {
    expect(readiness({ status: remoteStatus("stopped", false) })).toStrictEqual({
      state: "needs-setup",
      label: "需要部署模型",
      tone: "warn",
    });
  });

  it("is ready when the remote server is online and deployed", () => {
    expect(readiness({ status: remoteStatus("online", true) })).toStrictEqual({
      state: "ready",
      label: "就绪",
      tone: "ok",
    });
  });

  it("is ready but idle when the deployment exists but the server is stopped", () => {
    expect(readiness({ status: remoteStatus("stopped", true) })).toStrictEqual({
      state: "ready",
      label: "待启动",
      tone: "muted",
    });
  });

  it("ignores the remote state when tesseract is the selected engine", () => {
    const status = remoteStatus("failed", false);
    status.tesseract = true;

    expect(readiness({ status, engine: "tesseract" })).toStrictEqual({
      state: "ready",
      label: "就绪",
      tone: "ok",
    });
  });
});

describe("engineSummary", () => {
  it("describes an unknown engine as checking", () => {
    expect(engineSummary(null)).toStrictEqual({
      label: "检查 OCR 引擎…",
      detail: "本地 OCR 引擎",
      tone: "muted",
    });
  });

  it("reports a managed local model and an external service differently", () => {
    expect(engineSummary(BASE_STATUS)).toStrictEqual({
      label: "OCR 引擎已就绪",
      detail: "本地模型",
      tone: "ok",
    });

    const external = makeStatus((status) => {
      status.server.managed = false;
    });
    expect(engineSummary(external)).toStrictEqual({
      label: "OCR 引擎已就绪",
      detail: "外部服务",
      tone: "ok",
    });
  });

  it("reports the tesseract fallback with a warning", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = true;
      value.server.state = "stopped";
    });

    expect(engineSummary(status)).toStrictEqual({
      label: "tesseract",
      detail: "本地模型未启动",
      tone: "warn",
    });
  });

  it("tells the user to download the model first", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.tesseract = false;
      value.server.state = "stopped";
      value.setup.needed = true;
      value.model.installed = false;
    });

    expect(engineSummary(status)).toStrictEqual({
      label: "未下载模型",
      detail: "下载后即可截图",
      tone: "danger",
    });
  });

  it("reports a missing runtime before the server state", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.server.state = "stopped";
      value.runtime.bundled = false;
      value.runtime.system = false;
    });

    expect(engineSummary(status)).toStrictEqual({
      label: "运行时缺失",
      detail: "请使用完整安装包",
      tone: "danger",
    });
  });

  it("surfaces a starting or failed server as the engine state", () => {
    const starting = makeStatus((value) => {
      value.llama = false;
      value.server.state = "starting";
    });
    const failed = makeStatus((value) => {
      value.llama = false;
      value.server.state = "failed";
    });

    expect(engineSummary(starting)).toStrictEqual({
      label: "正在启动",
      detail: "正在加载模型",
      tone: "ok",
    });
    expect(engineSummary(failed)).toStrictEqual({
      label: "OCR 引擎启动失败",
      detail: "可在设置中重试",
      tone: "danger",
    });
  });

  it("prefers the remote summary when remote mode is enabled", () => {
    const status = makeStatus((value) => {
      value.remote.enabled = true;
      value.remote.phase = "online";
      value.remote.deployed = true;
    });

    expect(engineSummary(status)).toStrictEqual({
      label: "OCR 引擎已就绪",
      detail: "远程服务器",
      tone: "ok",
    });
  });
});

describe("engineForMode", () => {
  it("follows the selected engine rather than the health check result", () => {
    expect(engineForMode(BASE_STATUS, "tesseract")).toStrictEqual({
      label: "tesseract 不可用",
      detail: "安装 tesseract，或换回本地模型",
      tone: "danger",
    });

    const withTesseract = makeStatus((status) => {
      status.tesseract = true;
    });
    expect(engineForMode(withTesseract, "tesseract")).toStrictEqual({
      label: "tesseract 已就绪",
      detail: "tesseract · 不需要模型",
      tone: "ok",
    });
  });

  it("shows the download size when the local model is missing", () => {
    const status = makeStatus((value) => {
      value.llama = false;
      value.server.state = "stopped";
      value.model.installed = false;
    });

    expect(engineForMode(status, "auto")).toStrictEqual({
      label: "未下载模型",
      detail: "需要下载 1.03 GB",
      tone: "warn",
    });
  });

  it("uses the llama URL as the ready detail", () => {
    expect(engineForMode(BASE_STATUS, "llamacpp")).toStrictEqual({
      label: "OCR 引擎已就绪",
      detail: "http://127.0.0.1:8099",
      tone: "ok",
    });
  });

  it("falls back to the shared summary when no status is available", () => {
    expect(engineForMode(null, "auto")).toStrictEqual(engineSummary(null));
  });
});
