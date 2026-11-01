import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

import { prepareUpdate } from "../lib/bridge";
import { isTauri } from "../lib/platform";
import type { Settings } from "../lib/types";
import {
  checkForUpdate,
  downloadUpdate,
  installUpdate,
  type AvailableUpdate,
  type UpdatePhase,
} from "../lib/updater";
import type { PushToast } from "./useToast";

interface UseUpdatesOptions {
  settingsRef: RefObject<Settings>;
  appVersion: string;
  autoCheckUpdates: boolean;
  pushToast: PushToast;
}

export interface UpdatesController {
  updateInfo: AvailableUpdate | null;
  updatePhase: UpdatePhase;
  updateProgress: number;
  updateError: string | null;
  checkUpdates: (manual: boolean) => Promise<void>;
  downloadNow: () => Promise<void>;
  installNow: () => Promise<void>;
}

/** Desktop updater: check, download and install, with at most one in flight. */
export function useUpdates({
  settingsRef,
  appVersion,
  autoCheckUpdates,
  pushToast,
}: UseUpdatesOptions): UpdatesController {
  const [updateInfo, setUpdateInfo] = useState<AvailableUpdate | null>(null);
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>("idle");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const updateBusy = useRef(false);

  const beginDownloadFor = useCallback(
    async (next: AvailableUpdate, automatic: boolean) => {
      setUpdatePhase("downloading");
      setUpdateProgress(0);
      setUpdateError(null);
      try {
        await downloadUpdate(next.update, setUpdateProgress);
        setUpdatePhase("ready");
        pushToast(
          "ok",
          `v${next.version} 已下载`,
          automatic ? "到 设置 → 关于 重启并安装。" : "重启应用后完成安装。",
        );
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setUpdatePhase("error");
        setUpdateError(message);
        if (!automatic) pushToast("error", "更新下载失败", message);
      }
    },
    [pushToast],
  );

  const checkUpdates = useCallback(
    async (manual: boolean) => {
      if (!isTauri || import.meta.env.DEV) {
        if (manual) {
          pushToast(
            "info",
            "当前不检查更新",
            isTauri ? "开发模式请使用安装版测试更新。" : "浏览器预览不会检查版本。",
          );
        }
        return;
      }
      if (updateBusy.current) return;
      updateBusy.current = true;
      setUpdatePhase("checking");
      setUpdateError(null);
      try {
        const next = await checkForUpdate();
        if (!next) {
          setUpdatePhase("current");
          if (manual) {
            pushToast("ok", "已是最新版本", appVersion ? `当前 v${appVersion}` : undefined);
          }
          return;
        }
        setUpdateInfo(next);
        setUpdatePhase("available");
        setUpdateProgress(0);
        pushToast("info", `发现新版本 v${next.version}`, "可在 设置 → 关于 下载。");
        if (settingsRef.current.autoDownloadUpdates) {
          await beginDownloadFor(next, true);
        }
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setUpdatePhase("error");
        setUpdateError(message);
        if (manual) pushToast("error", "检查更新失败", message);
      } finally {
        updateBusy.current = false;
      }
    },
    [appVersion, beginDownloadFor, pushToast, settingsRef],
  );

  const downloadNow = useCallback(async () => {
    if (!updateInfo || updateBusy.current) return;
    updateBusy.current = true;
    try {
      await beginDownloadFor(updateInfo, false);
    } finally {
      updateBusy.current = false;
    }
  }, [beginDownloadFor, updateInfo]);

  const installNow = useCallback(async () => {
    if (!updateInfo || updateBusy.current) return;
    updateBusy.current = true;
    setUpdatePhase("installing");
    setUpdateError(null);
    try {
      // Release the runtime directory before NSIS replaces the bundled
      // llama-server/DLLs. Otherwise Windows reports the files as in use.
      await prepareUpdate();
      await installUpdate(updateInfo.update);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setUpdatePhase("error");
      setUpdateError(message);
      pushToast("error", "更新安装失败", message);
    } finally {
      updateBusy.current = false;
    }
  }, [pushToast, updateInfo]);

  useEffect(() => {
    if (!isTauri || !autoCheckUpdates) return;
    const timer = window.setTimeout(() => {
      void checkUpdates(false);
    }, 4000);
    return () => window.clearTimeout(timer);
  }, [autoCheckUpdates, checkUpdates]);

  return {
    updateInfo,
    updatePhase,
    updateProgress,
    updateError,
    checkUpdates,
    downloadNow,
    installNow,
  };
}
