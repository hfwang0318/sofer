import { describe, expect, it, vi } from "vitest";
import {
  acceleratorFromEvent,
  isMac,
  isModifierOnlyEvent,
  modifierCount,
  needsModifier,
  shortcutKeyLabel,
  shortcutKeys,
  shortcutText,
} from "./platform";

function keyEvent(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", init);
}

async function loadPlatform() {
  vi.resetModules();
  return import("./platform");
}

describe("isTauri", () => {
  it("is false in a plain browser window", async () => {
    const platform = await loadPlatform();

    expect(platform.isTauri).toBe(false);
    expect(platform.isPreview).toBe(true);
  });

  it("is true once Tauri injects its internals object", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: { metadata: {} },
      configurable: true,
    });

    const platform = await loadPlatform();

    expect(platform.isTauri).toBe(true);
    expect(platform.isPreview).toBe(false);
  });
});

describe("shortcutKeyLabel", () => {
  it("strips the code prefix from physical keys", () => {
    expect(shortcutKeyLabel("KeyA")).toBe("A");
    expect(shortcutKeyLabel("Digit5")).toBe("5");
    expect(shortcutKeyLabel("Numpad7")).toBe("Num7");
  });

  it("keeps function keys as written", () => {
    expect(shortcutKeyLabel("F12")).toBe("F12");
    expect(shortcutKeyLabel("F24")).toBe("F24");
  });

  it("uses symbols for known keys and uppercases single characters", () => {
    expect(shortcutKeyLabel("ArrowLeft")).toBe("←");
    expect(shortcutKeyLabel("Space")).toBe("Space");
    expect(shortcutKeyLabel("q")).toBe("Q");
    expect(shortcutKeyLabel("UnknownKey")).toBe("UnknownKey");
  });
});

describe("shortcutKeys / shortcutText", () => {
  it("splits an accelerator into display keycaps", () => {
    const keys = shortcutKeys("Shift+KeyA");

    expect(keys).toStrictEqual([isMac ? "⇧" : "Shift", "A"]);
  });

  it("drops empty parts and pads an empty accelerator", () => {
    expect(shortcutKeys("  ")).toStrictEqual(["—"]);
    expect(shortcutKeys("Shift+ +KeyA")).toStrictEqual([isMac ? "⇧" : "Shift", "A"]);
  });

  it("joins keycaps with the platform's native style", () => {
    expect(shortcutText("Shift+KeyA")).toBe(isMac ? "⇧A" : "Shift+A");
  });
});

describe("needsModifier", () => {
  it("allows bare function keys", () => {
    expect(needsModifier("F1")).toBe(false);
    expect(needsModifier("F24")).toBe(false);
  });

  it("requires a modifier for bare letters, digits and F25+", () => {
    expect(needsModifier("KeyA")).toBe(true);
    expect(needsModifier("2")).toBe(true);
    expect(needsModifier("F25")).toBe(true);
  });

  it("does not add requirements to a full chord", () => {
    expect(needsModifier("CommandOrControl+Shift+2")).toBe(false);
  });
});

describe("acceleratorFromEvent", () => {
  it("accepts a bare function key", () => {
    expect(acceleratorFromEvent(keyEvent({ key: "F2", code: "F2" }))).toBe("F2");
  });

  it("uses Control for ctrl+letter on every platform", () => {
    expect(
      acceleratorFromEvent(
        keyEvent({ key: "a", code: "KeyA", ctrlKey: true }),
      ),
    ).toBe("Control+KeyA");
  });

  it("rejects Escape, modifier-only presses and non-main keys", () => {
    expect(acceleratorFromEvent(keyEvent({ key: "Escape", code: "Escape" }))).toBeNull();
    expect(
      acceleratorFromEvent(keyEvent({ key: "Shift", code: "ShiftLeft", shiftKey: true })),
    ).toBeNull();
  });

  it("accepts a bare main key", () => {
    expect(acceleratorFromEvent(keyEvent({ key: "a", code: "KeyA" }))).toBe("KeyA");
  });

  it("rejects more than two modifiers", () => {
    expect(
      acceleratorFromEvent(
        keyEvent({
          key: "a",
          code: "KeyA",
          ctrlKey: true,
          altKey: true,
          shiftKey: true,
        }),
      ),
    ).toBeNull();
  });
});

describe("modifier helpers", () => {
  it("detects modifier-only events", () => {
    expect(isModifierOnlyEvent(keyEvent({ key: "Control", code: "ControlLeft" }))).toBe(true);
    expect(isModifierOnlyEvent(keyEvent({ key: "a", code: "KeyA" }))).toBe(false);
  });

  it("counts every held modifier", () => {
    expect(
      modifierCount(keyEvent({ key: "a", code: "KeyA", ctrlKey: true, shiftKey: true })),
    ).toBe(2);
    expect(modifierCount(keyEvent({ key: "a", code: "KeyA" }))).toBe(0);
  });
});
