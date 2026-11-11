/** Turn key events into Tauri accelerator strings (`Ctrl+Alt+Shift+Cmd+G`) and back to glyphs. */

export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight",
  "MetaLeft", "MetaRight", "OSLeft", "OSRight", "CapsLock", "Fn",
]);

const NAMED: Record<string, string> = {
  Space: "SPACE", Tab: "TAB", Enter: "ENTER", Escape: "ESCAPE", Backspace: "BACKSPACE",
  Delete: "DELETE", ArrowUp: "UP", ArrowDown: "DOWN", ArrowLeft: "LEFT", ArrowRight: "RIGHT",
  Home: "HOME", End: "END", PageUp: "PAGEUP", PageDown: "PAGEDOWN", Comma: "COMMA",
  Period: "PERIOD", Slash: "SLASH", Semicolon: "SEMICOLON", Quote: "QUOTE",
  Backquote: "BACKQUOTE", Minus: "MINUS", Equal: "EQUAL", BracketLeft: "BRACKETLEFT",
  BracketRight: "BRACKETRIGHT", Backslash: "BACKSLASH",
};

/** The modifier glyphs currently held, e.g. "⌃⌘", for live feedback while recording. */
export function modifierGlyphs(e: KeyLike): string {
  return (e.ctrlKey ? "⌃" : "") + (e.altKey ? "⌥" : "") + (e.shiftKey ? "⇧" : "") + (e.metaKey ? "⌘" : "");
}

/** Key name from the physical key (layout independent, so ⌥G does not become "©"). */
export function keyName(code: string): string | null {
  if (MODIFIER_CODES.has(code)) return null;
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1];
  m = /^Digit([0-9])$/.exec(code);
  if (m) return m[1];
  m = /^F([0-9]{1,2})$/.exec(code);
  if (m && Number(m[1]) >= 1 && Number(m[1]) <= 20) return `F${m[1]}`;
  return NAMED[code] ?? null;
}

/** Accelerator for a complete combination, or null while only modifiers are held. */
export function toAccelerator(e: KeyLike): string | null {
  const key = keyName(e.code);
  if (!key) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Cmd");
  parts.push(key);
  return parts.join("+");
}

const GLYPH_KEYS: Record<string, string> = {
  SPACE: "Space", TAB: "⇥", ENTER: "↩", ESCAPE: "⎋", UP: "↑", DOWN: "↓", LEFT: "←", RIGHT: "→",
  BACKQUOTE: "`", COMMA: ",", PERIOD: ".", SLASH: "/",
};

/** `Ctrl+Shift+Cmd+G` → `⌃⇧⌘G`. Unknown input gives "". */
export function display(acc: string | null | undefined): string {
  if (!acc) return "";
  const parts = acc.split("+");
  const key = parts.pop() ?? "";
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const has = (...names: string[]) => names.some((n) => mods.has(n));
  return (
    (has("ctrl", "control") ? "⌃" : "") +
    (has("alt", "option") ? "⌥" : "") +
    (has("shift") ? "⇧" : "") +
    (has("cmd", "command", "super", "meta") ? "⌘" : "") +
    (GLYPH_KEYS[key.toUpperCase()] ?? key.toUpperCase())
  );
}
