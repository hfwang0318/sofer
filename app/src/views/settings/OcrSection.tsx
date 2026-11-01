import { Icon } from "../../components/Icon";
import { Segmented, SettingRow, Switch } from "../../components/Controls";
import { RemotePanel } from "../../components/RemotePanel";
import { PROMPT_PRESETS, promptPresetOf } from "../../lib/defaults";
import type { EngineSummary } from "../../lib/status";
import type {
  DownloadProgress,
  EngineBusy,
  EngineStatus,
  Settings,
  ToastTone,
} from "../../lib/types";
import { LocalEnginePanel } from "./LocalEnginePanel";
import { Field, Section, type UpdateSetting } from "./shared";

interface OcrSectionProps {
  draft: Settings;
  update: UpdateSetting;
  status: EngineStatus | null;
  download: DownloadProgress | null;
  engineBusy: EngineBusy;
  engine: EngineSummary;
  testing: boolean;
  onTest: () => void;
  onSetup: () => void;
  onDownloadModel: () => void;
  onStartEngine: () => void;
  onStopEngine: () => void;
  onRemoveModel: () => void;
  onCancelSetup: () => void;
  onSave: (draft: Settings) => Promise<void>;
  onNotify: (tone: ToastTone, title: string, detail?: string) => void;
  proxyError: string | null;
  proxyAuthError: string | null;
}

/** Engine health, model pack management, presets and download settings. */
export function OcrSection({
  draft,
  update,
  status,
  download,
  engineBusy,
  engine,
  testing,
  onTest,
  onSetup,
  onDownloadModel,
  onStartEngine,
  onStopEngine,
  onRemoveModel,
  onCancelSetup,
  onSave,
  onNotify,
  proxyError,
  proxyAuthError,
}: OcrSectionProps) {
  const activePreset = promptPresetOf(draft.promptPreset);

  return (
    <Section
      id="recognition"
      title="OCR"
      description="截图交给哪个 OCR 引擎，以及模型放在哪里。"
      meta={
        <div className={`engine-health tone-${engine.tone}`}>
          <span className="status-dot" />
          <span>{engine.label}</span>
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={onTest}
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
      {status && !draft.remoteEnabled && (
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
      {status && (
        <RemotePanel
          draft={draft}
          remote={status.remote}
          onUpdate={update}
          onSave={onSave}
          onNotify={onNotify}
        />
      )}
      {draft.engine !== "tesseract" && (
        <SettingRow
          title="识别预设"
          description="按截图类型选择提示词，只影响 OCR 引擎怎么读图，不改变引擎。"
          stack
        >
          <div className="preset-group">
            <Segmented
              label="识别预设"
              value={draft.promptPreset}
              onChange={(promptPreset) => update("promptPreset", promptPreset)}
              options={PROMPT_PRESETS.map(({ value, label }) => ({
                value,
                label,
              }))}
            />
            <span className="field-hint">{activePreset.description}</span>
          </div>
        </SettingRow>
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
  );
}
