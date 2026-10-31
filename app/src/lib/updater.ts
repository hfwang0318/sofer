import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

import { isTauri } from "./platform";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "ready"
  | "installing"
  | "error";

export interface AvailableUpdate {
  version: string;
  body?: string;
  date?: string;
  update: Update;
}

export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (!isTauri) return null;
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    body: update.body,
    date: update.date,
    update,
  };
}

export async function downloadUpdate(
  update: Update,
  onProgress: (percent: number) => void,
): Promise<void> {
  let total = 0;
  let downloaded = 0;
  await update.download((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? 0;
      onProgress(0);
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      onProgress(total > 0 ? Math.min(99, (downloaded / total) * 100) : 0);
    } else if (event.event === "Finished") {
      onProgress(100);
    }
  });
  onProgress(100);
}

export async function installUpdate(update: Update): Promise<void> {
  await update.install();
  // Windows exits the app when the NSIS updater starts; macOS/Linux need this
  // explicit relaunch once the new version has been installed.
  await relaunch().catch(() => undefined);
}
