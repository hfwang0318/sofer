import { useCallback, useEffect, useState } from "react";

import type { SettingsSection, View } from "../lib/types";
import type { Dialogs } from "./useDialogs";

type DialogSetters = Pick<Dialogs, "setSetupOpen" | "setPermissionOpen">;

export interface Navigation {
  view: View;
  setView: (view: View) => void;
  settingsSection: SettingsSection;
  settingsNav: number;
  openSettings: (section?: SettingsSection) => void;
  navigate: (view: View) => void;
  handleSettingsSection: (section: SettingsSection) => void;
  handleSettingsActive: (section: SettingsSection) => void;
}

interface UseNavigationOptions {
  dialogs: DialogSetters;
  initialView: () => View;
}

/**
 * Main-view and settings-section navigation. Opening settings always closes
 * the first-run dialogs, and every explicit section jump bumps `settingsNav`
 * so the settings page scrolls even when the section did not change.
 */
export function useNavigation({
  dialogs,
  initialView,
}: UseNavigationOptions): Navigation {
  const { setSetupOpen, setPermissionOpen } = dialogs;
  const [view, setView] = useState<View>(initialView);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("general");
  const [settingsNav, setSettingsNav] = useState(0);

  const openSettings = useCallback(
    (section: SettingsSection = "general") => {
      setSettingsSection(section);
      setSettingsNav((value) => value + 1);
      setView("settings");
      setSetupOpen(false);
      setPermissionOpen(false);
    },
    [setPermissionOpen, setSetupOpen],
  );

  const navigate = useCallback(
    (next: View) => {
      setView(next);
      setSetupOpen(false);
      setPermissionOpen(false);
    },
    [setPermissionOpen, setSetupOpen],
  );

  const handleSettingsSection = useCallback((section: SettingsSection) => {
    setSettingsSection(section);
    setSettingsNav((value) => value + 1);
    setView("settings");
  }, []);

  const handleSettingsActive = useCallback((section: SettingsSection) => {
    setSettingsSection(section);
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const typing = Boolean(
        target?.closest("input, textarea, select, [contenteditable='true']"),
      );
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        openSettings("general");
        return;
      }
      if (typing) return;
      if ((event.metaKey || event.ctrlKey) && (event.key === "1" || event.key === "2")) {
        event.preventDefault();
        setView(event.key === "1" ? "capture" : "history");
        setSetupOpen(false);
        setPermissionOpen(false);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openSettings, setPermissionOpen, setSetupOpen]);

  return {
    view,
    setView,
    settingsSection,
    settingsNav,
    openSettings,
    navigate,
    handleSettingsSection,
    handleSettingsActive,
  };
}
