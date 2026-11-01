import { useCallback, type Dispatch, type SetStateAction } from "react";

import { engineStatus, saveSettings, startEngine } from "../lib/bridge";
import { SETUP_DISMISSED_KEY } from "../lib/defaults";
import { writeFlag } from "../lib/flags";
import type { EngineBusy, EngineStatus, Settings } from "../lib/types";
import type { PushToast } from "./useToast";

interface UseSaveSettingsOptions {
  settings: Settings;
  setSettings: Dispatch<SetStateAction<Settings>>;
  setShortcutActive: (active: boolean) => void;
  setSetupOpen: (open: boolean) => void;
  setSetupDismissed: (dismissed: boolean) => void;
  setStatus: Dispatch<SetStateAction<EngineStatus | null>>;
  setEngineBusy: (busy: EngineBusy) => void;
  refreshStatus: () => Promise<void>;
  pushToast: PushToast;
}

/**
 * Persist settings, then reconcile the managed engine: a quality change needs a
 * fresh download, and a server-affecting change restarts the newly selected
 * weight pack so the next screenshot is instant.
 */
export function useSaveSettings({
  settings,
  setSettings,
  setShortcutActive,
  setSetupOpen,
  setSetupDismissed,
  setStatus,
  setEngineBusy,
  refreshStatus,
  pushToast,
}: UseSaveSettingsOptions): (draft: Settings) => Promise<void> {
  return useCallback(
    async (draft: Settings) => {
      const qualityChanged = draft.modelQuality !== settings.modelQuality;
      const serverChanged =
        qualityChanged ||
        draft.engine !== settings.engine ||
        draft.jobs !== settings.jobs ||
        draft.maxTokens !== settings.maxTokens ||
        draft.kvCacheQuant !== settings.kvCacheQuant ||
        draft.idleSleepSecs !== settings.idleSleepSecs;
      const saved = await saveSettings(draft);
      setSettings(saved);
      setShortcutActive(true);
      if (qualityChanged) {
        setSetupDismissed(false);
        writeFlag(SETUP_DISMISSED_KEY, false);
      }
      const next = await engineStatus().catch(() => null);
      if (next) {
        setStatus(next);
        if (
          next.setup.needed &&
          saved.engine !== "tesseract" &&
          !saved.remoteEnabled
        ) {
          setSetupOpen(true);
        } else if (
          !saved.remoteEnabled &&
          serverChanged &&
          next.model.installed &&
          saved.engine !== "tesseract"
        ) {
          // Save already stopped the old managed server; bring the newly
          // selected weight pack up so the next screenshot is instant.
          setEngineBusy("start");
          try {
            const started = await startEngine();
            setStatus(started);
          } catch (cause) {
            pushToast(
              "error",
              "模型切换后启动失败",
              cause instanceof Error ? cause.message : String(cause),
            );
          } finally {
            setEngineBusy(null);
          }
        }
      } else {
        void refreshStatus();
      }
    },
    [
      pushToast,
      refreshStatus,
      setEngineBusy,
      setSetupDismissed,
      setSetupOpen,
      setSettings,
      setShortcutActive,
      setStatus,
      settings.engine,
      settings.idleSleepSecs,
      settings.jobs,
      settings.kvCacheQuant,
      settings.maxTokens,
      settings.modelQuality,
    ],
  );
}
