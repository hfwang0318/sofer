import { Icon } from "../../components/Icon";
import { Segmented, SettingRow, Switch } from "../../components/Controls";
import type { Settings } from "../../lib/types";
import { Section, type UpdateSetting } from "./shared";

interface OutputSectionProps {
  draft: Settings;
  update: UpdateSetting;
  fenceError: string | null;
}

/** Output format, fence language, history limit and the layout switches. */
export function OutputSection({ draft, update, fenceError }: OutputSectionProps) {
  return (
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
  );
}
