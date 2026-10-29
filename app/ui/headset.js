"use strict";
// Headset pages: Sound (onboard EQ presets), Mic (noise suppression and mic settings), Device.

const GAIN_MAX = 10;
const Q_MIN = 0.2;
const Q_MAX = 10;
const F_MIN = 20;
const F_MAX = 20000;
const DB_RANGE = 12; // graph shows ±12 dB
const SAMPLE_RATE = 48000;
const BASS_FREQ = 60; // corner of the bass slider's low shelf
const FILTERS = [
  ["peaking", "Peak", "PK"],
  ["lowShelf", "Low shelf", "LS"],
  ["highShelf", "High shelf", "HS"],
  ["lowPass", "Low pass", "LP"],
  ["highPass", "High pass", "HP"],
];
const AUTO_OFF = [[0, "Never"], [5, "5 min"], [10, "10 min"], [15, "15 min"], [30, "30 min"], [60, "1 hour"]];
const LED = ["Off", "Low", "Medium", "High"];

let H = null; // backend state: { config, headset, audio, configPath }
const hv = { band: 0, log: null };
let installing = false;
let drag = null; // { i, rect } while a graph node is dragged
let graphSize = { w: 0, h: 0 };

const hcfg = () => H.config;
const preset = () => hcfg().presets.find((p) => p.id === hcfg().active) || hcfg().presets[0];
const band = () => preset().bands[hv.band];
const num = (v, fallback) => (Number.isFinite(Number(v)) && String(v).trim() !== "" ? Number(v) : fallback);
const roundGain = (g) => Math.round(clamp(g, -GAIN_MAX, GAIN_MAX) * 2) / 2;
const roundQ = (q) => Math.round(clamp(q, Q_MIN, Q_MAX) * 1000) / 1000;
const roundFreq = (f) => Math.round(clamp(f, F_MIN, F_MAX));
const hasGain = (b) => b.filter === "peaking" || b.filter === "lowShelf" || b.filter === "highShelf";

const headsetSaver = makeSaver("headset_save", hcfg, (st) => {
  H.headset = st.headset;
  H.audio = st.audio;
});

// Live EQ on the headset while dragging (not saved to its memory until the drag ends).
let previewTimer = null;
function preview() {
  if (previewTimer) return;
  previewTimer = setTimeout(() => {
    previewTimer = null;
    invoke("preview_eq", { bands: preset().bands, bass: hcfg().headset.bass });
  }, 40);
}

function fmtFreq(f) {
  if (f >= 1000) return `${+(f / 1000).toFixed(f >= 10000 ? 1 : 2)}k`;
  return `${Math.round(f)}`;
}
const fmtBass = (g) => (g ? `${fmtGain(g)}<small> dB</small>` : "Off");
const fmtGain = (g) => `${g > 0 ? "+" : g < 0 ? "−" : ""}${Math.abs(g).toFixed(1)}`;
const filterInfo = (f) => FILTERS.find((x) => x[0] === f) || FILTERS[0];

function headsetStatus() {
  const h = H.headset;
  if (h.connected) return { text: "On", live: true };
  if (h.present) return { text: "Off" };
  return { text: "Not found", hint: "Plug in the Nova 5 base station.", bad: true };
}

// Sonar's virtual devices hold the Windows default: apps then go through Sonar,
// not the headset, so our EQ and noise suppression aren't what they hear.
function sonarHolds() {
  const a = H.audio;
  return [a.defaultOutput, a.defaultInput].filter(Boolean).some((n) => /sonar/i.test(n));
}

function sonarNotice() {
  if (!sonarHolds()) return "";
  return `<div class="notice"><span class="grow">SteelSeries Sonar's virtual devices are the Windows default, so apps go through Sonar instead of the headset. Quit SteelSeries GG, then switch back.</span>
    <button class="btn quiet" data-act="use-headset">Use headset</button></div>`;
}

function defaultCell(key, span = 1) {
  const a = H.audio;
  if (sonarHolds()) return cell(key, "Windows default", "Sonar", { span, cls: "text warn" });
  if (!a.output) return cell(key, "Windows default", "Not connected", { span, cls: "text dim" });
  return cell(key, "Windows default", a.headsetIsDefault ? "This headset" : esc(a.defaultOutput || "None"), { span, cls: `text ${a.headsetIsDefault ? "" : "warn"}` });
}

function micCell(key) {
  const m = hcfg().mic;
  if (!H.audio.apoInstalled) return cell(key, "Mic", "Not set up", { cls: "text dim" });
  return cell(key, "Mic", m.denoise ? `Clean <small>${m.strength}%</small>` : "Unfiltered", { cls: `text ${m.denoise ? "" : "dim"}` });
}

function statusCellH(key) {
  const s = headsetStatus();
  const mark = s.live ? `<span class="live-mark">${s.text}</span>` : `<span class="off-mark">${s.text}</span>`;
  return cell(key, "Status", mark, { cls: `text ${s.bad ? "bad" : ""}` });
}

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

// The bands the headset gets once the bass slider is applied. Mirrors
// `Preset::with_bass` in config.rs: a low shelf in the first band that is off
// or at 0 dB, else added to the preset's own low shelf, else nothing.
function withBass(bands, bass) {
  if (!bass) return bands;
  const out = bands.map((b) => ({ ...b }));
  const idle = out.find((b) => !b.enabled || (b.gain === 0 && hasGain(b)));
  if (idle) {
    Object.assign(idle, { enabled: true, filter: "lowShelf", freq: BASS_FREQ, gain: bass, q: 0.707 });
    return out;
  }
  const shelf = out.find((b) => b.filter === "lowShelf");
  if (shelf) shelf.gain = clamp(shelf.gain + bass, -GAIN_MAX, GAIN_MAX);
  return out;
}

// False when the preset uses every band and has no low shelf to add bass to.
const bassFits = (p) => p.bands.some((b) => !b.enabled || (b.gain === 0 && hasGain(b)) || b.filter === "lowShelf");

// ---------- sound page ----------

function presetStrip() {
  const tabs = hcfg().presets
    .map((p) => {
      const on = p.id === hcfg().active;
      return `<button class="stab ${on ? `sel ${H.headset.connected ? "live" : "stored"}` : ""}" data-act="preset" data-id="${esc(p.id)}" ${on ? 'title="Stored on the headset"' : ""}><span class="pdot"></span>${esc(p.name)}</button>`;
    })
    .join("");
  return `<nav class="strip" aria-label="EQ presets">${tabs}<button class="tbtn" data-act="preset-new">${icons.plus}New preset</button></nav>`;
}

const GRAPH = { L: 38, R: 14, T: 12, B: 26 };

function graphSvg(p) {
  const { w: W, h: Ht } = graphSize;
  if (!W || !Ht) return "";
  const { L, R, T, B } = GRAPH;
  const lf0 = Math.log10(F_MIN), lf1 = Math.log10(F_MAX);
  const x = (f) => L + ((Math.log10(f) - lf0) / (lf1 - lf0)) * (W - L - R);
  const y = (db) => T + ((DB_RANGE - clamp(db, -DB_RANGE - 1, DB_RANGE + 1)) / (2 * DB_RANGE)) * (Ht - T - B);
  const active = p.bands.map((b, i) => ({ b, i })).filter((e) => e.b.enabled);
  const heard = withBass(p.bands, hcfg().headset.bass).filter((b) => b.enabled).map(coeffs);
  const N = 220;
  const freqs = Array.from({ length: N + 1 }, (_, k) => Math.pow(10, lf0 + ((lf1 - lf0) * k) / N));
  const total = (f) => heard.reduce((s, c) => s + bandDb(c, f), 0);
  const pts = freqs.map((f) => [x(f), y(total(f))]);
  const line = pts.map(([px, py], k) => `${k ? "L" : "M"}${px.toFixed(1)},${py.toFixed(1)}`).join("");
  const zero = y(0);
  const fill = `${line}L${x(F_MAX).toFixed(1)},${zero}L${x(F_MIN).toFixed(1)},${zero}Z`;

  let grid = "";
  for (const f of [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) {
    grid += `<line class="eq-grid" x1="${x(f)}" y1="${T}" x2="${x(f)}" y2="${Ht - B}"/>`;
    grid += `<text class="eq-axis" x="${x(f)}" y="${Ht - 7}" text-anchor="middle">${fmtFreq(f)}</text>`;
  }
  for (let db = -DB_RANGE; db <= DB_RANGE; db += 6) {
    grid += `<line class="eq-grid ${db === 0 ? "zero" : ""}" x1="${L}" y1="${y(db)}" x2="${W - R}" y2="${y(db)}"/>`;
    grid += `<text class="eq-axis" x="${L - 8}" y="${y(db) + 4}" text-anchor="end">${db > 0 ? "+" : db < 0 ? "−" : ""}${Math.abs(db)}</text>`;
  }

  const sel = p.bands[hv.band];
  let selCurve = "";
  if (sel.enabled) {
    const c = coeffs(sel);
    selCurve = `<path class="eq-band-curve" d="${freqs.map((f, k) => `${k ? "L" : "M"}${x(f).toFixed(1)},${y(bandDb(c, f)).toFixed(1)}`).join("")}"/>`;
  }
  const nodes = active
    .map(({ b, i }) => {
      const nx = x(b.freq);
      const ny = hasGain(b) ? y(b.gain) : y(total(b.freq));
      return `<g class="eq-node ${i === hv.band ? "sel" : ""}" data-node="${i}">
        <circle class="hit" cx="${nx}" cy="${ny}" r="16"/><circle class="dot" cx="${nx}" cy="${ny}" r="9"/>
        <text x="${nx}" y="${ny}">${i + 1}</text></g>`;
    })
    .join("");

  return `<svg class="eq-svg ${drag ? "dragging" : ""}" viewBox="0 0 ${W} ${Ht}" data-graph="1" role="img" aria-label="EQ curve">
    ${grid}
    <path class="eq-fill" d="${fill}"/>
    ${selCurve}
    <path class="eq-curve" d="${line}"/>
    ${nodes}
  </svg>`;
}

function bandPlate(p) {
  const cells = p.bands.map((b, i) => {
    const cls = ["cell", "band", i === hv.band ? "sel" : "", b.enabled ? "" : "off"].join(" ");
    const detail = !b.enabled ? "Off" : hasGain(b) ? `${fmtGain(b.gain)}<small> dB</small>` : `Q ${b.q}`;
    return {
      span: 1,
      html: (end) => `<button class="${cls} ${end}" data-act="band" data-i="${i}" aria-label="Band ${i + 1}">
        <div class="cap">${i + 1} · <span class="btype">${filterInfo(b.filter)[2]}</span></div>
        <div class="val">${fmtFreq(b.freq)}</div><div class="bsub">${detail}</div></button>`,
    };
  });
  return plate(cells, 10, "bands");
}

function soundPage() {
  const p = preset();
  const b = band();
  const h = H.headset;
  const deleting = confirmDelete === p.id;
  const lastPreset = hcfg().presets.length < 2;
  const used = p.bands.filter((x) => x.enabled).length;
  const bass = hcfg().headset.bass;
  const filterOpts = FILTERS.map(([id, label]) => `<button class="choice ${b.filter === id ? "on" : ""}" data-act="filter" data-filter="${id}" role="radio" aria-checked="${b.filter === id}">${label}</button>`).join("");
  return `<div class="page devpage" data-key="headset-sound">
    ${presetStrip()}
    <div class="work">
      <section class="canvas">
        ${sonarNotice()}
        <div class="canvas-head">
          <input class="pname" data-field="preset-name" value="${esc(p.name)}" spellcheck="false" maxlength="40" aria-label="Preset name" />
          <div class="pactions">
            ${h.connected ? `<span class="live-mark" title="The EQ runs on the headset itself">On headset</span>` : `<span class="off-mark" title="The headset uses it as soon as it turns on">Saved on headset</span>`}
            <button class="tbtn" data-act="preset-duplicate">${icons.copy}Duplicate</button>
            <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="preset-delete" ${lastPreset ? "disabled" : ""}>${icons.trash}${deleting ? "Confirm delete" : "Delete"}</button>
          </div>
        </div>
        ${plate([statusCellH("status"), battery("bat", h.connected ? h.battery : null, h.charging), cell("bands", "Bands in use", `${used}<small>of 10</small>`), defaultCell("default")])}
        <div class="eq-wrap" id="eq-wrap">${graphSvg(p)}</div>
        ${bandPlate(p)}
      </section>
      <aside class="panel">
        <section class="sec">
          <div class="sec-head">
            <span class="sec-title">Band ${hv.band + 1}<small>${b.enabled ? filterInfo(b.filter)[1] : "off"}</small></span>
            <label class="switch" title="Band on or off"><input type="checkbox" data-field="band-enabled" ${b.enabled ? "checked" : ""} aria-label="Band ${hv.band + 1} on" /><span></span></label>
          </div>
          <div class="choices" role="radiogroup" aria-label="Filter type">${filterOpts}</div>
        </section>
        <section class="sec">
          <div class="fieldrow">
            <label><span class="cap">Frequency (Hz)</span><input class="field" type="number" data-field="band-freq" value="${b.freq}" min="${F_MIN}" max="${F_MAX}" /></label>
            <label><span class="cap">Gain (dB)</span><input class="field" type="number" step="0.5" data-field="band-gain" value="${b.gain}" ${hasGain(b) ? "" : "disabled"} /></label>
            <label><span class="cap">Width (Q)</span><input class="field" type="number" step="0.1" data-field="band-q" value="${b.q}" /></label>
          </div>
          <p class="hint">Drag a point on the curve to move it. Scroll over the graph to widen or narrow the selected band. Double-click empty space to turn on a free band there.</p>
        </section>
        <section class="sec">
          <div class="sec-head"><span class="sec-title">Bass<small>all presets</small></span><span class="rval" id="v-bass">${fmtBass(bass)}</span></div>
          ${slider("bass", bass, GAIN_MAX, false, "Bass", -GAIN_MAX, 0.5)}
          <div class="scale"><span>Less</span><span>More</span></div>
          <p class="hint">${bassFits(p) ? `A low shelf below ${BASS_FREQ} Hz added on top of the preset.` : "This preset uses all 10 bands and has no low shelf, so the bass slider can't apply. Turn a band off to make room."}</p>
        </section>
        <section class="sec">
          <button class="tbtn" data-act="band-reset">${icons.reset}Flatten band ${hv.band + 1}</button>
          <p class="hint">The EQ runs on the headset itself (10 bands, ±10 dB), so it applies to everything you hear, even when kit isn't running.</p>
        </section>
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
  const { L, R, T, B } = GRAPH;
  const lf0 = Math.log10(F_MIN), lf1 = Math.log10(F_MAX);
  const fx = clamp((clientX - rect.left - L) / (rect.width - L - R), 0, 1);
  const fy = clamp((clientY - rect.top - T) / (rect.height - T - B), 0, 1);
  return { freq: roundFreq(Math.pow(10, lf0 + fx * (lf1 - lf0))), gain: roundGain(DB_RANGE - fy * 2 * DB_RANGE) };
}

// ---------- mic page ----------

function micPage() {
  const m = hcfg().mic;
  const h = hcfg().headset;
  const a = H.audio;
  let status;
  if (installing) {
    status = `<div class="state-line"><span class="off-mark">Installing… approve the Windows prompt.</span></div>`;
  } else if (!a.input) {
    status = `<div class="state-line warn">The Arctis Nova 5 microphone isn't connected.</div>`;
  } else if (a.apoInstalled) {
    status = `<div class="state-line">${m.denoise ? `<span class="live-mark">Active on ${esc(a.input)}</span>` : `<span class="off-mark">Installed on ${esc(a.input)}, switched off</span>`}
      <span class="grow"></span><button class="tbtn danger" data-act="apo-uninstall">Remove</button></div>`;
  } else {
    status = `<div class="state-line warn"><span class="grow">Not installed yet. Windows asks for permission once.</span>
      <button class="btn" data-act="apo-install">${icons.shield}Install</button></div>`;
  }
  return `<div class="page fpage" data-key="headset-mic"><div class="fpage-inner">
    ${sonarNotice()}
    <h1 class="h1">Microphone</h1>

    <div class="row col">
      <div class="row-head">
        <div><div class="rtitle">Noise suppression</div>
        <p class="hint">Removes keyboard, fan and room noise from the headset mic before any app hears it. It runs inside Windows' audio engine on the real "Headset Microphone", so Discord, games and everything else get the clean mic with no virtual device to pick.</p></div>
        <label class="switch" ${a.apoInstalled ? "" : 'title="Install noise suppression first"'}><input type="checkbox" data-field="denoise" ${m.denoise && a.apoInstalled ? "checked" : ""} ${a.apoInstalled ? "" : "disabled"} aria-label="Noise suppression" /><span></span></label>
      </div>
      ${status}
    </div>
    <div class="row col">
      <div class="row-head"><div class="rtitle">Strength</div><span class="rval" id="v-strength">${m.strength}%</span></div>
      ${slider("strength", m.strength, 100, !m.denoise || !a.apoInstalled, "Noise suppression strength")}
      <div class="scale"><span>Light</span><span>Full</span></div>
      <p class="hint">Below 100% some of the original sound is mixed back in, which keeps your voice more natural. Sonar's setting was 90%.</p>
    </div>

    <h2 class="h2">On the headset</h2>
    <div class="row col">
      <div class="row-head"><div class="rtitle">Mic volume</div><span class="rval" id="v-micVolume">${h.micVolume}</span></div>
      ${slider("micVolume", h.micVolume, 15, false, "Mic volume")}
      <p class="hint">Hardware gain on the headset, applied before noise suppression.</p>
    </div>
    <div class="row col">
      <div class="row-head"><div class="rtitle">Sidetone</div><span class="rval" id="v-sidetone">${h.sidetone || "Off"}</span></div>
      ${slider("sidetone", h.sidetone, 10, false, "Sidetone")}
      <p class="hint">How much of your own voice you hear in the headphones.</p>
    </div>
    <div class="row">
      <div><div class="rtitle">Mute light</div><p class="hint">Brightness of the red light on the mic while muted.</p></div>
      <div class="seg">${LED.map((l, i) => `<button class="${h.micLed === i ? "on" : ""}" data-act="led" data-v="${i}">${l}</button>`).join("")}</div>
    </div>
  </div></div>`;
}

// ---------- device page ----------

function headsetDevicePage() {
  const c = hcfg();
  const h = H.headset;
  const a = H.audio;
  const line = (label, name, def) => {
    if (!name) return `<div class="dev-line"><span class="cap">${label}</span><span class="muted">Not connected</span></div>`;
    return `<div class="dev-line"><span class="cap">${label}</span><span>${esc(name)}</span>${def === name ? '<span class="ok">default</span>' : `<span class="muted">Windows default: ${esc(def || "none")}</span>`}</div>`;
  };
  const s = headsetStatus();
  return `<div class="page fpage" data-key="headset-device"><div class="fpage-inner">
    ${sonarNotice()}
    <h1 class="h1">${PRODUCTS.headset.name}</h1>
    ${plate([
      statusCellH("status"),
      battery("bat", h.connected ? h.battery : null, h.charging),
      cell("base", "Base station", h.present ? "Connected" : "Not found", { cls: `text ${h.present ? "" : "bad"}` }),
      defaultCell("default"),
    ])}
    ${s.hint ? `<p class="hint">${s.hint}</p>` : ""}

    <h2 class="h2">Windows audio</h2>
    <div class="row col">
      <div class="row-head">
        <div><div class="rtitle">Keep the headset as the default device</div>
        <p class="hint">Whenever the Nova 5 connects, make it the Windows default output and mic, including for calls. Picking another device yourself still works.</p></div>
        <label class="switch"><input type="checkbox" data-field="keepDefault" ${c.keepDefault ? "checked" : ""} aria-label="Keep the headset as the default device" /><span></span></label>
      </div>
      ${line("Output", a.output, a.defaultOutput)}
      ${line("Mic", a.input, a.defaultInput)}
      ${a.output && !a.headsetIsDefault ? `<div><button class="btn quiet" data-act="use-headset">Use headset now</button></div>` : ""}
      ${a.error ? `<div class="state-line bad">${esc(a.error)}</div>` : ""}
    </div>

    <h2 class="h2">Power and hearing</h2>
    <div class="row">
      <div><div class="rtitle">Turn off when idle</div><p class="hint">The headset switches itself off after this long without sound.</p></div>
      <div class="seg">${AUTO_OFF.map(([v, l]) => `<button class="${c.headset.autoOff === v ? "on" : ""}" data-act="auto-off" data-v="${v}">${l}</button>`).join("")}</div>
    </div>
    <div class="row">
      <div><div class="rtitle">Volume limiter</div><p class="hint">Caps the headset's maximum loudness to protect your hearing.</p></div>
      <label class="switch"><input type="checkbox" data-field="volumeLimiter" ${c.headset.volumeLimiter ? "checked" : ""} aria-label="Volume limiter" /><span></span></label>
    </div>
    <p class="hint">Keep SteelSeries GG closed or uninstalled: it changes the same headset settings.</p>
  </div></div>`;
}

// ---------- actions ----------

async function activatePreset(id) {
  // Send pending edits first, or the state that comes back would drop them.
  await headsetSaver.flush();
  H = await invoke("activate_preset", { id });
  render();
}

async function setApo(install) {
  await headsetSaver.flush();
  installing = true;
  render();
  try {
    H = await invoke("set_apo_installed", { install });
    toast(install ? "Noise suppression installed" : "Noise suppression removed");
  } catch (err) {
    toast(String(err), 4000);
  } finally {
    installing = false;
    render();
  }
}

// A band edit: update the headset live, save shortly after.
function bandChanged(delay = 250) {
  preview();
  headsetSaver.save(delay);
  render();
}

Object.assign(handlers, {
  preset(el) {
    if (el.dataset.id !== hcfg().active) activatePreset(el.dataset.id);
  },
  async "preset-new"() {
    const p = {
      id: newId(),
      name: `Preset ${hcfg().presets.length + 1}`,
      bands: [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((freq) => ({ enabled: true, filter: "peaking", freq, gain: 0, q: 1.414 })),
    };
    hcfg().presets.push(p);
    headsetSaver.save(0);
    await activatePreset(p.id);
    focusTitle();
  },
  async "preset-duplicate"() {
    const p = structuredClone(preset());
    p.id = newId();
    p.name = `${p.name} copy`;
    hcfg().presets.push(p);
    headsetSaver.save(0);
    await activatePreset(p.id);
    focusTitle();
  },
  "preset-delete"() {
    const p = preset();
    armDelete(p.id, async () => {
      const i = hcfg().presets.indexOf(p);
      hcfg().presets.splice(i, 1);
      const next = hcfg().presets[Math.max(0, i - 1)];
      headsetSaver.save(0);
      await activatePreset(next.id);
      toast(`Deleted "${p.name}"`);
    });
  },
  band(el) {
    hv.band = Number(el.dataset.i);
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
    hcfg().headset.micLed = Number(el.dataset.v);
    headsetSaver.save(0);
    render();
  },
  "auto-off"(el) {
    hcfg().headset.autoOff = Number(el.dataset.v);
    headsetSaver.save(0);
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
});

Object.assign(inputFields, {
  "preset-name"(el) {
    preset().name = el.value;
    headsetSaver.save();
    render();
  },
});
for (const f of ["strength", "micVolume", "sidetone"]) {
  inputFields[f] = (el) => {
    const v = Number(el.value);
    el.style.setProperty("--p", `${(v / Number(el.max)) * 100}%`);
    (f === "strength" ? hcfg().mic : hcfg().headset)[f] = v;
    const label = $(`#v-${f}`);
    if (label) label.textContent = f === "strength" ? `${v}%` : f === "sidetone" && !v ? "Off" : `${v}`;
    headsetSaver.save(300);
  };
}

// The bass slider redraws only the graph, so the slider keeps the pointer.
inputFields.bass = (el) => {
  const v = Number(el.value);
  el.style.setProperty("--p", `${((v + GAIN_MAX) / (2 * GAIN_MAX)) * 100}%`);
  hcfg().headset.bass = v;
  $("#v-bass").innerHTML = fmtBass(v);
  $("#eq-wrap").innerHTML = graphSvg(preset());
  preview();
};

Object.assign(changeFields, {
  bass(el) {
    hcfg().headset.bass = Number(el.value);
    headsetSaver.save(0);
    render();
  },
  "band-enabled"(el) {
    band().enabled = el.checked;
    bandChanged(0);
  },
  "band-freq"(el) {
    band().freq = roundFreq(num(el.value, band().freq));
    bandChanged(0);
  },
  "band-gain"(el) {
    band().gain = roundGain(num(el.value, band().gain));
    bandChanged(0);
  },
  "band-q"(el) {
    band().q = roundQ(num(el.value, band().q));
    bandChanged(0);
  },
  denoise(el) {
    hcfg().mic.denoise = el.checked;
    headsetSaver.save(0);
    render();
  },
  keepDefault(el) {
    hcfg().keepDefault = el.checked;
    headsetSaver.save(0);
    render();
  },
  volumeLimiter(el) {
    hcfg().headset.volumeLimiter = el.checked;
    headsetSaver.save(0);
    render();
  },
});

// ---------- graph dragging ----------

document.addEventListener("pointerdown", (e) => {
  const node = e.target.closest("[data-node]");
  if (!node || e.button !== 0) return;
  e.preventDefault();
  hv.band = Number(node.dataset.node);
  drag = { i: hv.band, rect: $(".eq-svg").getBoundingClientRect() };
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
  headsetSaver.save(0);
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
  hv.band = i;
  bandChanged(0);
});

function onHeadsetState(payload) {
  if (headsetSaver.unsaved() || drag) {
    // Keep edits the backend doesn't have yet; take only the preset switch.
    const config = { ...hcfg(), active: payload.config.active, mic: { ...hcfg().mic, denoise: payload.config.mic.denoise } };
    H = { ...payload, config };
  } else {
    H = payload;
  }
}
