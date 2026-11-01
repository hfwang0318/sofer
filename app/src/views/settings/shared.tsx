import type { ReactNode } from "react";

import type { Settings, SettingsSection } from "../../lib/types";

/** Order used by the scroll spy; keep in sync with the rendered sections. */
export const SECTION_IDS: SettingsSection[] = [
  "general",
  "recognition",
  "output",
  "advanced",
  "about",
];

export const PSM_OPTIONS = [
  { value: 3, label: "3 · 完全自动" },
  { value: 4, label: "4 · 单列" },
  { value: 6, label: "6 · 单块" },
  { value: 7, label: "7 · 单行" },
  { value: 11, label: "11 · 稀疏文本" },
  { value: 13, label: "13 · 原始行" },
];

export type UpdateSetting = <K extends keyof Settings>(
  key: K,
  value: Settings[K],
) => void;

export function Section({
  id,
  title,
  description,
  meta,
  children,
}: {
  id: SettingsSection;
  title: string;
  description?: string;
  meta?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="settings-section panel" id={`settings-${id}`}>
      <header className="section-head">
        <div>
          <h2>{title}</h2>
          {description && <p>{description}</p>}
        </div>
        {meta}
      </header>
      <div className="section-body">{children}</div>
    </section>
  );
}

export function Field({
  label,
  hint,
  error,
  action,
  full = false,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  action?: ReactNode;
  full?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`field ${full ? "field-full" : ""}`}>
      <div className="field-head">
        <span className="field-label">{label}</span>
        {action}
      </div>
      {children}
      {error ? (
        <span className="field-error">{error}</span>
      ) : hint ? (
        <span className="field-hint">{hint}</span>
      ) : null}
    </div>
  );
}

export function sameSettings(a: Settings, b: Settings): boolean {
  const keys = Object.keys(a) as Array<keyof Settings>;
  return (
    keys.length === Object.keys(b).length &&
    keys.every((key) => a[key] === b[key])
  );
}
