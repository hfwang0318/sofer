import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from "react";

import {
  openScreenSettings,
  requestScreenAccess,
  restartApp,
  screenAccess,
} from "../lib/bridge";
import { isMac } from "../lib/platform";
import type { ScreenAccess } from "../lib/types";
import type { PushToast } from "./useToast";

interface UseScreenPermissionOptions {
  initialAccess: ScreenAccess;
  setSetupOpen: (open: boolean) => void;
  setPermissionOpen: (open: boolean) => void;
  pushToast: PushToast;
}

export interface ScreenPermission {
  screenAccessState: ScreenAccess;
  setScreenAccessState: Dispatch<SetStateAction<ScreenAccess>>;
  permissionBusy: boolean;
  refreshScreenAccess: () => Promise<ScreenAccess | null>;
  requestPermission: () => void;
  handleRequestPermission: () => Promise<void>;
  handleOpenPermissionSettings: () => Promise<void>;
  handleRecheckPermission: () => Promise<void>;
  handleRestart: () => Promise<void>;
}

/** macOS screen-recording permission: state, the focus re-check and the dialog actions. */
export function useScreenPermission({
  initialAccess,
  setSetupOpen,
  setPermissionOpen,
  pushToast,
}: UseScreenPermissionOptions): ScreenPermission {
  const [screenAccessState, setScreenAccessState] =
    useState<ScreenAccess>(initialAccess);
  const [permissionBusy, setPermissionBusy] = useState(false);

  const refreshScreenAccess = useCallback(async (): Promise<ScreenAccess | null> => {
    try {
      const access = await screenAccess();
      setScreenAccessState(access);
      return access;
    } catch {
      return null;
    }
  }, []);

  // Re-check after the user comes back from System Settings.
  useEffect(() => {
    if (!isMac) return;
    function onFocus() {
      void refreshScreenAccess();
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshScreenAccess]);

  const requestPermission = useCallback(() => {
    setSetupOpen(false);
    setPermissionOpen(true);
  }, [setPermissionOpen, setSetupOpen]);

  const handleRequestPermission = useCallback(async () => {
    if (permissionBusy) return;
    setPermissionBusy(true);
    try {
      const granted = await requestScreenAccess();
      const access = await screenAccess().catch(() => null);
      if (access) setScreenAccessState(access);
      if (granted || access === "granted") {
        pushToast("ok", "屏幕录制权限已开启", "现在可以截图了。");
      } else {
        pushToast(
          "info",
          "还需要在系统设置里勾选",
          "勾选 sofer 后回来点“重新检查”，或重启应用。",
        );
      }
    } catch (cause) {
      pushToast(
        "error",
        "无法请求权限",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setPermissionBusy(false);
    }
  }, [permissionBusy, pushToast]);

  const handleOpenPermissionSettings = useCallback(async () => {
    try {
      await openScreenSettings();
    } catch {
      pushToast(
        "error",
        "无法打开系统设置",
        "请手动打开 系统设置 → 隐私与安全性 → 屏幕录制。",
      );
    }
  }, [pushToast]);

  const handleRecheckPermission = useCallback(async () => {
    setPermissionBusy(true);
    try {
      const access = await screenAccess();
      setScreenAccessState(access);
      if (access === "granted" || access === "not-needed") {
        pushToast("ok", "权限已开启", "可以开始截图了。");
      } else {
        pushToast(
          "info",
          "还没有授权",
          "在系统设置里勾选 sofer；如果已经勾选，请重启应用。",
        );
      }
    } catch (cause) {
      pushToast(
        "error",
        "无法检查权限",
        cause instanceof Error ? cause.message : String(cause),
      );
    } finally {
      setPermissionBusy(false);
    }
  }, [pushToast]);

  const handleRestart = useCallback(async () => {
    try {
      await restartApp();
    } catch (cause) {
      pushToast(
        "error",
        "无法重启应用",
        cause instanceof Error ? cause.message : String(cause),
      );
    }
  }, [pushToast]);

  return {
    screenAccessState,
    setScreenAccessState,
    permissionBusy,
    refreshScreenAccess,
    requestPermission,
    handleRequestPermission,
    handleOpenPermissionSettings,
    handleRecheckPermission,
    handleRestart,
  };
}
