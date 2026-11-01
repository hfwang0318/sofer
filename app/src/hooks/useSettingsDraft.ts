import {
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";

import { appInfo, engineStatus, loadSettings, screenAccess } from "../lib/bridge";
import { DEFAULT_SETTINGS } from "../lib/defaults";
import type { EngineStatus, ScreenAccess, Settings } from "../lib/types";
import type { PushToast } from "./useToast";

export interface SettingsDraft {
  settings: Settings;
  setSettings: Dispatch<SetStateAction<Settings>>;
  /** Always-current settings for event handlers that must not re-subscribe. */
  settingsRef: RefObject<Settings>;
  shortcutActive: boolean;
  setShortcutActive: (active: boolean) => void;
  appVersion: string;
  setAppVersion: (version: string) => void;
}

/** The editable settings value shared by the shell, the views and the dialogs. */
export function useSettingsDraft(): SettingsDraft {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [shortcutActive, setShortcutActive] = useState(true);
  const [appVersion, setAppVersion] = useState("");
  const settingsRef = useRef(settings);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  return {
    settings,
    setSettings,
    settingsRef,
    shortcutActive,
    setShortcutActive,
    appVersion,
    setAppVersion,
  };
}

interface SettingsBootstrapOptions {
  setSettings: Dispatch<SetStateAction<Settings>>;
  setStatus: (status: EngineStatus) => void;
  setShortcutActive: (active: boolean) => void;
  setAppVersion: (version: string) => void;
  setScreenAccess: (access: ScreenAccess) => void;
  pushToast: PushToast;
}

/** Load persisted settings and check the engine once, before the first capture. */
export function useSettingsBootstrap({
  setSettings,
  setStatus,
  setShortcutActive,
  setAppVersion,
  setScreenAccess,
  pushToast,
}: SettingsBootstrapOptions): void {
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [loaded, engine, info, access] = await Promise.all([
          loadSettings(),
          engineStatus(),
          appInfo(),
          screenAccess().catch(() => null),
        ]);
        if (!alive) return;
        setSettings(loaded);
        setStatus(engine);
        setShortcutActive(info.shortcutActive);
        setAppVersion(info.version);
        if (access) setScreenAccess(access);
        if (!info.shortcutActive) {
          pushToast(
            "error",
            "全局快捷键不可用",
            `${info.shortcut} 可能被其他应用占用，请到设置里换一个组合。`,
          );
        }
      } catch (cause) {
        if (!alive) return;
        pushToast(
          "error",
          "无法加载设置",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    })();
    return () => {
      alive = false;
    };
  }, [pushToast, setAppVersion, setScreenAccess, setSettings, setShortcutActive, setStatus]);
}
