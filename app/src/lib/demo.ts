import { DEMO_RESULT, IMAGE_FORMAT_LABEL, MAX_IMAGE_BYTES } from "./defaults";
import { isMac, isTauri } from "./platform";
import type { OcrError, OcrResult, ScreenAccess, View } from "./types";

const params = new URLSearchParams(window.location.search);

/** `?demo=` is only honoured outside the Tauri window. */
export const demo = isTauri ? null : params.get("demo");

export function initialView(): View {
  if (demo === "settings") return "settings";
  if (demo === "history") return "history";
  return "capture";
}

export function initialScreenAccess(): ScreenAccess {
  if (!isMac) return "not-needed";
  return demo === "permission" ? "denied" : "unknown";
}

export const DEMO_ERROR: OcrError = {
  message: "本地 OCR 引擎没有响应。",
  hint: "可能是模型还没加载完。稍等几秒再试，或到设置里检查 OCR 引擎状态。",
  code: "engine",
};

export const IMAGE_FORMATS = `${IMAGE_FORMAT_LABEL}，最大 ${MAX_IMAGE_BYTES / (1024 * 1024)} MB`;

export const INITIAL_RESULT: OcrResult | null = demo === "result" ? DEMO_RESULT : null;
export const INITIAL_ERROR: OcrError | null = demo === "error" ? DEMO_ERROR : null;
export const INITIAL_FIRST_CAPTURE_TIP = demo === "result";
