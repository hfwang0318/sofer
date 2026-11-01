import { useEffect, useRef, type RefObject } from "react";

import type { SettingsSection } from "../../lib/types";
import { SECTION_IDS } from "./shared";

interface UseSettingsScrollOptions {
  section: SettingsSection;
  sectionNav: number;
  onSectionChange: (section: SettingsSection) => void;
}

/**
 * Keeps the settings view and the sidebar section list in sync: explicit
 * sidebar clicks scroll the page, and manual scrolling highlights the section
 * currently under the header.
 */
export function useSettingsScroll({
  section,
  sectionNav,
  onSectionChange,
}: UseSettingsScrollOptions): RefObject<HTMLDivElement | null> {
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastNav = useRef<number | null>(null);
  const mounted = useRef(false);

  useEffect(() => {
    // The first section sits directly under the page title: do not scroll the
    // header out of view just because the user opened Settings.
    if (!mounted.current) {
      mounted.current = true;
      if (section === "general") {
        lastNav.current = sectionNav;
        return;
      }
    } else if (lastNav.current === sectionNav) {
      // Only scroll on an explicit sidebar click. The scroll observer also
      // updates `section`; without this guard the two would fight.
      return;
    }
    lastNav.current = sectionNav;
    const target = document.getElementById(`settings-${section}`);
    if (!target) return;
    onSectionChange(section);
    target.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [section, sectionNav, onSectionChange]);

  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const onScroll = () => {
      const top = root.getBoundingClientRect().top + 120;
      let active = SECTION_IDS[0];
      for (const id of SECTION_IDS) {
        const element = document.getElementById(`settings-${id}`);
        if (element && element.getBoundingClientRect().top <= top) active = id;
      }
      onSectionChange(active);
    };
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => root.removeEventListener("scroll", onScroll);
  }, [onSectionChange]);

  return scrollRef;
}
