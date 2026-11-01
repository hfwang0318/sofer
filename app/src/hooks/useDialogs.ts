import { useCallback, useState } from "react";

import { SETUP_DISMISSED_KEY } from "../lib/defaults";
import { readFlag } from "../lib/flags";

export interface Dialogs {
  setupOpen: boolean;
  setSetupOpen: (open: boolean) => void;
  setupDismissed: boolean;
  setSetupDismissed: (dismissed: boolean) => void;
  permissionOpen: boolean;
  setPermissionOpen: (open: boolean) => void;
  /** The two first-run dialogs are mutually exclusive. */
  closeDialogs: () => void;
}

/**
 * Visibility for the setup and screen-permission dialogs, plus the persisted
 * "setup was dismissed" flag. The two dialogs never stay open together.
 */
export function useDialogs(initialPermissionOpen: boolean): Dialogs {
  const [setupOpen, setSetupOpen] = useState(false);
  const [permissionOpen, setPermissionOpen] = useState(initialPermissionOpen);
  const [setupDismissed, setSetupDismissed] = useState(() =>
    readFlag(SETUP_DISMISSED_KEY),
  );

  const closeDialogs = useCallback(() => {
    setSetupOpen(false);
    setPermissionOpen(false);
  }, []);

  return {
    setupOpen,
    setSetupOpen,
    setupDismissed,
    setSetupDismissed,
    permissionOpen,
    setPermissionOpen,
    closeDialogs,
  };
}
