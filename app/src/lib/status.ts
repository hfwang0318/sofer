import { formatBytes } from "./format";
import type {
  EngineMode,
  EngineStatus,
  Phase,
  ReadinessState,
  ScreenAccess,
} from "./types";

export type StatusTone = "ok" | "warn" | "danger" | "muted";

export interface Readiness {
  state: ReadinessState;
  label: string;
  tone: StatusTone;
}

interface ReadinessInput {
  status: EngineStatus | null;
  engine: EngineMode;
  phase: Phase;
  shortcutActive: boolean;
  screenAccess: ScreenAccess;
}

/** One state machine for the titlebar, sidebar and capture page. */
export function captureReadiness({
  status,
  engine,
  phase,
  shortcutActive,
  screenAccess,
}: ReadinessInput): Readiness {
  if (phase === "selecting") {
    return { state: "busy", label: "选择区域中", tone: "ok" };
  }
  if (phase === "starting") {
    return { state: "starting", label: "正在启动", tone: "ok" };
  }
  if (phase === "recognizing") {
    return { state: "busy", label: "正在识别", tone: "ok" };
  }
  if (!shortcutActive) {
    return { state: "shortcut-unavailable", label: "快捷键不可用", tone: "warn" };
  }
  if (!status) {
    return { state: "checking", label: "检查中", tone: "muted" };
  }
  if (engine !== "tesseract" && status.remote?.enabled) {
    const remote = status.remote;
    if (remote.phase === "probing") {
      return { state: "starting", label: "正在检查服务器", tone: "ok" };
    }
    if (remote.phase === "deploying") {
      return { state: "starting", label: "正在部署模型", tone: "ok" };
    }
    if (remote.phase === "starting") {
      return { state: "starting", label: "正在启动", tone: "ok" };
    }
    if (remote.phase === "failed") {
      return { state: "error", label: "远程识别服务出错", tone: "danger" };
    }
    if (!remote.deployed) {
      return { state: "needs-setup", label: "需要部署模型", tone: "warn" };
    }
    if (remote.phase === "online") {
      return { state: "ready", label: "就绪", tone: "ok" };
    }
    return { state: "ready", label: "待启动", tone: "muted" };
  }
  const setupBlocking =
    status.setup.needed && (engine === "llamacpp" || !status.tesseract);
  if (engine !== "tesseract" && setupBlocking) {
    return { state: "needs-setup", label: "需要下载模型", tone: "warn" };
  }
  if (screenAccess === "denied") {
    return { state: "needs-permission", label: "需要授权", tone: "warn" };
  }
  if (engine === "tesseract") {
    return status.tesseract
      ? { state: "ready", label: "就绪", tone: "ok" }
      : { state: "error", label: "备用识别不可用", tone: "danger" };
  }
  if (status.server.state === "starting") {
    return { state: "starting", label: "正在启动", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { state: "error", label: "识别服务出错", tone: "danger" };
  }
  if (!status.runtime.bundled && !status.runtime.system) {
    return { state: "error", label: "运行时缺失", tone: "danger" };
  }
  if (status.llama || status.tesseract) {
    return { state: "ready", label: "就绪", tone: "ok" };
  }
  // The model is installed but the server starts on demand; capture still works.
  return { state: "ready", label: "待启动", tone: "muted" };
}

export interface EngineSummary {
  label: string;
  detail: string;
  tone: StatusTone;
}

/** The server is part of the deployment, so its state is the engine state. */
function remoteSummary(remote: NonNullable<EngineStatus["remote"]>): EngineSummary {
  if (remote.phase === "probing") {
    return { label: "正在检查服务器", detail: "远程服务器", tone: "ok" };
  }
  if (remote.phase === "deploying") {
    return { label: "正在部署模型", detail: "远程服务器", tone: "ok" };
  }
  if (remote.phase === "starting") {
    return { label: "正在启动", detail: "远程服务器", tone: "ok" };
  }
  if (remote.phase === "failed") {
    return { label: "远程识别服务出错", detail: "检查设置后重试", tone: "danger" };
  }
  if (!remote.deployed) {
    return { label: "未部署模型", detail: "到设置里部署到服务器", tone: "danger" };
  }
  if (remote.phase === "online") {
    return { label: "识别已就绪", detail: "远程服务器", tone: "ok" };
  }
  return { label: "待启动", detail: "截图时自动启动", tone: "muted" };
}

export function engineSummary(status: EngineStatus | null): EngineSummary {
  if (!status) {
    return { label: "检查识别服务…", detail: "本地识别", tone: "muted" };
  }
  if (status.remote?.enabled) {
    return remoteSummary(status.remote);
  }
  if (status.llama) {
    return {
      label: "识别已就绪",
      detail: status.server.managed ? "本地模型" : "外部服务",
      tone: "ok",
    };
  }
  if (status.tesseract) {
    return {
      label: "备用识别",
      detail: status.model.installed ? "本地模型未启动" : "本地模型未下载",
      tone: "warn",
    };
  }
  if (status.setup.needed) {
    return {
      label: "未下载模型",
      detail: "下载后即可截图",
      tone: "danger",
    };
  }
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "运行时缺失", detail: "请使用完整安装包", tone: "danger" };
  }
  if (status.server.state === "starting") {
    return { label: "正在启动", detail: "正在加载模型", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "识别服务启动失败", detail: "可在设置中重试", tone: "danger" };
  }
  return { label: "待启动", detail: "截图时自动启动", tone: "warn" };
}

/** 设置页的引擎状态跟随当前选择的引擎，而不仅是健康检查结果。 */
export function engineForMode(
  status: EngineStatus | null,
  mode: EngineMode,
): EngineSummary {
  if (mode === "tesseract") {
    return status?.tesseract
      ? { label: "备用识别已就绪", detail: "Tesseract · 不需要模型", tone: "ok" }
      : { label: "备用识别不可用", detail: "安装 tesseract，或换回本地模型", tone: "danger" };
  }

  if (!status) return engineSummary(null);
  if (status.remote?.enabled) return remoteSummary(status.remote);
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "运行时缺失", detail: "请使用完整安装包", tone: "danger" };
  }
  if (status.llama) {
    return { label: "识别已就绪", detail: status.llamaUrl, tone: "ok" };
  }
  if (status.server.state === "starting") {
    return { label: "正在启动", detail: "正在加载模型", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "识别服务启动失败", detail: "重试或重新设置", tone: "danger" };
  }
  if (!status.model.installed) {
    return {
      label: "未下载模型",
      detail: `需要下载 ${formatBytes(status.setup.downloadBytes)}`,
      tone: "warn",
    };
  }
  return { label: "待启动", detail: "截图时自动启动", tone: "warn" };
}
