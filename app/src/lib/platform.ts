export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const isPreview = !isTauri;

export const isMac =
  typeof navigator !== "undefined" &&
  (/Mac|iPhone|iPad/.test(navigator.platform) ||
    navigator.userAgent.includes("Mac OS X"));

const KEY_LABELS: Record<string, string> = {
  CommandOrControl: isMac ? "⌘" : "Ctrl",
  Command: isMac ? "⌘" : "Ctrl",
  Cmd: isMac ? "⌘" : "Ctrl",
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
  Insert: "Ins",
  Home: "Home",
  End: "End",
  PageUp: "PgUp",
  PageDown: "PgDn",
  CapsLock: "Caps",
  NumLock: "Num",
  ScrollLock: "Scroll",
  PrintScreen: "PrtSc",
  Pause: "Pause",
  Minus: "−",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backquote: "`",
  NumpadAdd: "Num+",
  NumpadSubtract: "Num−",
  NumpadMultiply: "Num*",
  NumpadDivide: "Num/",
  NumpadDecimal: "Num.",
  NumpadEnter: "Num↵",
};

const MAIN_KEYS = new Set([
  "Space", "Enter", "Tab", "Backspace", "Delete", "Insert",
  "Home", "End", "PageUp", "PageDown", "CapsLock", "NumLock",
  "ScrollLock", "PrintScreen", "Pause",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
  "Minus", "Equal", "Comma", "Period", "Slash", "Backslash",
  "Semicolon", "Quote", "BracketLeft", "BracketRight", "Backquote",
  "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide",
  "NumpadDecimal", "NumpadEnter",
]);

function mainKeyFromCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code;
  if (/^Digit[0-9]$/.test(code)) return code;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  if (MAIN_KEYS.has(code)) return code;
  return null;
}

export function shortcutKeyLabel(part: string): string {
  if (/^Key[A-Z]$/.test(part)) return part.slice(3);
  if (/^Digit[0-9]$/.test(part)) return part.slice(5);
  if (/^Numpad[0-9]$/.test(part)) return `Num${part.slice(6)}`;
  return KEY_LABELS[part] ?? (part.length === 1 ? part.toUpperCase() : part);
}

/** Split a Tauri accelerator into display-friendly keycaps. */
export function shortcutKeys(accelerator: string): string[] {
  if (!accelerator.trim()) return ["—"];
  return accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map(shortcutKeyLabel);
}

export function shortcutText(accelerator: string): string {
  const keys = shortcutKeys(accelerator);
  return isMac ? keys.join("") : keys.join("+");
}

/**
 * Turn a keyboard event into a Tauri accelerator. At most two modifiers plus
 * one main key; a bare main key is valid. Tauri/global-hotkey requires exactly
 * one main key, so modifier-only and multi-main-key chords are not possible.
 */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
  if (event.key === "Escape") return null;
  const mods: string[] = [];
  if (isMac) {
    if (event.metaKey) mods.push("Command");
    if (event.ctrlKey) mods.push("Control");
  } else {
    if (event.ctrlKey) mods.push("Control");
    if (event.metaKey) mods.push("Super");
  }
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");

  if (mods.length > 2) return null;
  const main = mainKeyFromCode(event.code);
  if (!main) return null;
  return [...mods, main].join("+");
}

export function isModifierOnlyEvent(event: KeyboardEvent): boolean {
  return isPlatformModifier(event);
}

/**
 * A bare letter, digit, Space or arrow key would be swallowed in every app.
 * Only function keys can be registered without a modifier.
 */
export function needsModifier(accelerator: string): boolean {
  const parts = accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length !== 1) return false;
  return !/^F([1-9]|1[0-9]|2[0-4])$/.test(parts[0]);
}

export function modifierCount(event: KeyboardEvent): number {
  let count = 0;
  if (event.metaKey) count += 1;
  if (event.ctrlKey) count += 1;
  if (event.altKey) count += 1;
  if (event.shiftKey) count += 1;
  return count;
}

export function isPlatformModifier(event: KeyboardEvent): boolean {
  return event.key === "Meta" || event.key === "Control" || event.key === "Alt" || event.key === "Shift";
}
