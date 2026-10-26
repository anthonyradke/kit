"use strict";
// The shell: first-run setup, device rail, title bar tabs, home screen, app settings, events and boot.

let A = null; // app config: { setupDone, mouse, keyboard, headset }

const PAGES = {
  mouse: { title: () => PRODUCTS.mouse.name, tabs: [["profiles", "Profiles"], ["macros", "Macros"], ["device", "Device"]] },
  keyboard: { title: () => keyboardName(), tabs: [["keys", "Keys"], ["lighting", "Lighting"], ["macros", "Macros"], ["device", "Device"]] },
  headset: { title: () => PRODUCTS.headset.name, tabs: [["sound", "Sound"], ["mic", "Mic"], ["device", "Device"]] },
};
const DEVICE_ORDER = ["mouse", "keyboard", "headset"];
const devices = () => DEVICE_ORDER.filter((d) => A[d]);

let autostart = false;
let upd = { current: "", available: null, notes: null, checking: false, checked: false, progress: null, error: null };
// Which page is open; remembered across window closes (the webview is destroyed on close).
const view = { page: "home", mouse: "profiles", keyboard: "keys", headset: "sound", ...JSON.parse(localStorage.getItem("kit.view") || "{}") };

function go(page, tab) {
  if (view.page === "mouse" && (page !== "mouse" || tab !== view.mouse)) leaveMouse();
  if (view.page === "keyboard") leaveKeyboard();
  if (view.page === "keyboard" && view.keyboard === "macros") leaveMouse();
  view.page = page;
  if (tab) view[page] = tab;
  localStorage.setItem("kit.view", JSON.stringify({ page: view.page, mouse: view.mouse, keyboard: view.keyboard, headset: view.headset }));
  render();
}

// ---------- chrome ----------

function render() {
  if (!M || !H || !K || !A) return;
  document.body.classList.toggle("in-setup", !A.setupDone);
  if (!A.setupDone) {
    morph($("#rail"), "");
    morph($("#tb-title"), "Setup");
    morph($("#tabs"), "");
    morph($("#main"), setupPage());
    return;
  }
  // A page for a device that was removed in Settings goes home.
  if (PAGES[view.page] && !A[view.page]) view.page = "home";
  renderRail();
  renderTitle();
  morph($("#main"), pageHtml());
  watchGraph();
  flashChangedCells();
}

function pageHtml() {
  switch (view.page) {
    case "mouse":
      return view.mouse === "macros" ? macrosPage() : view.mouse === "device" ? mouseDevicePage() : profilesPage();
    case "keyboard":
      return view.keyboard === "macros" ? macrosPage() : view.keyboard === "lighting" ? lightingPage() : view.keyboard === "device" ? keyboardDevicePage() : keysPage();
    case "headset":
      return view.headset === "mic" ? micPage() : view.headset === "device" ? headsetDevicePage() : soundPage();
    case "settings":
      return settingsPage();
    default:
      return homePage();
  }
}

function renderTitle() {
  const p = PAGES[view.page];
  morph($("#tb-title"), p ? esc(p.title()) : view.page === "settings" ? "Settings" : "");
  const tabs = p ? p.tabs.map(([id, label]) => `<button class="tab ${view[view.page] === id ? "on" : ""}" data-act="${view.page}-tab" data-tab="${id}">${label}</button>`).join("") : "";
  morph($("#tabs"), tabs);
}

function renderRail() {
  const bat = (b) => (b == null ? "" : `<span class="ri-bat ${b <= 15 ? "low" : ""}">${b}%</span>`);
  const pip = (live, present) => `<span class="pip ${live ? "live" : present ? "" : "off"}"></span>`;
  const item = (page, inner, label) =>
    `<button class="rail-item ${view.page === page ? "on" : ""}" data-act="go" data-page="${page}" title="${label}" aria-label="${label}" ${view.page === page ? 'aria-current="page"' : ""}>${inner}</button>`;
  const dev = {
    mouse: () => {
      const d = M.device;
      return item("mouse", `${pip(d.connected, d.present)}<img src="${productImg("mouse")}" alt="" />${bat(d.connected ? d.battery : null) || '<span class="ri-label">Mouse</span>'}`, PRODUCTS.mouse.name);
    },
    keyboard: () => {
      const s = kst();
      return item("keyboard", `${pip(s.connected, s.present)}<img class="wide" src="${productImg("keyboard")}" alt="" />${bat(s.connected ? s.battery : null) || '<span class="ri-label">Keyboard</span>'}`, keyboardName());
    },
    headset: () => {
      const h = H.headset;
      return item("headset", `${pip(h.connected, h.present)}<img src="${productImg("headset")}" alt="" />${bat(h.connected ? h.battery : null) || '<span class="ri-label">Headset</span>'}`, PRODUCTS.headset.name);
    },
  };
  morph(
    $("#rail"),
    item("home", `${icons.home}<span class="ri-label">Home</span>`, "Home") +
      devices().map((d) => dev[d]()).join("") +
      `<div class="rail-foot">${item("settings", `${upd.available ? '<span class="pip"></span>' : ""}${icons.gear}<span class="ri-label">Settings</span>`, "Settings")}</div>`,
  );
}

// ---------- home ----------

function homePage() {
  const list = devices();
  // Three devices share the width, so their plates fold to two columns.
  const cols = list.length > 2 ? 2 : 4;
  const sections = list.map((d) => homeDevice(d, cols)).join("");
  const empty = list.length
    ? ""
    : `<section class="dev"><div class="empty"><b>No devices picked</b>Choose which devices are on this PC in Settings.<br><button class="btn" data-act="go" data-page="settings">Open settings</button></div></section>`;
  return `<div class="page home" data-key="home" style="--n:${Math.max(1, list.length)}">${sections}${empty}</div>`;
}

function homeDevice(d, cols) {
  if (d === "mouse") {
    const p = mcfg().profiles.find((q) => q.id === mcfg().active) || mcfg().profiles[0];
    const sideVal = (side) => esc(actionSummary(p, side));
    const cells = [
      ...mousePlateCells(p, "m-"),
      cell("m-profile", "Profile", esc(p.name), { span: 2, cls: "text" }),
      cell("m-side", "Side buttons", `${sideVal("back")} <small>/</small> ${sideVal("forward")}`, { span: 2, cls: "text" }),
    ];
    return devColumn("mouse", M.device.connected, productImg("mouse"), PRODUCTS.mouse.name, cells, mouseStatus().hint, `
      <button class="btn" data-act="go" data-page="mouse" data-tab="profiles">Open mouse</button>
      <button class="btn quiet" data-act="go" data-page="mouse" data-tab="macros">Macros</button>`, cols);
  }
  if (d === "keyboard") {
    return devColumn("keyboard", kst().connected, productImg("keyboard"), keyboardName(), keyboardPlateCells(), keyStatus().hint, `
      <button class="btn" data-act="go" data-page="keyboard" data-tab="keys">Open keyboard</button>
      <button class="btn quiet" data-act="go" data-page="keyboard" data-tab="lighting">Lighting</button>`, cols, "wide");
  }
  const h = H.headset;
  const cells = [
    statusCellH("h-status"),
    battery("h-bat", h.connected ? h.battery : null, h.charging),
    micCell("h-mic"),
    cell("h-side", "Sidetone", hcfg().headset.sidetone || "Off", { cls: hcfg().headset.sidetone ? "" : "text dim" }),
    cell("h-preset", "EQ preset · on headset", esc(preset().name), { span: 2, cls: "text" }),
    defaultCell("h-default", 2),
  ];
  const second = H.audio.output && !H.audio.headsetIsDefault ? `<button class="btn quiet" data-act="use-headset">Use as Windows default</button>` : `<button class="btn quiet" data-act="go" data-page="headset" data-tab="mic">Mic</button>`;
  return devColumn("headset", h.connected, productImg("headset"), PRODUCTS.headset.name, cells, headsetStatus().hint, `
    <button class="btn" data-act="go" data-page="headset" data-tab="sound">Open headset</button>${second}`, cols, "tall");
}

function devColumn(page, connected, img, name, cells, hint, actions, cols, imgCls = "") {
  return `<section class="dev">
    <div class="stage ${connected ? "" : "off"}" data-act="go" data-page="${page}"><img class="${imgCls}" src="${img}" alt="${esc(name)}" /></div>
    <h2 class="dev-name">${esc(name)}</h2>
    ${plate(cells, cols)}
    <p class="hint dev-hint">${hint || ""}</p>
    <div class="dev-actions">${actions}</div>
  </section>`;
}

// ---------- first-run setup ----------
// Three steps: which devices (and colors) are here, which vendor apps to close, finish.

const setup = { step: 0, draft: null, found: null, detecting: false, vendors: null, autostart: true };

function setupDraft() {
  if (!setup.draft) {
    setup.draft = {
      mouse: A.mouse ? { ...A.mouse } : null,
      keyboard: A.keyboard ? { ...A.keyboard } : null,
      headset: A.headset ? { ...A.headset } : null,
      // Remember a choice even while its device is switched off.
      mouseColor: A.mouse?.color || "white",
      headsetColor: A.headset?.color || "white",
      keyboardModel: A.keyboard?.model || "nk65",
    };
  }
  return setup.draft;
}

async function detect() {
  setup.detecting = true;
  render();
  try {
    setup.found = await invoke("detect_devices");
  } catch {
    setup.found = { mouse: false, headset: false, keyboard: null };
  }
  setup.detecting = false;
  const d = setupDraft();
  // Preselect what is plugged in, the first time only.
  if (!setup.touched) {
    if (setup.found.mouse && !d.mouse) d.mouse = { color: d.mouseColor };
    if (setup.found.headset && !d.headset) d.headset = { color: d.headsetColor };
    if (setup.found.keyboard && !d.keyboard) {
      d.keyboardModel = setup.found.keyboard;
      d.keyboard = { model: d.keyboardModel, color: "white" };
    }
  }
  render();
}

async function checkVendors() {
  setup.vendors = await invoke("vendor_apps");
  render();
}

function setupPage() {
  const d = setupDraft();
  const steps = ["Your devices", "Close the old apps", "Finish"];
  const head = plate(
    steps.map((label, i) => ({
      span: 1,
      html: (end) => `<div class="cell step-cell ${i === setup.step ? "now" : ""} ${i < setup.step ? "done" : ""} ${end}"><div class="cap">Step ${i + 1}</div><div class="val text">${i < setup.step ? icons.check : ""}${label}</div></div>`,
    })),
    3,
    "steps-plate",
  );
  let body;
  if (setup.step === 0) body = setupDevices(d);
  else if (setup.step === 1) body = setupVendors(d);
  else body = setupFinish(d);
  return `<div class="page setup" data-key="setup-${setup.step}">
    <div class="setup-head">
      <div><h1 class="h1">${setup.step === 0 ? "Welcome to kit" : steps[setup.step]}</h1>
      <p class="hint">${setup.step === 0 ? "kit replaces G HUB, SteelSeries GG and VIA for the devices on this PC. Pick the ones you have; kit only shows those." : setup.step === 1 ? "kit talks to the devices directly. The vendor apps do too, and they fight over the same settings." : "That's it. You can change any of this later in Settings."}</p></div>
      ${head}
    </div>
    ${body}
  </div>`;
}

function setupDevices(d) {
  const found = setup.found;
  const foundMark = (yes) =>
    setup.detecting ? `<span class="off-mark">Looking…</span>` : !found ? "" : yes ? `<span class="live-mark">Plugged in</span>` : `<span class="off-mark">Not detected</span>`;
  const colorSeg = (dev) =>
    `<div class="seg">${["white", "black"].map((c) => `<button class="${d[`${dev}Color`] === c ? "on" : ""}" data-act="setup-color" data-dev="${dev}" data-v="${c}">${c === "white" ? "White" : "Black"}</button>`).join("")}</div>`;
  const card = (dev, name, img, imgCls, options, isFound) => {
    const on = !!d[dev];
    return `<section class="pick ${on ? "on" : ""}">
      <button class="pick-stage" data-act="setup-toggle" data-dev="${dev}" aria-pressed="${on}"><img class="${imgCls}" src="${img}" alt="${esc(name)}" /></button>
      <div class="pick-head"><h2 class="dev-name">${esc(name)}</h2>
        <label class="switch" title="On this PC"><input type="checkbox" data-field="setup-have" data-dev="${dev}" ${on ? "checked" : ""} aria-label="I have the ${esc(name)}" /><span></span></label></div>
      <div class="pick-line">${foundMark(isFound)}</div>
      <div class="pick-opts">${options}</div>
    </section>`;
  };
  const kbModel = d.keyboardModel;
  const kbSeg = `<div class="seg">${[["nk65", "NK65 Milkshake"], ["proxtkl", "PRO X TKL"]].map(([v, l]) => `<button class="${kbModel === v ? "on" : ""}" data-act="setup-kb" data-v="${v}">${l}</button>`).join("")}</div>`;
  return `<div class="picks">
      ${card("mouse", PRODUCTS.mouse.name, productImg("mouse", "side", d.mouseColor), "", colorSeg("mouse"), found?.mouse)}
      ${card("keyboard", PRODUCTS[kbModel].long, productImg("keyboard", null, kbModel), "wide", kbSeg, found?.keyboard === kbModel)}
      ${card("headset", PRODUCTS.headset.name, productImg("headset", null, d.headsetColor), "tall", colorSeg("headset"), found?.headset)}
    </div>
    <div class="setup-actions">
      <button class="btn quiet" data-act="setup-detect" ${setup.detecting ? "disabled" : ""}>${setup.detecting ? "Looking…" : "Look again"}</button>
      <span class="grow hint">${d.mouse || d.keyboard || d.headset ? "" : "Pick at least one device."}</span>
      <button class="btn" data-act="setup-next" ${d.mouse || d.keyboard || d.headset ? "" : "disabled"}>Next</button>
    </div>`;
}

function setupVendors(d) {
  const running = setup.vendors || [];
  const row = (name, why, how) => {
    const on = running.includes(name);
    return `<div class="row"><div><div class="rtitle">${name}</div><p class="hint">${why} ${how}</p></div>
      ${setup.vendors === null ? `<span class="off-mark">Checking…</span>` : on ? `<span class="warn-mark">Running</span>` : `<span class="ok-mark">${icons.check}Closed</span>`}</div>`;
  };
  const rows = [];
  if (d.mouse || d.keyboard?.model === "proxtkl") rows.push(row("Logitech G HUB", "It holds the Logitech receivers, so the mouse and keyboard won't answer kit.", "Quit it from its tray icon and turn off \"Start G HUB on system startup\", or uninstall it."));
  if (d.headset) rows.push(row("SteelSeries GG", "Sonar takes over your default audio devices and GG rewrites the headset's settings.", "Quit it from its tray icon, or uninstall it."));
  if (d.keyboard?.model === "nk65") rows.push(`<div class="row"><div><div class="rtitle">VIA</div><p class="hint">Close VIA's browser tab; kit now does its job, and two apps talking to the keyboard at once get in each other's way.</p></div><span class="muted">In your browser</span></div>`);
  const blocking = running.length > 0;
  return `<div class="setup-body">${rows.join("") || '<p class="hint">Nothing to close for these devices.</p>'}</div>
    <div class="setup-actions">
      <button class="btn quiet" data-act="setup-back">Back</button>
      <button class="btn quiet" data-act="setup-vendors">Check again</button>
      <span class="grow hint">${blocking ? "You can go on anyway; close them before using kit." : ""}</span>
      <button class="btn" data-act="setup-next">Next</button>
    </div>`;
}

function setupFinish(d) {
  const line = (dev, name, detail) => (d[dev] ? cell(`s-${dev}`, name, esc(detail), { span: 1, cls: "text" }) : cell(`s-${dev}`, name, "Not on this PC", { span: 1, cls: "text dim" }));
  const cap = (c) => c[0].toUpperCase() + c.slice(1);
  return `<div class="setup-body">
      ${plate([
        line("mouse", "Mouse", `${PRODUCTS.mouse.name} · ${cap(d.mouseColor)}`),
        line("keyboard", "Keyboard", PRODUCTS[d.keyboardModel].long),
        line("headset", "Headset", `${PRODUCTS.headset.name} · ${cap(d.headsetColor)}`),
      ], 3)}
      <div class="row" style="margin-top:18px">
        <div><div class="rtitle">Start kit with Windows</div><p class="hint">kit waits in the tray and puts your settings back on the devices whenever they wake up.</p></div>
        <label class="switch"><input type="checkbox" data-field="setup-autostart" ${setup.autostart ? "checked" : ""} aria-label="Launch at startup" /><span></span></label>
      </div>
    </div>
    <div class="setup-actions">
      <button class="btn quiet" data-act="setup-back">Back</button>
      <span class="grow"></span>
      <button class="btn" data-act="setup-finish">Open kit</button>
    </div>`;
}

function startSetup() {
  setup.step = 0;
  setup.draft = null;
  setup.touched = false;
  setup.found = null;
  setup.autostart = autostart;
  A = { ...A, setupDone: false };
  render();
  detect();
}

async function finishSetup() {
  const d = setupDraft();
  const config = {
    setupDone: true,
    mouse: d.mouse ? { color: d.mouseColor } : null,
    keyboard: d.keyboard ? { model: d.keyboardModel, color: "white" } : null,
    headset: d.headset ? { color: d.headsetColor } : null,
  };
  try {
    A = await invoke("app_save", { config });
  } catch (err) {
    toast(`Couldn't save: ${err}`);
    return;
  }
  if (setup.autostart !== autostart) autostart = await invoke("set_autostart", { enabled: setup.autostart }).catch(() => autostart);
  K = await invoke("keyboard_state");
  setup.draft = null;
  view.page = "home";
  render();
}

// ---------- settings ----------

function settingsPage() {
  const deviceRow = (dev, name, detail) =>
    `<div class="dev-line"><span class="cap">${dev}</span>${detail ? `<span>${esc(name)}</span><span class="muted">${esc(detail)}</span>` : `<span class="muted">Not on this PC</span>`}</div>`;
  const cap = (c) => (c ? c[0].toUpperCase() + c.slice(1) : "");
  return `<div class="page fpage" data-key="settings"><div class="fpage-inner">
    <h1 class="h1">Settings</h1>
    ${updateRow()}
    <div class="row">
      <div><div class="rtitle">Launch at startup</div><p class="hint">Start kit in the tray when you sign in to Windows.</p></div>
      <label class="switch"><input type="checkbox" data-field="autostart" ${autostart ? "checked" : ""} aria-label="Launch at startup" /><span></span></label>
    </div>

    <h2 class="h2">Devices</h2>
    <div class="row col">
      ${deviceRow("Mouse", PRODUCTS.mouse.name, A.mouse && cap(A.mouse.color))}
      ${deviceRow("Keyboard", A.keyboard ? keyboardName(true) : "", A.keyboard && (A.keyboard.model === "nk65" ? "VIA firmware" : "LIGHTSPEED"))}
      ${deviceRow("Headset", PRODUCTS.headset.name, A.headset && cap(A.headset.color))}
      <div><button class="btn quiet" data-act="setup-again">Change devices…</button></div>
    </div>

    <h2 class="h2">Files</h2>
    <dl class="kv">
      <dt>Mouse and macros</dt><dd class="path">${esc(M.configPath)}</dd>
      <dt>Keyboard</dt><dd class="path">${esc(K.configPath)}</dd>
      <dt>Headset</dt><dd class="path">${esc(H.configPath)}</dd>
    </dl>

    ${A.headset ? `<h2 class="h2">Noise suppression log</h2>
    <p class="hint">What Windows' audio engine reported when it loaded kit's noise suppression.</p>
    <div style="margin-top:10px"><button class="btn quiet" data-act="show-log">${hv.log === null ? "Show log" : "Refresh log"}</button></div>
    ${hv.log !== null ? `<div class="logbox">${esc(hv.log || "Empty: the audio engine hasn't loaded noise suppression yet.")}</div>` : ""}` : ""}

    <h2 class="h2">About</h2>
    <p class="hint">Closing this window keeps kit running in the tray; use Quit in the tray menu to exit. Keep Logitech G HUB, SteelSeries GG and VIA closed: they fight kit for the same devices.</p>
  </div></div>`;
}

function updateRow() {
  let title = `kit ${esc(upd.current)}`;
  let hint = "kit checks GitHub for a new version every few hours.";
  let btn = `<button class="btn quiet" data-act="check-update" ${upd.checking ? "disabled" : ""}>${upd.checking ? "Checking…" : "Check now"}</button>`;
  if (upd.progress != null) {
    title = `Updating to kit ${esc(upd.available)}`;
    hint = `Downloading, ${upd.progress}%. kit restarts on its own when it's done.`;
    btn = "";
  } else if (upd.available) {
    title = `kit ${esc(upd.available)} is available`;
    hint = `You have ${esc(upd.current)}. Updating takes a few seconds and restarts kit; your settings stay.${upd.notes ? `<br>${esc(upd.notes)}` : ""}`;
    btn = `<button class="btn" data-act="install-update">Update and restart</button>`;
  } else if (upd.error && upd.checked) {
    hint = `Couldn't check for updates: ${esc(upd.error)}`;
  } else if (upd.checked) {
    hint = "Up to date. kit checks GitHub for a new version every few hours.";
  }
  return `<div class="row">
      <div><div class="rtitle">${title}</div><p class="hint">${hint}</p></div>
      ${btn}
    </div>`;
}

// ---------- events ----------

Object.assign(handlers, {
  "win-min"() {
    appWindow.minimize();
  },
  "win-max"() {
    appWindow.toggleMaximize();
  },
  // Same as a native close: the webview is destroyed and the tray keeps running.
  "win-close"() {
    appWindow.close();
  },
  go(el) {
    go(el.dataset.page, el.dataset.tab);
  },
  "mouse-tab"(el) {
    go("mouse", el.dataset.tab);
  },
  "headset-tab"(el) {
    go("headset", el.dataset.tab);
  },
  async "check-update"() {
    upd = { ...upd, checking: true };
    render();
    upd = await invoke("check_update");
    render();
  },
  async "install-update"() {
    try {
      await invoke("install_update");
    } catch (err) {
      toast(`Update failed: ${err}`);
    }
  },
  async "show-log"() {
    hv.log = await invoke("apo_log");
    render();
  },
  "setup-toggle"(el) {
    toggleDevice(el.dataset.dev);
  },
  "setup-color"(el) {
    const d = setupDraft();
    d[`${el.dataset.dev}Color`] = el.dataset.v;
    d[el.dataset.dev] = { color: el.dataset.v };
    setup.touched = true;
    render();
  },
  "setup-kb"(el) {
    const d = setupDraft();
    d.keyboardModel = el.dataset.v;
    d.keyboard = { model: el.dataset.v, color: "white" };
    setup.touched = true;
    render();
  },
  "setup-detect"() {
    detect();
  },
  "setup-next"() {
    setup.step++;
    if (setup.step === 1) {
      setup.vendors = null;
      checkVendors();
    }
    render();
  },
  "setup-back"() {
    setup.step = Math.max(0, setup.step - 1);
    render();
  },
  "setup-vendors"() {
    checkVendors();
  },
  "setup-finish"() {
    finishSetup();
  },
  "setup-again"() {
    startSetup();
  },
});

function toggleDevice(dev, on) {
  const d = setupDraft();
  const want = on ?? !d[dev];
  if (!want) d[dev] = null;
  else if (dev === "keyboard") d.keyboard = { model: d.keyboardModel, color: "white" };
  else d[dev] = { color: d[`${dev}Color`] };
  setup.touched = true;
  render();
}

Object.assign(changeFields, {
  async autostart(el) {
    try {
      autostart = await invoke("set_autostart", { enabled: el.checked });
      toast(autostart ? "kit will start with Windows" : "kit won't start with Windows");
    } catch (err) {
      toast(`Couldn't change startup: ${err}`);
    }
    el.checked = autostart;
  },
  "setup-have"(el) {
    toggleDevice(el.dataset.dev, el.checked);
  },
  "setup-autostart"(el) {
    setup.autostart = el.checked;
  },
});

document.addEventListener("click", (e) => {
  const el = e.target.closest("[data-act]");
  if (!el || el.disabled) return;
  const fn = handlers[el.dataset.act];
  if (fn) fn(el, e);
});

document.addEventListener("input", (e) => {
  const fn = inputFields[e.target.dataset.field];
  if (fn) fn(e.target);
});

document.addEventListener("change", (e) => {
  const fn = changeFields[e.target.dataset.field];
  if (fn) fn(e.target);
});

document.addEventListener("keyup", (e) => {
  if (captureKeycodeUp(e)) return;
  forwardKey(e, false);
});

document.addEventListener("keydown", (e) => {
  if (captureKeycode(e)) return;
  if (e.key === "F11" && !e.repeat) {
    e.preventDefault();
    appWindow.isFullscreen().then((f) => appWindow.setFullscreen(!f));
    return;
  }
  if (forwardKey(e, true)) return;
  if (captureHotkey(e)) return;
  if (e.key === "Enter" && e.target.matches("input")) e.target.blur();
});

// ---------- window ----------

// The maximize button swaps to a restore icon while the window fills the screen
// (maximized, or fullscreen with F11).
const MAX_ICON = '<svg viewBox="0 0 12 12"><rect x="1.5" y="1.5" width="9" height="9" /></svg>';
const RESTORE_ICON = '<svg viewBox="0 0 12 12"><rect x="1.5" y="3.5" width="7" height="7" /><path d="M3.5 3.5v-2h7v7h-2" /></svg>';

async function syncMaxButton() {
  const [max, full] = await Promise.all([appWindow.isMaximized(), appWindow.isFullscreen()]);
  const btn = document.getElementById("win-max");
  const filled = max || full;
  btn.innerHTML = filled ? RESTORE_ICON : MAX_ICON;
  btn.title = full ? "Exit full screen (F11)" : filled ? "Restore" : "Maximize";
  btn.dataset.act = full ? "win-unfull" : "win-max";
  document.body.classList.toggle("filled", filled);
}
handlers["win-unfull"] = () => appWindow.setFullscreen(false);
appWindow.onResized(syncMaxButton);
syncMaxButton();

// ---------- boot ----------

(async () => {
  [M, H, K, A, autostart, upd] = await Promise.all([
    invoke("mouse_state"),
    invoke("headset_state"),
    invoke("keyboard_state"),
    invoke("app_config"),
    invoke("get_autostart"),
    invoke("update_state"),
  ]);
  mv.profileId = mcfg().active;
  render();
  if (!A.setupDone) detect();

  // State pushed by the backend (tray clicks, hotkeys, side buttons, hardware) is a hardware change.
  const hw = (fn) => (e) => {
    fn(e.payload);
    hardwareChange = true;
    render();
  };
  await listen("mouse-state", hw(onMouseState));
  await listen("mouse", hw((p) => (M.device = p)));
  await listen("keyboard-state", hw(onKeyboardState));
  await listen("keyboard", hw((p) => (K.status = p)));
  await listen("headset-state", hw(onHeadsetState));
  await listen("headset", hw((p) => (H.headset = p)));
  await listen("audio", hw((p) => (H.audio = p)));
  await listen("update", (e) => {
    upd = e.payload;
    render();
  });
})();
