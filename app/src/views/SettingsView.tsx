import { useCallback, useEffect, useState, type ReactNode } from "react";

import { Icon } from "../components/Icon";
import { Keycaps, Segmented, SettingRow, ShortcutRecorder, Switch } from "../components/Controls";
import { DEFAULT_PROMPT, DEFAULT_SETTINGS } from "../lib/defaults";
import { engineForMode } from "../lib/status";
import { setShortcutEnabled } from "../lib/bridge";
import { formatBytes, formatEta, formatSpeed } from "../lib/format";
import { isMac } from "../lib/platform";
import type {
  DownloadProgress,
  EngineStatus,
  Settings,
  ToastTone,
} from "../lib/types";

interface SettingsViewProps {
  settings: Settings;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  engineBusy: "start" | "stop" | "remove" | null;
  onSave: (draft: Settings) => Promise<void>;
  onTestEngine: () => Promise<EngineStatus>;
  onNotify: (tone: ToastTone, title: string, detail?: string) => void;
  onQuit: () => void;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
}

const PSM_OPTIONS = [
  { value: 3, label: "3 · 完全自动" },
  { value: 4, label: "4 · 单列" },
  { value: 6, label: "6 · 单块" },
  { value: 7, label: "7 · 单行" },
  { value: 11, label: "11 · 稀疏文本" },
  { value: 13, label: "13 · 原始行" },
];

function Section({
  title,
  description,
  meta,
  children,
}: {
  title: string;
  description?: string;
  meta?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="settings-section panel">
      <header className="section-head">
        <div>
          <h2>{title}</h2>
          {description && <p>{description}</p>}
        </div>
        {meta}
      </header>
      <div className="section-body">{children}</div>
    </section>
  );
}

function Field({
  label,
  hint,
  error,
  action,
  full = false,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  action?: ReactNode;
  full?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`field ${full ? "field-full" : ""}`}>
      <div className="field-head">
        <span className="field-label">{label}</span>
        {action}
      </div>
      {children}
      {error ? (
        <span className="field-error">{error}</span>
      ) : hint ? (
        <span className="field-hint">{hint}</span>
      ) : null}
    </div>
  );
}

function sameSettings(a: Settings, b: Settings): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function LocalEnginePanel({
  status,
  download,
  engineBusy,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
}: {
  status: EngineStatus;
  download: DownloadProgress | null;
  engineBusy: "start" | "stop" | "remove" | null;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const { runtime, model, server } = status;
  const downloading = Boolean(
    download && ["downloading", "verifying", "starting"].includes(download.phase),
  );
  const percent =
    download && download.total > 0
      ? Math.min(100, (download.downloaded / download.total) * 100)
      : 0;
  const managed = server.managed;
  const busy = engineBusy !== null;

  return (
    <div className="runtime-panel">
      <div className="runtime-head">
        <div className="runtime-heading">
          <span className={`status-dot tone-${status.llama ? "ok" : model.installed ? "warn" : "danger"}`} />
          <strong>本地引擎</strong>
          <span className="badge">
            {runtime.bundled ? "内置运行时" : runtime.system ? "系统运行时" : "运行时缺失"}
          </span>
          {model.installed && (
            <span className={`badge ${model.verified ? "badge-ok" : "badge-warn"}`}>
              {model.verified ? "已校验" : "未校验"}
            </span>
          )}
        </div>
        <div className="runtime-actions">
          {!model.installed ? (
            <button type="button" className="btn btn-primary btn-sm" onClick={onDownloadModel} disabled={busy}>
              <Icon name="arrow-right" size={14} />
              下载模型
            </button>
          ) : server.state === "starting" ? (
            <button type="button" className="btn btn-secondary btn-sm" disabled>
              <span className="btn-spinner btn-spinner-muted" />
              正在启动…
            </button>
          ) : managed && server.state === "online" ? (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStopEngine} disabled={busy}>
              {engineBusy === "stop" ? "正在停止…" : "停止"}
            </button>
          ) : (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStartEngine} disabled={busy}>
              <Icon name="power" size={14} />
              {engineBusy === "start" ? "正在启动…" : "启动引擎"}
            </button>
          )}
        </div>
      </div>

      <div className="runtime-grid">
        <div>
          <span className="runtime-label">运行时</span>
          <span className="runtime-value">
            {runtime.version ?? (runtime.bundled || runtime.system ? "llama.cpp" : "未找到")}
          </span>
        </div>
        <div>
          <span className="runtime-label">模型</span>
          <span className="runtime-value">
            {model.installed
              ? `${formatBytes(model.bytes)} · ${model.verified ? "SHA-256 已校验" : "未校验"}`
              : `${formatBytes(model.total)} 待下载`}
          </span>
        </div>
        <div>
          <span className="runtime-label">服务</span>
          <span className={`runtime-value tone-${server.state === "online" ? "ok" : server.state === "failed" ? "danger" : "muted"}`}>
            {server.state === "online"
              ? `${managed ? "内置" : "外部"} · ${server.url ?? status.llamaUrl}`
              : server.state === "starting"
                ? "正在启动…"
                : server.state === "failed"
                  ? "启动失败"
                  : "已停止"}
          </span>
        </div>
      </div>

      {downloading && download && (
        <div className="runtime-download">
          <div className="runtime-download-head">
            <span>
              {download.phase === "verifying"
                ? "正在校验 SHA-256…"
                : download.phase === "starting"
                  ? "正在启动 llama-server…"
                  : download.file ?? "正在下载模型"}
            </span>
            <span className="mono">
              {download.phase === "downloading"
                ? `${Math.round(percent)}% · ${formatSpeed(download.bytesPerSec)}${
                    download.etaSecs != null ? ` · ${formatEta(download.etaSecs)}` : ""
                  }`
                : "…"}
            </span>
          </div>
          <div className="runtime-bar">
            <span style={{ width: `${download.phase === "starting" ? 100 : percent}%` }} />
          </div>
          {download.phase === "downloading" && (
            <button type="button" className="link-btn" onClick={onCancelSetup}>
              取消下载
            </button>
          )}
        </div>
      )}

      {download?.phase === "error" && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {download.message ?? "设置失败。"}
          <button type="button" className="link-btn" onClick={onSetup}>
            重试
          </button>
        </p>
      )}

      {server.state === "failed" && server.error && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {server.error.split("\n")[0]}
        </p>
      )}

      <div className="runtime-foot">
        <button type="button" className="link-btn" onClick={onSetup}>
          {model.installed ? "重新运行设置" : "打开设置向导"}
        </button>
        {model.installed && (
          <button
            type="button"
            className="link-btn link-btn-danger"
            onClick={() => setConfirmRemove(true)}
            disabled={busy}
          >
            移除模型
          </button>
        )}
        <span className="runtime-foot-note">
          {model.installed ? "完全离线运行" : "一次性 1.43 GB · 支持续传"}
        </span>
      </div>

      {confirmRemove && (
        <div className="runtime-confirm">
          <span>移除 {formatBytes(model.bytes)} 的模型文件？</span>
          <div>
            <button
              type="button"
              className="btn btn-danger btn-sm"
              onClick={() => {
                setConfirmRemove(false);
                onRemoveModel();
              }}
              disabled={busy}
            >
              移除
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setConfirmRemove(false)}
            >
              取消
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function SettingsView({
  settings,
  status,
  download,
  engineBusy,
  onSave,
  onTestEngine,
  onNotify,
  onQuit,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
}: SettingsViewProps) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [baseline, setBaseline] = useState<Settings>(settings);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    setDraft(settings);
    setBaseline(settings);
    setSaveError(null);
  }, [settings]);

  const dirty = !sameSettings(draft, baseline);
  const engine = engineForMode(status, draft.engine);

  const handleRecordingChange = useCallback(
    (recording: boolean) => {
      void setShortcutEnabled(!recording).catch((cause) => {
        onNotify(
          "error",
          "全局快捷键恢复失败",
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
  const proxyError =
    draft.downloadProxy.trim() && !/^http:\/\//i.test(draft.downloadProxy.trim())
      ? "下载代理必须以 http:// 开头。"
      : null;
  const mirrorError =
    draft.modelBase.trim() && !/^https?:\/\//i.test(draft.modelBase.trim())
      ? "模型镜像必须以 http:// 或 https:// 开头。"
      : null;
  const promptError = draft.prompt.trim() ? null : "提示词不能为空。";
  const langError = draft.tessLang.trim() ? null : "例如 `eng` 或 `eng+chi_sim`。";
  const fenceError =
    draft.language.trim() && !/^[a-zA-Z0-9+#._-]*$/.test(draft.language.trim())
      ? "请使用简短的语言名，如 rust、python、diff。"
      : null;
  const valid =
    !urlError &&
    !proxyError &&
    !mirrorError &&
    !promptError &&
    !fenceError &&
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
      onNotify(
        summary.tone === "danger" ? "error" : summary.tone === "warn" ? "info" : "ok",
        summary.label,
        next.llama
          ? next.llamaUrl
          : next.tesseract
            ? "llama.cpp 未响应，但 tesseract 可用。"
            : "请启动 llama-server，或安装 tesseract。",
      );
    } catch (error) {
      onNotify(
        "error",
        "引擎检测失败",
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setTesting(false);
    }
  }

  function resetDefaults() {
    setDraft({ ...DEFAULT_SETTINGS });
    setSaveError(null);
    onNotify("info", "已载入默认值", "确认后保存即可生效。");
  }

  return (
    <div className="view settings-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">偏好设置</p>
            <h1>设置</h1>
            <p className="lede">
              桌面层暴露的全部内核参数。更改会应用于下一次截图，OCR 内核本身保持不变。
            </p>
          </div>
          <span className="settings-version">ocr4code 0.1.6</span>
        </header>

        <Section
          title="截图"
          description="应用把截图交给内核的方式。"
        >
          <SettingRow
            title="全局快捷键"
            description="ocr4code 运行时，在任何应用里都可以使用。选区沿用系统的截图权限。"
          >
            <ShortcutRecorder
              value={draft.shortcut}
              onChange={(shortcut) => update("shortcut", shortcut)}
              onRecordingChange={handleRecordingChange}
            />
          </SettingRow>
          <SettingRow
            title="自动复制"
            description="识别完成后立即写入剪贴板。"
          >
            <Switch
              checked={draft.autoCopy}
              onChange={(autoCopy) => update("autoCopy", autoCopy)}
              label="自动复制"
            />
          </SettingRow>
        </Section>

        <Section
          title="引擎"
          description="文字来自模型；无论用哪个引擎，版面都由纯 CV 几何层重建。"
          meta={
            <div className={`engine-health tone-${engine.tone}`}>
              <span className="status-dot" />
              <span>{engine.label}</span>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() => void handleTest()}
                disabled={testing}
              >
                <Icon
                  name="refresh"
                  size={13}
                  className={testing ? "is-spinning" : ""}
                />
                {testing ? "检测中" : "检测"}
              </button>
            </div>
          }
        >
          {status && (
            <LocalEnginePanel
              status={status}
              download={download}
              engineBusy={engineBusy}
              onSetup={onSetup}
              onDownloadModel={onDownloadModel}
              onStartEngine={onStartEngine}
              onStopEngine={onStopEngine}
              onRemoveModel={onRemoveModel}
              onCancelSetup={onCancelSetup}
            />
          )}
          <SettingRow
            title="模型下载"
            description="首次下载 1.43 GB 模型时使用；保存后对下一次下载生效。"
            stack
          >
            <div className="field-grid">
              <Field
                label="下载代理（可选）"
                error={proxyError}
                hint="HTTP 代理，例如 http://user:pass@127.0.0.1:7890。密码中的 @ 可写成 %40，程序会自动解码；留空则读取环境变量。"
                full
                action={
                  draft.downloadProxy ? (
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => update("downloadProxy", "")}
                    >
                      清除
                    </button>
                  ) : undefined
                }
              >
                <input
                  className="input mono"
                  type="text"
                  value={draft.downloadProxy}
                  placeholder="http://127.0.0.1:7890"
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="下载代理"
                  aria-invalid={Boolean(proxyError)}
                  onChange={(event) => update("downloadProxy", event.target.value)}
                />
              </Field>
              <Field
                label="模型镜像（可选）"
                error={mirrorError}
                hint="填包含 resolve 路径的自建镜像基址；浏览器需打开「基址/GLM-OCR-Q8_0.gguf」才能下载，只开基址会 404。留空则跟随环境变量，再退回官方源。"
                full
              >
                <input
                  className="input mono"
                  type="url"
                  value={draft.modelBase}
                  placeholder="https://your-mirror.example/ggml-org/GLM-OCR-GGUF/resolve/<revision>"
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="模型镜像地址"
                  aria-invalid={Boolean(mirrorError)}
                  onChange={(event) => update("modelBase", event.target.value)}
                />
              </Field>
            </div>
          </SettingRow>
          <SettingRow
            title="识别引擎"
            description={
              draft.engine === "auto"
                ? "优先使用能响应的 llama.cpp，不可用时回落到 tesseract。"
                : draft.engine === "llamacpp"
                  ? "把截图发送给本地 llama-server，版面与符号保真度最好。"
                  : "不依赖模型服务，完全本机运行；速度较快，但符号和中文较弱。"
            }
          >
            <Segmented
              label="识别引擎"
              value={draft.engine}
              onChange={(next) => update("engine", next)}
              options={[
                { value: "auto", label: "自动" },
                { value: "llamacpp", label: "llama.cpp" },
                { value: "tesseract", label: "tesseract" },
              ]}
            />
          </SettingRow>

          {draft.engine !== "tesseract" && (
            <>
              <SettingRow
                title="外部服务（可选）"
                description="如果这个地址已经有 llama.cpp 服务，ocr4code 会直接使用，不再启动内置服务。"
                stack
              >
                <div className="field-grid">
                  <Field
                    label="外部 llama.cpp 地址"
                    error={urlError}
                    hint={
                      urlError
                        ? undefined
                        : `环境变量覆盖：OCR4CODE_LLAMA_URL · ${draft.llamaUrl}`
                    }
                    full
                  >
                    <div className="input-with-action">
                      <input
                        className="input mono"
                        type="url"
                        value={draft.llamaUrl}
                        spellCheck={false}
                        aria-label="外部 llama.cpp 地址"
                        aria-invalid={Boolean(urlError)}
                        onChange={(event) => update("llamaUrl", event.target.value)}
                      />
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        onClick={() => void handleTest()}
                        disabled={testing}
                      >
                        {testing ? "检测中…" : "检测"}
                      </button>
                    </div>
                  </Field>

                  <Field
                    label="提示词"
                    error={promptError}
                    hint="提示词对版面的影响最大。默认值在 26 个场景上标定过。"
                    full
                    action={
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => update("prompt", DEFAULT_PROMPT)}
                      >
                        恢复默认
                      </button>
                    }
                  >
                    <textarea
                      className="input textarea mono"
                      rows={3}
                      value={draft.prompt}
                      spellCheck={false}
                      aria-label="模型提示词"
                      aria-invalid={Boolean(promptError)}
                      onChange={(event) => update("prompt", event.target.value)}
                    />
                  </Field>

                  <Field
                    label={`最大输出 token 数 — ${draft.maxTokens}`}
                    hint="同时决定切块的行数预算。3072 是实测默认值；8192 可能超发。"
                    full
                  >
                    <div className="range-row">
                      <input
                        type="range"
                        min={512}
                        max={8192}
                        step={128}
                        value={draft.maxTokens}
                        aria-label="最大输出 token 数"
                        onChange={(event) =>
                          update("maxTokens", Number(event.target.value))
                        }
                      />
                      <span className="range-value">{draft.maxTokens}</span>
                    </div>
                  </Field>

                  <Field
                    label={`并行引擎调用数 — ${draft.jobs}`}
                    hint="仅在长截图切块时使用。3 与 llama-server 的四个 slot 相匹配，避免排队。"
                    full
                  >
                    <div className="range-row">
                      <input
                        type="range"
                        min={1}
                        max={6}
                        step={1}
                        value={draft.jobs}
                        aria-label="并行引擎调用数"
                        onChange={(event) => update("jobs", Number(event.target.value))}
                      />
                      <span className="range-value">{draft.jobs}</span>
                    </div>
                  </Field>
                </div>
              </SettingRow>
            </>
          )}

          {draft.engine !== "llamacpp" && (
            <div className={draft.engine === "auto" ? "engine-fallback" : ""}>
              {draft.engine === "auto" && (
                <div className="fallback-label">
                  <span>兜底</span>
                  <span className="fallback-line" />
                  <span>tesseract</span>
                </div>
              )}
              <div className="field-grid">
                <Field
                  label="Tesseract 语言"
                  error={draft.engine === "tesseract" ? langError : null}
                  hint="需要先安装语言包，例如 eng+chi_sim。"
                >
                  <input
                    className="input mono"
                    value={draft.tessLang}
                    spellCheck={false}
                    aria-label="Tesseract 语言"
                    onChange={(event) => update("tessLang", event.target.value)}
                  />
                </Field>
                <Field
                  label="页面分割模式"
                  hint="Tesseract 的 PSM。大多数截图用单块模式即可。"
                >
                  <div className="select-wrap">
                    <select
                      className="input select"
                      value={draft.psm}
                      aria-label="Tesseract 页面分割模式"
                      onChange={(event) => update("psm", Number(event.target.value))}
                    >
                      {PSM_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                    <Icon name="chevron-down" size={14} className="select-chevron" />
                  </div>
                </Field>
              </div>
            </div>
          )}
        </Section>

        <Section
          title="版面重建"
          description="几何层对每个引擎都会运行，它决定粘贴后是否还能用。"
        >
          <SettingRow
            title="从像素重建缩进"
            description="用墨迹掩码计算行首空格，而不是相信模型输出的空格。"
          >
            <Switch
              checked={draft.geometry}
              onChange={(geometry) => update("geometry", geometry)}
              label="从像素重建缩进"
            />
          </SettingRow>
          <SettingRow
            title="去除行号栏"
            description="识别并移除行号列。关闭后保留编辑器截图里模型自己的空格。"
          >
            <Switch
              checked={!draft.keepGutter}
              onChange={(checked) => update("keepGutter", !checked)}
              label="去除行号栏"
            />
          </SettingRow>
          <SettingRow
            title="合并软换行"
            description="启发式处理。刚好顶到右边距的长行可能被误合并，因此默认关闭。"
          >
            <Switch
              checked={draft.joinWrap}
              onChange={(joinWrap) => update("joinWrap", joinWrap)}
              label="合并软换行"
            />
          </SettingRow>
          <SettingRow
            title="长截图切块"
            description="输出预算不足时在空白行处切块；回复过短时自动重试一次。"
          >
            <Switch
              checked={draft.tile}
              onChange={(tile) => update("tile", tile)}
              label="长截图切块"
            />
          </SettingRow>
        </Section>

        <Section
          title="输出"
          description="复制什么，以及保留多少历史记录。"
        >
          <SettingRow
            title="输出格式"
            description="纯文本可直接粘贴；Markdown 会带语言围栏；JSON 携带逐行缩进。"
          >
            <Segmented
              label="输出格式"
              value={draft.format}
              onChange={(format) => update("format", format)}
              options={[
                { value: "text", label: "纯文本" },
                { value: "markdown", label: "Markdown" },
                { value: "json", label: "JSON" },
              ]}
            />
          </SettingRow>
          <SettingRow
            title="围栏语言"
            description="仅用于 Markdown。留空则由内核根据识别文字猜测。"
          >
            <input
              className="input mono input-inline"
              value={draft.language}
              placeholder="自动"
              spellCheck={false}
              disabled={draft.format !== "markdown"}
              aria-label="Markdown 围栏语言"
              onChange={(event) => update("language", event.target.value)}
            />
          </SettingRow>
          {fenceError && (
            <p className="row-error">
              <Icon name="alert" size={13} />
              {fenceError}
            </p>
          )}
          <SettingRow
            title="历史记录上限"
            description="超过上限后，较早的记录会从本机删除。"
          >
            <div className="select-wrap select-compact">
              <select
                className="input select"
                value={draft.historyLimit}
                aria-label="历史记录上限"
                onChange={(event) => update("historyLimit", Number(event.target.value))}
              >
                {[5, 10, 20, 50].map((limit) => (
                  <option key={limit} value={limit}>
                    {limit} 条
                  </option>
                ))}
              </select>
              <Icon name="chevron-down" size={14} className="select-chevron" />
            </div>
          </SettingRow>
        </Section>

        <Section
          title="关于"
          description="ocr4code 内核的桌面外壳。"
          meta={<span className="about-version">v0.1.6</span>}
        >
          <div className="about-grid">
            <div>
              <span className="about-label">引擎</span>
              <span className={`about-value tone-${engine.tone}`}>
                <span className="status-dot" />
                {engine.label}
              </span>
            </div>
            <div>
              <span className="about-label">隐私</span>
              <span className="about-value">完全在本机离线运行</span>
            </div>
            <div>
              <span className="about-label">截图</span>
              <span className="about-value">
                {isMac ? "macOS 原生选区 · screencapture" : "Windows 自绘浮层 · GDI 抓屏"}
              </span>
            </div>
            <div>
              <span className="about-label">快捷键</span>
              <span className="about-value">
                <Keycaps accelerator={draft.shortcut} size="sm" />
              </span>
            </div>
          </div>
          <div className="about-foot">
            <button type="button" className="btn btn-ghost btn-danger btn-sm" onClick={onQuit}>
              <Icon name="power" size={14} />
              退出 ocr4code
            </button>
            <p>
              {isMac
                ? "如果截图是空白，请在系统设置 → 隐私与安全 → 屏幕录制中启用 ocr4code，然后重启应用。"
                : "Windows 选区由应用自己绘制并直接用 GDI 抓屏，不调用系统截屏工具。"}
            </p>
          </div>
        </Section>
      </div>

      <footer className="settings-bar">
        <div className="save-state">
          {saveError ? (
            <span className="save-error" role="alert">
              <Icon name="alert" size={14} />
              {saveError}
            </span>
          ) : dirty ? (
            <span className="save-dirty">
              <span className="dirty-dot" />
              有未保存的更改
            </span>
          ) : (
            <span className="save-clean">
              <Icon name="check" size={14} />
              已保存
            </span>
          )}
        </div>
        <div className="save-actions">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={resetDefaults}
            disabled={saving}
          >
            恢复默认
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void handleSave()}
            disabled={!dirty || saving || !valid}
          >
            {saving ? <span className="btn-spinner" /> : <Icon name="check" size={15} />}
            {saving ? "保存中…" : "保存更改"}
          </button>
        </div>
      </footer>
    </div>
  );
}

