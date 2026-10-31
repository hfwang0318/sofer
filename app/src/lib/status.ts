import { formatBytes } from "./format";
import type { EngineMode, EngineStatus } from "./types";

export type StatusTone = "ok" | "warn" | "danger" | "muted";

export interface EngineSummary {
  label: string;
  detail: string;
  tone: StatusTone;
}

export function engineSummary(status: EngineStatus | null): EngineSummary {
  if (!status) {
    return { label: "正在检查引擎…", detail: "llama.cpp · tesseract", tone: "muted" };
  }
  if (status.llama) {
    return {
      label: status.server.managed && status.server.state === "online"
        ? "llama.cpp 已就绪"
        : "llama.cpp 在线",
      detail: status.server.managed ? "内置本地引擎" : "外部服务",
      tone: "ok",
    };
  }
  if (status.tesseract) {
    return {
      label: "tesseract 兜底",
      detail: status.model.installed ? "llama.cpp 不可用" : "本地模型未安装",
      tone: "warn",
    };
  }
  if (status.setup.needed) {
    return {
      label: "需要初始化",
      detail: "下载本地 OCR 模型",
      tone: "danger",
    };
  }
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "运行时缺失", detail: "此构建未内置 llama.cpp", tone: "danger" };
  }
  if (status.server.state === "starting") {
    return { label: "正在启动引擎", detail: "正在加载本地模型", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "引擎启动失败", detail: "可在设置中重试", tone: "danger" };
  }
  return { label: "引擎未启动", detail: "下次截图时自动启动", tone: "warn" };
}

/** 设置页的引擎状态跟随当前选择的引擎，而不仅是健康检查结果。 */
export function engineForMode(
  status: EngineStatus | null,
  mode: EngineMode,
): EngineSummary {
  if (mode === "tesseract") {
    return status?.tesseract
      ? { label: "tesseract 已就绪", detail: "本机 CPU 引擎", tone: "ok" }
      : { label: "未安装 tesseract", detail: "安装它或切换引擎", tone: "danger" };
  }

  if (!status) return engineSummary(null);
  if (!status.runtime.bundled && !status.runtime.system) {
    return { label: "运行时缺失", detail: "此构建没有 llama.cpp 运行时", tone: "danger" };
  }
  if (status.llama) {
    return { label: "llama.cpp 在线", detail: status.llamaUrl, tone: "ok" };
  }
  if (status.server.state === "starting") {
    return { label: "正在启动 llama.cpp", detail: "正在加载本地模型", tone: "ok" };
  }
  if (status.server.state === "failed") {
    return { label: "llama.cpp 启动失败", detail: "重试或重新运行设置", tone: "danger" };
  }
  if (!status.model.installed) {
    return {
      label: "模型未安装",
      detail: `首次需要下载 ${formatBytes(status.setup.downloadBytes)}`,
      tone: "warn",
    };
  }
  return { label: "llama.cpp 未启动", detail: "启动它，或截图时自动唤醒", tone: "warn" };
}

export function phaseLabel(
  phase: "idle" | "selecting" | "starting" | "recognizing",
  shortcutActive = true,
): { label: string; tone: StatusTone } {
  if (phase === "selecting") return { label: "请选择区域", tone: "ok" };
  if (phase === "starting") return { label: "正在启动引擎", tone: "ok" };
  if (phase === "recognizing") return { label: "正在识别", tone: "ok" };
  return shortcutActive
    ? { label: "就绪", tone: "ok" }
    : { label: "快捷键不可用", tone: "warn" };
}
