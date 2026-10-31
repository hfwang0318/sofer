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
  { value: 3, label: "3 · Fully automatic" },
  { value: 4, label: "4 · Single column" },
  { value: 6, label: "6 · Single block" },
  { value: 7, label: "7 · Single line" },
  { value: 11, label: "11 · Sparse text" },
  { value: 13, label: "13 · Raw line" },
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
          <strong>Local engine</strong>
          <span className="badge">
            {runtime.bundled ? "Bundled runtime" : runtime.system ? "System runtime" : "Runtime missing"}
          </span>
          {model.installed && (
            <span className={`badge ${model.verified ? "badge-ok" : "badge-warn"}`}>
              {model.verified ? "Verified" : "Unverified"}
            </span>
          )}
        </div>
        <div className="runtime-actions">
          {!model.installed ? (
            <button type="button" className="btn btn-primary btn-sm" onClick={onDownloadModel} disabled={busy}>
              <Icon name="arrow-right" size={14} />
              Download model
            </button>
          ) : server.state === "starting" ? (
            <button type="button" className="btn btn-secondary btn-sm" disabled>
              <span className="btn-spinner btn-spinner-muted" />
              Starting…
            </button>
          ) : managed && server.state === "online" ? (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStopEngine} disabled={busy}>
              {engineBusy === "stop" ? "Stopping…" : "Stop"}
            </button>
          ) : (
            <button type="button" className="btn btn-secondary btn-sm" onClick={onStartEngine} disabled={busy}>
              <Icon name="power" size={14} />
              {engineBusy === "start" ? "Starting…" : "Start engine"}
            </button>
          )}
        </div>
      </div>

      <div className="runtime-grid">
        <div>
          <span className="runtime-label">Runtime</span>
          <span className="runtime-value">
            {runtime.version ?? (runtime.bundled || runtime.system ? "llama.cpp" : "Not found")}
          </span>
        </div>
        <div>
          <span className="runtime-label">Model</span>
          <span className="runtime-value">
            {model.installed
              ? `${formatBytes(model.bytes)} · ${model.verified ? "SHA-256 verified" : "not verified"}`
              : `${formatBytes(model.total)} download`}
          </span>
        </div>
        <div>
          <span className="runtime-label">Server</span>
          <span className={`runtime-value tone-${server.state === "online" ? "ok" : server.state === "failed" ? "danger" : "muted"}`}>
            {server.state === "online"
              ? `${managed ? "Managed" : "External"} · ${server.url ?? status.llamaUrl}`
              : server.state === "starting"
                ? "Starting…"
                : server.state === "failed"
                  ? "Failed"
                  : "Stopped"}
          </span>
        </div>
      </div>

      {downloading && download && (
        <div className="runtime-download">
          <div className="runtime-download-head">
            <span>
              {download.phase === "verifying"
                ? "Verifying SHA-256…"
                : download.phase === "starting"
                  ? "Starting llama-server…"
                  : download.file ?? "Downloading model"}
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
              Cancel download
            </button>
          )}
        </div>
      )}

      {download?.phase === "error" && (
        <p className="runtime-error">
          <Icon name="alert" size={13} />
          {download.message ?? "Setup failed."}
          <button type="button" className="link-btn" onClick={onSetup}>
            Retry
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
          {model.installed ? "Re-run setup" : "Open setup guide"}
        </button>
        {model.installed && (
          <button
            type="button"
            className="link-btn link-btn-danger"
            onClick={() => setConfirmRemove(true)}
            disabled={busy}
          >
            Remove model
          </button>
        )}
        <span className="runtime-foot-note">
          {model.installed ? "Runs offline · no cloud" : "One-time 1.43 GB · resumable"}
        </span>
      </div>

      {confirmRemove && (
        <div className="runtime-confirm">
          <span>Remove {formatBytes(model.bytes)} of model files?</span>
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
              Remove
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setConfirmRemove(false)}
            >
              Cancel
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
          "Global shortcut could not be restored",
          cause instanceof Error ? cause.message : String(cause),
        );
      });
    },
    [onNotify],
  );

  const urlError =
    !draft.llamaUrl.trim() || !/^https?:\/\//i.test(draft.llamaUrl.trim())
      ? "Use a full http:// or https:// URL."
      : null;
  const promptError = draft.prompt.trim() ? null : "The prompt cannot be empty.";
  const langError = draft.tessLang.trim() ? null : "Try `eng` or `eng+chi_sim`.";
  const fenceError =
    draft.language.trim() && !/^[a-zA-Z0-9+#._-]*$/.test(draft.language.trim())
      ? "Use a short fence name like rust, python or diff."
      : null;
  const valid =
    !urlError &&
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
      setSaveError("Some fields need attention before saving.");
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
            ? "llama.cpp did not answer; tesseract is available."
            : "Start llama-server or install tesseract.",
      );
    } catch (error) {
      onNotify(
        "error",
        "Engine check failed",
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setTesting(false);
    }
  }

  function resetDefaults() {
    setDraft({ ...DEFAULT_SETTINGS });
    setSaveError(null);
    onNotify("info", "Defaults loaded", "Review them, then save to apply.");
  }

  return (
    <div className="view settings-view">
      <div className="view-scroll">
        <header className="view-head">
          <div>
            <p className="eyebrow">Preferences</p>
            <h1>Settings</h1>
            <p className="lede">
              Every kernel knob the desktop layer exposes. Changes apply to the next
              capture; the OCR core itself is unchanged.
            </p>
          </div>
          <span className="settings-version">ocr4code 0.1.0</span>
        </header>

        <Section
          title="Capture"
          description="How the app hands a screenshot to the kernel."
        >
          <SettingRow
            title="Global shortcut"
            description="Works from any app while ocr4code is running. The system selection follows your normal screenshot permissions."
          >
            <ShortcutRecorder
              value={draft.shortcut}
              onChange={(shortcut) => update("shortcut", shortcut)}
              onRecordingChange={handleRecordingChange}
            />
          </SettingRow>
          <SettingRow
            title="Copy automatically"
            description="Write the recognised text to the clipboard as soon as it is ready."
          >
            <Switch
              checked={draft.autoCopy}
              onChange={(autoCopy) => update("autoCopy", autoCopy)}
              label="Copy automatically"
            />
          </SettingRow>
        </Section>

        <Section
          title="Engine"
          description="Text comes from the model; layout comes from a pure-CV geometry pass either way."
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
                {testing ? "Testing" : "Test"}
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
            title="Recognition engine"
            description={
              draft.engine === "auto"
                ? "Uses llama.cpp when the server answers, then falls back to tesseract."
                : draft.engine === "llamacpp"
                  ? "Sends pixels to your local llama-server. Best layout and symbol fidelity."
                  : "Runs locally with no model server. Faster, but weaker on symbols and CJK."
            }
          >
            <Segmented
              label="Recognition engine"
              value={draft.engine}
              onChange={(next) => update("engine", next)}
              options={[
                { value: "auto", label: "Auto" },
                { value: "llamacpp", label: "llama.cpp" },
                { value: "tesseract", label: "tesseract" },
              ]}
            />
          </SettingRow>

          {draft.engine !== "tesseract" && (
            <>
              <SettingRow
                title="External server (optional)"
                description="If a llama.cpp server is already running at this URL, ocr4code uses it instead of starting the bundled one."
                stack
              >
                <div className="field-grid">
                  <Field
                    label="External llama.cpp URL"
                    error={urlError}
                    hint={
                      urlError
                        ? undefined
                        : `Environment override: OCR4CODE_LLAMA_URL · ${draft.llamaUrl}`
                    }
                    full
                  >
                    <div className="input-with-action">
                      <input
                        className="input mono"
                        type="url"
                        value={draft.llamaUrl}
                        spellCheck={false}
                        aria-label="External llama.cpp URL"
                        aria-invalid={Boolean(urlError)}
                        onChange={(event) => update("llamaUrl", event.target.value)}
                      />
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        onClick={() => void handleTest()}
                        disabled={testing}
                      >
                        {testing ? "Testing…" : "Test"}
                      </button>
                    </div>
                  </Field>

                  <Field
                    label="Prompt"
                    error={promptError}
                    hint="Prompt wording changes layout more than any other knob. The default is measured on 26 scenes."
                    full
                    action={
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => update("prompt", DEFAULT_PROMPT)}
                      >
                        Restore default
                      </button>
                    }
                  >
                    <textarea
                      className="input textarea mono"
                      rows={3}
                      value={draft.prompt}
                      spellCheck={false}
                      aria-label="Model prompt"
                      aria-invalid={Boolean(promptError)}
                      onChange={(event) => update("prompt", event.target.value)}
                    />
                  </Field>

                  <Field
                    label={`Max output tokens — ${draft.maxTokens}`}
                    hint="Also sets the row budget for tiling. 3072 is the measured default; 8192 can over-generate."
                    full
                  >
                    <div className="range-row">
                      <input
                        type="range"
                        min={512}
                        max={8192}
                        step={128}
                        value={draft.maxTokens}
                        aria-label="Max output tokens"
                        onChange={(event) =>
                          update("maxTokens", Number(event.target.value))
                        }
                      />
                      <span className="range-value">{draft.maxTokens}</span>
                    </div>
                  </Field>

                  <Field
                    label={`Parallel engine calls — ${draft.jobs}`}
                    hint="Only used when a screenshot is tiled. 3 matches llama-server's four slots without queueing."
                    full
                  >
                    <div className="range-row">
                      <input
                        type="range"
                        min={1}
                        max={6}
                        step={1}
                        value={draft.jobs}
                        aria-label="Parallel engine calls"
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
                  <span>Fallback</span>
                  <span className="fallback-line" />
                  <span>tesseract</span>
                </div>
              )}
              <div className="field-grid">
                <Field
                  label="Tesseract language"
                  error={draft.engine === "tesseract" ? langError : null}
                  hint="Install language data first, e.g. eng+chi_sim."
                >
                  <input
                    className="input mono"
                    value={draft.tessLang}
                    spellCheck={false}
                    aria-label="Tesseract language"
                    onChange={(event) => update("tessLang", event.target.value)}
                  />
                </Field>
                <Field
                  label="Page segmentation"
                  hint="Tesseract's PSM. Single block works for most screenshots."
                >
                  <div className="select-wrap">
                    <select
                      className="input select"
                      value={draft.psm}
                      aria-label="Tesseract page segmentation mode"
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
          title="Layout reconstruction"
          description="The geometry pass runs on every engine and is what keeps a paste useful."
        >
          <SettingRow
            title="Rebuild indentation from pixels"
            description="Compute leading spaces from the ink mask instead of trusting the model's own spacing."
          >
            <Switch
              checked={draft.geometry}
              onChange={(geometry) => update("geometry", geometry)}
              label="Rebuild indentation from pixels"
            />
          </SettingRow>
          <SettingRow
            title="Drop line-number gutters"
            description="Detect a line-number column and remove it. Turn this off to keep the model's spacing for editor captures."
          >
            <Switch
              checked={!draft.keepGutter}
              onChange={(checked) => update("keepGutter", !checked)}
              label="Drop line-number gutters"
            />
          </SettingRow>
          <SettingRow
            title="Join soft-wrapped lines"
            description="Heuristic. A long line that happens to reach the margin can be merged by mistake, so this is off by default."
          >
            <Switch
              checked={draft.joinWrap}
              onChange={(joinWrap) => update("joinWrap", joinWrap)}
              label="Join soft-wrapped lines"
            />
          </SettingRow>
          <SettingRow
            title="Tile tall screenshots"
            description="Split at blank rows when the output budget would truncate a 4K capture, then retry once if the reply is short."
          >
            <Switch
              checked={draft.tile}
              onChange={(tile) => update("tile", tile)}
              label="Tile tall screenshots"
            />
          </SettingRow>
        </Section>

        <Section
          title="Output"
          description="What gets copied, and how much history the app keeps."
        >
          <SettingRow
            title="Format"
            description="Text pastes directly; markdown adds a fenced block with a guessed language; JSON carries per-line indentation."
          >
            <Segmented
              label="Output format"
              value={draft.format}
              onChange={(format) => update("format", format)}
              options={[
                { value: "text", label: "Text" },
                { value: "markdown", label: "Markdown" },
                { value: "json", label: "JSON" },
              ]}
            />
          </SettingRow>
          <SettingRow
            title="Fence language"
            description="Used only for markdown. Leave empty to let the kernel guess from the recognised text."
          >
            <input
              className="input mono input-inline"
              value={draft.language}
              placeholder="auto"
              spellCheck={false}
              disabled={draft.format !== "markdown"}
              aria-label="Markdown fence language"
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
            title="History limit"
            description="Older captures are dropped from this device once the limit is reached."
          >
            <div className="select-wrap select-compact">
              <select
                className="input select"
                value={draft.historyLimit}
                aria-label="History limit"
                onChange={(event) => update("historyLimit", Number(event.target.value))}
              >
                {[5, 10, 20, 50].map((limit) => (
                  <option key={limit} value={limit}>
                    {limit} captures
                  </option>
                ))}
              </select>
              <Icon name="chevron-down" size={14} className="select-chevron" />
            </div>
          </SettingRow>
        </Section>

        <Section
          title="About"
          description="A desktop shell around the ocr4code kernel."
          meta={<span className="about-version">v0.1.0</span>}
        >
          <div className="about-grid">
            <div>
              <span className="about-label">Engine</span>
              <span className={`about-value tone-${engine.tone}`}>
                <span className="status-dot" />
                {engine.label}
              </span>
            </div>
            <div>
              <span className="about-label">Privacy</span>
              <span className="about-value">Runs offline on this machine</span>
            </div>
            <div>
              <span className="about-label">Capture</span>
              <span className="about-value">
                Native region selection · {isMac ? "screencapture" : "Snipping Tool"}
              </span>
            </div>
            <div>
              <span className="about-label">Shortcut</span>
              <span className="about-value">
                <Keycaps accelerator={draft.shortcut} size="sm" />
              </span>
            </div>
          </div>
          <div className="about-foot">
            <button type="button" className="btn btn-ghost btn-danger btn-sm" onClick={onQuit}>
              <Icon name="power" size={14} />
              Quit ocr4code
            </button>
            <p>
              {isMac
                ? "If capture comes back empty, enable ocr4code under System Settings → Privacy & Security → Screen Recording and relaunch."
                : "The Windows Snipping Tool handles the selection; ocr4code reads the image it puts on the clipboard."}
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
              Unsaved changes
            </span>
          ) : (
            <span className="save-clean">
              <Icon name="check" size={14} />
              All changes saved
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
            Reset to defaults
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void handleSave()}
            disabled={!dirty || saving || !valid}
          >
            {saving ? <span className="btn-spinner" /> : <Icon name="check" size={15} />}
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </footer>
    </div>
  );
}

