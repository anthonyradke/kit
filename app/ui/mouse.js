"use strict";
// Mouse pages: Profiles (DPI, report rate, side buttons, shortcut), Macros, Device.

const RATES = [125, 250, 500, 1000];
const MAX_STAGES = 5;
const MAX_DELAY = 600000; // 10 minutes
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

let M = null; // backend state: { config, dpiStage, device, hotkeyErrors, configPath }
const mv = { profileId: null, macroId: null, side: "back" };
let hotkeyCapture = null; // "profile" | "next" | "kprofile" | "knext" while listening for a hotkey
let keyCapture = null; // { kind: "back" | "forward" | "step" } while the backend captures keys
let recording = null; // { start, timer, macroId }

const mcfg = () => M.config;
const profile = () => mcfg().profiles.find((p) => p.id === mv.profileId) || mcfg().profiles[0];
const currentMacro = () => mcfg().macros.find((m) => m.id === mv.macroId);
const clampDpi = (v) => Math.round(Math.min(25600, Math.max(100, Number(v) || 800)) / 50) * 50;

const mouseSaver = makeSaver("mouse_save", mcfg, (st) => {
  M.dpiStage = st.dpiStage;
  M.hotkeyErrors = st.hotkeyErrors;
});

function mouseStatus() {
  const d = M.device;
  if (d.connected) return { text: "Connected", live: true };
  if (d.present) return { text: "Asleep", hint: "Move the mouse to wake it" };
  if (!d.error) return { text: "Searching" };
  return { text: "Receiver not found", hint: "Is G HUB still running?", bad: true };
}

function statusCell(key, s) {
  return cell(key, "Status", statusMark(s), { cls: "text" });
}

function modeLabel(mode) {
  return (MODES.find((m) => m[0] === mode) || MODES[0])[1];
}

// A short summary of a side-button binding.
function actionSummary(p, side) {
  const a = p[side];
  switch (a.type) {
    case "keys": return a.keys.length ? a.keys.map((k) => k.name).join(" + ") : "No keys set";
    case "macro": return mcfg().macros.find((m) => m.id === a.id)?.name || "No macro chosen";
    case "dpiShift": return `${a.dpi} DPI while held`;
    case "dpiCycle": return "Cycle DPI stages";
    case "nextProfile": return "Next profile";
    case "profile": return `Switch to ${mcfg().profiles.find((q) => q.id === a.id)?.name || "…"}`;
    case "disabled": return "Disabled";
    default: return `${SIDE_NAMES[side]} (default)`;
  }
}

// The plate shown on the home screen and at the top of the mouse page.
function mousePlateCells(p, prefix) {
  const d = M.device;
  const active = mcfg().profiles.find((q) => q.id === mcfg().active) || mcfg().profiles[0];
  const live = d.connected && p.id === active.id;
  const dpi = live && d.dpi ? d.dpi : p.dpiStages[p.dpiIndex];
  const rate = live && d.rate ? d.rate : p.reportRate;
  const dim = live ? "" : "dim";
  return [
    statusCell(`${prefix}status`, mouseStatus()),
    battery(`${prefix}bat`, d.battery, d.charging),
    cell(`${prefix}dpi`, live ? "Sensitivity · live" : "Sensitivity", `${dpi}<small>DPI</small>`, { cls: dim }),
    cell(`${prefix}rate`, "Report rate", `${rate}<small>Hz</small>`, { cls: dim }),
  ];
}

function errorsHtml() {
  return (M.hotkeyErrors || []).map((e) => `<div class="error">${esc(e)}</div>`).join("");
}

function hotkeyField(target, value) {
  const listening = hotkeyCapture === target;
  const inner = listening ? "Press a shortcut… (Esc to cancel)" : value ? hotkeyHtml(value) : "Click to set a shortcut";
  const clear = value && !listening ? `<button class="tbtn" data-act="hotkey-clear" data-target="${target}">Clear</button>` : "";
  return `<div class="hotkey-field"><button class="capture ${listening ? "listening" : ""}" data-act="hotkey-set" data-target="${target}">${inner}</button>${clear}</div>`;
}

// ---------- profiles page ----------

// The white PRO X Superlight from above (official product photo), placed over
// the outline the callouts were measured against (symmetric about x = 240).
const MOUSE_IMG = { x: 44, y: 22, w: 392, h: 777 };

function mouseSvg(p, dpi, rate, live) {
  const fixed = (label, value, y, x1, x2, anchor) => `
    <g class="co fixed">
      <text class="co-cap" x="${x1}" y="${y - 14}" text-anchor="${anchor}">${label}</text>
      <line class="co-line" x1="${x1}" y1="${y}" x2="${x2}" y2="${y}" />
      <text class="co-val" x="${x1}" y="${y + 30}" text-anchor="${anchor}">${value}</text>
      <circle class="co-dot" cx="${x2}" cy="${y}" r="6" />
    </g>`;
  const side = (name, y, top, h) => {
    const cls = ["co", "side"];
    if (p[name].type !== "default") cls.push("bound");
    if (mv.side === name) cls.push("sel");
    return `
    <g class="${cls.join(" ")}" data-act="side-pick" data-side="${name}" role="button" aria-label="${SIDE_NAMES[name]} button">
      <rect class="co-hit" x="-300" y="${y - 40}" width="370" height="84" />
      <text class="co-cap" x="-300" y="${y - 14}">${SIDE_NAMES[name].toUpperCase()}</text>
      <line class="co-line" x1="-300" y1="${y}" x2="30" y2="${y}" />
      <text class="co-val" x="-300" y="${y + 30}">${esc(actionSummary(p, name))}</text>
      <rect class="sidebtn" x="32" y="${top}" width="13" height="${h}" rx="3" />
    </g>`;
  };
  const i = MOUSE_IMG;
  return `<svg class="mouse-svg" viewBox="-310 -64 1100 874" role="img" aria-label="PRO X Superlight, top view">
    <defs>
      <filter id="drop" x="-30%" y="-20%" width="160%" height="150%">
        <feDropShadow dx="0" dy="22" stdDeviation="22" flood-color="#000" style="flood-opacity: var(--shadow-a)" />
      </filter>
    </defs>
    <image href="${productImg("mouse", "top")}" x="${i.x}" y="${i.y}" width="${i.w}" height="${i.h}" preserveAspectRatio="xMidYMid meet" filter="url(#drop)" />

    <g class="co fixed">
      <text class="co-cap" x="240" y="-40" text-anchor="middle">MIDDLE CLICK</text>
      <line class="co-line" x1="240" y1="-28" x2="240" y2="150" />
      <circle class="co-dot" cx="240" cy="162" r="6" />
    </g>
    ${fixed("PRIMARY CLICK", "Left click", 210, -300, 150, "start")}
    ${fixed("SECONDARY CLICK", "Right click", 210, 780, 330, "end")}
    ${side("forward", 305, 262, 86)}
    ${side("back", 439, 388, 102)}
    <g class="co fixed">
      <text class="co-cap" x="780" y="560" text-anchor="end">${live ? "LIVE ON MOUSE" : "STORED IN PROFILE"}</text>
      <line class="co-line" x1="780" y1="574" x2="440" y2="574" />
      <text class="m-dpi" x="780" y="626" text-anchor="end">${dpi}</text>
      <text class="m-dpi-sub" x="780" y="652" text-anchor="end">DPI · ${rate} HZ</text>
    </g>
  </svg>`;
}

function profileStrip() {
  const tabs = mcfg().profiles.map((p) => {
    const cls = ["stab"];
    if (p.id === mv.profileId) cls.push("sel");
    if (p.id === mcfg().active) cls.push(M.device.connected ? "live" : "stored");
    const title = p.id === mcfg().active ? ' title="Active profile"' : "";
    return `<button class="${cls.join(" ")}" data-act="profile-open" data-id="${p.id}"${title}><span class="pdot"></span>${esc(p.name || "Untitled")}</button>`;
  });
  return `<nav class="strip" aria-label="Profiles">${tabs.join("")}<button class="tbtn" data-act="profile-new">${icons.plus}New profile</button></nav>`;
}

function dpiTrack(p) {
  const lo = Math.log(100);
  const span = Math.log(25600) - lo;
  const pos = (v) => (((Math.log(v) - lo) / span) * 100).toFixed(2);
  const knobs = p.dpiStages
    .map((v, i) => `<button class="knob ${i === p.dpiIndex ? "on" : ""}" style="left:${pos(v)}%" data-act="stage-default" data-i="${i}" title="Stage ${i + 1}: ${v} DPI" aria-label="Make stage ${i + 1} the default"></button>`)
    .join("");
  return `<div class="track"><div class="track-line"></div><div class="track-fill" style="width:${pos(p.dpiStages[p.dpiIndex])}%"></div>${knobs}</div>
    <div class="track-scale">${[100, 800, 6400, 25600].map((v) => `<span style="left:${pos(v)}%">${v}</span>`).join("")}</div>`;
}

function stagePlate(p, live) {
  const d = M.device;
  const cells = p.dpiStages.map((v, i) => {
    const cls = ["stage-cell"];
    if (i === p.dpiIndex) cls.push("default");
    if (live && d.dpi === v) cls.push("live");
    const x = p.dpiStages.length > 1 ? `<button class="x" data-act="stage-remove" data-i="${i}" title="Remove stage" aria-label="Remove stage ${i + 1}">${icons.x}</button>` : "";
    return {
      span: 1,
      html: (end) => `<div class="cell ${cls.join(" ")} ${end}" data-act="stage-default" data-i="${i}">
        <div class="cap">Stage ${i + 1}</div>
        <input type="number" min="100" max="25600" step="50" value="${v}" data-field="stage" data-i="${i}" aria-label="Stage ${i + 1} DPI" />${x}</div>`,
    };
  });
  if (p.dpiStages.length < MAX_STAGES) {
    cells.push({ span: 1, html: (end) => `<button class="cell stage-add ${end}" data-act="stage-add" title="Add a stage" aria-label="Add a DPI stage">${icons.plus}</button>` });
  }
  return plate(cells, cells.length, "stages");
}

function profilesPage() {
  const p = profile();
  const d = M.device;
  const isActive = p.id === mcfg().active;
  const live = isActive && d.connected;
  const dpi = (live && d.dpi) || p.dpiStages[p.dpiIndex];
  const rate = (live && d.rate) || p.reportRate;
  const only = mcfg().profiles.length === 1;
  const deleting = confirmDelete === p.id;

  return `<div class="page devpage" data-key="mouse-profiles">
    ${profileStrip()}
    <div class="work">
      <section class="canvas">
        <div class="canvas-head">
          <input class="pname" value="${esc(p.name)}" data-field="profile-name" spellcheck="false" aria-label="Profile name" />
          <div class="pactions">
            ${isActive ? (d.connected ? `<span class="live-mark">Active on mouse</span>` : `<span class="off-mark" title="Applied when the mouse wakes">Active profile</span>`) : `<button class="btn" data-act="activate">Activate</button>`}
            <button class="tbtn" data-act="profile-duplicate">${icons.copy}Duplicate</button>
            <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="profile-delete" ${only ? "disabled" : ""}>${icons.trash}${deleting ? "Confirm delete" : "Delete"}</button>
          </div>
        </div>
        ${plate(mousePlateCells(p, ""))}
        ${mouseSvg(p, dpi, rate, live)}
      </section>

      <aside class="panel">
        <section class="sec">
          <div class="sec-head"><span class="sec-title">Sensitivity<small>default ${p.dpiStages[p.dpiIndex]} DPI</small></span></div>
          ${dpiTrack(p)}
          ${stagePlate(p, live)}
          <p class="hint">Click a stage to make it this profile's default. A side button can cycle through them.</p>
        </section>

        <section class="sec">
          <div class="sec-head"><span class="sec-title">Report rate</span></div>
          <div class="seg fill" role="radiogroup" aria-label="Report rate">${RATES.map((r) => `<button class="${p.reportRate === r ? "on" : ""}" data-act="rate" data-v="${r}" role="radio" aria-checked="${p.reportRate === r}">${r} Hz</button>`).join("")}</div>
        </section>

        <section class="sec">
          <div class="sec-head">
            <span class="sec-title">Side buttons</span>
            <div class="seg" role="tablist">${["back", "forward"].map((s) => `<button class="${mv.side === s ? "on" : ""}" data-act="side-pick" data-side="${s}" role="tab" aria-selected="${mv.side === s}">${SIDE_NAMES[s]}</button>`).join("")}</div>
          </div>
          ${assignment(p, mv.side)}
        </section>

        <section class="sec">
          <div class="sec-head"><span class="sec-title">Profile shortcut</span></div>
          ${hotkeyField("profile", p.hotkey)}
          ${errorsHtml()}
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
    ([v, label]) => `<button class="choice ${a.type === v ? "on" : ""}" data-act="bind-type" data-v="${v}" role="radio" aria-checked="${a.type === v}">${v === "default" ? `Default (${name})` : label}</button>`,
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
      if (!mcfg().macros.length) {
        extra = `<span class="muted">No macros yet. <button class="link" data-act="mouse-tab" data-tab="macros">Create one</button></span>`;
      } else {
        const opts = mcfg().macros.map((m) => `<option value="${m.id}" ${a.id === m.id ? "selected" : ""}>${esc(m.name)}</option>`);
        const missing = !mcfg().macros.some((m) => m.id === a.id) ? `<option value="" selected>Choose a macro…</option>` : "";
        extra = `<select data-field="bind-macro" data-side="${side}" aria-label="Macro">${missing}${opts.join("")}</select>`;
      }
      break;
    }
    case "dpiShift":
      extra = `<div class="inline"><input class="field num" type="number" min="100" max="25600" step="50" value="${a.dpi}" data-field="bind-dpi" data-side="${side}" aria-label="DPI while held" /> DPI while held</div>`;
      break;
    case "profile": {
      const opts = mcfg().profiles.map((q) => `<option value="${q.id}" ${a.id === q.id ? "selected" : ""}>${esc(q.name)}</option>`);
      extra = `<select data-field="bind-profile" data-side="${side}" aria-label="Profile">${opts.join("")}</select>`;
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
  return `<div class="choices" role="radiogroup" aria-label="${name} button action">${opts}</div><div class="extra">${extra}</div>`;
}

// ---------- macros page ----------

function macrosPage() {
  const macros = mcfg().macros;
  if (!mv.macroId && macros.length) mv.macroId = macros[0].id;
  const list = macros
    .map(
      (m) => `<button class="mrow ${m.id === mv.macroId ? "sel" : ""}" data-act="macro-open" data-id="${m.id}">
        <span class="mname">${esc(m.name || "Untitled")}</span>
        <span class="mmeta">${m.steps.filter((s) => s.type !== "delay").length} actions · ${modeLabel(m.mode)}</span>
      </button>`,
    )
    .join("");
  const m = currentMacro();
  const editor = m
    ? macroEditor(m)
    : `<div class="empty"><b>No macros yet</b>Record keys and clicks once, then bind the macro to a side button or a keyboard key.<br><button class="btn" data-act="macro-new">${icons.plus}New macro</button></div>`;
  return `<div class="page mpage" data-key="macros">
    <aside class="mlist">
      <div class="list-head"><span class="sec-title">Macros</span><button class="tbtn" data-act="macro-new">${icons.plus}New</button></div>
      ${list}
    </aside>
    <section class="meditor">${editor}</section>
  </div>`;
}

function macroEditor(m) {
  const deleting = confirmDelete === m.id;
  const usedBy = [];
  for (const p of mcfg().profiles) {
    for (const side of ["back", "forward"]) {
      if (p[side].type === "macro" && p[side].id === m.id) usedBy.push(`${p.name} · ${SIDE_NAMES[side]}`);
    }
  }
  usedBy.push(...keyboardMacroUses(m.id));
  const into = recording && recording.macroId !== m.id ? mcfg().macros.find((x) => x.id === recording.macroId) : null;
  const recBar = recording
    ? `<div class="rec-bar"><span class="rec-dot"></span><span class="rec-time" id="rec-time">0.0s</span>
        <span class="grow">${into ? `Recording into "${esc(into.name)}".` : "Recording keys and clicks."} Clicks inside this window are ignored.</span>
        <button class="tbtn" data-act="rec-stop">${icons.stop}Stop</button></div>`
    : "";
  const addKeyListening = keyCapture && keyCapture.kind === "step";
  const hint = m.mode === "once" ? "Plays each time the button is pressed." : m.mode === "whileHeld" ? "Loops for as long as the button is held." : "First press starts looping, second press stops.";
  const actions = m.steps.filter((s) => s.type !== "delay").length;
  const delays = m.steps.filter((s) => s.type === "delay").reduce((t, s) => t + s.ms, 0);

  const playback = {
    span: 2,
    html: (end) => `<div class="cell span2 ${end}"><div class="cap">Playback</div><div class="seg" style="margin-top:5px">${MODES.map(([v, l]) => `<button class="${m.mode === v ? "on" : ""}" data-act="macro-mode" data-v="${v}">${l}</button>`).join("")}</div></div>`,
  };
  const repeat = {
    span: 1,
    html: (end) => `<div class="cell ${end}"><div class="cap">Repeat</div>${m.mode === "once" ? `<div class="inline" style="margin-top:4px"><input class="field num" type="number" min="1" max="1000" value="${m.repeat}" data-field="macro-repeat" aria-label="Repeat count" style="width:64px;height:28px" /> times</div>` : `<div class="val text dim">Held or toggled</div>`}</div>`,
  };

  return `<div class="canvas-head">
      <input class="pname" value="${esc(m.name)}" data-field="macro-name" spellcheck="false" aria-label="Macro name" />
      <div class="pactions">
        <button class="btn" data-act="macro-test" ${m.steps.length && !recording ? "" : "disabled"} title="Plays once after 2 seconds, so you can click into the target window">${icons.play}Test</button>
        <button class="tbtn ${deleting ? "confirm" : "danger"}" data-act="macro-delete">${icons.trash}${deleting ? "Confirm delete" : "Delete"}</button>
      </div>
    </div>
    ${plate([
      playback,
      repeat,
      cell("len", "Length", `${actions}<small>actions</small> ${delays ? `${(delays / 1000).toFixed(2)}<small>s</small>` : ""}`),
      cell("used", "Bound to", usedBy.length ? esc(usedBy.join(", ")) : "Not bound to a button", { span: 2, cls: `text ${usedBy.length ? "" : "dim"}` }),
      cell("hint", "Behaviour", hint, { span: 2, cls: "text" }),
    ])}

    <div class="steps-head">
      <span class="sec-title">Steps<small>${actions} actions${m.steps.length > actions ? `, ${m.steps.length - actions} delays` : ""}</small></span>
      <div class="tools">
        ${recording ? "" : `<button class="tbtn rec" data-act="rec-start"><span class="rdot"></span>${m.steps.length ? "Re-record" : "Record"}</button>`}
        <button class="tbtn" data-act="step-add-key">${icons.key}${addKeyListening ? "Press a key…" : "Add key"}</button>
        <button class="tbtn" data-act="step-add-delay">${icons.clock}Add delay</button>
        <span class="sep"></span>
        <span class="inline">Every delay <input class="field num" type="number" min="0" value="50" id="all-delays" aria-label="Delay in milliseconds" /> ms</span>
        <button class="tbtn" data-act="delays-set">Apply</button>
        <button class="tbtn" data-act="delays-clear">Remove delays</button>
      </div>
    </div>
    ${recBar}
    <div class="steps">${m.steps.map(stepHtml).join("") || `<div class="empty">No steps yet. Record your keys and clicks, or add them one by one.</div>`}</div>`;
}

function stepHtml(s, i) {
  let body;
  const arrow = (down) => `<span class="arrow ${down ? "down" : "up"}" title="${down ? "Press" : "Release"}">${down ? icons.down : icons.up}</span>`;
  if (s.type === "delay") {
    body = `${icons.clock}<span>Wait</span><input class="field num" type="number" min="0" value="${s.ms}" data-field="step-ms" data-i="${i}" aria-label="Wait in milliseconds" /><span>ms</span>`;
  } else if (s.type === "key") {
    body = `${arrow(s.down)}<kbd>${esc(s.key.name)}</kbd><span class="muted">${s.down ? "press" : "release"}</span>`;
  } else {
    body = `${arrow(s.down)}${icons.mouse}<span>${MOUSE_NAMES[s.button]}</span><span class="muted">${s.down ? "press" : "release"}</span>`;
  }
  return `<div class="step ${s.type}">
    <span class="step-idx">${i + 1}</span>
    <span class="step-kind">${body}</span>
    <div class="step-tools">
      <button data-act="step-up" data-i="${i}" title="Move up" aria-label="Move step up">${icons.up}</button>
      <button data-act="step-down" data-i="${i}" title="Move down" aria-label="Move step down">${icons.down}</button>
      <button class="del" data-act="step-del" data-i="${i}" title="Remove" aria-label="Remove step">${icons.x}</button>
    </div>
  </div>`;
}

// ---------- device page ----------

function mouseDevicePage() {
  const d = M.device;
  const s = mouseStatus();
  return `<div class="page fpage" data-key="mouse-device"><div class="fpage-inner">
    <h1 class="h1">${PRODUCTS.mouse.name}</h1>
    ${plate([
      statusCell("status", s),
      battery("bat", d.battery, d.charging),
      cell("dpi", "Current DPI", d.dpi ?? "—", { cls: d.dpi ? "" : "dim" }),
      cell("rate", "Report rate", d.rate ? `${d.rate}<small>Hz</small>` : "—", { cls: d.rate ? "" : "dim" }),
      cell("name", "Reported as", esc(d.name || "—"), { span: 2, cls: "text" }),
      cell("profile", "Active profile", esc((mcfg().profiles.find((p) => p.id === mcfg().active) || {}).name || "—"), { span: 2, cls: "text" }),
    ])}
    ${s.hint ? `<p class="hint">${s.hint}</p>` : ""}

    <h2 class="h2">Switching</h2>
    <div class="row">
      <div><div class="rtitle">Next-profile shortcut</div><p class="hint">Cycle through every profile with one shortcut.</p></div>
      ${hotkeyField("next", mcfg().settings.nextProfileHotkey)}
    </div>
    ${errorsHtml()}
    <p class="hint">Profiles also switch from the tray menu, each profile's own shortcut, or a side button set to "Next profile" or "Switch to profile". kit re-applies the active profile whenever the mouse wakes. G HUB must stay closed, or the mouse won't answer kit.</p>
  </div></div>`;
}

// ---------- actions ----------

function defaultAction(type) {
  switch (type) {
    case "keys": return { type, keys: [] };
    case "macro": return { type, id: mcfg().macros[0]?.id || "" };
    case "dpiShift": return { type, dpi: 400 };
    case "profile": return { type, id: (mcfg().profiles.find((q) => q.id !== profile().id) || profile()).id };
    default: return { type };
  }
}

async function activateProfile(id) {
  // Send pending edits first, or the state that comes back would drop them.
  await mouseSaver.flush();
  M = await invoke("activate_profile", { id });
  render();
}

async function captureKeys(kind) {
  // Remember what the keys are for: the user may switch tabs while capturing.
  const targetId = kind === "step" ? mv.macroId : profile().id;
  await captureCombo(kind, (keys) => {
    if (kind === "step") {
      const m = mcfg().macros.find((x) => x.id === targetId);
      if (m) {
        for (const key of keys) m.steps.push({ type: "key", key, down: true });
        for (const key of [...keys].reverse()) m.steps.push({ type: "key", key, down: false });
      }
    } else {
      const p = mcfg().profiles.find((x) => x.id === targetId);
      if (p) p[kind] = { type: "keys", keys };
    }
    mouseSaver.save(0);
  });
}

// Let the backend capture one key combination (it also sees keys typed into
// this window), then hand the keys to `onKeys`. Clicking again cancels.
async function captureCombo(kind, onKeys) {
  if (keyCapture) {
    await invoke("cancel_capture");
    return;
  }
  keyCapture = { kind };
  render();
  await invoke("pause_hotkeys");
  const keys = await invoke("capture_keys");
  invoke("resume_hotkeys");
  keyCapture = null;
  if (keys && keys.length) onKeys(keys);
  render();
}

async function startRecording() {
  await invoke("start_recording");
  recording = { start: performance.now(), macroId: mv.macroId };
  recording.timer = setInterval(() => {
    const el = $("#rec-time");
    if (el) el.textContent = ((performance.now() - recording.start) / 1000).toFixed(1) + "s";
  }, 100);
  render();
}

async function stopRecording() {
  const steps = await invoke("stop_recording");
  clearInterval(recording.timer);
  // Into the macro the recording started on, even if another one is open now.
  const m = mcfg().macros.find((x) => x.id === recording.macroId);
  recording = null;
  if (m) {
    m.steps = steps;
    mouseSaver.save(0);
    toast(`Recorded ${steps.filter((s) => s.type !== "delay").length} actions`);
  } else {
    toast("Recording discarded: the macro was deleted");
  }
  render();
}

function moveStep(i, dir) {
  const steps = currentMacro().steps;
  const j = i + dir;
  if (j < 0 || j >= steps.length) return;
  [steps[i], steps[j]] = [steps[j], steps[i]];
  mouseSaver.save(0);
  render();
}

function setHotkey(target, value) {
  if (target === "kprofile" || target === "knext") {
    if (target === "knext") kcfg().nextProfileHotkey = value;
    else keyProfile().hotkey = value;
    keyboardSaver.save(0);
  } else {
    if (target === "next") mcfg().settings.nextProfileHotkey = value;
    else profile().hotkey = value;
    mouseSaver.save(0);
  }
  setHotkeyCapture(null);
  render();
}

// Global hotkeys are off while capturing, so a combo that is already in use
// reaches this window instead of switching profiles.
function setHotkeyCapture(target) {
  const was = hotkeyCapture;
  hotkeyCapture = target;
  if (!was && target) invoke("pause_hotkeys");
  else if (was && !target) invoke("resume_hotkeys");
}

Object.assign(handlers, {
  "profile-open"(el) {
    setHotkeyCapture(null);
    mv.profileId = el.dataset.id;
    render();
  },
  "profile-new"() {
    const p = { id: newId(), name: `Profile ${mcfg().profiles.length + 1}`, dpiStages: [400, 800, 1600], dpiIndex: 1, reportRate: 1000, back: { type: "default" }, forward: { type: "default" }, hotkey: null };
    mcfg().profiles.push(p);
    mv.profileId = p.id;
    mouseSaver.save(0);
    render();
    focusTitle();
  },
  "profile-duplicate"() {
    const p = structuredClone(profile());
    p.id = newId();
    p.name = `${p.name} copy`;
    p.hotkey = null;
    mcfg().profiles.splice(mcfg().profiles.indexOf(profile()) + 1, 0, p);
    mv.profileId = p.id;
    mouseSaver.save(0);
    render();
  },
  "profile-delete"() {
    const p = profile();
    armDelete(p.id, () => {
      mcfg().profiles = mcfg().profiles.filter((q) => q.id !== p.id);
      if (mcfg().active === p.id) mcfg().active = mcfg().profiles[0].id;
      for (const q of mcfg().profiles) {
        for (const side of ["back", "forward"]) if (q[side].type === "profile" && q[side].id === p.id) q[side] = { type: "default" };
      }
      mv.profileId = mcfg().profiles[0].id;
      mouseSaver.save(0);
      toast(`Deleted "${p.name}"`);
    });
  },
  activate() {
    activateProfile(profile().id);
  },
  "side-pick"(el) {
    mv.side = el.dataset.side;
    render();
  },
  "bind-type"(el) {
    const p = profile();
    if (p[mv.side].type === el.dataset.v) return;
    p[mv.side] = defaultAction(el.dataset.v);
    mouseSaver.save(0);
    render();
  },
  "stage-default"(el, e) {
    if (e.target.closest("input, .x")) return;
    profile().dpiIndex = Number(el.dataset.i);
    mouseSaver.save(0);
    render();
  },
  "stage-remove"(el) {
    const p = profile();
    const i = Number(el.dataset.i);
    p.dpiStages.splice(i, 1);
    if (p.dpiIndex >= i && p.dpiIndex > 0) p.dpiIndex--;
    mouseSaver.save(0);
    render();
  },
  "stage-add"() {
    const p = profile();
    const last = p.dpiStages[p.dpiStages.length - 1];
    p.dpiStages.push(clampDpi(last * 2));
    mouseSaver.save(0);
    render();
    document.querySelector(`.stage-cell input[data-i="${p.dpiStages.length - 1}"]`)?.select();
  },
  rate(el) {
    profile().reportRate = Number(el.dataset.v);
    mouseSaver.save(0);
    render();
  },
  "keys-capture"(el) {
    captureKeys(el.dataset.side);
  },
  "hotkey-set"(el) {
    stopListening();
    setHotkeyCapture(hotkeyCapture === el.dataset.target ? null : el.dataset.target);
    render();
  },
  "hotkey-clear"(el) {
    setHotkey(el.dataset.target, null);
  },
  "macro-new"() {
    const m = { id: newId(), name: `Macro ${mcfg().macros.length + 1}`, mode: "once", repeat: 1, steps: [] };
    mcfg().macros.push(m);
    mv.macroId = m.id;
    mouseSaver.save(0);
    render();
    focusTitle();
  },
  "macro-open"(el) {
    mv.macroId = el.dataset.id;
    render();
  },
  "macro-delete"() {
    const m = currentMacro();
    armDelete(m.id, () => {
      mcfg().macros = mcfg().macros.filter((x) => x.id !== m.id);
      for (const p of mcfg().profiles) {
        for (const side of ["back", "forward"]) if (p[side].type === "macro" && p[side].id === m.id) p[side] = { type: "default" };
      }
      mv.macroId = mcfg().macros[0]?.id || null;
      mouseSaver.save(0);
      toast(`Deleted "${m.name}"`);
    });
  },
  "macro-mode"(el) {
    currentMacro().mode = el.dataset.v;
    mouseSaver.save(0);
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
    mouseSaver.save(0);
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
    mouseSaver.save(0);
    render();
  },
  "delays-set"() {
    const ms = clampInt($("#all-delays").value, 0, MAX_DELAY);
    const m = currentMacro();
    // Normalise: one delay between every pair of actions.
    const actions = m.steps.filter((s) => s.type !== "delay");
    m.steps = actions.flatMap((s, i) => (i && ms ? [{ type: "delay", ms }, s] : [s]));
    mouseSaver.save(0);
    render();
  },
  "delays-clear"() {
    const m = currentMacro();
    m.steps = m.steps.filter((s) => s.type !== "delay");
    mouseSaver.save(0);
    render();
  },
});

Object.assign(inputFields, {
  "profile-name"(el) {
    profile().name = el.value;
    mouseSaver.save();
    render();
  },
  "macro-name"(el) {
    currentMacro().name = el.value;
    mouseSaver.save();
    render();
  },
  "step-ms"(el) {
    currentMacro().steps[Number(el.dataset.i)].ms = clampInt(el.value, 0, MAX_DELAY);
    mouseSaver.save();
  },
  "macro-repeat"(el) {
    currentMacro().repeat = clampInt(el.value, 1, 1000);
    mouseSaver.save();
  },
});

// Numbers that need clamping are committed on change (blur / Enter), not per keystroke.
Object.assign(changeFields, {
  stage(el) {
    const v = clampDpi(el.value);
    el.value = v;
    profile().dpiStages[Number(el.dataset.i)] = v;
    mouseSaver.save(0);
    render();
  },
  "bind-macro"(el) {
    profile()[el.dataset.side].id = el.value;
    mouseSaver.save(0);
    render();
  },
  "bind-profile"(el) {
    profile()[el.dataset.side].id = el.value;
    mouseSaver.save(0);
    render();
  },
  "bind-dpi"(el) {
    const v = clampDpi(el.value);
    el.value = v;
    profile()[el.dataset.side].dpi = v;
    mouseSaver.save(0);
    render();
  },
});

// Windows doesn't run the global keyboard hook for keys typed into our own
// window, so forward them to the recorder ourselves while it is listening.
function forwardKey(e, down) {
  if (!keyCapture && !recording) return false;
  e.preventDefault();
  if (!e.repeat) invoke("ui_key", { code: e.code, down });
  return true;
}

// Returns true when the key was used for a hotkey capture.
function captureHotkey(e) {
  if (!hotkeyCapture) return false;
  e.preventDefault();
  if (e.key === "Escape") {
    setHotkeyCapture(null);
    render();
    return true;
  }
  const mods = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(e.code) || ["Control", "Alt", "Shift", "Meta"].includes(e.key)) return true;
  // Some synthetic/remapped keyboards leave `code` empty; fall back to `key`.
  let code = e.code;
  if (!code && /^[a-z]$/i.test(e.key)) code = "Key" + e.key.toUpperCase();
  else if (!code && /^[0-9]$/.test(e.key)) code = "Digit" + e.key;
  if (!code) return true;
  // A bare key would be taken from every app: typing it would switch profiles.
  if (!mods.length && !/^F\d+$/.test(code)) {
    toast("Add Ctrl, Alt, Shift or Win (F-keys work on their own)");
    return true;
  }
  setHotkey(hotkeyCapture, [...mods, code].join("+"));
  return true;
}

// Leaving the mouse pages ends a shortcut capture.
function leaveMouse() {
  setHotkeyCapture(null);
}

function onMouseState(payload) {
  if (mouseSaver.unsaved()) {
    // Keep edits the backend doesn't have yet; take only the profile switch.
    const config = { ...mcfg(), active: payload.config.active };
    M = { ...payload, config };
  } else {
    M = payload;
  }
  if (!mcfg().profiles.some((p) => p.id === mv.profileId)) mv.profileId = mcfg().active;
}
