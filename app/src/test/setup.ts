import { afterEach, beforeEach } from "vitest";

/**
 * The pure lib modules only touch two pieces of browser state: localStorage
 * (history/settings) and the Tauri internals marker. Reset both so tests never
 * leak into each other.
 */
beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
