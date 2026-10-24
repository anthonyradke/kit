"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const BANDS = 10;
const GAIN_MAX = 10;
const Q_MIN = 0.2;
const Q_MAX = 10;
const F_MIN = 20;
const F_MAX = 20000;
const DB_RANGE = 12; // graph shows ±12 dB
const SAMPLE_RATE = 48000;
const FILTERS = [
  ["peaking", "Peak", "PK"],
  ["lowShelf", "Low shelf", "LS"],
  ["highShelf", "High shelf", "HS"],
  ["lowPass", "Low pass", "LP"],
  ["highPass", "High pass", "HP"],
];
const AUTO_OFF = [[0, "Never"], [5, "5 min"], [10, "10 min"], [15, "15 min"], [30, "30 min"], [60, "1 hour"]];
const LED = ["Off", "Low", "Medium", "High"];

let S = null; // backend state: { config, headset, audio, configPath }
let autostart = false;
const view = { page: "sound", band: 0, log: null };
let confirmDelete = null;
let installing = false;
let drag = null; // { i, rect } while a graph node is dragged
let graphSize = { w: 0, h: 0 };

const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const cfg = () => S.config;
const preset = () => cfg().presets.find((p) => p.id === cfg().active) || cfg().presets[0];
const band = () => preset().bands[view.band];
const newId = () => Date.now().toString(16) + Math.random().toString(16).slice(2, 8);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, fallback) => (Number.isFinite(Number(v)) && String(v).trim() !== "" ? Number(v) : fallback);
const roundGain = (g) => Math.round(clamp(g, -GAIN_MAX, GAIN_MAX) * 2) / 2;
const roundQ = (q) => Math.round(clamp(q, Q_MIN, Q_MAX) * 1000) / 1000;
const roundFreq = (f) => Math.round(clamp(f, F_MIN, F_MAX));
const hasGain = (b) => b.filter === "peaking" || b.filter === "lowShelf" || b.filter === "highShelf";

const icons = {
  copy: '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>',
  reset: '<svg viewBox="0 0 24 24"><path d="M4 12a8 8 0 1 0 2.3-5.6M4 4v4h4"/></svg>',
  shield: '<svg viewBox="0 0 24 24"><path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"/></svg>',
};

// ---------- DOM patching ----------
// Rendering builds HTML strings, then patches the live DOM in place instead of
// replacing it, so clicks don't flash the page and focus/hover survive.

function morph(root, html) {
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  patchChildren(root, tpl.content);
}

function sameNode(a, b) {
  if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return false;
  return a.nodeType !== 1 || a.getAttribute("data-key") === b.getAttribute("data-key");
}

function patchChildren(parent, next) {
  const olds = [...parent.childNodes];
  const news = [...next.childNodes];
  news.forEach((n, i) => {
    const o = olds[i];
    if (!o) parent.appendChild(n);
    else if (!sameNode(o, n)) parent.replaceChild(n, o);
    else if (o.nodeType !== 1) {
      if (o.nodeValue !== n.nodeValue) o.nodeValue = n.nodeValue;
    } else {
      patchAttrs(o, n);
      patchChildren(o, n);
    }
  });
  for (let i = news.length; i < olds.length; i++) olds[i].remove();
}

function patchAttrs(o, n) {
  for (const { name } of [...o.attributes]) if (!n.hasAttribute(name)) o.removeAttribute(name);
  for (const { name, value } of [...n.attributes]) if (o.getAttribute(name) !== value) o.setAttribute(name, value);
  // Form state lives in properties, not attributes. Leave the focused field alone.
  if (o.tagName === "INPUT" && o !== document.activeElement) {
    if (o.type === "checkbox") o.checked = n.hasAttribute("checked");
    else o.value = n.getAttribute("value") ?? "";
  }
}

// ---------- persistence ----------

let saveTimer = null;
let saving = 0; // saves in flight

function save(delay = 250) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(flushSave, delay);
}

// Local edits the backend hasn't stored yet.
const unsaved = () => saveTimer !== null || saving > 0;

async function flushSave() {
  if (saveTimer === null) return;
  clearTimeout(saveTimer);
  saveTimer = null;
  saving++;
  try {
    const st = await invoke("save_config", { config: cfg() });
    S.headset = st.headset;
    S.audio = st.audio;
  } catch (err) {
    toast(`Couldn't save: ${err}`);
  } finally {
    saving--;
  }
  render();
}

// Live EQ on the headset while dragging (not saved to its memory until the drag ends).
let previewTimer = null;
function preview() {
  if (previewTimer) return;
  previewTimer = setTimeout(() => {
    previewTimer = null;
    invoke("preview_eq", { bands: preset().bands });
  }, 40);
}

function toast(text) {
  const t = $("#toast");
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), 3200);
}

// ---------- formatting ----------

function fmtFreq(f) {
  if (f >= 1000) return `${+(f / 1000).toFixed(f >= 10000 ? 1 : 2)}k`;
  return `${Math.round(f)}`;
}
const fmtGain = (g) => `${g > 0 ? "+" : ""}${g.toFixed(1)}`;
const filterInfo = (f) => FILTERS.find((x) => x[0] === f) || FILTERS[0];

// ---------- EQ math ----------
// RBJ audio-EQ-cookbook biquads, the same filter shapes the headset uses.

function coeffs(b) {
  const w = (2 * Math.PI * b.freq) / SAMPLE_RATE;
  const cos = Math.cos(w);
  const alpha = Math.sin(w) / (2 * b.q);
  const A = Math.pow(10, b.gain / 40);
  const sA = 2 * Math.sqrt(A) * alpha;
  switch (b.filter) {
    case "lowShelf":
      return [A * (A + 1 - (A - 1) * cos + sA), 2 * A * (A - 1 - (A + 1) * cos), A * (A + 1 - (A - 1) * cos - sA),
        A + 1 + (A - 1) * cos + sA, -2 * (A - 1 + (A + 1) * cos), A + 1 + (A - 1) * cos - sA];
    case "highShelf":
      return [A * (A + 1 + (A - 1) * cos + sA), -2 * A * (A - 1 + (A + 1) * cos), A * (A + 1 + (A - 1) * cos - sA),
        A + 1 - (A - 1) * cos + sA, 2 * (A - 1 - (A + 1) * cos), A + 1 - (A - 1) * cos - sA];
    case "lowPass":
      return [(1 - cos) / 2, 1 - cos, (1 - cos) / 2, 1 + alpha, -2 * cos, 1 - alpha];
    case "highPass":
      return [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2, 1 + alpha, -2 * cos, 1 - alpha];
    default:
      return [1 + alpha * A, -2 * cos, 1 - alpha * A, 1 + alpha / A, -2 * cos, 1 - alpha / A];
  }
}

// Response of one band in dB at frequency f.
function bandDb(c, f) {
  const w = (2 * Math.PI * f) / SAMPLE_RATE;
  const c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const [b0, b1, b2, a0, a1, a2] = c;
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2);
  const dr = a0 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
}

// ---------- chrome ----------

function render() {
  document.querySelectorAll(".nav-tab").forEach((b) => b.classList.toggle("on", b.dataset.page === view.page));
  renderDevice();
  const html = view.page === "mic" ? micPage() : view.page === "settings" ? settingsPage() : soundPage();
  morph($("#main"), html);
  watchGraph();
}

function renderDevice() {
  const h = S.headset;
  let dot = "", text;
  if (h.connected) {
    dot = "on";
    text = "Arctis Nova 5";
  } else if (h.present) {
    dot = "sleep";
    text = "Headset off";
  } else {
    text = "Base station not found";
  }
  let bat = "";
  if (h.connected && h.battery != null) {
    const lvl = h.battery <= 15 ? "low" : h.battery <= 35 ? "mid" : "";
    bat = `<span class="bat ${lvl}"><span style="width:${h.battery}%"></span></span><span>${h.battery}%${h.charging ? " ⚡" : ""}</span>`;
  }
  morph($("#device"), `<span class="status-dot ${dot}"></span><span>${esc(text)}</span>${bat}`);
}

// Warn while Sonar's virtual devices still hold the defaults: then our EQ and
// noise suppression aren't what apps hear.
function sonarWarning() {
  const a = S.audio;
  const names = [a.defaultOutput, a.defaultInput].filter(Boolean);
  if (!names.some((n) => /sonar/i.test(n))) return "";
  return `<div class="warn-bar"><span class="grow">SteelSeries Sonar's virtual devices are your Windows default, so apps go through Sonar instead of the headset. Quit SteelSeries GG, then switch back.</span>
    <button class="tbtn" data-act="use-headset">Use headset</button></div>`;
}

// ---------- sound page ----------

function presetTabs() {
  const tabs = cfg().presets
    .map((p) => `<button class="ptab ${p.id === cfg().active ? "sel" : ""}" data-act="preset" data-id="${esc(p.id)}"><span class="pdot"></span>${esc(p.name)}</button>`)
    .join("");
  return `<div class="ptabs">${tabs}<button class="tbtn" data-act="preset-new">${icons.plus}New</button></div>`;
}

function graphSvg(p) {
  const { w: W, h: H } = graphSize;
  if (!W || !H) return "";
  const L = 36, R = 12, T = 10, B = 24;
  const lf0 = Math.log10(F_MIN), lf1 = Math.log10(F_MAX);
  const x = (f) => L + ((Math.log10(f) - lf0) / (lf1 - lf0)) * (W - L - R);
  const y = (db) => T + ((DB_RANGE - clamp(db, -DB_RANGE - 1, DB_RANGE + 1)) / (2 * DB_RANGE)) * (H - T - B);
  const active = p.bands.map((b, i) => ({ b, i, c: coeffs(b) })).filter((e) => e.b.enabled);
  const N = 220;
  const freqs = Array.from({ length: N + 1 }, (_, k) => Math.pow(10, lf0 + ((lf1 - lf0) * k) / N));
  const total = (f) => active.reduce((s, e) => s + bandDb(e.c, f), 0);
  const pts = freqs.map((f) => [x(f), y(total(f))]);
  const line = pts.map(([px, py], k) => `${k ? "L" : "M"}${px.toFixed(1)},${py.toFixed(1)}`).join("");
  const zero = y(0);
  const fill = `${line}L${x(F_MAX).toFixed(1)},${zero}L${x(F_MIN).toFixed(1)},${zero}Z`;

  let grid = "";
  for (const f of [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) {
    grid += `<line class="eq-grid" x1="${x(f)}" y1="${T}" x2="${x(f)}" y2="${H - B}"/>`;
    grid += `<text class="eq-axis" x="${x(f)}" y="${H - 6}" text-anchor="middle">${fmtFreq(f)}</text>`;
  }
  for (let db = -DB_RANGE; db <= DB_RANGE; db += 6) {
    grid += `<line class="eq-grid ${db === 0 ? "zero" : ""}" x1="${L}" y1="${y(db)}" x2="${W - R}" y2="${y(db)}"/>`;
    grid += `<text class="eq-axis" x="${L - 8}" y="${y(db) + 4}" text-anchor="end">${db > 0 ? "+" : ""}${db}</text>`;
  }

  const sel = p.bands[view.band];
  let selCurve = "";
  if (sel.enabled) {
    const c = coeffs(sel);
    selCurve = `<path class="eq-band-curve" d="${freqs.map((f, k) => `${k ? "L" : "M"}${x(f).toFixed(1)},${y(bandDb(c, f)).toFixed(1)}`).join("")}"/>`;
  }
  const nodes = active
    .map(({ b, i }) => {
      const nx = x(b.freq);
      const ny = hasGain(b) ? y(b.gain) : y(total(b.freq));
      return `<g class="eq-node ${i === view.band ? "sel" : ""}" data-node="${i}">
        <circle class="hit" cx="${nx}" cy="${ny}" r="16"/><circle class="dot" cx="${nx}" cy="${ny}" r="9"/>
        <text x="${nx}" y="${ny}">${i + 1}</text></g>`;
    })
    .join("");

  return `<svg class="eq-svg ${drag ? "dragging" : ""}" viewBox="0 0 ${W} ${H}" data-graph="1">
    <defs>
      <linearGradient id="eqStroke" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#8b5cf6"/><stop offset=".55" stop-color="#6d8bff"/><stop offset="1" stop-color="#38bdf8"/></linearGradient>
      <linearGradient id="eqFill" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="rgba(139,92,246,.28)"/><stop offset=".5" stop-color="rgba(109,139,255,.06)"/><stop offset="1" stop-color="rgba(56,189,248,.18)"/></linearGradient>
      <linearGradient id="accentGrad" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#8b5cf6"/><stop offset="1" stop-color="#38bdf8"/></linearGradient>
      <filter id="eqGlow" x="-20%" y="-50%" width="140%" height="200%"><feGaussianBlur stdDeviation="3" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
    </defs>
    ${grid}
    <path class="eq-fill" d="${fill}"/>
    ${selCurve}
    <path class="eq-curve" d="${line}"/>
    ${nodes}
  </svg>`;
}

function bandStrip(p) {
  return p.bands
    .map((b, i) => {
      const cls = [i === view.band ? "sel" : "", b.enabled ? "" : "off"].join(" ");
      const detail = !b.enabled ? "off" : hasGain(b) ? `${fmtGain(b.gain)} dB` : `Q ${b.q}`;
      return `<button class="band ${cls}" data-act="band" data-i="${i}">
        <span class="band-num">${i + 1}</span><span class="band-freq">${fmtFreq(b.freq)}</span>
        <span class="band-gain">${detail}</span><span class="band-type">${filterInfo(b.filter)[2]}</span></button>`;
    })
    .join("");
}

function soundPage() {
  const p = preset();
  const b = band();
  const deleting = confirmDelete === p.id;
  const lastPreset = cfg().presets.length < 2;
  const filterOpts = FILTERS.map(([id, label]) => `<button class="opt ${b.filter === id ? "on" : ""}" data-act="filter" data-filter="${id}">${label}</button>`).join("");
  return `<div class="page devpage" data-key="sound">
    ${presetTabs()}
    <div class="dev-grid">
      <section class="canvas">
        ${sonarWarning()}
        <div class="canvas-head">
          <input class="pname" data-field="preset-name" value="${esc(p.name)}" spellcheck="false" maxlength="40" />
          <div class="pactions">
            <span class="active-mark">On headset</span>
            <button class="tbtn" data-act="preset-duplicate">${icons.copy}Duplicate</button>
            <button class="tbtn danger ${deleting ? "confirm" : ""}" data-act="preset-delete" ${lastPreset ? "disabled" : ""}>${icons.trash}${deleting ? "Confirm" : "Delete"}</button>
          </div>
        </div>
        <div class="eq-wrap" id="eq-wrap">${graphSvg(p)}</div>
        <div class="bands">${bandStrip(p)}</div>
      </section>
      <aside class="panel">
        <div class="sec">
          <div class="sec-head">
            <span class="big-num ${b.enabled ? "" : "off"}">Band ${view.band + 1}</span>
            <label class="switch" title="Band on/off"><input type="checkbox" data-field="band-enabled" ${b.enabled ? "checked" : ""} /><span></span></label>
          </div>
          <div class="opts">${filterOpts}</div>
        </div>
        <div class="sec">
          <div class="fieldrow">
            <label><span class="label">Freq (Hz)</span><input class="field" type="number" data-field="band-freq" value="${b.freq}" min="${F_MIN}" max="${F_MAX}" /></label>
            <label><span class="label">Gain (dB)</span><input class="field" type="number" step="0.5" data-field="band-gain" value="${b.gain}" ${hasGain(b) ? "" : "disabled"} /></label>
            <label><span class="label">Q</span><input class="field" type="number" step="0.1" data-field="band-q" value="${b.q}" /></label>
          </div>
          <p class="hint">Drag a point on the curve to move it; scroll over the graph to change the selected band's width (Q). Double-click empty space to turn on a free band there.</p>
        </div>
        <div class="sec">
          <div class="sec-head"><span class="label">Preset</span></div>
          <button class="tbtn" data-act="band-reset">${icons.reset}Flatten band ${view.band + 1}</button>
          <p class="hint">The EQ runs on the headset itself (10 bands, ±10 dB), so it applies to everything you hear with no software in the way, even when headroom isn't running.</p>
        </div>
      </aside>
    </div>
  </div>`;
}

// The graph is drawn in real pixels, so re-render when its box changes size.
let resizeObserver = null;
function watchGraph() {
  const wrap = $("#eq-wrap");
  if (!wrap) return;
  const r = wrap.getBoundingClientRect();
  if (Math.round(r.width) !== graphSize.w || Math.round(r.height) !== graphSize.h) {
    graphSize = { w: Math.round(r.width), h: Math.round(r.height) };
    requestAnimationFrame(render);
  }
  if (!resizeObserver) {
    resizeObserver = new ResizeObserver(() => {
      const el = $("#eq-wrap");
      if (!el) return;
      const rr = el.getBoundingClientRect();
      if (Math.round(rr.width) !== graphSize.w || Math.round(rr.height) !== graphSize.h) render();
    });
  }
  resizeObserver.disconnect();
  resizeObserver.observe(wrap);
}

// Pixel position in the graph -> frequency / gain.
function graphToValues(clientX, clientY, rect) {
  const L = 36, R = 12, T = 10, B = 24;
  const lf0 = Math.log10(F_MIN), lf1 = Math.log10(F_MAX);
  const fx = clamp((clientX - rect.left - L) / (rect.width - L - R), 0, 1);
  const fy = clamp((clientY - rect.top - T) / (rect.height - T - B), 0, 1);
  return { freq: roundFreq(Math.pow(10, lf0 + fx * (lf1 - lf0))), gain: roundGain(DB_RANGE - fy * 2 * DB_RANGE) };
}

// ---------- mic page ----------

function slider(field, value, max, disabled = false) {
  const pct = (value / max) * 100;
  return `<input class="slider" type="range" min="0" max="${max}" step="1" value="${value}" data-field="${field}" style="--p:${pct}%" ${disabled ? "disabled" : ""} />`;
}

function micPage() {
  const m = cfg().mic;
  const h = cfg().headset;
  const a = S.audio;
  let status;
  if (installing) {
    status = `<div class="state-line"><span class="status-dot sleep"></span>Installing… approve the Windows prompt.</div>`;
  } else if (!a.input) {
    status = `<div class="state-line warn"><span class="status-dot"></span>The Arctis Nova 5 microphone isn't connected.</div>`;
  } else if (a.apoInstalled) {
    status = `<div class="state-line"><span class="status-dot ${m.denoise ? "on" : ""}"></span>${m.denoise ? "Active on" : "Installed on"} ${esc(a.input)}
      <span class="grow"></span><button class="tbtn danger" data-act="apo-uninstall">Remove</button></div>`;
  } else {
    status = `<div class="state-line warn"><span class="status-dot sleep"></span>Not installed yet. Windows will ask for permission once.
      <button class="btn-grad" data-act="apo-install">${icons.shield}Install</button></div>`;
  }
  return `<div class="page spage" data-key="mic"><div class="spage-inner">
    ${sonarWarning()}
    <h1 class="h1">Microphone</h1>

    <div class="srow col">
      <div class="srow-head">
        <div><div class="stitle">Noise suppression</div>
        <p class="hint">Removes keyboard, fan and room noise from the headset mic before any app hears it. It runs inside Windows' audio engine on the real "Headset Microphone", so Discord, games and everything else get the clean mic automatically: no virtual devices to pick.</p></div>
        <label class="switch"><input type="checkbox" data-field="denoise" ${m.denoise ? "checked" : ""} /><span></span></label>
      </div>
      ${status}
    </div>
    <div class="srow col">
      <div class="srow-head"><div class="stitle">Strength</div><span class="sval" id="v-strength">${m.strength}%</span></div>
      ${slider("strength", m.strength, 100, !m.denoise)}
      <div class="scale"><span>Light</span><span>Full</span></div>
      <p class="hint">Below 100% some of the original sound is mixed back in, which keeps your voice more natural. Sonar's setting was 90%.</p>
    </div>

    <span class="label block">Headset</span>
    <div class="srow col">
      <div class="srow-head"><div class="stitle">Mic volume</div><span class="sval" id="v-micVolume">${h.micVolume}</span></div>
      ${slider("micVolume", h.micVolume, 15)}
      <p class="hint">Hardware gain on the headset, applied before noise suppression.</p>
    </div>
    <div class="srow col">
      <div class="srow-head"><div class="stitle">Sidetone</div><span class="sval" id="v-sidetone">${h.sidetone || "Off"}</span></div>
      ${slider("sidetone", h.sidetone, 10)}
      <p class="hint">How much of your own voice you hear in the headphones.</p>
    </div>
    <div class="srow">
      <div><div class="stitle">Mute light</div><p class="hint">Brightness of the red light on the mic when muted.</p></div>
      <div class="toggles">${LED.map((l, i) => `<button class="${h.micLed === i ? "on" : ""}" data-act="led" data-v="${i}">${l}</button>`).join("")}</div>
    </div>
  </div></div>`;
}

// ---------- settings page ----------

function settingsPage() {
  const c = cfg();
  const h = S.headset;
  const a = S.audio;
  const line = (label, name, def) => {
    if (!name) return `<div class="dev-line"><b>${label}</b><span>Not connected</span></div>`;
    const isDef = def === name;
    return `<div class="dev-line"><b>${label}</b><span>${esc(name)}</span>${isDef ? '<span class="ok">default</span>' : `<span class="muted">Windows default: ${esc(def || "none")}</span>`}</div>`;
  };
  return `<div class="page spage" data-key="settings"><div class="spage-inner">
    ${sonarWarning()}
    <h1 class="h1">Settings</h1>

    <div class="srow col">
      <div class="srow-head">
        <div><div class="stitle">Keep the headset as the default device</div>
        <p class="hint">Whenever the Nova 5 connects, make it the Windows default output and mic (including for calls). Picking another device yourself still works.</p></div>
        <label class="switch"><input type="checkbox" data-field="keepDefault" ${c.keepDefault ? "checked" : ""} /><span></span></label>
      </div>
      ${line("Output", a.output, a.defaultOutput)}
      ${line("Mic", a.input, a.defaultInput)}
      ${a.output && !a.headsetIsDefault ? `<div><button class="tbtn" data-act="use-headset">Use headset now</button></div>` : ""}
      ${a.error ? `<div class="state-line bad">${esc(a.error)}</div>` : ""}
    </div>
    <div class="srow">
      <div><div class="stitle">Turn off when idle</div><p class="hint">The headset switches itself off after this long without sound.</p></div>
      <div class="toggles">${AUTO_OFF.map(([v, l]) => `<button class="${c.headset.autoOff === v ? "on" : ""}" data-act="auto-off" data-v="${v}">${l}</button>`).join("")}</div>
    </div>
    <div class="srow">
      <div><div class="stitle">Volume limiter</div><p class="hint">Caps the headset's maximum loudness to protect your hearing.</p></div>
      <label class="switch"><input type="checkbox" data-field="volumeLimiter" ${c.headset.volumeLimiter ? "checked" : ""} /><span></span></label>
    </div>
    <div class="srow">
      <div><div class="stitle">Launch at startup</div><p class="hint">Start headroom in the tray when you sign in to Windows.</p></div>
      <label class="switch"><input type="checkbox" data-field="autostart" ${autostart ? "checked" : ""} /><span></span></label>
    </div>

    <span class="label block">Headset</span>
    <dl class="kv">
      <dt>Base station</dt><dd>${h.present ? "Connected" : "Not found"}</dd>
      <dt>Headset</dt><dd>${h.connected ? "On" : "Off or out of range"}</dd>
      <dt>Battery</dt><dd>${h.connected && h.battery != null ? `${h.battery}%${h.charging ? " (charging)" : ""}` : "—"}</dd>
      ${h.error && !h.present ? `<dt>Error</dt><dd>${esc(h.error)}</dd>` : ""}
    </dl>

    <span class="label block">About</span>
    <dl class="kv"><dt>Config file</dt><dd class="path">${esc(S.configPath)}</dd></dl>
    <p class="hint">Closing this window keeps headroom running in the tray. Use Quit from the tray menu to exit. Keep SteelSeries GG closed (or uninstall it): it changes the same headset settings.</p>
    <div style="margin-top:10px"><button class="tbtn" data-act="show-log">${view.log === null ? "Show" : "Refresh"} noise suppression log</button></div>
    ${view.log !== null ? `<div class="logbox">${esc(view.log || "(empty: the audio engine hasn't loaded noise suppression yet)")}</div>` : ""}
  </div></div>`;
}

// ---------- actions ----------

async function activate(id) {
  // Send pending edits first, or the state that comes back would drop them.
  await flushSave();
  S = await invoke("activate_preset", { id });
  render();
}

function armDelete(id, onConfirm) {
  if (confirmDelete === id) {
    confirmDelete = null;
    onConfirm();
  } else {
    confirmDelete = id;
    setTimeout(() => {
      if (confirmDelete === id) {
        confirmDelete = null;
        render();
      }
    }, 3000);
  }
  render();
}

function focusTitle() {
  const input = $(".pname");
  input?.focus();
  input?.select();
}

async function setApo(install) {
  await flushSave();
  installing = true;
  render();
  try {
    S = await invoke("set_apo_installed", { install });
    toast(install ? "Noise suppression installed" : "Noise suppression removed");
  } catch (err) {
    toast(String(err));
  } finally {
    installing = false;
    render();
  }
}

// A band edit: update the headset live, save shortly after.
function bandChanged(delay = 250) {
  preview();
  save(delay);
  render();
}

const handlers = {
  "win-min"() {
    appWindow.minimize();
  },
  // Same as a native close: the webview is destroyed and the tray keeps running.
  "win-close"() {
    appWindow.close();
  },
  page(el) {
    view.page = el.dataset.page;
    render();
  },
  preset(el) {
    if (el.dataset.id !== cfg().active) activate(el.dataset.id);
  },
  async "preset-new"() {
    const p = {
      id: newId(),
      name: `Preset ${cfg().presets.length + 1}`,
      bands: [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((freq) => ({ enabled: true, filter: "peaking", freq, gain: 0, q: 1.414 })),
    };
    cfg().presets.push(p);
    save(0);
    await activate(p.id);
    focusTitle();
  },
  async "preset-duplicate"() {
    const p = structuredClone(preset());
    p.id = newId();
    p.name = `${p.name} copy`;
    cfg().presets.push(p);
    save(0);
    await activate(p.id);
    focusTitle();
  },
  "preset-delete"() {
    const p = preset();
    armDelete(p.id, async () => {
      const i = cfg().presets.indexOf(p);
      cfg().presets.splice(i, 1);
      const next = cfg().presets[Math.max(0, i - 1)];
      save(0);
      await activate(next.id);
    });
  },
  band(el) {
    view.band = Number(el.dataset.i);
    render();
  },
  filter(el) {
    const b = band();
    b.filter = el.dataset.filter;
    b.enabled = true;
    bandChanged(0);
  },
  "band-reset"() {
    Object.assign(band(), { filter: "peaking", gain: 0, q: 1.414 });
    bandChanged(0);
  },
  led(el) {
    cfg().headset.micLed = Number(el.dataset.v);
    save(0);
    render();
  },
  "auto-off"(el) {
    cfg().headset.autoOff = Number(el.dataset.v);
    save(0);
    render();
  },
  "use-headset"() {
    invoke("use_headset");
    toast("Switching Windows to the headset…");
  },
  "apo-install"() {
    setApo(true);
  },
  "apo-uninstall"() {
    setApo(false);
  },
  async "show-log"() {
    view.log = await invoke("apo_log");
    render();
  },
};

// ---------- events ----------

document.addEventListener("click", (e) => {
  const el = e.target.closest("[data-act]");
  if (!el || el.disabled) return;
  const fn = handlers[el.dataset.act];
  if (fn) fn(el, e);
});

document.addEventListener("input", (e) => {
  const el = e.target;
  const f = el.dataset.field;
  if (!f) return;
  if (f === "preset-name") {
    preset().name = el.value;
    save();
    render();
  } else if (el.type === "range") {
    const v = Number(el.value);
    el.style.setProperty("--p", `${(v / Number(el.max)) * 100}%`);
    const target = f === "strength" ? cfg().mic : cfg().headset;
    target[f] = v;
    const label = $(`#v-${f}`);
    if (label) label.textContent = f === "strength" ? `${v}%` : f === "sidetone" && !v ? "Off" : `${v}`;
    save(300);
  }
});

// Numbers are committed on change (blur / Enter), not per keystroke.
document.addEventListener("change", async (e) => {
  const el = e.target;
  const f = el.dataset.field;
  if (!f) return;
  const b = view.page === "sound" ? band() : null;
  if (f === "band-enabled") {
    b.enabled = el.checked;
    bandChanged(0);
  } else if (f === "band-freq") {
    b.freq = roundFreq(num(el.value, b.freq));
    bandChanged(0);
  } else if (f === "band-gain") {
    b.gain = roundGain(num(el.value, b.gain));
    bandChanged(0);
  } else if (f === "band-q") {
    b.q = roundQ(num(el.value, b.q));
    bandChanged(0);
  } else if (f === "denoise") {
    cfg().mic.denoise = el.checked;
    save(0);
    render();
  } else if (f === "keepDefault") {
    cfg().keepDefault = el.checked;
    save(0);
    render();
  } else if (f === "volumeLimiter") {
    cfg().headset.volumeLimiter = el.checked;
    save(0);
    render();
  } else if (f === "autostart") {
    try {
      autostart = await invoke("set_autostart", { enabled: el.checked });
      toast(autostart ? "headroom will start with Windows" : "Startup disabled");
    } catch (err) {
      toast(`Couldn't change startup: ${err}`);
    }
    el.checked = autostart;
  }
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && e.target.matches("input")) e.target.blur();
});

// ---------- graph dragging ----------

document.addEventListener("pointerdown", (e) => {
  const node = e.target.closest("[data-node]");
  if (!node || e.button !== 0) return;
  e.preventDefault();
  view.band = Number(node.dataset.node);
  drag = { i: view.band, rect: $(".eq-svg").getBoundingClientRect() };
  render();
});

document.addEventListener("pointermove", (e) => {
  if (!drag) return;
  const b = preset().bands[drag.i];
  const v = graphToValues(e.clientX, e.clientY, drag.rect);
  b.freq = v.freq;
  if (hasGain(b)) b.gain = v.gain;
  preview();
  render();
});

document.addEventListener("pointerup", () => {
  if (!drag) return;
  drag = null;
  save(0);
  render();
});

document.addEventListener(
  "wheel",
  (e) => {
    if (!e.target.closest(".eq-svg")) return;
    e.preventDefault();
    const b = band();
    if (!b.enabled) return;
    b.q = roundQ(b.q * (e.deltaY < 0 ? 1.12 : 1 / 1.12));
    bandChanged(400);
  },
  { passive: false },
);

document.addEventListener("dblclick", (e) => {
  const svg = e.target.closest(".eq-svg");
  if (!svg || e.target.closest("[data-node]")) return;
  const i = preset().bands.findIndex((b) => !b.enabled);
  if (i < 0) {
    toast("All 10 bands are in use. Turn one off first.");
    return;
  }
  const v = graphToValues(e.clientX, e.clientY, svg.getBoundingClientRect());
  Object.assign(preset().bands[i], { enabled: true, filter: "peaking", freq: v.freq, gain: v.gain, q: 1 });
  view.band = i;
  bandChanged(0);
});

// ---------- boot ----------

(async () => {
  S = await invoke("get_state");
  autostart = await invoke("get_autostart");
  render();

  await listen("state", (e) => {
    if (unsaved() || drag) {
      // Keep edits the backend doesn't have yet; take only the preset switch.
      const config = { ...cfg(), active: e.payload.config.active, mic: { ...cfg().mic, denoise: e.payload.config.mic.denoise } };
      S = { ...e.payload, config };
    } else {
      S = e.payload;
    }
    render();
  });
  await listen("headset", (e) => {
    S.headset = e.payload;
    renderDevice();
    if (view.page === "settings") render();
  });
  await listen("audio", (e) => {
    S.audio = e.payload;
    render();
  });
})();
