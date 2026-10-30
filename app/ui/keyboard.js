"use strict";
// Keyboard pages: Keys (remap and bind), Lighting (presets, effects, per-key paint), Device.
// Two boards: the NK65 (VIA firmware: keymap, macros and lighting live on the board)
// and the PRO X TKL LIGHTSPEED (kit drives its lighting; kit's input hook handles its keys).

let K = null; // backend state: { config, status, configPath }
const kv = { layer: 0, key: null, pick: "keys", listening: false, brush: "#ffffff", erase: false, painting: false, macroMode: "board", fxGroup: null, colorSlot: "color1" };

const kcfg = () => K.config;
const kst = () => K.status;
const isNk = () => (A.keyboard?.model || "nk65") === "nk65";
const lightPreset = () => kcfg().presets.find((p) => p.id === kcfg().active) || kcfg().presets[0];
const KIT_KEYS = 12;
const F13 = 0x68;

// Status (keymap, battery) arrives as "keyboard" events from the worker; a save
// reply may predate a key write still in its queue, so it isn't taken from here.
const keyboardSaver = makeSaver("keyboard_save", kcfg, () => {});

// ---------- key names (HID usages = QMK basic keycodes) ----------

const USAGE = {};
"ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").forEach((c, i) => (USAGE[4 + i] = c));
"1234567890".split("").forEach((c, i) => (USAGE[0x1e + i] = c));
for (let i = 1; i <= 12; i++) USAGE[0x39 + i] = `F${i}`;
for (let i = 13; i <= 24; i++) USAGE[F13 + i - 13] = `F${i}`;
for (let i = 1; i <= 9; i++) USAGE[0x58 + i] = `Num ${i}`;
Object.assign(USAGE, {
  0x28: "Enter", 0x29: "Esc", 0x2a: "Bksp", 0x2b: "Tab", 0x2c: "Space", 0x2d: "-", 0x2e: "=", 0x2f: "[", 0x30: "]", 0x31: "\\",
  0x32: "ISO #", 0x33: ";", 0x34: "'", 0x35: "`", 0x36: ",", 0x37: ".", 0x38: "/", 0x39: "Caps", 0x46: "PrtSc", 0x47: "ScrLk",
  0x48: "Pause", 0x49: "Ins", 0x4a: "Home", 0x4b: "PgUp", 0x4c: "Del", 0x4d: "End", 0x4e: "PgDn", 0x4f: "→", 0x50: "←", 0x51: "↓",
  0x52: "↑", 0x53: "NumLk", 0x54: "Num /", 0x55: "Num *", 0x56: "Num -", 0x57: "Num +", 0x58: "Num Ent", 0x62: "Num 0", 0x63: "Num .",
  0x64: "ISO \\", 0x65: "Menu", 0xe0: "Ctrl", 0xe1: "Shift", 0xe2: "Alt", 0xe3: "Win", 0xe4: "R Ctrl", 0xe5: "R Shift", 0xe6: "R Alt",
  0xe7: "R Win", 0xa5: "Power", 0xa6: "Sleep", 0xa7: "Wake", 0xa8: "Mute", 0xa9: "Vol +", 0xaa: "Vol −", 0xab: "Next", 0xac: "Prev",
  0xad: "Stop", 0xae: "Play", 0xaf: "Media", 0xb0: "Eject", 0xb1: "Mail", 0xb2: "Calc", 0xb3: "My PC", 0xbd: "Bright +", 0xbe: "Bright −",
});

// Browser KeyboardEvent.code -> HID usage, for "press a key" on the NK65.
const CODE_USAGE = {
  Enter: 0x28, Escape: 0x29, Backspace: 0x2a, Tab: 0x2b, Space: 0x2c, Minus: 0x2d, Equal: 0x2e, BracketLeft: 0x2f, BracketRight: 0x30,
  Backslash: 0x31, Semicolon: 0x33, Quote: 0x34, Backquote: 0x35, Comma: 0x36, Period: 0x37, Slash: 0x38, CapsLock: 0x39,
  PrintScreen: 0x46, ScrollLock: 0x47, Pause: 0x48, Insert: 0x49, Home: 0x4a, PageUp: 0x4b, Delete: 0x4c, End: 0x4d, PageDown: 0x4e,
  ArrowRight: 0x4f, ArrowLeft: 0x50, ArrowDown: 0x51, ArrowUp: 0x52, NumLock: 0x53, NumpadDivide: 0x54, NumpadMultiply: 0x55,
  NumpadSubtract: 0x56, NumpadAdd: 0x57, NumpadEnter: 0x58, Numpad0: 0x62, NumpadDecimal: 0x63, IntlBackslash: 0x64, ContextMenu: 0x65,
  ControlLeft: 0xe0, ShiftLeft: 0xe1, AltLeft: 0xe2, MetaLeft: 0xe3, ControlRight: 0xe4, ShiftRight: 0xe5, AltRight: 0xe6, MetaRight: 0xe7,
};
for (let i = 0; i < 26; i++) CODE_USAGE[`Key${String.fromCharCode(65 + i)}`] = 4 + i;
for (let i = 1; i <= 9; i++) CODE_USAGE[`Digit${i}`] = 0x1d + i;
CODE_USAGE.Digit0 = 0x27;
for (let i = 1; i <= 12; i++) CODE_USAGE[`F${i}`] = 0x39 + i;
for (let i = 13; i <= 24; i++) CODE_USAGE[`F${i}`] = F13 + i - 13;
for (let i = 1; i <= 9; i++) CODE_USAGE[`Numpad${i}`] = 0x58 + i;

// HID usage -> [set-1 scan code, extended], to turn onboard macros into kit macros.
const USAGE_SCAN = {
  0x28: [0x1c], 0x29: [0x01], 0x2a: [0x0e], 0x2b: [0x0f], 0x2c: [0x39], 0x2d: [0x0c], 0x2e: [0x0d], 0x2f: [0x1a], 0x30: [0x1b],
  0x31: [0x2b], 0x33: [0x27], 0x34: [0x28], 0x35: [0x29], 0x36: [0x33], 0x37: [0x34], 0x38: [0x35], 0x39: [0x3a],
  0x44: [0x57], 0x45: [0x58], 0x46: [0x37, 1], 0x47: [0x46], 0x48: [0x45], 0x49: [0x52, 1], 0x4a: [0x47, 1], 0x4b: [0x49, 1],
  0x4c: [0x53, 1], 0x4d: [0x4f, 1], 0x4e: [0x51, 1], 0x4f: [0x4d, 1], 0x50: [0x4b, 1], 0x51: [0x50, 1], 0x52: [0x48, 1],
  0x65: [0x5d, 1], 0xe0: [0x1d], 0xe1: [0x2a], 0xe2: [0x38], 0xe3: [0x5b, 1], 0xe4: [0x1d, 1], 0xe5: [0x36], 0xe6: [0x38, 1], 0xe7: [0x5c, 1],
};
[0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c]
  .forEach((scan, i) => (USAGE_SCAN[4 + i] = [scan]));
for (let i = 0; i < 10; i++) USAGE_SCAN[0x1e + i] = [0x02 + i];
for (let i = 0; i < 10; i++) USAGE_SCAN[0x3a + i] = [0x3b + i];

// Typed text in an onboard macro -> (usage, shift).
const SHIFTED = { "!": 0x1e, "@": 0x1f, "#": 0x20, $: 0x21, "%": 0x22, "^": 0x23, "&": 0x24, "*": 0x25, "(": 0x26, ")": 0x27, _: 0x2d, "+": 0x2e, "{": 0x2f, "}": 0x30, "|": 0x31, ":": 0x33, '"': 0x34, "~": 0x35, "<": 0x36, ">": 0x37, "?": 0x38 };
const PLAIN = { " ": 0x2c, "\n": 0x28, "\t": 0x2b, "-": 0x2d, "=": 0x2e, "[": 0x2f, "]": 0x30, "\\": 0x31, ";": 0x33, "'": 0x34, "`": 0x35, ",": 0x36, ".": 0x37, "/": 0x38 };

// ---------- QMK keycodes beyond the basics ----------
// VIA protocol 12 (QMK 0.19+) renumbered layers, macros and lighting keys.

const v12 = () => (kst().protocol || 0) >= 12;
// [name, picker heading, key sub-label, v12 base, older base]
const LAYER_FNS = [
  ["MO", "Hold for layer", "hold", 0x5220, 0x5100],
  ["TG", "Toggle layer", "toggle", 0x5260, 0x5300],
  ["TO", "Switch to layer", "switch", 0x5200, 0x5010],
  ["DF", "Default layer", "default", 0x5240, 0x5200],
  ["OSL", "Next key on layer", "one shot", 0x5280, 0x5400],
];
const layerCode = (fn, n) => {
  const f = LAYER_FNS.find((x) => x[0] === fn);
  return (v12() ? f[3] : f[4]) + n;
};
const macroCode = (n) => (v12() ? 0x7700 : 0x5f12) + n;
const customCode = (n) => (v12() ? 0x7e00 : 0x5f80) + n;
const RGB_KEYS = ["Lights on/off", "Effect +", "Effect −", "Hue +", "Hue −", "Saturation +", "Saturation −", "Brightness +", "Brightness −", "Speed +", "Speed −"];
// Current QMK gives RGB matrix its own keys (RM_*, 0x7840); 0x7820 is underglow.
const RM_KEYS = ["Lights on", "Lights off", "Lights on/off", "Effect +", "Effect −", "Hue +", "Hue −", "Saturation +", "Saturation −", "Brightness +", "Brightness −", "Speed +", "Speed −"];
const rmKeys = () => v12() && kst().lighting === "rgbMatrix";
const rgbKeys = () => (rmKeys() ? RM_KEYS : RGB_KEYS);
const rgbCode = (i) => (rmKeys() ? 0x7840 : v12() ? 0x7820 : 0x5cc2) + i;
const WILBA_KEYS = ["Brightness +", "Brightness −", "Effect +", "Effect −", "Speed +", "Speed −", "Color 1 hue +", "Color 1 hue −", "Color 2 hue +", "Color 2 hue −", "Color 1 sat +", "Color 1 sat −", "Color 2 sat +", "Color 2 sat −"];
const MEDIA = [0xa8, 0xaa, 0xa9, 0xae, 0xad, 0xac, 0xab, 0xbd, 0xbe, 0xb2, 0xb1, 0xb3, 0xa6];

function modNames(m) {
  const right = m & 0x10 ? "R " : "";
  return [[1, "Ctrl"], [2, "Shift"], [4, "Alt"], [8, "Win"]].filter(([b]) => m & b).map(([, n]) => right + n).join("+");
}

// { text, sub, cls } describing a keycode.
function kcInfo(code) {
  const hex = `0x${code.toString(16).padStart(4, "0")}`;
  if (code === 0) return { text: "", sub: "Nothing", cls: "none" };
  if (code === 1) return { text: "▽", sub: "Falls through", cls: "trns" };
  if (code <= 0xff) {
    const i = code - F13;
    if (isNk() && i >= 0 && i < KIT_KEYS && kcfg().kitKeys[i]) {
      const m = mcfg().macros.find((x) => x.id === kcfg().kitKeys[i]);
      return { text: m ? m.name : "Macro", sub: "kit macro", cls: "kitm" };
    }
    return { text: USAGE[code] || hex, sub: "" };
  }
  if (code < 0x2000) return { text: USAGE[code & 0xff] || hex, sub: modNames(code >> 8), cls: "mod" };
  if (code >= 0x4000 && code < 0x5000) return { text: USAGE[code & 0xff] || hex, sub: `hold: layer ${(code >> 8) & 0xf}`, cls: "layer" };
  for (const [fn, , sub, a, b] of LAYER_FNS) {
    const base = v12() ? a : b;
    const span = v12() ? 0x20 : fn === "TO" ? 0x10 : 0x20;
    if (code >= base && code < base + span) return { text: `${fn} ${code - base}`, sub, cls: "layer" };
  }
  const mBase = macroCode(0);
  if (code >= mBase && code < mBase + 32) {
    const slot = code - mBase;
    const owner = kcfg().onboard[slot];
    const m = owner && mcfg().macros.find((x) => x.id === owner);
    return { text: m ? m.name : `M${slot}`, sub: "on keyboard", cls: "kitm" };
  }
  const cBase = customCode(0);
  if (code >= cBase && code < cBase + WILBA_KEYS.length && kst().lighting === "wilba") return { text: WILBA_KEYS[code - cBase], sub: "lighting", cls: "fn" };
  const rBase = rgbCode(0);
  if (code >= rBase && code < rBase + rgbKeys().length) return { text: rgbKeys()[code - rBase], sub: "lighting", cls: "fn" };
  if (code === (v12() ? 0x7c00 : 0x5c00)) return { text: "Boot", sub: "flash mode", cls: "fn" };
  return { text: hex, sub: "", cls: "fn" };
}

// ---------- layouts ----------

// The NK65 from its VIA definition (matrix row,col per key).
const NK65_KLE = [
  ["0,0", "0,1", "0,2", "0,3", "0,4", "0,5", "0,6", "0,7", "0,8", "0,9", "0,10", "0,11", "0,12", { w: 2 }, "0,13", "0,14"],
  [{ w: 1.5 }, "1,0", "1,1", "1,2", "1,3", "1,4", "1,5", "1,6", "1,7", "1,8", "1,9", "1,10", "1,11", "1,12", { w: 1.5 }, "2,12", "1,14"],
  [{ w: 1.75 }, "2,0", "2,1", "2,2", "2,3", "2,4", "2,5", "2,6", "2,7", "2,8", "2,9", "2,10", "2,11", { w: 2.25 }, "2,13", "2,14"],
  [{ w: 2.25 }, "3,0", "3,2", "3,3", "3,4", "3,5", "3,6", "3,7", "3,8", "3,9", "3,10", "3,11", { w: 1.75 }, "3,12", "3,13", "3,14"],
  [{ w: 1.25 }, "4,0", { w: 1.25 }, "4,1", { w: 1.25 }, "4,2", { w: 6.25 }, "4,6", "4,9", "4,10", "4,11", "4,12", "4,13", "4,14"],
];

// The NK65's factory keymap (QMK's novelkeys/nk65 default), in drawing order. "mo1" is MO(1).
const NK65_BASE = [
  [0x29, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e, 0x2a, 0x4a],
  [0x2b, 0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30, 0x31, 0x4b],
  [0x39, 0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, 0x28, 0x4e],
  [0xe1, 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0xe5, 0x52, 0x4d],
  [0xe0, 0xe3, 0xe2, 0x2c, 0xe6, "mo1", 0xe4, 0x50, 0x51, 0x4f],
].flat();
// Layer 1's defaults (` F1-F12 Del, volume); every other upper-layer key falls through.
const NK65_FN = { "0,0": 0x35, "0,13": 0x4c, "4,0": 0xa9, "4,1": 0xaa, "4,2": 0xa8 };
for (let i = 1; i <= 12; i++) NK65_FN[`0,${i}`] = 0x39 + i;

function defaultKeycode(layer, k) {
  if (layer === 0) {
    const code = NK65_BASE[LAYOUT.nk65.keys.indexOf(k)];
    return code === "mo1" ? layerCode("MO", 1) : code;
  }
  return (layer === 1 && NK65_FN[k.id]) || 1;
}

// The PRO X TKL by HID usage ({ g } is a gap in key units). The Fn key can't be bound.
const TKL_ROWS = [
  [0, [0x29, { g: 1 }, 0x3a, 0x3b, 0x3c, 0x3d, { g: 0.5 }, 0x3e, 0x3f, 0x40, 0x41, { g: 0.5 }, 0x42, 0x43, 0x44, 0x45, { g: 0.25 }, 0x46, 0x47, 0x48]],
  [1.25, [0x35, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e, [0x2a, 2], { g: 0.25 }, 0x49, 0x4a, 0x4b]],
  [2.25, [[0x2b, 1.5], 0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30, [0x31, 1.5], { g: 0.25 }, 0x4c, 0x4d, 0x4e]],
  [3.25, [[0x39, 1.75], 0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, [0x28, 2.25]]],
  [4.25, [[0xe1, 2.25], 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, [0xe5, 2.75], { g: 1.25 }, 0x52]],
  [5.25, [[0xe0, 1.25], [0xe3, 1.25], [0xe2, 1.25], [0x2c, 6.25], [0xe6, 1.25], ["fn", 1.25], [0x65, 1.25], [0xe4, 1.25], { g: 0.25 }, 0x50, 0x51, 0x4f]],
];

function nk65Keys() {
  const keys = [];
  NK65_KLE.forEach((row, y) => {
    let x = 0;
    let w = 1;
    for (const item of row) {
      if (typeof item === "object") {
        w = item.w || 1;
        continue;
      }
      const [r, c] = item.split(",").map(Number);
      keys.push({ id: item, r, c, x, y, w });
      x += w;
      w = 1;
    }
  });
  return keys;
}

function tklKeys() {
  const keys = [];
  for (const [y, row] of TKL_ROWS) {
    let x = 0;
    for (const item of row) {
      if (item && typeof item === "object" && !Array.isArray(item)) {
        x += item.g;
        continue;
      }
      const [usage, w] = Array.isArray(item) ? item : [item, 1];
      keys.push({ id: usage === "fn" ? "fn" : `u${usage}`, usage: usage === "fn" ? null : usage, x, y, w });
      x += w;
    }
  }
  return keys;
}

const LAYOUT = {
  nk65: { keys: nk65Keys(), w: 16, h: 5 },
  proxtkl: { keys: tklKeys(), w: 18.25, h: 6.25 },
};
const layout = () => LAYOUT[isNk() ? "nk65" : "proxtkl"];
// Per-key lighting zone of a HID usage on the PRO X TKL.
const zoneOf = (usage) => (usage >= 0xe0 ? usage - 120 : usage - 3);

function keycodeAt(layer, r, c) {
  const s = kst();
  return s.keymap[(layer * s.rows + r) * s.cols + c] ?? 0;
}

// ---------- the keyboard drawing ----------
// Keys are ruled cells like the rating plates; an LED color shows as a tint
// with a bar along the key's lower edge.

function boardHtml(keyHtml, cls = "", hint = "") {
  const L = layout();
  return `<div class="board-wrap"><div class="board ${cls}" style="aspect-ratio:${L.w} / ${L.h};--ratio:${(L.w / L.h).toFixed(4)}">${L.keys
    .map((k) => {
      const box = `left:calc(${(k.x / L.w) * 100}% + 2px);top:calc(${(k.y / L.h) * 100}% + 2px);width:calc(${(k.w / L.w) * 100}% - 4px);height:calc(${(1 / L.h) * 100}% - 4px)`;
      return keyHtml(k, box);
    })
    .join("")}</div>${hint ? `<p class="hint board-hint">${hint}</p>` : ""}</div>`;
}

function keyStatus() {
  const s = kst();
  if (s.connected) return { text: "Connected", live: true };
  if (s.present) return isNk() ? { text: "Not answering", hint: "Close VIA (and any other keyboard app), then replug the NK65." } : { text: "Asleep", hint: "Press a key to wake the keyboard." };
  if (!s.error) return { text: "Searching" };
  return isNk() ? { text: "Not found", hint: "Plug in the NK65.", bad: true } : { text: "Not found", hint: "Plug in the LIGHTSPEED receiver, and keep G HUB closed.", bad: true };
}

function keyStatusCell(key) {
  const s = keyStatus();
  return cell(key, "Status", statusMark(s), { cls: "text" });
}

function lightingCell(key, span = 2) {
  const p = lightPreset();
  const caption = kst().connected ? "Lighting · on keyboard" : "Lighting";
  return cell(key, caption, esc(p.name), { span, cls: "text" });
}

// Cells for the home screen.
function keyboardPlateCells() {
  const s = kst();
  const p = lightPreset();
  const cells = [keyStatusCell("k-status")];
  cells.push(isNk() ? cell("k-layers", "Layers", s.layers ? s.layers : "—", { cls: s.layers ? "" : "dim" }) : battery("k-bat", s.connected ? s.battery : null, s.charging));
  cells.push(cell("k-fx", "Effect", esc(fxOf(p).name), { cls: "text" }));
  cells.push(cell("k-bright", "Brightness", `${p.brightness}<small>%</small>`));
  cells.push(lightingCell("k-light"));
  if (isNk()) {
    const kit = kcfg().kitKeys.filter(Boolean).length + Object.keys(kcfg().onboard).length;
    cells.push(cell("k-macros", "Macros on keys", kit ? `${kit} ${kit > 1 ? "keys" : "key"}` : "None", { span: 2, cls: `text ${kit ? "" : "dim"}` }));
  } else {
    const n = kcfg().bindings.length;
    cells.push(cell("k-bind", "Keys handled by kit", n ? `${n} ${n > 1 ? "keys" : "key"}` : "None", { span: 2, cls: `text ${n ? "" : "dim"}` }));
  }
  return cells;
}

// Labels for the macro editor's "Bound to" cell.
function keyboardMacroUses(id) {
  if (!A.keyboard) return [];
  const out = [];
  if (isNk()) {
    const s = kst();
    kcfg().kitKeys.forEach((m, i) => {
      if (m === id) out.push(...keysWithCode(F13 + i).map((k) => `${keyboardName()} · ${k}`));
    });
    for (const [slot, m] of Object.entries(kcfg().onboard)) {
      if (m === id && s.connected) out.push(...keysWithCode(macroCode(Number(slot))).map((k) => `${keyboardName()} · ${k} (on keyboard)`));
    }
  } else {
    for (const b of kcfg().bindings) if (b.action.type === "macro" && b.action.id === id) out.push(`${keyboardName()} · ${USAGE[b.usage] || "key"}`);
  }
  return out;
}

// "A (layer 1)" for every key holding `code`.
function keysWithCode(code) {
  const s = kst();
  const out = [];
  if (!s.keymap.length) return out;
  for (let l = 0; l < s.layers; l++) {
    for (const k of LAYOUT.nk65.keys) if (keycodeAt(l, k.r, k.c) === code) out.push(`${USAGE[keycodeAt(0, k.r, k.c)] || k.id}${l ? ` (layer ${l})` : ""}`);
  }
  return out;
}

// ---------- keys page ----------

function keysPage() {
  return isNk() ? nkKeysPage() : tklKeysPage();
}

function layerStrip() {
  const n = kst().layers || 4;
  const tabs = Array.from({ length: n }, (_, i) => `<button class="stab ${kv.layer === i ? "sel" : ""}" data-act="layer" data-v="${i}">${i ? `Layer ${i}` : "Base layer"}</button>`).join("");
  return `<nav class="strip" aria-label="Layers">${tabs}</nav>`;
}

function nkKeysPage() {
  const s = kst();
  const sel = LAYOUT.nk65.keys.find((k) => k.id === kv.key);
  const ready = s.connected && s.keymap.length;
  const board = ready
    ? boardHtml((k, box) => {
        const info = kcInfo(keycodeAt(kv.layer, k.r, k.c));
        const cls = ["kkey", info.cls || "", k.id === kv.key ? "sel" : ""].join(" ");
        return `<button class="${cls}" style="${box}" data-act="key-pick" data-k="${k.id}" title="${esc(info.text)}${info.sub ? ` · ${esc(info.sub)}` : ""}"><span class="kl">${esc(info.text)}</span>${info.sub ? `<span class="ks">${esc(info.sub)}</span>` : ""}</button>`;
      }, "", kv.layer ? "Keys showing ▽ fall through to the layer below. Hold a layer key (MO) on the base layer to reach this one." : "Click a key, then pick what it does. Changes are written to the keyboard right away and work on any computer.")
    : `<div class="empty"><b>${s.present ? "The NK65 isn't answering" : "Plug in the NK65"}</b>${keyStatus().hint || "kit reads the keymap straight from the keyboard."}</div>`;
  const kitUsed = kcfg().kitKeys.filter(Boolean).length;
  const resetting = confirmDelete === `layer-${kv.layer}`;
  return `<div class="page devpage" data-key="kb-keys">
    ${layerStrip()}
    <div class="work">
      <section class="canvas">
        <div class="canvas-head"><h1 class="h1 inline-h">${kv.layer ? `Layer ${kv.layer}` : "Base layer"}</h1>
          <div class="pactions">${s.connected ? `<span class="live-mark">Stored on keyboard</span>` : ""}${ready ? `<button class="tbtn ${resetting ? "confirm" : ""}" data-act="layer-reset">${icons.reset}${resetting ? "Confirm reset" : "Reset layer"}</button>` : ""}</div></div>
        ${plate([
          keyStatusCell("status"),
          cell("fw", "Firmware", s.kitFirmware ? `kit <small>v${s.kitFirmware}</small>` : s.protocol ? `VIA <small>v${s.protocol}</small>` : "—", { cls: s.protocol ? "" : "dim" }),
          cell("kit", "kit-run macros", `${kitUsed}<small>of ${KIT_KEYS}</small>`),
          cell("board", "Onboard macros", s.macros.length ? `${s.macros.filter((m) => m.length).length}<small>of ${s.macros.length}</small>` : "—", { cls: s.macros.length ? "" : "dim" }),
        ])}
        ${board}
      </section>
      <aside class="panel">${sel && ready ? nkKeyPanel(sel) : `<section class="sec"><div class="sec-title">No key selected</div><p class="hint">Click a key on the drawing.</p></section>`}</aside>
    </div>
  </div>`;
}

const PICKS = [["keys", "Key"], ["media", "Media"], ["layers", "Layers"], ["macro", "Macro"], ["light", "Lighting"]];

function nkKeyPanel(k) {
  const code = keycodeAt(kv.layer, k.r, k.c);
  const info = kcInfo(code);
  const pick = kv.pick;
  let body = "";
  const opt = (c, label, sub = "") => `<button class="kopt ${c === code ? "on" : ""}" data-act="kc-set" data-code="${c}">${esc(label)}${sub ? `<small>${esc(sub)}</small>` : ""}</button>`;
  if (pick === "keys") {
    const group = (title, codes, cls = "dense") => `<div class="cap kgroup">${title}</div><div class="kopts ${cls}">${codes.map((c) => opt(c, USAGE[c])).join("")}</div>`;
    const range = (a, n) => Array.from({ length: n }, (_, i) => a + i);
    body = `<button class="capture ${kv.listening ? "listening" : ""}" data-act="kc-listen">${kv.listening ? "Press the key (with Ctrl, Shift, Alt or Win for a shortcut)…" : "Press a key to assign it"}</button>
      ${kv.listening ? `<button class="tbtn" data-act="kc-listen-stop">Cancel</button>` : ""}
      <p class="hint">A key you've changed types its new job, so to set one back, click it in the lists below or use Reset.</p>
      ${group("Letters", range(0x04, 26))}
      ${group("Numbers and symbols", [...range(0x1e, 10), 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38])}
      ${group("Function keys", range(0x3a, 12))}
      ${group("Editing", [0x29, 0x2a, 0x28, 0x2b, 0x2c, 0x39, 0x49, 0x4c], "")}
      ${group("Navigation", [0x4a, 0x4d, 0x4b, 0x4e, 0x52, 0x51, 0x50, 0x4f, 0x46, 0x47, 0x48, 0x65], "")}
      ${group("Modifiers", range(0xe0, 8), "")}
      <div class="kopts">${opt(0, "Nothing")}${kv.layer ? opt(1, "Fall through", "▽") : ""}</div>`;
  } else if (pick === "media") {
    body = `<div class="kopts">${MEDIA.map((c) => opt(c, USAGE[c])).join("")}</div>`;
  } else if (pick === "layers") {
    const n = kst().layers || 4;
    body = LAYER_FNS.slice(0, 3)
      .map(([fn, label]) => `<div class="cap kgroup">${label}</div><div class="kopts">${Array.from({ length: n }, (_, i) => opt(layerCode(fn, i), `${fn} ${i}`)).join("")}</div>`)
      .join("");
    body += `<p class="hint">MO: the layer is on while the key is held (the usual Fn key). TG: each press turns it on or off. TO: stays on that layer.</p>`;
  } else if (pick === "light") {
    const wilba = kst().lighting === "wilba";
    body = `<div class="kopts">${(wilba ? WILBA_KEYS : rgbKeys()).map((n, i) => opt(wilba ? customCode(i) : rgbCode(i), n)).join("")}</div>`;
  } else {
    body = macroPick(code);
  }
  const def = defaultKeycode(kv.layer, k);
  const defInfo = kcInfo(def);
  return `<section class="sec">
      <div class="sec-head"><span class="sec-title">${esc(info.text || "Nothing")}<small>${esc(info.sub || "")}</small></span><span class="cap">Row ${k.r + 1} · col ${k.c + 1}</span></div>
      <button class="tbtn kreset" data-act="kc-set" data-code="${def}" ${code === def ? "disabled" : ""}>${icons.reset}Reset to ${esc(def === 1 ? "fall through" : defInfo.text)}</button>
      <div class="seg fill">${PICKS.map(([id, label]) => `<button class="${pick === id ? "on" : ""}" data-act="kc-pick" data-v="${id}">${label}</button>`).join("")}</div>
    </section>
    <section class="sec">${body}</section>`;
}

function macroPick(code) {
  const macros = mcfg().macros;
  const s = kst();
  const modes = [
    ["board", "Store on the keyboard", "Works on any computer, even without kit. Keys and waits only; plays once (or its repeat count) per press."],
    ["kit", "Run through kit", "Clicks, hold-to-repeat and toggle work. Needs kit running on this PC. Uses one of 12 spare keys (F13-F24)."],
  ];
  const list = macros.length
    ? `<select data-field="kc-macro" aria-label="Macro"><option value="">Choose a macro…</option>${macros.map((m) => `<option value="${m.id}">${esc(m.name)}</option>`).join("")}</select>`
    : `<span class="muted">No macros yet. <button class="link" data-act="keyboard-tab" data-tab="macros">Record one</button></span>`;
  const onboard = s.macros
    .map((steps, i) => {
      if (!steps.length) return "";
      const owner = kcfg().onboard[i] && mcfg().macros.find((m) => m.id === kcfg().onboard[i]);
      return `<button class="kopt wide ${code === macroCode(i) ? "on" : ""}" data-act="kc-set" data-code="${macroCode(i)}">M${i}<small>${esc(owner ? owner.name : macroPreview(steps))}</small></button>`;
    })
    .join("");
  return `<div class="choices one" role="radiogroup">${modes.map(([v, l]) => `<button class="choice ${kv.macroMode === v ? "on" : ""}" data-act="kc-macro-mode" data-v="${v}">${l}</button>`).join("")}</div>
    <p class="hint">${modes.find((m) => m[0] === kv.macroMode)[2]}</p>
    <div class="extra">${list}</div>
    ${onboard ? `<div class="cap kgroup">Already on the keyboard</div><div class="kopts">${onboard}</div>` : ""}`;
}

function macroPreview(steps) {
  const parts = steps.map((st) => {
    switch (st.type) {
      case "text": return st.text;
      case "tap": return `‹${USAGE[st.code] || st.code}›`;
      case "down": return `‹${USAGE[st.code] || st.code}↓›`;
      case "up": return `‹${USAGE[st.code] || st.code}↑›`;
      default: return `‹${st.ms} ms›`;
    }
  });
  const text = parts.join("");
  return text.length > 40 ? `${text.slice(0, 40)}…` : text;
}

function tklKeysPage() {
  const sel = LAYOUT.proxtkl.keys.find((k) => k.id === kv.key && k.usage != null);
  const bound = (u) => kcfg().bindings.find((b) => b.usage === u);
  const board = boardHtml((k, box) => {
    const b = k.usage != null && bound(k.usage);
    const cls = ["kkey", b ? "kitm" : "", k.id === kv.key ? "sel" : "", k.usage == null ? "fixed" : ""].join(" ");
    const sub = b ? bindingSummary(b.action) : "";
    return `<button class="${cls}" style="${box}" data-act="key-pick" data-k="${k.id}" ${k.usage == null ? "disabled" : ""}><span class="kl">${esc(k.usage == null ? "Fn" : USAGE[k.usage])}</span>${sub ? `<span class="ks">${esc(sub)}</span>` : ""}</button>`;
  }, "", "Click a key to give it a macro or a shortcut. kit handles these keys while it runs, in games too; every other key works as normal.");
  const n = kcfg().bindings.length;
  return `<div class="page devpage" data-key="kb-keys">
    <div class="work">
      <section class="canvas">
        <div class="canvas-head"><h1 class="h1 inline-h">Keys</h1></div>
        ${plate([keyStatusCell("status"), battery("bat", kst().connected ? kst().battery : null, kst().charging), cell("bind", "Keys handled by kit", n ? n : "None", { span: 2, cls: n ? "" : "text dim" })])}
        ${board}
      </section>
      <aside class="panel">${sel ? tklKeyPanel(sel, bound(sel.usage)) : `<section class="sec"><div class="sec-title">No key selected</div><p class="hint">Click a key on the drawing.</p></section>`}</aside>
    </div>
  </div>`;
}

const BIND_TYPES = [["default", "Normal key"], ["disabled", "Disabled"], ["keys", "Key combination"], ["macro", "Macro"]];

function bindingSummary(a) {
  switch (a.type) {
    case "keys": return a.keys.length ? a.keys.map((k) => k.name).join("+") : "No keys set";
    case "macro": return mcfg().macros.find((m) => m.id === a.id)?.name || "No macro";
    case "disabled": return "Disabled";
    default: return "";
  }
}

function tklKeyPanel(k, b) {
  const a = b ? b.action : { type: "default" };
  const opts = BIND_TYPES.map(([v, l]) => `<button class="choice ${a.type === v ? "on" : ""}" data-act="kb-bind" data-v="${v}">${l}</button>`).join("");
  let extra = `<span class="muted">Types ${esc(USAGE[k.usage])} as usual.</span>`;
  if (a.type === "keys") {
    const listening = keyCapture && keyCapture.kind === "kbkey";
    extra = `<div><button class="capture ${listening ? "listening" : ""}" data-act="kb-capture">${listening ? "Press a key combination…" : a.keys.length ? keysHtml(a.keys) : "Click to set keys"}</button></div>`;
  } else if (a.type === "macro") {
    extra = mcfg().macros.length
      ? `<select data-field="kb-bind-macro" aria-label="Macro">${a.id ? "" : `<option value="" selected>Choose a macro…</option>`}${mcfg().macros.map((m) => `<option value="${m.id}" ${a.id === m.id ? "selected" : ""}>${esc(m.name)}</option>`).join("")}</select>`
      : `<span class="muted">No macros yet. <button class="link" data-act="keyboard-tab" data-tab="macros">Record one</button></span>`;
  } else if (a.type === "disabled") {
    extra = `<span class="muted">The key does nothing while kit runs.</span>`;
  }
  return `<section class="sec">
    <div class="sec-head"><span class="sec-title">${esc(USAGE[k.usage])}<small>${a.type === "default" ? "normal" : "handled by kit"}</small></span></div>
    <div class="choices" role="radiogroup">${opts}</div><div class="extra">${extra}</div>
  </section>`;
}

// ---------- lighting ----------

// The effect cards this board offers (catalog in lighting.js).
const kitFirst = () => (isNk() && kst().kitFirmware ? (kst().kitEffects || [0, 0])[0] : null);
function fxCatalog() {
  if (!isNk()) {
    const tag = (f) => ({ ...f, key: `l${f.id}` });
    if (!kst().effects.length) return LIGHTSYNC_FX.map(tag);
    // Only what this keyboard offers (cycle, wave and ripple each have two firmware ids).
    const have = new Set(kst().effects);
    const ids = { 0: [0], 1: [1], 10: [10, 2], 3: [3, 0x15], 4: [4, 0x16], 11: [11, 0x17] };
    return LIGHTSYNC_FX.filter((f) => ids[f.id].some((x) => have.has(x))).map(tag);
  }
  const app = (onKitFw) => KIT_FX.map((f) => ({ ...f, key: `a${f.id}`, hidden: f.hidden || (onKitFw && f.onKitFw === false) }));
  if (kst().lighting !== "rgbMatrix") return [...WILBA_FX.map((f) => ({ ...f, key: `w${f.id}` })), ...app(false)];
  const first = kitFirst();
  const count = (kst().kitEffects || [0, 0])[1];
  const fw = first == null ? [] : KITFW_FX.slice(0, count).map((f) => ({ ...f, id: first + f.id, key: `k${f.id}`, onBoard: true }));
  // kit's two-color Gradient replaces QMK's rainbow one.
  const kitGradient = count > 8;
  const qmk = RGBM_FX.map((f) => ({ ...f, key: `q${f.id}`, hidden: f.hidden || (f.qmkGradient && kitGradient) }));
  return [...fw, ...qmk, ...app(first != null)];
}
const fxOf = (p) => fxCard(fxCatalog(), p.effect) || fxCatalog().find((f) => !f.hidden && f.id !== 0);
// The NK65 preview runs the effect itself (lighting.js); the others use CSS stand-ins.
const simulated = (p) => isNk() && (kst().lighting === "rgbMatrix" || p.effect >= 200);
const LEGACY_PREVIEW = {
  wilba: { 0: "off", 1: "solid", 2: "alphas", 3: "gradient", 4: "drops", 5: "cycle", 6: "wave-x", 7: "wave-y", 8: "jelly", 9: "radial", 10: "solid" },
  lightsync: { 0: "off", 1: "solid", 10: "breathe", 3: "cycle", 4: "wave-x", 11: "dim" },
};
const legacyKind = (p) => LEGACY_PREVIEW[isNk() ? "wilba" : "lightsync"][p.effect] || "solid";

const SWATCHES = ["#ffffff", "#fff1dc", "#ff4040", "#ff8a1f", "#ffd23f", "#4ade80", "#9fe8cf", "#38bdf8", "#4a6cff", "#a78bfa", "#ffb3d1", "#ff4fa3"];
const INDICATORS = ["Caps Lock", "Layer 1", "Layer 2", "Layer 3"];
// The NK65 takes only hue and saturation (brightness is its own setting), so its colors are kept at full value.
const boardColor = (c) => (isNk() ? fullValue(c) : c);

const hexToRgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const rgbHex = (rgb) => "#" + rgb.map((v) => Math.round(clamp(v, 0, 255)).toString(16).padStart(2, "0")).join("");
const mix = (a, b, t) => rgbHex(hexToRgb(a).map((v, i) => v + (hexToRgb(b)[i] - v) * t));
const hue = (deg) => {
  const f = (n) => {
    const k = (n + deg / 30) % 12;
    return 255 * (0.5 - 0.5 * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
  };
  return rgbHex([f(0), f(8), f(4)]);
};
// A fixed pseudo-random value per key, so drops and jellybeans don't reshuffle on every render.
const keyNoise = (k) => (Math.sin((k.x + 1) * 12.9898 + (k.y + 1) * 78.233) * 43758.5453) % 1;

// What color the stand-in preview shows on key `k` (null = dark): the NK65's stock firmware and the PRO X TKL.
function ledColor(k, p) {
  const kind = legacyKind(p);
  const L = layout();
  let c;
  switch (kind) {
    case "off": return null;
    case "alphas": {
      const code = isNk() ? keycodeAt(0, k.r, k.c) : k.usage;
      const alpha = code >= 0x04 && code <= 0x38 && code !== 0x28 && code !== 0x29 && code !== 0x2a && code !== 0x2b;
      c = alpha ? p.color1 : p.color2;
      break;
    }
    case "gradient": c = mix(p.color1, p.color2, k.y / (L.h - 1)); break;
    case "drops": c = Math.abs(keyNoise(k)) > 0.5 ? p.color1 : p.color2; break;
    case "jelly": c = hue(Math.abs(keyNoise(k)) * 360); break;
    case "wave-x": c = hue((k.x / L.w) * 360); break;
    case "wave-y": c = hue((k.y / L.h) * 360); break;
    case "radial": c = hue(((Math.atan2(k.y - L.h / 2, k.x - L.w / 2) / Math.PI) * 180 + 360) % 360); break;
    case "cycle": c = hue(0); break;
    case "dim": c = mix("#000000", p.color1, 0.25); break;
    default: {
      const z = !isNk() && k.usage != null ? p.keys[zoneOf(k.usage)] : null;
      c = z || p.color1;
    }
  }
  return mix("#000000", boardColor(c), p.brightness / 100);
}

// The simulated board on the Lighting page: one frame every ~33 ms while it's on screen.
const sim = new LightSim();
let simLeds = null;
let simAt = 0;
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
function simStep(now) {
  requestAnimationFrame(simStep);
  const board = document.querySelector(".board.fx-sim");
  if (!board) {
    simAt = 0;
    return;
  }
  if (simAt && now - simAt < 33) return;
  const dt = simAt && !reducedMotion.matches ? Math.min(100, now - simAt) : 0;
  simAt = now;
  simLeds = sim.step(dt, lightPreset(), kitFirst(), kst().kitFirmware || 0, !reducedMotion.matches);
  for (const el of board.children) {
    const l = LED_BY_ID.get(el.dataset.k);
    if (l) el.style.setProperty("--led", ledHex(simLeds[l.i]));
  }
}
requestAnimationFrame(simStep);

function lightStrip() {
  const tabs = kcfg()
    .presets.map((p) => {
      const on = p.id === kcfg().active;
      return `<button class="stab ${on ? `sel ${kst().connected ? "live" : "stored"}` : ""}" data-act="light-preset" data-id="${esc(p.id)}"><span class="pdot"></span>${esc(p.name)}</button>`;
    })
    .join("");
  return `<nav class="strip" aria-label="Lighting presets">${tabs}<button class="tbtn" data-act="light-new">${icons.plus}New lighting</button></nav>`;
}

function lightingPage() {
  const p = lightPreset();
  const fx = fxOf(p);
  const simOn = simulated(p);
  const kind = simOn ? "sim" : legacyKind(p);
  const deleting = confirmDelete === p.id;
  const painting = !isNk() && p.effect === 1;
  const speedMs = 20 - p.speed * 0.17; // stand-in preview seconds per cycle
  const board = boardHtml((k, box) => {
    const label = isNk() ? kcInfo(keycodeAt(0, k.r, k.c)).text : k.usage == null ? "Fn" : USAGE[k.usage];
    if (simOn) {
      // Keep the last frame's color across re-renders; the loop above updates it.
      const l = LED_BY_ID.get(k.id);
      const c = l && simLeds ? ledHex(simLeds[l.i]) : "#000000";
      return `<button class="kkey lit" style="${box};--led:${c}" data-act="light-hit" data-k="${k.id}" tabindex="-1"><span class="kl">${esc(label)}</span></button>`;
    }
    const c = ledColor(k, p);
    const painted = painting && k.usage != null && p.keys[zoneOf(k.usage)];
    return `<button class="kkey ${c ? "lit" : ""} ${painted ? "painted" : ""}" style="${box};${c ? `--led:${c}` : ""}" data-act="paint-key" data-k="${k.id}" ${painting && k.usage != null ? "" : "tabindex=-1"}><span class="kl">${esc(label)}</span></button>`;
  }, `leds fx-${kind} ${painting ? "paintable" : ""}`, painting ? "Paint keys with the brush on the right; unpainted keys use the static color." : simOn ? "The preview runs the same effect as the keyboard, with made-up typing. Click keys to press them." : "The drawing is a preview of the effect; the keyboard shows the real thing.");

  return `<div class="page devpage" data-key="kb-light">
    ${lightStrip()}
    <div class="work">
      <section class="canvas">
        <div class="canvas-head">
          <input class="pname" data-field="light-name" value="${esc(p.name)}" spellcheck="false" maxlength="40" aria-label="Lighting name" />
          <div class="pactions">
            ${kst().connected ? `<span class="live-mark">On keyboard</span>` : `<span class="off-mark">Active lighting</span>`}
            <button class="tbtn" data-act="light-duplicate">${icons.copy}Duplicate</button>
            <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="light-delete" ${kcfg().presets.length < 2 ? "disabled" : ""}>${icons.trash}${deleting ? "Confirm delete" : "Delete"}</button>
          </div>
        </div>
        ${plate([
          keyStatusCell("status"),
          cell("fx", "Effect", esc(fx.name), { cls: "text" }),
          cell("bright", "Brightness", `${p.brightness}<small>%</small>`),
          isNk() ? cell("store", "Stored", kst().connected ? "On keyboard" : "In kit", { cls: "text" }) : cell("lights", "Lights", kst().lightsAsleep ? "Off · idle" : kst().connected ? "On" : "—", { cls: `text ${kst().connected && !kst().lightsAsleep ? "" : "dim"}` }),
        ])}
        <div style="--fx-speed:${speedMs.toFixed(1)}s;display:contents">${board}</div>
      </section>
      <aside class="panel">
        ${fxPicker(p, fx)}
        ${fxSettings(p, fx)}
        ${painting ? paintSection(p) : ""}
        ${isNk() && kst().lighting === "wilba" ? indicatorSection(p) : ""}
      </aside>
    </div>
  </div>`;
}

// Effects as picture cards, one group at a time, with starred ones first.
function fxPicker(p, fx) {
  const all = fxCatalog();
  // Hidden effects only show while a preset uses them.
  const shown = all.filter((f) => !f.hidden || f.id === fx.id);
  const favs = kcfg().favorites || [];
  const starred = shown.filter((f) => favs.includes(f.key));
  // A short list (the PRO X TKL) shows every card; longer ones a group at a time.
  const groups = shown.length > 8 ? FX_GROUPS.filter(([g]) => shown.some((f) => f.group === g)) : [];
  if (groups.length && starred.length) groups.unshift(["fav", "★ Favorites", "Effects you starred. Click the star on any card to add or remove it."]);
  const fxFav = starred.some((f) => f.key === fx.key);
  const group = groups.some(([g]) => g === kv.fxGroup) ? kv.fxGroup : fxFav ? "fav" : fx.group;
  const about = groups.find(([g]) => g === group)?.[2] || "";
  const tabs = groups.map(([g, name]) => `<button class="${g === group ? "on" : ""} ${g === fx.group || (g === "fav" && fxFav) ? "has" : ""}" data-act="fx-group" data-v="${g}">${esc(name)}</button>`).join("");
  const list = !groups.length ? shown : group === "fav" ? starred : shown.filter((f) => f.group === group);
  const cards = list
    .map((f) => {
      const on = f.id === fx.id;
      const fav = favs.includes(f.key);
      const thumb = isNk() && (kst().lighting === "rgbMatrix" || f.id >= 200) ? fxThumb(f.id, p, kitFirst(), kst().kitFirmware || 0) : drawThumb(["lg", isNk(), f.id, p.color1, p.color2, JSON.stringify(p.keys)].join("|"), layout(), (k) => ledColor(k, { ...p, effect: f.id, brightness: 100 }));
      return `<button class="fxcard ${on ? "on" : ""}" data-act="light-fx" data-v="${f.id}" role="radio" aria-checked="${on}"><span class="fxthumb" style="background-image:url(${thumb})"></span><span class="fxname">${esc(f.name)}</span><span class="fxstar ${fav ? "on" : ""}" data-act="fx-fav" data-key="${f.key}" title="${fav ? "Remove from favorites" : "Add to favorites"}">${fav ? "★" : "☆"}</span></button>`;
    })
    .join("");
  return `<section class="sec">
    <div class="sec-head"><span class="sec-title">Effect</span></div>
    ${groups.length ? `<div class="fxtabs" role="tablist">${tabs}</div><p class="hint fxabout">${esc(about)}</p>` : ""}
    <div class="fxcards" role="radiogroup" aria-label="Effects">${cards}</div>
  </section>`;
}

// The colors an effect uses, by role: [field, label].
function colorSlots(fx) {
  switch (fx.color) {
    case "one": return [["color1", fx.c1 || "Color"]];
    case "base": return [["color1", fx.c1 || "Base color"]];
    case "two":
    case "mods": return [["color1", fx.c1 || "First color"], ["color2", fx.c2 || "Second color"]];
    default: return [];
  }
}

// Plain hue (degrees) and saturation (0-1) of a color, and back at full value.
function hexHs(hex) {
  const [r, g, b] = hexToRgb(hex).map((v) => v / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d) h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [Math.round(h * 60) % 360, max ? d / max : 0];
}
function hsHex(h, s) {
  const f = (n) => {
    const k = (n + h / 60) % 6;
    return 255 * (1 - s * Math.max(0, Math.min(k, 4 - k, 1)));
  };
  return rgbHex([f(5), f(3), f(1)]);
}

// One color slot open at a time: chips for each role, then swatches and hue/saturation sliders.
function colorPicker(p, fx) {
  const slots = colorSlots(fx);
  if (!slots.length) return "";
  const field = slots.some(([f]) => f === kv.colorSlot) ? kv.colorSlot : slots[0][0];
  const value = boardColor(p[field]);
  const [h, sat] = hexHs(value);
  const chips = slots.length > 1
    ? `<div class="cslots" role="tablist">${slots.map(([f, label]) => `<button class="cslot ${f === field ? "on" : ""}" data-act="color-slot" data-v="${f}"><span class="cdot" style="--c:${boardColor(p[f])}"></span>${esc(label)}</button>`).join("")}</div>`
    : `<div class="row-head"><span class="sec-title">${esc(slots[0][1])}</span></div>`;
  const sw = isNk() ? SWATCHES.map(fullValue) : SWATCHES;
  const hint = fx.color === "mods" && field === "color2" ? `<p class="hint">Modifiers take their hue from this color and the saturation of the letter keys' color.</p>` : fx.color === "base" ? `<p class="hint">The effect moves a little either side of this color.</p>` : "";
  return `<section class="sec">
    ${chips}
    <div class="cpick">
      <span class="cbig" style="--c:${value}"></span>
      <div class="cbars">
        <input class="cbar hue" type="range" min="0" max="359" step="1" value="${h}" data-field="hs-hue" data-slot="${field}" aria-label="Hue" />
        <input class="cbar sat" type="range" min="0" max="100" step="1" value="${Math.round(sat * 100)}" data-field="hs-sat" data-slot="${field}" style="--hue:${hsHex(h, 1)}" aria-label="Saturation" />
      </div>
    </div>
    <div class="swatches">${sw.map((c) => `<button class="sw ${value === c ? "on" : ""}" style="--c:${c}" data-act="light-color" data-field="${field}" data-c="${c}" title="${c}" aria-label="Color ${c}"></button>`).join("")}</div>
    ${hint}
  </section>`;
}

// What the chosen effect does, and only the controls it uses.
function fxSettings(p, fx) {
  const fw = kst().kitFirmware || 0;
  const notes = [];
  if (fx.single != null) {
    notes.push(`<div class="ind-row fxsingle"><label class="switch"><input type="checkbox" data-field="fx-single" ${p.effect === fx.single ? "checked" : ""} aria-label="Only the latest press" /><span></span></label><span class="grow">Only the latest press<small>A new press cuts off the one before</small></span></div>`);
  }
  if (fx.fw && fw < fx.fw) notes.push(`<p class="hint warn">Needs kit's firmware ${fx.fw} or newer. Older firmware counts every key as a letter key, so the whole board shows the letter color.</p>`);
  if (fx.onBoard && fw < 4) notes.push(`<p class="hint warn">The second color, background and smooth fades need kit's firmware 4. This keyboard has version ${fw}, so it shows one color with a dim glow left over.</p>`);
  if (fx.onBoard) notes.push(`<p class="hint">Runs on the keyboard itself, so it works without kit too.</p>`);
  if (fx.group === "kit") notes.push(`<p class="hint">Runs while kit is running (the window can be closed). Typing in kit's own window doesn't count. Without kit, the keyboard shows the first color.</p>`);
  if (fx.color === "fixed") notes.push(`<p class="hint">Always green, so it has no color setting.</p>`);
  const row = (field, label, value) => `<div class="row-head" style="margin-top:12px"><span class="sec-title">${esc(label)}</span><span class="rval" id="v-${field}">${value}%</span></div>${slider(field, value, 100, false, label)}`;
  const background = fx.bg ? row("light-bg", "Background", p.background || 0) + `<p class="hint">How lit the board stays ${fx.group === "typing" ? "between presses" : "in the effect's dark parts"}. 0% is fully off.</p>` : "";
  return `<section class="sec">
      <div class="sec-head"><span class="sec-title">${esc(fx.name)}<small>${esc(FX_GROUPS.find(([g]) => g === fx.group)?.[1] || "")}</small></span></div>
      <p class="hint fxdesc">${esc(fx.about)}</p>
      ${notes.join("")}
    </section>
    ${colorPicker(p, fx)}
    ${fx.color === "none" ? "" : `<section class="sec">
      <div class="row-head"><span class="sec-title">Brightness</span><span class="rval" id="v-light-bright">${p.brightness}%</span></div>
      ${slider("light-bright", p.brightness, 100, false, "Brightness")}
      ${background}
      ${fx.speed ? row("light-speed", fx.speed, p.speed) : ""}
    </section>`}`;
}

function paintSection(p) {
  const n = Object.keys(p.keys).length;
  return `<section class="sec">
    <div class="sec-head"><span class="sec-title">Paint keys<small>${n ? `${n} painted` : "none yet"}</small></span>${n ? `<button class="tbtn" data-act="paint-clear">Clear</button>` : ""}</div>
    <div class="seg" style="margin-bottom:10px"><button class="${kv.erase ? "" : "on"}" data-act="paint-mode" data-v="brush">${icons.brush} Brush</button><button class="${kv.erase ? "on" : ""}" data-act="paint-mode" data-v="erase">${icons.eraser} Eraser</button></div>
    <div class="swatches">${SWATCHES.map((c) => `<button class="sw ${kv.brush === c ? "on" : ""}" style="--c:${c}" data-act="brush" data-c="${c}" aria-label="Brush ${c}"></button>`).join("")}<label class="sw custom ${SWATCHES.includes(kv.brush) ? "" : "on"}" style="--c:${kv.brush}"><input type="color" value="${kv.brush}" data-field="brush" aria-label="Custom brush color" /></label></div>
    <p class="hint">Click or drag across keys. Painted keys show over the static color.</p>
  </section>`;
}

function indicatorSection(p) {
  const rows = INDICATORS.map((name, i) => {
    const c = p.indicators[i];
    return `<div class="ind-row"><label class="switch"><input type="checkbox" data-field="ind-on" data-i="${i}" ${c ? "checked" : ""} aria-label="${name} light" /><span></span></label>
      <span class="grow">${name}</span>${c ? `<label class="sw custom on small" style="--c:${c}"><input type="color" value="${c}" data-field="ind-color" data-i="${i}" aria-label="${name} color" /></label>` : ""}</div>`;
  }).join("");
  return `<section class="sec"><div class="sec-head"><span class="sec-title">Indicator lights</span></div>${rows}
    <p class="hint">The whole board shows this color while Caps Lock (or the layer) is on.</p></section>`;
}

// ---------- device page ----------

// After a firmware flash the board comes back with default keys: offer what kit backed up before.
function restoreSection(s) {
  if (s.restore == null || !s.connected) return "";
  return `<h2 class="h2">Put your keys back</h2>
    <p class="hint">The keyboard is on different firmware now and came back with the default keymap. kit kept a copy of your keymap and onboard macros from before (VIA v${s.restore}), translated for this firmware.</p>
    <div class="btn-row"><button class="btn" data-act="nk-restore">Restore my keymap and macros</button><button class="btn quiet" data-act="nk-restore-skip">Keep the defaults</button></div>`;
}

function firmwareSection(s) {
  const flashBtn = s.connected ? `<button class="btn quiet" data-act="nk-bootloader">Restart into flash mode</button>` : "";
  const fnBoot = "Fn + \\";
  if (s.flashing && !s.connected) {
    return `<h2 class="h2">Firmware</h2>
      <p class="hint">The keyboard should be in flash mode now (its lights go off and it stops typing). Flash it with QMK Toolbox; kit picks it back up when it restarts. If it is still typing, press ${fnBoot} instead, or plug it back in while holding Esc.</p>`;
  }
  if (s.kitFirmware) {
    return `<h2 class="h2">Firmware</h2>
      <p class="hint">kit's firmware, version ${s.kitFirmware}: QMK with VIA plus kit's own lighting effects (the kit effects on the Lighting page). For a newer version, download it, restart into flash mode and flash it with QMK Toolbox.</p>
      <div class="btn-row"><button class="btn quiet" data-act="nk-firmware-page">Open the download page</button>${flashBtn}</div>`;
  }
  if (s.name !== "NK65") return "";
  return `<h2 class="h2">Firmware</h2>
    <p class="hint">This NK65 runs its original firmware, which lights the whole board one way at a time. kit's firmware (current QMK with VIA) adds per-key effects that run on the keyboard itself: ripples from the key you press, splashes, ink, afterglow, and calmer ones like aurora and lava.</p>
    <ol class="hint flash-steps">
      <li>kit has already backed up your keymap and macros, and offers to put them back after flashing.</li>
      <li>Download <b>kit-nk65.bin</b> from the download page, and QMK Toolbox from qmk.fm/toolbox. The one file fits both NK65 PCBs (v1 and v1.4).</li>
      <li>Click <b>Restart into flash mode</b> (or press ${fnBoot}, or plug the board in while holding Esc). Its lights go off.</li>
      <li>In QMK Toolbox, open the file and click Flash. It takes a few seconds.</li>
    </ol>
    <div class="btn-row"><button class="btn quiet" data-act="nk-firmware-page">Open the download page</button>${flashBtn}</div>`;
}

function keyboardDevicePage() {
  const s = kst();
  const st = keyStatus();
  if (isNk()) {
    const rows = s.macros
      .map((steps, i) => {
        const owner = kcfg().onboard[i] && mcfg().macros.find((m) => m.id === kcfg().onboard[i]);
        return `<div class="mrow-line"><span class="cap">M${i}</span><span class="grow">${steps.length ? esc(owner ? `${owner.name} (from kit)` : macroPreview(steps)) : '<span class="muted">Empty</span>'}</span></div>`;
      })
      .join("");
    const importable = s.macros.some((m, i) => m.length && !kcfg().onboard[i]);
    return `<div class="page fpage" data-key="kb-device"><div class="fpage-inner">
      <h1 class="h1">${esc(keyboardName(true))}</h1>
      ${plate([
        keyStatusCell("status"),
        cell("pcb", "Board", `${esc(s.name || "—")}${s.pcb ? ` <small>PCB ${s.pcb}</small>` : ""}`, { cls: "text" }),
        cell("fw", "Firmware", s.kitFirmware ? `kit <small>v${s.kitFirmware}</small>` : s.protocol ? `VIA <small>v${s.protocol}</small>` : "—", { cls: s.protocol ? "" : "dim" }),
        cell("layers", "Layers", s.layers || "—", { cls: s.layers ? "" : "dim" }),
      ])}
      ${st.hint && !s.flashing ? `<p class="hint">${st.hint}</p>` : ""}
      ${restoreSection(s)}
      ${firmwareSection(s)}
      <h2 class="h2">Onboard macros</h2>
      <p class="hint">What the keyboard has stored. Macros you put on keys from kit use the last free slots, so ones made in VIA stay as they are.</p>
      <div class="mlines">${rows || '<span class="muted">Connect the keyboard to see them.</span>'}</div>
      ${importable ? `<div style="margin-top:12px"><button class="btn quiet" data-act="nk-import">Copy them into kit's macros</button></div>` : ""}
      <h2 class="h2">Good to know</h2>
      <p class="hint">Keep VIA closed while kit runs; both talk to the keyboard over the same channel. Everything here is stored on the NK65 itself, except macros set to run through kit.</p>
    </div></div>`;
  }
  const IDLE = [[60, "1 min"], [120, "2 min"], [300, "5 min"], [600, "10 min"], [900, "15 min"]];
  return `<div class="page fpage" data-key="kb-device"><div class="fpage-inner">
    <h1 class="h1">${esc(keyboardName(true))}</h1>
    ${plate([
      keyStatusCell("status"),
      battery("bat", s.connected ? s.battery : null, s.charging),
      cell("lights", "Lights", s.lightsAsleep ? "Off · idle" : s.connected ? "On" : "—", { cls: `text ${s.connected ? "" : "dim"}` }),
      cell("name", "Reported as", esc(s.name || "—"), { cls: "text" }),
    ])}
    ${st.hint ? `<p class="hint">${st.hint}</p>` : ""}
    <h2 class="h2">Battery</h2>
    <div class="row">
      <div><div class="rtitle">Lights off after</div><p class="hint">The lights fade out when you stop typing for this long and come back with the next key press.</p></div>
      <div class="seg">${IDLE.map(([v, l]) => `<button class="${kcfg().idleOff === v ? "on" : ""}" data-act="idle-off" data-v="${v}">${l}</button>`).join("")}</div>
    </div>
    <h2 class="h2">Good to know</h2>
    <p class="hint">kit drives the lighting while it runs and hands it back to the keyboard's onboard profile when you quit. Keep G HUB closed: it fights kit for the keyboard.</p>
    ${s.effects.length ? `<p class="hint">Effects reported by the keyboard: ${s.effects.map((e) => `0x${e.toString(16).padStart(2, "0")}`).join(", ")}; ${s.zones.length} per-key zones.</p>` : ""}
  </div></div>`;
}

// ---------- actions ----------

async function activateLighting(id) {
  kv.fxGroup = null;
  await keyboardSaver.flush();
  K = await invoke("activate_lighting", { id });
  render();
}

// Write one keycode on the NK65 and keep kit's bookkeeping in step.
function setKeycode(code) {
  const k = LAYOUT.nk65.keys.find((x) => x.id === kv.key);
  if (!k) return;
  freeIfUnused(writeKey(kv.layer, k, code));
  render();
}

// Returns the keycode that was there.
function writeKey(layer, k, code) {
  const s = kst();
  const old = keycodeAt(layer, k.r, k.c);
  if (old !== code) {
    s.keymap[(layer * s.rows + k.r) * s.cols + k.c] = code;
    invoke("keyboard_set_key", { layer, row: k.r, col: k.c, code });
  }
  return old;
}

// Every key on a layer back to the factory keymap.
function resetLayer(layer) {
  const olds = LAYOUT.nk65.keys.map((k) => writeKey(layer, k, defaultKeycode(layer, k)));
  new Set(olds).forEach(freeIfUnused);
  render();
}

// A kit macro slot (F13+i or an onboard slot) no key uses any more is free again.
function freeIfUnused(code) {
  if (kst().keymap.includes(code)) return;
  const i = code - F13;
  let changed = false;
  if (i >= 0 && i < KIT_KEYS && kcfg().kitKeys[i]) {
    kcfg().kitKeys[i] = null;
    changed = true;
  }
  const slot = code - macroCode(0);
  if (slot >= 0 && slot < 32 && kcfg().onboard[slot]) {
    delete kcfg().onboard[slot];
    changed = true;
  }
  if (changed) keyboardSaver.save(0);
}

async function bindMacroToKey(macroId) {
  let code;
  if (kv.macroMode === "kit") {
    let i = kcfg().kitKeys.indexOf(macroId);
    if (i < 0) i = kcfg().kitKeys.indexOf(null);
    if (i < 0) {
      toast("All 12 kit-run macro keys are in use. Store this one on the keyboard instead.", 4000);
      return;
    }
    kcfg().kitKeys[i] = macroId;
    code = F13 + i;
  } else {
    const s = kst();
    const owned = Object.entries(kcfg().onboard).find(([, id]) => id === macroId);
    let slot = owned ? Number(owned[0]) : -1;
    // Free slots from the top down, so the ones made in VIA (M0, M1, ...) are kept.
    for (let n = s.macros.length - 1; slot < 0 && n >= 0; n--) {
      if (!kcfg().onboard[n] && !s.macros[n].length && !s.keymap.includes(macroCode(n))) slot = n;
    }
    if (slot < 0) {
      toast("The keyboard's macro slots are full.", 4000);
      return;
    }
    kcfg().onboard[slot] = macroId;
    code = macroCode(slot);
    const m = mcfg().macros.find((x) => x.id === macroId);
    if (m && m.steps.some((st) => st.type === "mouse")) toast("Clicks can't be stored on the keyboard; they're left out. Use \"Run through kit\" to keep them.", 5000);
  }
  // The macro has to be on the board before the key points at it.
  keyboardSaver.save(0);
  await keyboardSaver.flush();
  setKeycode(code);
}

// Onboard macros made in VIA -> kit macros (keys and waits), so they can be edited here.
function importOnboard() {
  const keyRef = (usage) => {
    const [scan, ext] = USAGE_SCAN[usage] || [0];
    return { vk: 0, scan, ext: !!ext, name: USAGE[usage] || `0x${usage.toString(16)}` };
  };
  let added = 0;
  kst().macros.forEach((steps, i) => {
    if (!steps.length || kcfg().onboard[i]) return;
    const out = [];
    const press = (u, down) => USAGE_SCAN[u] && out.push({ type: "key", key: keyRef(u), down });
    for (const st of steps) {
      if (st.type === "delay") out.push({ type: "delay", ms: st.ms });
      else if (st.type === "down") press(st.code, true);
      else if (st.type === "up") press(st.code, false);
      else if (st.type === "tap") {
        press(st.code, true);
        press(st.code, false);
      } else {
        for (const ch of st.text) {
          const lower = ch.toLowerCase();
          let u = PLAIN[ch] ?? SHIFTED[ch];
          if (u == null && /[a-z]/.test(lower)) u = 4 + lower.charCodeAt(0) - 97;
          if (u == null && /[0-9]/.test(ch)) u = ch === "0" ? 0x27 : 0x1d + Number(ch);
          if (u == null) continue;
          const shift = ch in SHIFTED || (ch !== lower && /[A-Z]/.test(ch));
          if (shift) press(0xe1, true);
          press(u, true);
          press(u, false);
          if (shift) press(0xe1, false);
        }
      }
    }
    const m = { id: newId(), name: `NK65 M${i}`, mode: "once", repeat: 1, steps: out };
    mcfg().macros.push(m);
    kcfg().onboard[i] = m.id;
    added++;
  });
  mouseSaver.save(0);
  keyboardSaver.save(0);
  toast(added ? `Copied ${added} macro${added > 1 ? "s" : ""} into kit. They stay on the keyboard too.` : "Nothing to copy");
  render();
}

function tklBinding() {
  const k = LAYOUT.proxtkl.keys.find((x) => x.id === kv.key);
  return k && k.usage != null ? k.usage : null;
}

function setTklAction(action) {
  const usage = tklBinding();
  if (usage == null) return;
  const list = kcfg().bindings.filter((b) => b.usage !== usage);
  if (action.type !== "default") list.push({ usage, action });
  kcfg().bindings = list;
  keyboardSaver.save(0);
  render();
}

function paintKey(id) {
  const k = LAYOUT.proxtkl.keys.find((x) => x.id === id);
  if (!k || k.usage == null) return;
  const p = lightPreset();
  const z = zoneOf(k.usage);
  if (kv.erase) delete p.keys[z];
  else p.keys[z] = kv.brush;
  keyboardSaver.save(150);
  render();
}

function lightChanged(delay = 0) {
  keyboardSaver.save(delay);
  render();
}

Object.assign(handlers, {
  "keyboard-tab"(el) {
    go("keyboard", el.dataset.tab);
  },
  layer(el) {
    kv.layer = Number(el.dataset.v);
    render();
  },
  "key-pick"(el) {
    kv.key = el.dataset.k;
    stopListening();
    render();
  },
  "kc-pick"(el) {
    kv.pick = el.dataset.v;
    stopListening();
    render();
  },
  "layer-reset"() {
    const layer = kv.layer;
    armDelete(`layer-${layer}`, () => {
      resetLayer(layer);
      toast(`${layer ? `Layer ${layer}` : "Base layer"} is back to the factory keymap`);
    });
  },
  "kc-set"(el) {
    setKeycode(Number(el.dataset.code));
  },
  "kc-listen"() {
    kv.listening = true;
    invoke("pause_hotkeys");
    render();
  },
  "kc-listen-stop"() {
    stopListening();
    render();
  },
  "kc-macro-mode"(el) {
    kv.macroMode = el.dataset.v;
    render();
  },
  "kb-bind"(el) {
    const v = el.dataset.v;
    const cur = kcfg().bindings.find((b) => b.usage === tklBinding());
    if ((cur ? cur.action.type : "default") === v) return;
    setTklAction(v === "keys" ? { type: v, keys: [] } : v === "macro" ? { type: v, id: mcfg().macros[0]?.id || "" } : { type: v });
  },
  "kb-capture"() {
    const usage = tklBinding();
    captureCombo("kbkey", (keys) => {
      const b = kcfg().bindings.find((x) => x.usage === usage);
      if (b) b.action = { type: "keys", keys };
      keyboardSaver.save(0);
    });
  },
  "light-preset"(el) {
    if (el.dataset.id !== kcfg().active) activateLighting(el.dataset.id);
  },
  async "light-new"() {
    const base = lightPreset();
    const p = { ...structuredClone(base), id: newId(), name: `Lighting ${kcfg().presets.length + 1}`, keys: {} };
    kcfg().presets.push(p);
    keyboardSaver.save(0);
    await activateLighting(p.id);
    focusTitle();
  },
  async "light-duplicate"() {
    const p = structuredClone(lightPreset());
    p.id = newId();
    p.name = `${p.name} copy`;
    kcfg().presets.push(p);
    keyboardSaver.save(0);
    await activateLighting(p.id);
    focusTitle();
  },
  "light-delete"() {
    const p = lightPreset();
    armDelete(p.id, async () => {
      const i = kcfg().presets.indexOf(p);
      kcfg().presets.splice(i, 1);
      keyboardSaver.save(0);
      await activateLighting(kcfg().presets[Math.max(0, i - 1)].id);
      toast(`Deleted "${p.name}"`);
    });
  },
  "light-fx"(el) {
    const p = lightPreset();
    const id = Number(el.dataset.v);
    // The current card again keeps its "only the latest press" choice.
    if (fxOf(p).id !== id) p.effect = id;
    if (p.effect !== 0 && p.brightness === 0) p.brightness = 80;
    lightChanged();
  },
  "fx-group"(el) {
    kv.fxGroup = el.dataset.v;
    render();
  },
  "fx-fav"(el, e) {
    // The star sits on the card: don't also pick the effect.
    e.stopPropagation();
    const favs = (kcfg().favorites ||= []);
    const i = favs.indexOf(el.dataset.key);
    if (i >= 0) favs.splice(i, 1);
    else favs.push(el.dataset.key);
    keyboardSaver.save(0);
    render();
  },
  "color-slot"(el) {
    kv.colorSlot = el.dataset.v;
    render();
  },
  "light-hit"(el) {
    sim.hit(el.dataset.k);
  },
  "light-color"(el) {
    lightPreset()[el.dataset.field] = el.dataset.c;
    lightChanged();
  },
  brush(el) {
    kv.brush = el.dataset.c;
    kv.erase = false;
    render();
  },
  "paint-mode"(el) {
    kv.erase = el.dataset.v === "erase";
    render();
  },
  "paint-clear"() {
    lightPreset().keys = {};
    lightChanged();
  },
  "idle-off"(el) {
    kcfg().idleOff = Number(el.dataset.v);
    keyboardSaver.save(0);
    render();
  },
  "nk-import"() {
    importOnboard();
  },
  "nk-restore"() {
    invoke("keyboard_restore", { apply: true });
    toast("Putting your keymap and macros back");
  },
  "nk-restore-skip"() {
    invoke("keyboard_restore", { apply: false });
  },
  "nk-bootloader"() {
    invoke("keyboard_bootloader");
  },
  "nk-firmware-page"() {
    invoke("keyboard_firmware_page");
  },
});

Object.assign(inputFields, {
  "light-name"(el) {
    lightPreset().name = el.value;
    keyboardSaver.save();
    render();
  },
  // Sliders preview live; the NK65 stores the value when the drag ends.
  "light-bright"(el) {
    sliderInput(el, "brightness", "#v-light-bright");
  },
  "light-speed"(el) {
    sliderInput(el, "speed", "#v-light-speed");
  },
  "light-bg"(el) {
    sliderInput(el, "background", "#v-light-bg");
  },
  // Hue and saturation drag live, like the sliders; the change event saves.
  "hs-hue"(el) {
    hsInput(el);
  },
  "hs-sat"(el) {
    hsInput(el);
  },
});

function hsInput(el) {
  const box = el.closest(".cpick");
  const h = Number(box.querySelector(".hue").value);
  const s = Number(box.querySelector(".sat").value) / 100;
  const c = hsHex(h, s);
  lightPreset()[el.dataset.slot] = c;
  box.querySelector(".cbig").style.setProperty("--c", c);
  box.querySelector(".sat").style.setProperty("--hue", hsHex(h, 1));
  invoke("preview_lighting", { preset: lightPreset() });
}

function sliderInput(el, field, label) {
  const v = Number(el.value);
  el.style.setProperty("--p", `${v}%`);
  lightPreset()[field] = v;
  const l = $(label);
  if (l) l.textContent = `${v}%`;
  invoke("preview_lighting", { preset: lightPreset() });
}

Object.assign(changeFields, {
  "light-bright"() {
    lightChanged();
  },
  "light-speed"() {
    lightChanged();
  },
  "light-bg"() {
    lightChanged();
  },
  "hs-hue"() {
    lightChanged();
  },
  "hs-sat"() {
    lightChanged();
  },
  "fx-single"(el) {
    const p = lightPreset();
    const fx = fxOf(p);
    if (fx.single != null) p.effect = el.checked ? fx.single : fx.id;
    lightChanged();
  },
  brush(el) {
    kv.brush = el.value;
    kv.erase = false;
    render();
  },
  "ind-on"(el) {
    lightPreset().indicators[Number(el.dataset.i)] = el.checked ? "#ff4040" : null;
    lightChanged();
  },
  "ind-color"(el) {
    lightPreset().indicators[Number(el.dataset.i)] = el.value;
    lightChanged();
  },
  "kc-macro"(el) {
    if (el.value) bindMacroToKey(el.value);
  },
  "kb-bind-macro"(el) {
    const b = kcfg().bindings.find((x) => x.usage === tklBinding());
    if (b) b.action = { type: "macro", id: el.value };
    keyboardSaver.save(0);
    render();
  },
});

// Paint by click or drag over the keys.
document.addEventListener("pointerdown", (e) => {
  const key = e.target.closest(".paintable [data-k]");
  if (!key || e.button !== 0) return;
  e.preventDefault();
  kv.painting = true;
  paintKey(key.dataset.k);
});
document.addEventListener("pointerover", (e) => {
  if (!kv.painting) return;
  const key = e.target.closest(".paintable [data-k]");
  if (key) paintKey(key.dataset.k);
});
document.addEventListener("pointerup", () => (kv.painting = false));

function stopListening() {
  if (kv.listening) invoke("resume_hotkeys");
  kv.listening = false;
}

// "Press a key" on the NK65 Keys page: the key (plus modifiers) becomes the keycode.
function captureKeycode(e) {
  if (!kv.listening) return false;
  e.preventDefault();
  const usage = CODE_USAGE[e.code];
  if (usage == null) return true;
  if (usage >= 0xe0) {
    // A modifier on its own, once it is clear no other key follows.
    kv.pendingMod = usage;
    return true;
  }
  let mods = 0;
  if (e.ctrlKey) mods |= 1;
  if (e.shiftKey) mods |= 2;
  if (e.altKey) mods |= 4;
  if (e.metaKey) mods |= 8;
  kv.pendingMod = null;
  stopListening();
  setKeycode((mods << 8) | usage);
  return true;
}

function captureKeycodeUp(e) {
  if (!kv.listening || kv.pendingMod == null) return false;
  if (CODE_USAGE[e.code] === kv.pendingMod) {
    const u = kv.pendingMod;
    kv.pendingMod = null;
    stopListening();
    setKeycode(u);
  }
  return true;
}

function leaveKeyboard() {
  stopListening();
  kv.painting = false;
}

function onKeyboardState(payload) {
  if (keyboardSaver.unsaved()) {
    K = { ...payload, config: { ...kcfg(), active: payload.config.active } };
  } else {
    K = payload;
  }
}
