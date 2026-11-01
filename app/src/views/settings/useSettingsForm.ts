import { useCallback, useEffect, useState } from "react";

import { setShortcutEnabled } from "../../lib/bridge";
import { DEFAULT_SETTINGS, MAX_PROMPT_CHARS } from "../../lib/defaults";
import { engineForMode } from "../../lib/status";
import type { EngineStatus, Settings, ToastTone } from "../../lib/types";
import { sameSettings } from "./shared";

export type SettingsNotify = (
  tone: ToastTone,
  title: string,
  detail?: string,
) => void;

interface UseSettingsFormOptions {
  settings: Settings;
  onSave: (draft: Settings) => Promise<void>;
  onTestEngine: () => Promise<EngineStatus>;
  onProbeEngine: (url: string) => Promise<boolean>;
  onNotify: SettingsNotify;
}

export interface SettingsForm {
  draft: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  dirty: boolean;
  valid: boolean;
  saving: boolean;
  saveError: string | null;
  testing: boolean;
  testingUrl: boolean;
  urlError: string | null;
  proxyError: string | null;
  proxyAuthError: string | null;
  promptError: string | null;
  langError: string | null;
  fenceError: string | null;
  handleSave: () => Promise<void>;
  handleTest: () => Promise<void>;
  handleTestUrl: () => Promise<void>;
  resetDefaults: () => void;
  handleRecordingChange: (recording: boolean) => void;
}

/**
 * The settings page's editable draft: local state, validation and the actions
 * that save or test it. Sections stay presentational and only receive the
 * values and errors they render.
 */
export function useSettingsForm({
  settings,
  onSave,
  onTestEngine,
  onProbeEngine,
  onNotify,
}: UseSettingsFormOptions): SettingsForm {
  const [draft, setDraft] = useState<Settings>(settings);
  const [baseline, setBaseline] = useState<Settings>(settings);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testingUrl, setTestingUrl] = useState(false);

  useEffect(() => {
    setDraft(settings);
    setBaseline(settings);
    setSaveError(null);
  }, [settings]);

  const dirty = !sameSettings(draft, baseline);

  const handleRecordingChange = useCallback(
    (recording: boolean) => {
      void setShortcutEnabled(!recording).catch((cause) => {
        onNotify(
          "error",
          "快捷键恢复失败",
          cause instanceof Error ? cause.message : String(cause),
        );
      });
    },
    [onNotify],
  );

  const urlError =
    !draft.llamaUrl.trim() || !/^https?:\/\//i.test(draft.llamaUrl.trim())
      ? "请输入完整的 http:// 或 https:// 地址。"
      : null;
  const proxyAddress = draft.proxyUrl.trim();
  const proxyError = proxyAddress.includes("@")
    ? "地址里不要写用户名或密码。"
    : proxyAddress && !/^(http:\/\/)?[^\s/]+$/i.test(proxyAddress)
      ? "请填 host:port，或 http://host:port。"
      : null;
  const proxyAuthError =
    !proxyAddress && (draft.proxyUsername || draft.proxyPassword)
      ? "先填代理地址。"
      : !draft.proxyUsername && draft.proxyPassword
        ? "先填用户名。"
        : null;
  const customPromptText = draft.customPrompt.trim();
  const promptError =
    Array.from(customPromptText).length > MAX_PROMPT_CHARS
      ? `自定义提示词不能超过 ${MAX_PROMPT_CHARS} 个字符。`
      : draft.promptPreset === "custom" && !customPromptText
        ? "自定义提示词不能为空。"
        : null;
  const langError = draft.tessLang.trim() ? null : "例如 eng 或 eng+chi_sim。";
  const fenceError =
    draft.language.trim() && !/^[a-zA-Z0-9+#._-]*$/.test(draft.language.trim())
      ? "请使用简短的语言名，如 rust、python、diff。"
      : null;
  const usesModel = draft.engine !== "tesseract";
  const fenceActive = draft.format === "markdown" && Boolean(draft.language.trim());
  const valid =
    (!usesModel || (!urlError && !promptError)) &&
    (!fenceActive || !fenceError) &&
    !proxyError &&
    !proxyAuthError &&
    (draft.engine === "llamacpp" || !langError) &&
    draft.maxTokens >= 128 &&
    draft.maxTokens <= 16384;

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
    setSaveError(null);
  }

  async function handleSave() {
    if (!dirty || saving) return;
    if (!valid) {
      setSaveError("有字段需要先修正。");
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      await onSave(draft);
      setBaseline(draft);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        const target = event.target as HTMLElement | null;
        // Let the shortcut recorder capture Cmd/Ctrl+S instead of saving.
        if (target?.closest(".shortcut-recorder")) return;
        event.preventDefault();
        void handleSave();
      }
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  });

  async function handleTest() {
    if (testing) return;
    setTesting(true);
    try {
      const next = await onTestEngine();
      const summary = engineForMode(next, draft.engine);
      if (next.remote.enabled) {
        // First-layer copy stays free of ports, URLs and ssh commands.
        const detail =
          next.remote.phase === "online"
            ? "远程服务器已连接。"
            : next.remote.phase === "reconnecting"
              ? "连接断开了，正在自动重连。"
              : next.remote.phase === "failed"
                ? "连接没有成功。检查服务器后点「重连」。"
                : next.remote.deployed
                  ? "截图时会自动连接服务器。"
                  : "到 OCR → 部署到服务器 里完成部署。";
        onNotify(
          summary.tone === "danger" ? "error" : summary.tone === "warn" ? "info" : "ok",
          summary.label,
          detail,
        );
        return;
      }
      onNotify(
        summary.tone === "danger" ? "error" : summary.tone === "warn" ? "info" : "ok",
        summary.label,
        next.llama
          ? next.llamaUrl
          : next.tesseract
            ? "本地模型没有响应，但 tesseract 可用。"
            : "先下载模型，或安装 tesseract。",
      );
    } catch (error) {
      onNotify(
        "error",
        "检测失败",
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setTesting(false);
    }
  }

  async function handleTestUrl() {
    if (testingUrl) return;
    setTestingUrl(true);
    try {
      const ok = await onProbeEngine(draft.llamaUrl);
      onNotify(
        ok ? "ok" : "error",
        ok ? "地址可用" : "连不上这个地址",
        ok ? draft.llamaUrl : "确认服务已经启动，或换一个地址。",
      );
    } catch (error) {
      onNotify(
        "error",
        "检测失败",
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setTestingUrl(false);
    }
  }

  function resetDefaults() {
    setDraft({ ...DEFAULT_SETTINGS });
    setSaveError(null);
    onNotify("info", "已载入默认值", "保存后生效。");
  }

  return {
    draft,
    update,
    dirty,
    valid,
    saving,
    saveError,
    testing,
    testingUrl,
    urlError,
    proxyError,
    proxyAuthError,
    promptError,
    langError,
    fenceError,
    handleSave,
    handleTest,
    handleTestUrl,
    resetDefaults,
    handleRecordingChange,
  };
}
