import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { Icon } from "../components/Icon";
import { Keycaps, Segmented, SettingRow, ShortcutRecorder, Switch } from "../components/Controls";
import { DEFAULT_PROMPT, DEFAULT_SETTINGS } from "../lib/defaults";
import { llamaSlots, perSlotCtx, totalCtx } from "../lib/llamaPlan";
import { engineForMode } from "../lib/status";
import { setShortcutEnabled } from "../lib/bridge";
import { formatBytes, formatEta, formatSpeed } from "../lib/format";
import { isMac } from "../lib/platform";
import type { AvailableUpdate, UpdatePhase } from "../lib/updater";
import type {
  DownloadProgress,
  EngineStatus,
  ScreenAccess,
  Settings,
  SettingsSection,
  ToastTone,
} from "../lib/types";

interface SettingsViewProps {
  settings: Settings;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  engineBusy: "start" | "stop" | "remove" | null;
  appVersion: string;
  screenAccess: ScreenAccess;
  updateInfo: AvailableUpdate | null;
  updatePhase: UpdatePhase;
  updateProgress: number;
  updateError: string | null;
  onCheckUpdates: () => void;
  onDownloadUpdate: () => void;
  onInstallUpdate: () => void;
  onSave: (draft: Settings) => Promise<void>;
  onTestEngine: () => Promise<EngineStatus>;
  onProbeEngine: (url: string) => Promise<boolean>;
  onNotify: (tone: ToastTone, title: string, detail?: string) => void;
  onQuit: () => void;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
  onRequestPermission: () => void;
  onOpenPermissionSettings: () => void;
  onResetOnboarding: () => void;
  section: SettingsSection;
  sectionNav: number;
  onSectionChange: (section: SettingsSection) => void;
}

const SECTION_IDS: SettingsSection[] = [
  "general",
  "recognition",
  "output",
  "advanced",
  "about",
];

const PSM_OPTIONS = [
  { value: 3, label: "3 · 完全自动" },
  { value: 4, label: "4 · 单列" },
  { value: 6, label: "6 · 单块" },
  { value: 7, label: "7 · 单行" },
  { value: 11, label: "11 · 稀疏文本" },
  { value: 13, label: "13 · 原始行" },
];

function Section({
  id,
  title,
  description,
  meta,
  children,
}: {
  id: SettingsSection;
  title: string;
  description?: string;
  meta?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="settings-section panel" id={`settings-${id}`}>
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
  const keys = Object.keys(a) as Array<keyof Settings>;
  return (
    keys.length === Object.keys(b).length &&
    keys.every((key) => a[key] === b[key])
  );
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

  useEffect(() => {
    if (!confirmRemove) return;
    const timer = window.setTimeout(() => setConfirmRemove(false), 5000);
    return () => window.clearTimeout(timer);
  }, [confirmRemove]);
  const busyPhase = download?.phase;
  const downloading = Boolean(
    busyPhase && ["downloading", "verifying", "starting"].includes(busyPhase),
  );
  const percent =
    download && download.total > 0
      ? Math.min(100, (download.downloaded / download.total) * 100)
      : 0;
  const managed = server.managed;
  const externalOnline = !managed && server.state === "online";
  const busy = engineBusy !== null;

  return (
    <div className="runtime-panel">
      <div className="runtime-head">
        <div className="runtime-heading">
          <span className={`status-dot tone-${status.llama ? "ok" : model.installed ? "warn" : "danger"}`} />
          <strong>本地识别</strong>
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
          {downloading ? (
            <span className="badge">
              {busyPhase === "verifying"
                ? "校验中"
                : busyPhase === "starting"
                  ? "加载中"
                  : "下载中"}
            </span>
          ) : externalOnline ? (
            <span className="badge badge-ok">外部服务</span>
          ) : !model.installed ? (
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
              {engineBusy === "start" ? "正在启动…" : "启动"}
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
            {externalOnline && !model.installed
              ? "未下载 · 当前使用外部服务"
              : `${status.setup.modelName} · ${
                  model.installed
                    ? `${formatBytes(model.bytes)} · ${model.verified ? "已校验" : "未校验"}`
                    : `${formatBytes(model.total)} 待下载`
                }`}
          </span>
        </div>
        <div>
          <span className="runtime-label">服务</span>
          <span className={`runtime-value tone-${server.state === "online" ? "ok" : server.state === "failed" ? "danger" : "muted"}`}>
            {server.state === "online"
              ? `${managed ? "本地" : "外部"} · ${server.url ?? status.llamaUrl}`
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
                ? "正在校验文件…"
                : download.phase === "starting"
                  ? "正在加载模型…"
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
          {download.phase === "downloading" ? (
            <button type="button" className="link-btn" onClick={onCancelSetup}>
              取消下载
            </button>
          ) : (
            <span className="runtime-foot-note">完成后会自动继续</span>
          )}
        </div>
      )}

      {download?.phase === "error" && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {download.message ?? "没有完成。"}
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
          {model.installed ? "重新设置" : "打开设置向导"}
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
          {externalOnline
            ? "当前使用外部服务"
            : model.installed
              ? "本地处理"
              : `下载 ${formatBytes(model.total)} · 支持续传`}
        </span>
      </div>

      {confirmRemove && (
        <div className="runtime-confirm">
          <span>移除当前模型？下次截图前需要重新下载。</span>
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
  appVersion,
  screenAccess,
  updateInfo,
  updatePhase,
  updateProgress,
  updateError,
  onCheckUpdates,
  onDownloadUpdate,
  onInstallUpdate,
  onSave,
  onTestEngine,
  onProbeEngine,
  onNotify,
  onQuit,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
  onRequestPermission,
  onOpenPermissionSettings,
  onResetOnboarding,
  section,
  sectionNav,
  onSectionChange,
}: SettingsViewProps) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [baseline, setBaseline] = useState<Settings>(settings);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testingUrl, setTestingUrl] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastNav = useRef<number | null>(null);
  const mounted = useRef(false);

  useEffect(() => {
    // The first section sits directly under the page title: do not scroll the
    // header out of view just because the user opened Settings.
    if (!mounted.current) {
      mounted.current = true;
      if (section === "general") {
        lastNav.current = sectionNav;
        return;
      }
    } else if (lastNav.current === sectionNav) {
      // Only scroll on an explicit sidebar click. The scroll observer also
      // updates `section`; without this guard the two would fight.
      return;
    }
    lastNav.current = sectionNav;
    const target = document.getElementById(`settings-${section}`);
    if (!target) return;
    onSectionChange(section);
    target.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [section, sectionNav, onSectionChange]);

  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const onScroll = () => {
      const top = root.getBoundingClientRect().top + 120;
      let active = SECTION_IDS[0];
      for (const id of SECTION_IDS) {
        const element = document.getElementById(`settings-${id}`);
        if (element && element.getBoundingClientRect().top <= top) active = id;
      }
      onSectionChange(active);
    };
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => root.removeEventListener("scroll", onScroll);
  }, [onSectionChange]);

  useEffect(() => {
    setDraft(settings);
    setBaseline(settings);
    setSaveError(null);
  }, [settings]);

  const dirty = !sameSettings(draft, baseline);
  const engine = engineForMode(status, draft.engine);
  const slots = llamaSlots(draft.jobs);
  const slotCtx = perSlotCtx(draft.maxTokens);
  const context = totalCtx(draft.jobs, draft.maxTokens);

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
  const promptError = draft.prompt.trim() ? null : "提示词不能为空。";
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
      onNotify(
        summary.tone === "danger" ? "error" : summary.tone === "warn" ? "info" : "ok",
        summary.label,
        next.llama
          ? next.llamaUrl
          : next.tesseract
            ? "本地模型没有响应，但备用识别可用。"
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

  const updateDescription =
    updatePhase === "checking"
      ? "正在检查更新…"
      : updatePhase === "current"
        ? "已是最新版本。"
        : updatePhase === "available" && updateInfo
          ? `有新版 v${updateInfo.version} 可以下载。`
          : updatePhase === "downloading" && updateInfo
            ? `正在下载 v${updateInfo.version}…`
            : updatePhase === "ready" && updateInfo
              ? `v${updateInfo.version} 已下载，重启后安装。`
              : updatePhase === "installing"
                ? "正在启动安装程序…"
                : updatePhase === "error"
                  ? "更新失败，请重试。"
                  : "尚未检查。";

  return (
    <div className="view settings-view">
      <div className="view-scroll" ref={scrollRef}>
        <header className="view-head">
          <div>
            <p className="eyebrow">设置</p>
            <h1>设置</h1>
            <p className="lede">
              调整截图、识别和输出的方式。修改后点保存；模型的启动和下载会立即生效。
            </p>
          </div>
          <span className="settings-version">ocr4code {appVersion || "—"}</span>
        </header>

        <Section id="general" title="通用" description="截图方式、快捷键和界面大小。">
          <SettingRow
            title="全局快捷键"
            description="在任何应用里都能截图。只有 F 键能单独使用；其他键请搭配 ⌘/Ctrl/⌥。"
          >
            <ShortcutRecorder
              value={draft.shortcut}
              onChange={(shortcut) => update("shortcut", shortcut)}
              onRecordingChange={handleRecordingChange}
            />
          </SettingRow>
          <SettingRow
            title="自动复制"
            description="识别完成后直接放进剪贴板。"
          >
            <Switch
              checked={draft.autoCopy}
              onChange={(autoCopy) => update("autoCopy", autoCopy)}
              label="自动复制"
            />
          </SettingRow>
          <SettingRow
            title="界面缩放"
            description="调整窗口内的字号和控件大小。"
          >
            <Segmented
              label="界面缩放"
              value={String(draft.uiScale)}
              onChange={(value) => update("uiScale", Number(value))}
              options={[
                { value: "1", label: "100%" },
                { value: "1.15", label: "115%" },
                { value: "1.3", label: "130%" },
              ]}
            />
          </SettingRow>
          {isMac && (
            <SettingRow
              title="屏幕录制权限"
              description={
                screenAccess === "granted"
                  ? "已授权，可以正常截图。"
                  : "未授权时截图会是空白。授权后如果状态没变，请重启应用。"
              }
            >
              <div className="permission-inline">
                <span className={`badge ${screenAccess === "granted" ? "badge-ok" : "badge-warn"}`}>
                  {screenAccess === "granted" ? "已授权" : "待授权"}
                </span>
                {screenAccess !== "granted" && (
                  <>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      onClick={onRequestPermission}
                    >
                      去授权
                    </button>
                    <button
                      type="button"
                      className="link-btn"
                      onClick={onOpenPermissionSettings}
                    >
                      系统设置
                    </button>
                  </>
                )}
              </div>
            </SettingRow>
          )}
        </Section>

        <Section
          id="recognition"
          title="识别"
          description="截图交给哪个模型，以及模型放在哪里。"
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
                {testing ? "检测中" : "刷新状态"}
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
            title="模型精度"
            description="标准更准，约 1.4 GB；轻量更省内存，约 1.0 GB。Windows 建议轻量。切换后可能需要下载。"
          >
            <Segmented
              label="模型精度"
              value={draft.modelQuality}
              onChange={(next) => update("modelQuality", next)}
              options={[
                { value: "high", label: "标准" },
                { value: "low", label: "轻量" },
              ]}
            />
          </SettingRow>
          <SettingRow
            title="空闲释放内存"
            description="闲置后释放内存。下次截图会稍慢一点。"
          >
            <Segmented
              label="空闲释放内存"
              value={String(draft.idleSleepSecs)}
              onChange={(value) => update("idleSleepSecs", Number(value))}
              options={[
                { value: "0", label: "关闭" },
                { value: "300", label: "5 分钟" },
                { value: "600", label: "10 分钟" },
                { value: "1800", label: "30 分钟" },
              ]}
            />
          </SettingRow>

          <details className="settings-disclosure">
            <summary>下载设置（镜像、代理）</summary>
            <div className="disclosure-body">
              <div className="field-grid">
                <Field
                  label="代理地址（可选）"
                  error={proxyError}
                  hint="例如 http://127.0.0.1:7897。留空则读取系统代理。"
                  full
                  action={
                    draft.proxyUrl || draft.proxyUsername || draft.proxyPassword ? (
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => {
                          update("proxyUrl", "");
                          update("proxyUsername", "");
                          update("proxyPassword", "");
                        }}
                      >
                        清除
                      </button>
                    ) : undefined
                  }
                >
                  <input
                    className="input mono"
                    type="text"
                    value={draft.proxyUrl}
                    placeholder="http://127.0.0.1:7897"
                    spellCheck={false}
                    autoComplete="off"
                    aria-label="下载代理地址"
                    aria-invalid={Boolean(proxyError)}
                    onChange={(event) => update("proxyUrl", event.target.value)}
                  />
                </Field>
                <Field
                  label="代理用户名（可选）"
                  error={proxyAuthError}
                  hint="保存时会自动编码。"
                >
                  <input
                    className="input mono"
                    type="text"
                    value={draft.proxyUsername}
                    spellCheck={false}
                    autoComplete="username"
                    aria-label="代理用户名"
                    onChange={(event) => update("proxyUsername", event.target.value)}
                  />
                </Field>
                <Field
                  label="代理密码（可选）"
                  hint="输入原字符即可，不会显示明文。"
                >
                  <input
                    className="input mono"
                    type="password"
                    value={draft.proxyPassword}
                    spellCheck={false}
                    autoComplete="new-password"
                    aria-label="代理密码"
                    onChange={(event) => update("proxyPassword", event.target.value)}
                  />
                </Field>
                <div className="field field-full">
                  <div className="field-head">
                    <span className="field-label">使用镜像</span>
                    <Switch
                      checked={draft.useMirror}
                      onChange={(useMirror) => update("useMirror", useMirror)}
                      label="使用 hf-mirror.com 镜像"
                    />
                  </div>
                  <span className="field-hint">无法直连 Hugging Face 时开启。</span>
                </div>
              </div>
            </div>
          </details>
        </Section>

        <Section
          id="output"
          title="输出"
          description="复制什么，以及保留多少记录。"
        >
          <SettingRow
            title="输出格式"
            description="纯文本适合直接粘贴；Markdown 带代码围栏；JSON 给脚本用。"
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
            description="仅用于 Markdown。留空则自动判断。"
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
            description="只保留最近的记录。"
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

          <div className="setting-group-label">版面</div>
          <SettingRow
            title="从像素重建缩进"
            description="按像素重新计算缩进，避免空格错位。"
          >
            <Switch
              checked={draft.geometry}
              onChange={(geometry) => update("geometry", geometry)}
              label="从像素重建缩进"
            />
          </SettingRow>
          <SettingRow
            title="去除行号栏"
            description="识别并移除编辑器截图里的行号列。"
          >
            <Switch
              checked={!draft.keepGutter}
              onChange={(checked) => update("keepGutter", !checked)}
              label="去除行号栏"
            />
          </SettingRow>
          <SettingRow
            title="合并软换行"
            description="合并被窗口折行的长行。偶尔会误合并，默认关闭。"
          >
            <Switch
              checked={draft.joinWrap}
              onChange={(joinWrap) => update("joinWrap", joinWrap)}
              label="合并软换行"
            />
          </SettingRow>
          <SettingRow
            title="长截图切块"
            description="长图在空白行处切块；回复过短时自动重试。"
          >
            <Switch
              checked={draft.tile}
              onChange={(tile) => update("tile", tile)}
              label="长截图切块"
            />
          </SettingRow>
        </Section>

        <Section
          id="advanced"
          title="高级"
          description="影响识别质量和内存占用。一般保持默认。"
        >
          <SettingRow
            title="识别引擎"
            description={
              draft.engine === "auto"
                ? "优先用本地模型，不可用时回落到 tesseract。"
                : draft.engine === "llamacpp"
                  ? "只用本地模型，符号和中文更准。"
                  : "不加载模型，速度较快，但符号和中文较弱。"
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
                description="已有 llama.cpp 服务时直接使用，不再启动内置服务。"
                stack
              >
                <div className="field-grid">
                  <Field
                    label="服务地址"
                    error={urlError}
                    hint={urlError ? undefined : `环境变量可覆盖：OCR4CODE_LLAMA_URL`}
                    full
                    action={
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => void handleTestUrl()}
                        disabled={testingUrl}
                      >
                        {testingUrl ? "检测中…" : "检测此地址"}
                      </button>
                    }
                  >
                    <input
                      className="input mono"
                      type="url"
                      value={draft.llamaUrl}
                      spellCheck={false}
                      aria-label="llama.cpp 服务地址"
                      aria-invalid={Boolean(urlError)}
                      onChange={(event) => update("llamaUrl", event.target.value)}
                    />
                  </Field>

                  <Field
                    label="提示词"
                    error={promptError}
                    hint="影响版面保留程度。默认值已按常见代码截图调过。"
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
                    label={`最大输出长度 — ${draft.maxTokens}`}
                    hint={`长截图需要更大的值。当前 -c ${context} -np ${slots}（每槽 ${slotCtx} token）。`}
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
                    label={`并行识别数 — ${draft.jobs}`}
                    hint={`只在长截图切块时使用。${slots} 个并行的结果会排队处理。`}
                    full
                  >
                    <div className="range-row">
                      <input
                        type="range"
                        min={1}
                        max={6}
                        step={1}
                        value={draft.jobs}
                        aria-label="并行识别数"
                        onChange={(event) => update("jobs", Number(event.target.value))}
                      />
                      <span className="range-value">{draft.jobs}</span>
                    </div>
                  </Field>
                </div>
              </SettingRow>
            </>
          )}

          <SettingRow
            title="压缩识别缓存"
            description="减少内存占用；Windows 上可能变慢。"
          >
            <Switch
              checked={draft.kvCacheQuant}
              onChange={(kvCacheQuant) => update("kvCacheQuant", kvCacheQuant)}
              label="压缩识别缓存"
            />
          </SettingRow>

          {draft.engine !== "llamacpp" && (
            <div className={draft.engine === "auto" ? "engine-fallback" : ""}>
              {draft.engine === "auto" && (
                <div className="fallback-label">
                  <span>备用识别</span>
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
                  hint="大多数截图用单块模式即可。"
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
          id="about"
          title="关于"
          description="版本、更新和退出。"
          meta={<span className="about-version">v{appVersion || "—"}</span>}
        >
          <div className="about-grid">
            <div>
              <span className="about-label">识别</span>
              <span className={`about-value tone-${engine.tone}`}>
                <span className="status-dot" />
                {engine.label}
              </span>
            </div>
            <div>
              <span className="about-label">隐私</span>
              <span className="about-value">本地处理，不上传</span>
            </div>
            <div>
              <span className="about-label">截图</span>
              <span className="about-value">
                {isMac ? "macOS 系统选区" : "Windows 自绘选区"}
              </span>
            </div>
            <div>
              <span className="about-label">快捷键</span>
              <span className="about-value">
                <Keycaps accelerator={draft.shortcut} size="sm" />
              </span>
            </div>
          </div>

          <div className="setting-group-label">更新</div>
          <SettingRow
            title="启动时检查更新"
            description="后台检查新版本，不打断截图。"
          >
            <Switch
              checked={draft.autoCheckUpdates}
              onChange={(autoCheckUpdates) => update("autoCheckUpdates", autoCheckUpdates)}
              label="启动时检查更新"
            />
          </SettingRow>
          <SettingRow
            title="发现有更新时自动下载"
            description="安装前仍会等你确认重启。"
          >
            <Switch
              checked={draft.autoDownloadUpdates}
              onChange={(autoDownloadUpdates) =>
                update("autoDownloadUpdates", autoDownloadUpdates)
              }
              label="发现有更新时自动下载"
            />
          </SettingRow>
          <SettingRow title="检查更新" description={updateDescription}>
            <div className="update-actions">
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={onCheckUpdates}
                disabled={
                  updatePhase === "checking" ||
                  updatePhase === "downloading" ||
                  updatePhase === "installing" ||
                  updatePhase === "ready"
                }
              >
                {updatePhase === "checking" ? "检查中…" : "检查更新"}
              </button>
              {updatePhase === "available" && (
                <button type="button" className="btn btn-primary btn-sm" onClick={onDownloadUpdate}>
                  下载更新
                </button>
              )}
              {updatePhase === "ready" && (
                <button type="button" className="btn btn-primary btn-sm" onClick={onInstallUpdate}>
                  重启并安装
                </button>
              )}
            </div>
          </SettingRow>
          {updatePhase === "downloading" && (
            <div className="update-progress" role="status">
              <div className="update-bar">
                <span style={{ width: `${Math.max(0, Math.min(100, updateProgress))}%` }} />
              </div>
              <span>{Math.round(updateProgress)}%</span>
            </div>
          )}
          {updatePhase === "error" && updateError && (
            <p className="field-error update-error">{updateError}</p>
          )}

          <div className="about-foot">
            <div className="about-actions">
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={onResetOnboarding}
              >
                <Icon name="refresh" size={14} />
                重新显示引导
              </button>
              <button type="button" className="btn btn-ghost btn-danger btn-sm" onClick={onQuit}>
                <Icon name="power" size={14} />
                退出 ocr4code
              </button>
            </div>
            <p>
              {isMac
                ? "截图需要屏幕录制权限。授权后如果状态没变，请重启应用。"
                : "Windows 选区由应用自己绘制，不调用系统截屏工具。"}
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
          ) : dirty && !valid ? (
            <span className="save-error">
              <Icon name="alert" size={14} />
              有字段需要修正后才能保存
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
