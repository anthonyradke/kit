"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const RATES = [125, 250, 500, 1000];
const MAX_STAGES = 5;
const ACTION_TYPES = [
  ["default", "Default"],
  ["disabled", "Disabled"],
  ["keys", "Key combination"],
  ["macro", "Macro"],
  ["dpiShift", "DPI shift (held)"],
  ["dpiCycle", "Cycle DPI stages"],
  ["nextProfile", "Next profile"],
  ["profile", "Switch to profile"],
];
const MODES = [
  ["once", "Once"],
  ["whileHeld", "While held"],
  ["toggle", "Toggle"],
];
const MOUSE_NAMES = { left: "Left click", right: "Right click", middle: "Middle click", back: "Back button", forward: "Forward button" };
const SIDE_NAMES = { back: "Back", forward: "Forward" };

let S = null; // backend state: { config, dpiStage, device, hotkeyErrors, configPath }
let autostart = false;
const view = { page: "profile", profileId: null, macroId: null, side: "back" };
let hotkeyCapture = null; // "profile" | "next" while listening for a hotkey
let keyCapture = null; // { kind: "back" | "forward" | "step" } while the backend captures keys
let recording = null; // { start, timer }
let confirmDelete = null;

const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const cfg = () => S.config;
const profile = () => cfg().profiles.find((p) => p.id === view.profileId) || cfg().profiles[0];
const currentMacro = () => cfg().macros.find((m) => m.id === view.macroId);
const newId = () => Date.now().toString(16) + Math.random().toString(16).slice(2, 8);
const clampDpi = (v) => Math.round(Math.min(25600, Math.max(100, Number(v) || 800)) / 50) * 50;

const icons = {
  up: '<svg viewBox="0 0 24 24"><path d="M12 19V5M5 12l7-7 7 7"/></svg>',
  down: '<svg viewBox="0 0 24 24"><path d="M12 5v14M19 12l-7 7-7-7"/></svg>',
  x: '<svg viewBox="0 0 24 24"><path d="M18 6L6 18M6 6l12 12"/></svg>',
  stop: '<svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/></svg>',
  clock: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
  mouse: '<svg viewBox="0 0 24 24"><rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/></svg>',
  copy: '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>',
  play: '<svg viewBox="0 0 24 24"><path d="M7 5l12 7-12 7z" fill="currentColor"/></svg>',
  key: '<svg viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M8 14h8"/></svg>',
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
  } else if (o.tagName === "OPTION") {
    o.selected = n.hasAttribute("selected");
  }
}

// ---------- persistence ----------

let saveTimer = null;
function save(delay = 250) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    const st = await invoke("save_config", { config: cfg() });
    S.dpiStage = st.dpiStage;
    S.hotkeyErrors = st.hotkeyErrors;
    render();
  }, delay);
}

function toast(text) {
  const t = $("#toast");
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), 2200);
}

// ---------- formatting ----------

function codeLabel(code) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return "Num " + code.slice(6);
  const arrows = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };
  return arrows[code] || code;
}

function hotkeyHtml(text) {
  const parts = text.split("+").map((p) => `<kbd>${esc(codeLabel(p))}</kbd>`);
  return `<span class="keys">${parts.join('<span class="plus">+</span>')}</span>`;
}

function keysHtml(keys) {
  return `<span class="keys">${keys.map((k) => `<kbd>${esc(k.name)}</kbd>`).join('<span class="plus">+</span>')}</span>`;
}

function modeLabel(mode) {
  return (MODES.find((m) => m[0] === mode) || MODES[0])[1];
}

// A short summary of a side-button binding, shown on the mouse callout.
function actionSummary(p, side) {
  const a = p[side];
  switch (a.type) {
    case "keys": return a.keys.length ? a.keys.map((k) => k.name).join(" + ") : "No keys set";
    case "macro": return cfg().macros.find((m) => m.id === a.id)?.name || "No macro chosen";
    case "dpiShift": return `${a.dpi} DPI while held`;
    case "dpiCycle": return "Cycle DPI stages";
    case "nextProfile": return "Next profile";
    case "profile": return `Switch to ${cfg().profiles.find((q) => q.id === a.id)?.name || "…"}`;
    case "disabled": return "Disabled";
    default: return `${SIDE_NAMES[side]} (default)`;
  }
}

// ---------- chrome ----------

function render() {
  document.querySelectorAll(".nav-tab").forEach((b) => b.classList.toggle("on", b.dataset.page === view.page));
  renderDevice();
  const html = view.page === "macros" ? macrosPage() : view.page === "settings" ? settingsPage() : profilePage();
  morph($("#main"), html);
}

function renderDevice() {
  const d = S.device;
  let dot = "", text;
  if (d.connected) {
    dot = "on";
    text = d.name || "Mouse";
  } else if (d.present) {
    dot = "sleep";
    text = "Asleep — move the mouse to wake it";
  } else {
    text = "Receiver not found — is G HUB closed?";
  }
  let bat = "";
  if (d.battery != null) {
    const lvl = d.battery <= 15 ? "low" : d.battery <= 35 ? "mid" : "";
    bat = `<span class="bat ${lvl}"><span style="width:${d.battery}%"></span></span><span>${d.battery}%${d.charging ? " ⚡" : ""}</span>`;
  }
  morph($("#device"), `<span class="status-dot ${dot}"></span><span>${esc(text)}</span>${bat}`);
}

function errorsHtml() {
  return (S.hotkeyErrors || []).map((e) => `<div class="error">${esc(e)}</div>`).join("");
}

function hotkeyField(target, value) {
  const listening = hotkeyCapture === target;
  const inner = listening ? "Press a shortcut… (Esc to cancel)" : value ? hotkeyHtml(value) : "Click to set a shortcut";
  const clear = value && !listening ? `<button class="tbtn" data-act="hotkey-clear" data-target="${target}">Clear</button>` : "";
  return `<div class="hotkey-field"><button class="capture ${listening ? "listening" : ""}" data-act="hotkey-set" data-target="${target}">${inner}</button>${clear}</div>`;
}

// ---------- mouse page ----------

// Outline of the PRO X Superlight, traced from a top-down product shot (symmetric
// about x = 240). Units are arbitrary; the callouts share the same space.
const BODY =
  "M240,24 C300,24 360,40 392,84 C412,112 414,140 420,170 C428,210 427,300 423,380 C420,440 432,520 433,575 " +
  "C434,660 400,740 330,776 C295,792 265,797 240,797 C215,797 185,792 150,776 C80,740 46,660 47,575 " +
  "C48,520 60,440 57,380 C53,300 52,210 60,170 C66,140 68,112 88,84 C120,40 180,24 240,24 Z";
const BUTTON_SEAM = "M72,100 V352 Q72,374 94,374 H386 Q408,374 408,352 V100";

function mouseSvg(p, dpi, rate, live) {
  const d = S.device;
  const fixed = (label, value, y, x1, x2, anchor) => `
    <g class="co fixed">
      <text class="co-label" x="${x1}" y="${y - 14}" text-anchor="${anchor}">${label}</text>
      <line class="co-line" x1="${x1}" y1="${y}" x2="${x2}" y2="${y}" />
      <text class="co-value" x="${x1}" y="${y + 30}" text-anchor="${anchor}">${value}</text>
      <circle class="co-dot" cx="${x2}" cy="${y}" r="6" />
    </g>`;
  const side = (name, y, top, h) => {
    const cls = ["co", "side"];
    if (p[name].type !== "default") cls.push("bound");
    if (view.side === name) cls.push("sel");
    return `
    <g class="${cls.join(" ")}" data-act="side-pick" data-side="${name}">
      <rect class="co-hit" x="-300" y="${y - 40}" width="370" height="84" />
      <text class="co-label" x="-300" y="${y - 14}">${SIDE_NAMES[name].toUpperCase()}</text>
      <line class="co-line" x1="-300" y1="${y}" x2="42" y2="${y}" />
      <text class="co-value" x="-300" y="${y + 30}">${esc(actionSummary(p, name))}</text>
      <rect class="sidebtn" x="46" y="${top}" width="15" height="${h}" rx="7.5" />
    </g>`;
  };

  return `<svg class="mouse-svg" viewBox="-310 -64 1100 874" role="img" aria-label="PRO X Superlight, top view">
    <defs>
      <linearGradient id="accent" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#8b5cf6" /><stop offset="1" stop-color="#38bdf8" />
      </linearGradient>
      <radialGradient id="mBody" cx="0.36" cy="0.3" r="0.85">
        <stop offset="0" stop-color="#2c313b" /><stop offset="0.55" stop-color="#1a1d24" /><stop offset="1" stop-color="#0e1014" />
      </radialGradient>
      <linearGradient id="mShell" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#333844" /><stop offset="1" stop-color="#1e2129" />
      </linearGradient>
      <linearGradient id="mRim" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#4a5060" /><stop offset="0.6" stop-color="#2a2e37" /><stop offset="1" stop-color="#15171c" />
      </linearGradient>
      <linearGradient id="mWheel" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" stop-color="#0d0e12" /><stop offset="0.5" stop-color="#3a3f4b" /><stop offset="1" stop-color="#0d0e12" />
      </linearGradient>
      <pattern id="mRidges" width="10" height="7" patternUnits="userSpaceOnUse">
        <rect width="10" height="2.2" fill="rgba(0,0,0,0.55)" />
      </pattern>
      <radialGradient id="mSheen" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stop-color="#fff" stop-opacity="0.07" /><stop offset="1" stop-color="#fff" stop-opacity="0" />
      </radialGradient>
      <clipPath id="mClip"><path d="${BODY}" /></clipPath>
      <filter id="glow" x="-200%" y="-200%" width="500%" height="500%">
        <feGaussianBlur stdDeviation="5" result="b" />
        <feMerge><feMergeNode in="b" /><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
      </filter>
      <filter id="drop" x="-30%" y="-20%" width="160%" height="150%">
        <feDropShadow dx="0" dy="26" stdDeviation="26" flood-color="#000" flood-opacity="0.75" />
      </filter>
    </defs>

    <path class="m-body" d="${BODY}" filter="url(#drop)" />
    <g clip-path="url(#mClip)">
      <path d="M72,0 V352 Q72,374 94,374 H386 Q408,374 408,352 V0 Z" fill="url(#mShell)" />
      <ellipse cx="190" cy="250" rx="140" ry="230" fill="url(#mSheen)" />
    </g>
    <path class="m-seam" d="${BUTTON_SEAM}" />
    <path class="m-seam-hi" d="${BUTTON_SEAM}" transform="translate(0 2)" />
    <path class="m-seam" d="M240,24 V106 M240,240 V374" />
    <path class="m-seam-hi" d="M241.5,24 V106 M241.5,240 V374" />

    <rect class="m-slot" x="207" y="106" width="66" height="134" rx="24" />
    <rect class="m-wheel" x="223" y="118" width="34" height="110" rx="16" />
    <rect class="m-ridges" x="223" y="118" width="34" height="110" rx="16" />

    <circle class="m-led ${d.connected ? "on" : ""}" cx="240" cy="418" r="4" />
    <text class="m-dpi" x="240" y="612" text-anchor="middle">${dpi}</text>
    <text class="m-dpi-sub" x="240" y="642" text-anchor="middle">DPI · ${rate} HZ${live ? "" : " · SAVED"}</text>

    <g class="co fixed">
      <text class="co-label" x="240" y="-40" text-anchor="middle">MIDDLE CLICK</text>
      <line class="co-line" x1="240" y1="-28" x2="240" y2="160" />
      <circle class="co-dot" cx="240" cy="172" r="6" />
    </g>
    ${fixed("PRIMARY CLICK", "Left click", 200, -300, 150, "start")}
    ${fixed("SECONDARY CLICK", "Right click", 200, 780, 330, "end")}
    ${side("forward", 305, 262, 86)}
    ${side("back", 439, 388, 102)}
  </svg>`;
}

function profileTabs() {
  const tabs = cfg().profiles.map((p) => {
    const cls = ["ptab"];
    if (p.id === view.profileId) cls.push("sel");
    if (p.id === cfg().active) cls.push("active");
    return `<button class="${cls.join(" ")}" data-act="profile-open" data-id="${p.id}"><span class="pdot"></span>${esc(p.name || "Untitled")}</button>`;
  });
  return `<nav class="ptabs">${tabs.join("")}<button class="tbtn" data-act="profile-new">${icons.plus}New profile</button></nav>`;
}

function dpiTrack(p) {
  const lo = Math.log(100);
  const span = Math.log(25600) - lo;
  const pos = (v) => (((Math.log(v) - lo) / span) * 100).toFixed(2);
  const knobs = p.dpiStages
    .map((v, i) => `<button class="knob ${i === p.dpiIndex ? "on" : ""}" style="left:${pos(v)}%" data-act="stage-default" data-i="${i}" title="Stage ${i + 1} · ${v} DPI"></button>`)
    .join("");
  return `<div class="track"><div class="track-line"></div><div class="track-fill" style="width:${pos(p.dpiStages[p.dpiIndex])}%"></div>${knobs}</div>
    <div class="track-scale">${[100, 800, 6400, 25600].map((v) => `<span style="left:${pos(v)}%">${v}</span>`).join("")}</div>`;
}

function profilePage() {
  const p = profile();
  const d = S.device;
  const isActive = p.id === cfg().active;
  const live = isActive && d.connected;
  const dpi = (live && d.dpi) || p.dpiStages[p.dpiIndex];
  const rate = (live && d.rate) || p.reportRate;
  const only = cfg().profiles.length === 1;
  const deleting = confirmDelete === p.id;

  const stages = p.dpiStages
    .map((v, i) => {
      const cls = ["stage"];
      if (i === p.dpiIndex) cls.push("default");
      if (live && d.dpi === v) cls.push("live");
      return `<div class="${cls.join(" ")}" data-act="stage-default" data-i="${i}">
        <span class="stage-num">STAGE ${i + 1}</span>
        <input type="number" min="100" max="25600" step="50" value="${v}" data-field="stage" data-i="${i}" />
        ${p.dpiStages.length > 1 ? `<button class="x" data-act="stage-remove" data-i="${i}" title="Remove stage">×</button>` : ""}
      </div>`;
    })
    .join("");
  const addStage = p.dpiStages.length < MAX_STAGES ? `<button class="tbtn" data-act="stage-add">${icons.plus}Add</button>` : "";

  return `<div class="page devpage" data-key="profile">
    ${profileTabs()}
    <div class="dev-grid">
      <section class="canvas">
        <div class="canvas-head">
          <input class="pname" value="${esc(p.name)}" data-field="profile-name" spellcheck="false" />
          <div class="pactions">
            ${isActive ? `<span class="active-mark">Active</span>` : `<button class="btn-grad" data-act="activate">Activate</button>`}
            <button class="tbtn" data-act="profile-duplicate">${icons.copy}Duplicate</button>
            <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="profile-delete" ${only ? "disabled" : ""}>${icons.trash}${deleting ? "Confirm" : "Delete"}</button>
          </div>
        </div>
        ${mouseSvg(p, dpi, rate, live)}
      </section>

      <aside class="panel">
        <section class="sec">
          <div class="sec-head"><span class="label">Sensitivity</span><span class="big-num">${p.dpiStages[p.dpiIndex]}<small>DPI</small></span></div>
          ${dpiTrack(p)}
          <div class="stages">${stages}${addStage}</div>
          <p class="hint">Click a stage to make it this profile's default. A side button can cycle through them.</p>
        </section>

        <section class="sec">
          <div class="sec-head"><span class="label">Report rate</span></div>
          <div class="toggles">${RATES.map((r) => `<button class="${p.reportRate === r ? "on" : ""}" data-act="rate" data-v="${r}">${r} Hz</button>`).join("")}</div>
        </section>

        <section class="sec">
          <div class="sec-head">
            <span class="label">Assignment</span>
            <div class="toggles caps">${["back", "forward"].map((s) => `<button class="${view.side === s ? "on" : ""}" data-act="side-pick" data-side="${s}">${SIDE_NAMES[s]}</button>`).join("")}</div>
          </div>
          ${assignment(p, view.side)}
        </section>

        <section class="sec">
          <div class="sec-head"><span class="label">Profile shortcut</span></div>
          ${hotkeyField("profile", p.hotkey)}
          <div id="hotkey-errors">${errorsHtml()}</div>
          <p class="hint">Switch to this profile from anywhere, even in games.</p>
        </section>
      </aside>
    </div>
  </div>`;
}

function assignment(p, side) {
  const a = p[side];
  const name = SIDE_NAMES[side];
  const opts = ACTION_TYPES.map(
    ([v, label]) => `<button class="opt ${a.type === v ? "on" : ""}" data-act="bind-type" data-v="${v}">${v === "default" ? `Default (${name})` : label}</button>`,
  ).join("");

  let extra;
  switch (a.type) {
    case "keys": {
      const listening = keyCapture && keyCapture.kind === side;
      const inner = listening ? "Press a key combination…" : a.keys.length ? keysHtml(a.keys) : "Click to set keys";
      extra = `<div><button class="capture ${listening ? "listening" : ""}" data-act="keys-capture" data-side="${side}">${inner}</button></div>`;
      break;
    }
    case "macro": {
      if (!cfg().macros.length) {
        extra = `<span class="muted">No macros yet. <button class="link" data-act="page" data-page="macros">Create one</button></span>`;
      } else {
        const opts = cfg().macros.map((m) => `<option value="${m.id}" ${a.id === m.id ? "selected" : ""}>${esc(m.name)}</option>`);
        const missing = !cfg().macros.some((m) => m.id === a.id) ? `<option value="" selected>Choose a macro…</option>` : "";
        extra = `<select data-field="bind-macro" data-side="${side}">${missing}${opts.join("")}</select>`;
      }
      break;
    }
    case "dpiShift":
      extra = `<div class="inline"><input class="field num" type="number" min="100" max="25600" step="50" value="${a.dpi}" data-field="bind-dpi" data-side="${side}" /> DPI while held</div>`;
      break;
    case "profile": {
      const opts = cfg().profiles.map((q) => `<option value="${q.id}" ${a.id === q.id ? "selected" : ""}>${esc(q.name)}</option>`);
      extra = `<select data-field="bind-profile" data-side="${side}">${opts.join("")}</select>`;
      break;
    }
    case "dpiCycle":
      extra = `<span class="muted">Steps through this profile's DPI stages.</span>`;
      break;
    case "nextProfile":
      extra = `<span class="muted">Cycles through your profiles in order.</span>`;
      break;
    case "disabled":
      extra = `<span class="muted">The button does nothing.</span>`;
      break;
    default:
      extra = `<span class="muted">Works as ${name.toLowerCase()} in browsers and apps.</span>`;
  }
  return `<div class="opts">${opts}</div><div class="opt-extra">${extra}</div>`;
}

// ---------- macros page ----------

function macrosPage() {
  const macros = cfg().macros;
  if (!view.macroId && macros.length) view.macroId = macros[0].id;
  const list = macros
    .map(
      (m) => `<button class="mrow ${m.id === view.macroId ? "sel" : ""}" data-act="macro-open" data-id="${m.id}">
        <span class="mname">${esc(m.name || "Untitled")}</span>
        <span class="mmeta">${m.steps.filter((s) => s.type !== "delay").length} actions · ${modeLabel(m.mode)}</span>
      </button>`,
    )
    .join("");
  const m = currentMacro();
  const editor = m
    ? macroEditor(m)
    : `<div class="empty"><div class="big">No macro selected</div>Create a macro, record it, then assign it to a side button.</div>`;
  return `<div class="page mpage" data-key="macros">
    <aside class="mlist">
      <div class="list-head"><span class="label">Macros</span><button class="tbtn" data-act="macro-new">${icons.plus}New</button></div>
      ${list || `<p class="hint" style="padding:0 10px">No macros yet.</p>`}
    </aside>
    <section class="meditor">${editor}</section>
  </div>`;
}

function macroEditor(m) {
  const deleting = confirmDelete === m.id;
  const usedBy = [];
  for (const p of cfg().profiles) {
    for (const side of ["back", "forward"]) {
      if (p[side].type === "macro" && p[side].id === m.id) usedBy.push(`${p.name} · ${SIDE_NAMES[side]}`);
    }
  }
  const recBar = recording
    ? `<div class="rec-bar"><span class="rec-dot"></span><span class="rec-time" id="rec-time">0.0s</span>
        <span class="grow">Recording keys and clicks. Clicks inside this window are ignored.</span>
        <button class="tbtn" data-act="rec-stop">${icons.stop}Stop</button></div>`
    : "";
  const addKeyListening = keyCapture && keyCapture.kind === "step";
  const hint = m.mode === "once" ? "Plays each time the button is pressed." : m.mode === "whileHeld" ? "Loops for as long as the button is held." : "First press starts looping, second press stops.";
  const actions = m.steps.filter((s) => s.type !== "delay").length;

  return `<div class="canvas-head">
      <input class="pname" value="${esc(m.name)}" data-field="macro-name" spellcheck="false" />
      <div class="pactions">
        <button class="btn-grad" data-act="macro-test" ${m.steps.length && !recording ? "" : "disabled"} title="Play once in 2 seconds">${icons.play}Test</button>
        <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="macro-delete">${icons.trash}${deleting ? "Confirm" : "Delete"}</button>
      </div>
    </div>

    <div class="me-row">
      <div class="field-block">
        <span class="label">Playback</span>
        <div class="toggles">${MODES.map(([v, l]) => `<button class="${m.mode === v ? "on" : ""}" data-act="macro-mode" data-v="${v}">${l}</button>`).join("")}</div>
      </div>
      ${m.mode === "once" ? `<div class="field-block"><span class="label">Repeat</span>
        <div class="inline"><input class="field num" type="number" min="1" max="1000" value="${m.repeat}" data-field="macro-repeat" /> times</div></div>` : ""}
      <div class="field-block">
        <span class="label">Assigned to</span>
        <div class="bound">${usedBy.length ? usedBy.map(esc).join(", ") : `<span class="muted">Not assigned to a button yet</span>`}</div>
      </div>
    </div>
    <p class="hint">${hint}</p>

    <div class="steps-head">
      <span class="label">Steps<span class="count">${actions}</span></span>
      <div class="tools">
        ${recording ? "" : `<button class="tbtn rec" data-act="rec-start"><span class="rdot"></span>${m.steps.length ? "Re-record" : "Record"}</button>`}
        <button class="tbtn" data-act="step-add-key">${icons.key}${addKeyListening ? "Press a key…" : "Add key"}</button>
        <button class="tbtn" data-act="step-add-delay">${icons.clock}Add delay</button>
        <span class="sep"></span>
        <span class="inline">All delays <input class="field num" type="number" min="0" value="50" id="all-delays" /> ms</span>
        <button class="tbtn" data-act="delays-set">Apply</button>
        <button class="tbtn" data-act="delays-clear">Remove</button>
      </div>
    </div>
    ${recBar}
    <div class="steps">${m.steps.map(stepHtml).join("") || `<div class="empty">No steps yet. Record your keys and clicks, or add steps by hand.</div>`}</div>`;
}

function stepHtml(s, i) {
  let body;
  if (s.type === "delay") {
    body = `${icons.clock}<span>Wait</span><input class="field num" type="number" min="0" value="${s.ms}" data-field="step-ms" data-i="${i}" /><span>ms</span>`;
  } else if (s.type === "key") {
    body = `<span class="arrow ${s.down ? "down" : "up"}">${s.down ? "↓" : "↑"}</span><kbd>${esc(s.key.name)}</kbd><span class="muted">${s.down ? "press" : "release"}</span>`;
  } else {
    body = `<span class="arrow ${s.down ? "down" : "up"}">${s.down ? "↓" : "↑"}</span>${icons.mouse}<span>${MOUSE_NAMES[s.button]}</span><span class="muted">${s.down ? "press" : "release"}</span>`;
  }
  return `<div class="step ${s.type}">
    <span class="step-idx">${i + 1}</span>
    <span class="step-kind">${body}</span>
    <div class="step-tools">
      <button data-act="step-up" data-i="${i}" title="Move up">${icons.up}</button>
      <button data-act="step-down" data-i="${i}" title="Move down">${icons.down}</button>
      <button class="del" data-act="step-del" data-i="${i}" title="Remove">${icons.x}</button>
    </div>
  </div>`;
}

// ---------- settings page ----------

function settingsPage() {
  const d = S.device;
  return `<div class="page spage" data-key="settings"><div class="spage-inner">
    <h1 class="h1">Settings</h1>

    <div class="srow">
      <div><div class="stitle">Launch at startup</div><p class="hint">Start lightswitch in the tray when you sign in to Windows.</p></div>
      <label class="switch"><input type="checkbox" data-field="autostart" ${autostart ? "checked" : ""} /><span></span></label>
    </div>
    <div class="srow">
      <div><div class="stitle">Next-profile shortcut</div><p class="hint">Cycle through all profiles with one shortcut.</p></div>
      ${hotkeyField("next", cfg().settings.nextProfileHotkey)}
    </div>
    <div id="hotkey-errors">${errorsHtml()}</div>

    <span class="label block">Device</span>
    <dl class="kv">
      <dt>Mouse</dt><dd>${esc(d.name || "—")}</dd>
      <dt>Status</dt><dd>${d.connected ? "Connected" : d.present ? "Asleep" : "Not found"}</dd>
      <dt>Battery</dt><dd>${d.battery != null ? `${d.battery}%${d.charging ? " (charging)" : ""}` : "—"}</dd>
      <dt>Current DPI</dt><dd>${d.dpi ?? "—"}</dd>
      <dt>Report rate</dt><dd>${d.rate ? `${d.rate} Hz` : "—"}</dd>
    </dl>

    <span class="label block">About</span>
    <dl class="kv"><dt>Config file</dt><dd class="path">${esc(S.configPath)}</dd></dl>
    <p class="hint">Closing this window keeps lightswitch running in the tray. Use Quit from the tray menu to exit. G HUB must stay closed.</p>
  </div></div>`;
}

// ---------- actions ----------

function defaultAction(type) {
  switch (type) {
    case "keys": return { type, keys: [] };
    case "macro": return { type, id: cfg().macros[0]?.id || "" };
    case "dpiShift": return { type, dpi: 400 };
    case "profile": return { type, id: (cfg().profiles.find((q) => q.id !== profile().id) || profile()).id };
    default: return { type };
  }
}

async function activate(id) {
  S = await invoke("activate_profile", { id });
  render();
}

async function captureKeys(kind) {
  if (keyCapture) {
    await invoke("cancel_capture");
    return;
  }
  keyCapture = { kind };
  render();
  const keys = await invoke("capture_keys");
  keyCapture = null;
  if (keys && keys.length) {
    if (kind === "step") {
      const m = currentMacro();
      for (const key of keys) m.steps.push({ type: "key", key, down: true });
      for (const key of [...keys].reverse()) m.steps.push({ type: "key", key, down: false });
    } else {
      profile()[kind] = { type: "keys", keys };
    }
    save(0);
  }
  render();
}

async function startRecording() {
  await invoke("start_recording");
  recording = { start: performance.now() };
  recording.timer = setInterval(() => {
    const el = $("#rec-time");
    if (el) el.textContent = ((performance.now() - recording.start) / 1000).toFixed(1) + "s";
  }, 100);
  render();
}

async function stopRecording() {
  const steps = await invoke("stop_recording");
  clearInterval(recording.timer);
  recording = null;
  const m = currentMacro();
  if (m) {
    m.steps = steps;
    save(0);
    toast(`Recorded ${steps.filter((s) => s.type !== "delay").length} actions`);
  }
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
  "profile-open"(el) {
    view.page = "profile";
    view.profileId = el.dataset.id;
    render();
  },
  "profile-new"() {
    const p = { id: newId(), name: `Profile ${cfg().profiles.length + 1}`, dpiStages: [400, 800, 1600], dpiIndex: 1, reportRate: 1000, back: { type: "default" }, forward: { type: "default" }, hotkey: null };
    cfg().profiles.push(p);
    view.page = "profile";
    view.profileId = p.id;
    save(0);
    render();
    focusTitle();
  },
  "profile-duplicate"() {
    const p = structuredClone(profile());
    p.id = newId();
    p.name = `${p.name} copy`;
    p.hotkey = null;
    cfg().profiles.splice(cfg().profiles.indexOf(profile()) + 1, 0, p);
    view.profileId = p.id;
    save(0);
    render();
  },
  "profile-delete"() {
    const p = profile();
    armDelete(p.id, () => {
      cfg().profiles = cfg().profiles.filter((q) => q.id !== p.id);
      if (cfg().active === p.id) cfg().active = cfg().profiles[0].id;
      view.profileId = cfg().profiles[0].id;
      save(0);
      toast(`Deleted "${p.name}"`);
    });
  },
  activate() {
    activate(profile().id);
  },
  "side-pick"(el) {
    view.side = el.dataset.side;
    render();
  },
  "bind-type"(el) {
    const p = profile();
    if (p[view.side].type === el.dataset.v) return;
    p[view.side] = defaultAction(el.dataset.v);
    save(0);
    render();
  },
  "stage-default"(el, e) {
    if (e.target.closest("input, .x")) return;
    profile().dpiIndex = Number(el.dataset.i);
    save(0);
    render();
  },
  "stage-remove"(el) {
    const p = profile();
    const i = Number(el.dataset.i);
    p.dpiStages.splice(i, 1);
    if (p.dpiIndex >= i && p.dpiIndex > 0) p.dpiIndex--;
    save(0);
    render();
  },
  "stage-add"() {
    const p = profile();
    const last = p.dpiStages[p.dpiStages.length - 1];
    p.dpiStages.push(clampDpi(last * 2));
    save(0);
    render();
    document.querySelector(`.stage input[data-i="${p.dpiStages.length - 1}"]`)?.select();
  },
  rate(el) {
    profile().reportRate = Number(el.dataset.v);
    save(0);
    render();
  },
  "keys-capture"(el) {
    captureKeys(el.dataset.side);
  },
  "hotkey-set"(el) {
    hotkeyCapture = hotkeyCapture === el.dataset.target ? null : el.dataset.target;
    render();
  },
  "hotkey-clear"(el) {
    setHotkey(el.dataset.target, null);
  },
  "macro-new"() {
    const m = { id: newId(), name: `Macro ${cfg().macros.length + 1}`, mode: "once", repeat: 1, steps: [] };
    cfg().macros.push(m);
    view.macroId = m.id;
    save(0);
    render();
    focusTitle();
  },
  "macro-open"(el) {
    view.macroId = el.dataset.id;
    render();
  },
  "macro-delete"() {
    const m = currentMacro();
    armDelete(m.id, () => {
      cfg().macros = cfg().macros.filter((x) => x.id !== m.id);
      for (const p of cfg().profiles) {
        for (const side of ["back", "forward"]) if (p[side].type === "macro" && p[side].id === m.id) p[side] = { type: "default" };
      }
      view.macroId = cfg().macros[0]?.id || null;
      save(0);
      toast(`Deleted "${m.name}"`);
    });
  },
  "macro-mode"(el) {
    currentMacro().mode = el.dataset.v;
    save(0);
    render();
  },
  "macro-test"(el) {
    // Drop focus so a Space/Enter inside the macro can't re-click this button.
    el.blur();
    // Delay so the user can focus the window the macro should type into.
    const id = currentMacro().id;
    toast("Playing in 2 seconds…");
    setTimeout(() => invoke("test_macro", { id }), 2000);
  },
  "rec-start"() {
    startRecording();
  },
  "rec-stop"() {
    stopRecording();
  },
  "step-add-key"() {
    captureKeys("step");
  },
  "step-add-delay"() {
    currentMacro().steps.push({ type: "delay", ms: 50 });
    save(0);
    render();
  },
  "step-up"(el) {
    moveStep(Number(el.dataset.i), -1);
  },
  "step-down"(el) {
    moveStep(Number(el.dataset.i), 1);
  },
  "step-del"(el) {
    currentMacro().steps.splice(Number(el.dataset.i), 1);
    save(0);
    render();
  },
  "delays-set"() {
    const ms = Math.max(0, Number($("#all-delays").value) || 0);
    const m = currentMacro();
    // Normalise: one delay between every pair of actions.
    const actions = m.steps.filter((s) => s.type !== "delay");
    m.steps = actions.flatMap((s, i) => (i && ms ? [{ type: "delay", ms }, s] : [s]));
    save(0);
    render();
  },
  "delays-clear"() {
    const m = currentMacro();
    m.steps = m.steps.filter((s) => s.type !== "delay");
    save(0);
    render();
  },
};

function moveStep(i, dir) {
  const steps = currentMacro().steps;
  const j = i + dir;
  if (j < 0 || j >= steps.length) return;
  [steps[i], steps[j]] = [steps[j], steps[i]];
  save(0);
  render();
}

function setHotkey(target, value) {
  if (target === "next") cfg().settings.nextProfileHotkey = value;
  else profile().hotkey = value;
  hotkeyCapture = null;
  save(0);
  render();
}

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
  if (f === "profile-name") {
    profile().name = el.value;
    save();
    render();
  } else if (f === "macro-name") {
    currentMacro().name = el.value;
    save();
    render();
  } else if (f === "step-ms") {
    currentMacro().steps[Number(el.dataset.i)].ms = Math.max(0, Number(el.value) || 0);
    save();
  } else if (f === "macro-repeat") {
    currentMacro().repeat = Math.min(1000, Math.max(1, Number(el.value) || 1));
    save();
  }
});

// Numbers that need clamping are committed on change (blur / Enter), not per keystroke.
document.addEventListener("change", async (e) => {
  const el = e.target;
  const f = el.dataset.field;
  const side = el.dataset.side;
  if (f === "stage") {
    const v = clampDpi(el.value);
    el.value = v;
    profile().dpiStages[Number(el.dataset.i)] = v;
    save(0);
    render();
  } else if (f === "bind-macro" || f === "bind-profile") {
    profile()[side].id = el.value;
    save(0);
    render();
  } else if (f === "bind-dpi") {
    const v = clampDpi(el.value);
    el.value = v;
    profile()[side].dpi = v;
    save(0);
    render();
  } else if (f === "autostart") {
    try {
      autostart = await invoke("set_autostart", { enabled: el.checked });
      toast(autostart ? "lightswitch will start with Windows" : "Startup disabled");
    } catch (err) {
      toast(`Couldn't change startup: ${err}`);
    }
    el.checked = autostart;
  }
});

// Windows doesn't run the global keyboard hook for keys typed into our own
// window, so forward them to the recorder ourselves while it is listening.
function forwardKey(e, down) {
  if (!keyCapture && !recording) return false;
  e.preventDefault();
  if (!e.repeat) invoke("ui_key", { code: e.code, down });
  return true;
}

document.addEventListener("keyup", (e) => forwardKey(e, false));

document.addEventListener("keydown", (e) => {
  if (forwardKey(e, true)) return;
  if (e.key === "Enter" && e.target.matches("input")) e.target.blur();
  if (!hotkeyCapture) return;
  e.preventDefault();
  if (e.key === "Escape") {
    hotkeyCapture = null;
    render();
    return;
  }
  const mods = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(e.code) || ["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
  // Some synthetic/remapped keyboards leave `code` empty; fall back to `key`.
  let code = e.code;
  if (!code && /^[a-z]$/i.test(e.key)) code = "Key" + e.key.toUpperCase();
  else if (!code && /^[0-9]$/.test(e.key)) code = "Digit" + e.key;
  if (!code) return;
  setHotkey(hotkeyCapture, [...mods, code].join("+"));
});

// ---------- boot ----------

(async () => {
  S = await invoke("get_state");
  autostart = await invoke("get_autostart");
  view.profileId = cfg().active;
  render();

  await listen("state", (e) => {
    S = e.payload;
    if (!cfg().profiles.some((p) => p.id === view.profileId)) view.profileId = cfg().active;
    render();
  });
  await listen("device", (e) => {
    S.device = e.payload;
    render();
  });
})();
