// Fake Tauri backend for scripts/ui-preview.sh: the same state shapes the Rust commands return.
(() => {
  const q = new URLSearchParams(location.search);
  const kb = q.get("kb") || "nk65";
  const color = q.get("color") || "white";
  const id = () => Math.random().toString(16).slice(2, 10);
  const key = (name, scan, ext = false) => ({ vk: 0, scan, ext, name });
  const M = {
    config: {
      active: "p1",
      profiles: [
        { id: "p1", name: "Default", dpiStages: [400, 800, 1600], dpiIndex: 1, reportRate: 1000, back: { type: "default" }, forward: { type: "default" }, hotkey: null },
        { id: "p2", name: "Valheim", dpiStages: [800, 1600], dpiIndex: 0, reportRate: 1000, back: { type: "macro", id: "m1" }, forward: { type: "dpiShift", dpi: 400 }, hotkey: "Ctrl+Alt+F2" },
        { id: "p3", name: "Wiz", dpiStages: [800], dpiIndex: 0, reportRate: 500, back: { type: "default" }, forward: { type: "default" }, hotkey: null },
      ],
      macros: [
        { id: "m1", name: "678 - valheim", mode: "once", repeat: 1, steps: [{ type: "key", key: key("6", 7), down: true }, { type: "delay", ms: 40 }, { type: "key", key: key("6", 7), down: false }, { type: "key", key: key("7", 8), down: true }, { type: "key", key: key("7", 8), down: false }] },
        { id: "m2", name: "e spam", mode: "whileHeld", repeat: 1, steps: [{ type: "key", key: key("E", 0x12), down: true }, { type: "delay", ms: 30 }, { type: "key", key: key("E", 0x12), down: false }, { type: "delay", ms: 30 }] },
        { id: "m3", name: "shift spam", mode: "toggle", repeat: 1, steps: [] },
      ],
      settings: { nextProfileHotkey: null },
    },
    dpiStage: 1,
    device: { present: true, connected: true, name: "PRO X Wireless", battery: 72, charging: false, dpi: 800, rate: 1000, error: null },
    hotkeyErrors: [],
    configPath: "C:\\Users\\you\\AppData\\Roaming\\com.anthonyradke.kit\\mouse.json",
  };
  const bands = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((freq, i) => ({ enabled: true, filter: "peaking", freq, gain: [3, 2, 0, -1, 0, 0, 1, 2, 1, 0][i], q: 1.414 }));
  const H = {
    config: { presets: [{ id: "h1", name: "Main", bands }, { id: "h2", name: "Footsteps", bands }], active: "h1", headset: { sidetone: 1, micVolume: 15, micLed: 3, volumeLimiter: true, autoOff: 30 }, mic: { denoise: true, strength: 90 }, keepDefault: true },
    headset: { present: true, connected: true, battery: 64, charging: false, error: null },
    audio: { output: "Headphones (Arctis Nova 5)", input: "Headset Microphone (Arctis Nova 5)", defaultOutput: "Headphones (Arctis Nova 5)", defaultInput: "Headset Microphone (Arctis Nova 5)", headsetIsDefault: true, apoInstalled: true, error: null },
    configPath: "C:\\Users\\you\\AppData\\Roaming\\com.anthonyradke.kit\\headset.json",
  };
  // NK65 keymap: 4 layers x 5 rows x 15 cols.
  const km = new Array(4 * 75).fill(1);
  const set = (l, r, c, v) => (km[(l * 5 + r) * 15 + c] = v);
  const row0 = [0x29, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e, 0x2a, 0x49];
  row0.forEach((v, c) => set(0, 0, c, v));
  [0x2b, 0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30, 0, 0x4c].forEach((v, c) => set(0, 1, c, v));
  [0x39, 0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, 0x31, 0x28, 0x4b].forEach((v, c) => set(0, 2, c, v));
  [0xe1, 0, 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0xe5, 0x52, 0x4e].forEach((v, c) => set(0, 3, c, v));
  [0xe0, 0xe3, 0xe2, 0, 0, 0, 0x2c, 0, 0, 0xe6, 0x5221, 0xe4, 0x50, 0x51, 0x4f].forEach((v, c) => set(0, 4, c, v));
  for (let c = 1; c <= 12; c++) set(1, 0, c, 0x39 + c);
  set(1, 0, 0, 0x35); set(1, 0, 13, 0x4c); set(1, 1, 1, 0x7e00); set(1, 1, 2, 0x7e01); set(1, 1, 3, 0x7e02); set(1, 2, 1, 0xa8); set(1, 2, 2, 0xaa); set(1, 2, 3, 0xa9); set(1, 2, 4, 0xae);
  set(0, 0, 14, 0x7701);
  const nkMacros = Array.from({ length: 16 }, () => []);
  nkMacros[0] = [{ type: "text", text: "gg wp" }, { type: "tap", code: 0x28 }];
  nkMacros[1] = [{ type: "down", code: 0xe0 }, { type: "tap", code: 0x06 }, { type: "up", code: 0xe0 }];
  const presetsFor = {
    nk65: [["Milkshake", 2, "#ffb3d1", "#9fe8cf"], ["Vanilla", 1, "#fff1dc", "#fff1dc"], ["Rainbow", 6, "#ffffff", "#ffffff"], ["Dark", 0, "#ffffff", "#ffffff"]],
    proxtkl: [["White", 1, "#ffffff", "#ffffff"], ["Breathe", 10, "#4aa8ff", "#4aa8ff"], ["Cycle", 3, "#ffffff", "#ffffff"], ["Dark", 0, "#ffffff", "#ffffff"]],
  };
  const presets = (presetsFor[kb] || presetsFor.nk65).map(([name, effect, c1, c2], i) => ({ id: `l${i}`, name, effect, color1: c1, color2: c2, speed: 50, brightness: i === 3 ? 0 : 80, keys: {}, indicators: [kb === "nk65" ? "#ff4040" : null, null, null, null] }));
  if (kb === "proxtkl") Object.assign(presets[0].keys, { 23: "#ff4040", 4: "#ff4040", 19: "#ff4040", 1: "#ff4040", 38: "#38bdf8" });
  const K = {
    config: { model: kb === "proxtkl" ? "proxtkl" : "nk65", presets, active: "l0", idleOff: 300, bindings: kb === "proxtkl" ? [{ usage: 0x3a, action: { type: "macro", id: "m1" } }] : [], kitKeys: [null, null, null, null, null, null, null, null, null, null, null, null], onboard: {} },
    status: kb === "proxtkl"
      ? { present: true, connected: true, name: "PRO X TKL", battery: 81, charging: false, lighting: "lightsync", protocol: null, layers: 0, rows: 0, cols: 0, keymap: [], macros: [], macroSpace: 0, zones: Array.from({ length: 105 }, (_, i) => i + 1).concat([104, 105, 106, 107, 108, 109, 110, 111]), effects: [0, 1, 3, 4, 10, 11, 0x15, 0x16], lightsAsleep: false, error: null }
      : { present: true, connected: true, name: "NK65", battery: null, charging: false, lighting: "wilba", protocol: 12, layers: 4, rows: 5, cols: 15, keymap: km, macros: nkMacros, macroSpace: 896, zones: [], effects: [], lightsAsleep: false, error: null },
    configPath: "C:\\Users\\you\\AppData\\Roaming\\com.anthonyradke.kit\\keyboard.json",
  };
  let A = {
    setupDone: !q.get("setup"),
    mouse: { color },
    keyboard: kb === "none" ? null : { model: kb === "proxtkl" ? "proxtkl" : "nk65", color: kb === "proxtkl" ? "black" : "white" },
    headset: { color },
  };
  const handlers = {
    mouse_state: () => M, headset_state: () => H, keyboard_state: () => K, app_config: () => A,
    get_autostart: () => false, set_autostart: ({ enabled }) => enabled,
    update_state: () => ({ current: "0.3.0", available: null, notes: null, checking: false, checked: true, progress: null, error: null }),
    detect_devices: () => new Promise((r) => setTimeout(() => r({ mouse: true, headset: true, keyboard: kb === "none" ? null : kb === "proxtkl" ? "proxtkl" : "nk65" }), 600)),
    vendor_apps: () => (q.get("ghub") ? ["Logitech G HUB"] : []),
    app_save: ({ config }) => (A = config),
    keyboard_save: ({ config }) => ((K.config = config), K),
    mouse_save: ({ config }) => ((M.config = config), M),
    headset_save: ({ config }) => ((H.config = config), H),
    activate_lighting: ({ id }) => ((K.config.active = id), K),
    activate_profile: ({ id }) => ((M.config.active = id), M),
    activate_preset: ({ id }) => ((H.config.active = id), H),
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => (handlers[cmd] ? structuredClone(await handlers[cmd](args || {})) : undefined) },
    event: { listen: async () => () => {} },
    window: { getCurrentWindow: () => ({ minimize() {}, close() {} }) },
  };
})();
