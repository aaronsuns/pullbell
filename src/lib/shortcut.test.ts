import { describe, expect, it } from "vitest";
import { display, keyName, modifierGlyphs, toAccelerator, type KeyLike } from "./shortcut";

const ev = (code: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: "",
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("toAccelerator", () => {
  it("builds canonical accelerators in macOS modifier order", () => {
    expect(toAccelerator(ev("KeyG", { metaKey: true, ctrlKey: true }))).toBe("Ctrl+Cmd+G");
    expect(toAccelerator(ev("KeyG", { metaKey: true, ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+Cmd+G");
    expect(toAccelerator(ev("Digit5", { altKey: true }))).toBe("Alt+5");
    expect(toAccelerator(ev("Space", { metaKey: true }))).toBe("Cmd+SPACE");
    expect(toAccelerator(ev("F5"))).toBe("F5");
  });
  it("waits while only modifiers are held", () => {
    expect(toAccelerator(ev("MetaLeft", { metaKey: true }))).toBeNull();
    expect(toAccelerator(ev("ControlRight", { ctrlKey: true }))).toBeNull();
  });
  it("uses the physical key, not the produced character", () => {
    expect(toAccelerator({ ...ev("KeyG", { altKey: true }), key: "©" })).toBe("Alt+G");
  });
  it("ignores unknown keys", () => {
    expect(keyName("IntlBackslash")).toBeNull();
    expect(keyName("F25")).toBeNull();
  });
});

describe("display", () => {
  it("renders glyphs", () => {
    expect(display("Ctrl+Cmd+G")).toBe("⌃⌘G");
    expect(display("Ctrl+Shift+Cmd+G")).toBe("⌃⇧⌘G");
    expect(display("Alt+SPACE")).toBe("⌥Space");
    expect(display(null)).toBe("");
  });
  it("shows held modifiers", () => {
    expect(modifierGlyphs(ev("MetaLeft", { metaKey: true, ctrlKey: true }))).toBe("⌃⌘");
  });
});
