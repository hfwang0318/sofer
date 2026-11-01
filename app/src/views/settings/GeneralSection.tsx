import {
  Segmented,
  SettingRow,
  ShortcutRecorder,
  Switch,
} from "../../components/Controls";
import { isMac } from "../../lib/platform";
import type { ScreenAccess, Settings } from "../../lib/types";
import { Section, type UpdateSetting } from "./shared";

interface GeneralSectionProps {
  draft: Settings;
  update: UpdateSetting;
  onRecordingChange: (recording: boolean) => void;
  screenAccess: ScreenAccess;
  onRequestPermission: () => void;
  onOpenPermissionSettings: () => void;
}

/** Shortcut, copy/notify behaviour, fast preview, UI scale and screen access. */
export function GeneralSection({
  draft,
  update,
  onRecordingChange,
  screenAccess,
  onRequestPermission,
  onOpenPermissionSettings,
}: GeneralSectionProps) {
  return (
    <Section id="general" title="通用" description="截图方式、快捷键和界面大小。">
      <SettingRow
        title="全局快捷键"
        description="在任何应用里都能截图。只有 F 键能单独使用；其他键请搭配 ⌘/Ctrl/⌥。"
      >
        <ShortcutRecorder
          value={draft.shortcut}
          onChange={(shortcut) => update("shortcut", shortcut)}
          onRecordingChange={onRecordingChange}
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
        title="识别完成提示"
        description="应用不在前台且结果已复制时，在屏幕底部短暂显示行数。"
      >
        <Switch
          checked={draft.notifyOnResult}
          onChange={(notifyOnResult) =>
            update("notifyOnResult", notifyOnResult)
          }
          label="识别完成提示"
        />
      </SettingRow>
      {isMac && (
        <SettingRow
          title="快速预览"
          description="先用系统 OCR 立即给出结果，再用本地模型精修；只影响识别过程中的预览，不改变最终结果。"
        >
          <Switch
            checked={draft.fastPreview}
            onChange={(fastPreview) => update("fastPreview", fastPreview)}
            label="快速预览"
          />
        </SettingRow>
      )}
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
  );
}
