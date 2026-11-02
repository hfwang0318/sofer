import { Icon } from "../../components/Icon";
import { Segmented, SettingRow, Switch } from "../../components/Controls";
import { DEFAULT_PROMPT } from "../../lib/defaults";
import { llamaSlots, perSlotCtx, totalCtx } from "../../lib/llamaPlan";
import type { Settings } from "../../lib/types";
import { Field, PSM_OPTIONS, Section, type UpdateSetting } from "./shared";

interface AdvancedSectionProps {
  draft: Settings;
  update: UpdateSetting;
  urlError: string | null;
  promptError: string | null;
  testingUrl: boolean;
  onTestUrl: () => void;
  langError: string | null;
}

/** Engine mode, external service, prompt/context limits and tesseract fallback. */
export function AdvancedSection({
  draft,
  update,
  urlError,
  promptError,
  testingUrl,
  onTestUrl,
  langError,
}: AdvancedSectionProps) {
  const slots = llamaSlots(draft.jobs);
  const slotCtx = perSlotCtx(draft.maxTokens);
  const context = totalCtx(draft.jobs, draft.maxTokens);

  return (
    <Section
      id="advanced"
      title="高级"
      description="影响识别质量和内存占用。一般保持默认。"
    >
      <SettingRow
        title="引擎模式"
        description={
          draft.engine === "auto"
            ? "优先用本地模型，不可用时回落到 tesseract。"
            : draft.engine === "llamacpp"
              ? "只用本地模型，符号和中文更准。"
              : "不加载模型，速度较快，但符号和中文较弱。"
        }
      >
        <Segmented
          label="引擎模式"
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
                hint={urlError ? undefined : `环境变量可覆盖：SOFER_LLAMA_URL`}
                full
                action={
                  <button
                    type="button"
                    className="link-btn"
                    onClick={onTestUrl}
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
                label="自定义提示词"
                error={promptError}
                hint={
                  draft.promptPreset === "custom"
                    ? "当前使用这一提示词。"
                    : "在 OCR → 识别预设里选择「自定义」后才用于识别。"
                }
                full
                action={
                  draft.promptPreset === "custom" ? (
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => update("customPrompt", DEFAULT_PROMPT)}
                    >
                      填入默认文案
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => update("promptPreset", "custom")}
                    >
                      使用自定义
                    </button>
                  )
                }
              >
                <textarea
                  className="input textarea mono"
                  rows={3}
                  value={draft.customPrompt}
                  placeholder={DEFAULT_PROMPT}
                  spellCheck={false}
                  aria-label="自定义提示词"
                  aria-invalid={Boolean(promptError)}
                  onChange={(event) => update("customPrompt", event.target.value)}
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
        description="默认开启，减少内存占用；关闭后识别略快，但占用更多内存。"
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
              <span>备用</span>
              <span className="fallback-line" />
              <span>tesseract</span>
            </div>
          )}
          <div className="field-grid">
            <Field
              label="tesseract 语言"
              error={draft.engine === "tesseract" ? langError : null}
              hint="需要先安装语言包，例如 eng+chi_sim。"
            >
              <input
                className="input mono"
                value={draft.tessLang}
                spellCheck={false}
                aria-label="tesseract 语言"
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
                  aria-label="tesseract 页面分割模式"
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
  );
}
