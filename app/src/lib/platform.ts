export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const isPreview = !isTauri;

export const isMac =
  typeof navigator !== "undefined" &&
  (/Mac|iPhone|iPad/.test(navigator.platform) ||
    navigator.userAgent.includes("Mac OS X"));

const KEY_LABELS: Record<string, string> = {
  CommandOrControl: isMac ? "⌘" : "Ctrl",
  Command: "⌘",
  Cmd: "⌘",
  Control: isMac ? "⌃" : "Ctrl",
  Ctrl: isMac ? "⌃" : "Ctrl",
  Alt: isMac ? "⌥" : "Alt",
  Option: isMac ? "⌥" : "Alt",
  Shift: isMac ? "⇧" : "Shift",
  Super: isMac ? "⌘" : "Win",
  Meta: isMac ? "⌘" : "Win",
  Space: "Space",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "⌫",
  Delete: "Del",
};

/** Split a Tauri accelerator into display-friendly keycaps. */
export function shortcutKeys(accelerator: string): string[] {
  if (!accelerator.trim()) return ["—"];
  return accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => KEY_LABELS[part] ?? (part.length === 1 ? part.toUpperCase() : part));
}

export function shortcutText(accelerator: string): string {
  const keys = shortcutKeys(accelerator);
  return isMac ? keys.join("") : keys.join("+");
}

/**
 * Turn a keyboard event into a Tauri accelerator. CommandOrControl follows the
 * platform, which is what people expect from a cross-platform shortcut.
 * Returns null when the key cannot be represented (or has no modifier).
 */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (event.metaKey || event.ctrlKey) mods.push("CommandOrControl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");

  const key = event.key;
  let main: string | null = null;
  if (/^[a-zA-Z0-9]$/.test(key)) main = key.toUpperCase();
  else if (/^F([1-9]|1[0-2])$/.test(key)) main = key.toUpperCase();
  else if (key === " " || key === "Spacebar") main = "Space";
  else if (key === "ArrowUp") main = "ArrowUp";
  else if (key === "ArrowDown") main = "ArrowDown";
  else if (key === "ArrowLeft") main = "ArrowLeft";
  else if (key === "ArrowRight") main = "ArrowRight";
  else if (key === "Enter") main = "Enter";
  else if (key === "Tab") main = "Tab";

  if (!main || mods.length === 0) return null;
  return [...mods, main].join("+");
}

export function isPlatformModifier(event: KeyboardEvent): boolean {
  return event.key === "Meta" || event.key === "Control" || event.key === "Alt" || event.key === "Shift";
}
