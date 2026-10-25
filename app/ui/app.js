"use strict";
// The shell: device rail, title bar tabs, home screen, app settings, events and boot.

const PAGES = {
  mouse: { title: "PRO X Superlight", tabs: [["profiles", "Profiles"], ["macros", "Macros"], ["device", "Device"]] },
  headset: { title: "Arctis Nova 5", tabs: [["sound", "Sound"], ["mic", "Mic"], ["device", "Device"]] },
};

let autostart = false;
let upd = { current: "", available: null, notes: null, checking: false, checked: false, progress: null, error: null };
// Which page is open; remembered across window closes (the webview is destroyed on close).
const view = { page: "home", mouse: "profiles", headset: "sound", ...JSON.parse(localStorage.getItem("kit.view") || "{}") };

function go(page, tab) {
  if (view.page === "mouse" && (page !== "mouse" || tab !== view.mouse)) leaveMouse();
  view.page = page;
  if (tab) view[page] = tab;
  localStorage.setItem("kit.view", JSON.stringify({ page: view.page, mouse: view.mouse, headset: view.headset }));
  render();
}

// ---------- chrome ----------

function render() {
  if (!M || !H) return;
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
  morph($("#tb-title"), p ? esc(p.title) : view.page === "settings" ? "Settings" : "");
  const tabs = p ? p.tabs.map(([id, label]) => `<button class="tab ${view[view.page] === id ? "on" : ""}" data-act="${view.page}-tab" data-tab="${id}">${label}</button>`).join("") : "";
  morph($("#tabs"), tabs);
}

function renderRail() {
  const d = M.device;
  const h = H.headset;
  const bat = (b) => (b == null ? "" : `<span class="ri-bat ${b <= 15 ? "low" : ""}">${b}%</span>`);
  const pip = (live, present) => `<span class="pip ${live ? "live" : present ? "" : "off"}"></span>`;
  const item = (page, inner, label) =>
    `<button class="rail-item ${view.page === page ? "on" : ""}" data-act="go" data-page="${page}" title="${label}" aria-label="${label}" ${view.page === page ? 'aria-current="page"' : ""}>${inner}</button>`;
  morph(
    $("#rail"),
    item("home", `${icons.home}<span class="ri-label">Home</span>`, "Home") +
      item("mouse", `${pip(d.connected, d.present)}<img src="img/mouse-side.webp" alt="" />${bat(d.connected ? d.battery : null) || '<span class="ri-label">Mouse</span>'}`, "PRO X Superlight") +
      item("headset", `${pip(h.connected, h.present)}<img src="img/headset.webp" alt="" />${bat(h.connected ? h.battery : null) || '<span class="ri-label">Headset</span>'}`, "Arctis Nova 5") +
      `<div class="rail-foot">${item("settings", `${upd.available ? '<span class="pip"></span>' : ""}${icons.gear}<span class="ri-label">Settings</span>`, "Settings")}</div>`,
  );
}

// ---------- home ----------

function homePage() {
  const d = M.device;
  const h = H.headset;
  const p = mcfg().profiles.find((q) => q.id === mcfg().active) || mcfg().profiles[0];
  const pr = preset();
  const sideVal = (side) => esc(actionSummary(p, side));

  const mouseCells = [
    ...mousePlateCells(p, "m-"),
    cell("m-profile", "Profile", esc(p.name), { span: 2, cls: "text" }),
    cell("m-side", "Side buttons", `${sideVal("back")} <small>/</small> ${sideVal("forward")}`, { span: 2, cls: "text" }),
  ];
  const headsetCells = [
    statusCellH("h-status"),
    battery("h-bat", h.connected ? h.battery : null, h.charging),
    micCell("h-mic"),
    cell("h-side", "Sidetone", hcfg().headset.sidetone || "Off", { cls: hcfg().headset.sidetone ? "" : "text dim" }),
    cell("h-preset", "EQ preset · on headset", esc(pr.name), { span: 2, cls: "text" }),
    defaultCell("h-default", 2),
  ];
  const ms = mouseStatus();
  const hs = headsetStatus();

  return `<div class="page home" data-key="home">
    <section class="dev">
      <div class="stage ${d.connected ? "" : "off"}" data-act="go" data-page="mouse"><img src="img/mouse-side.webp" alt="White Logitech PRO X Superlight" /></div>
      <h2 class="dev-name">PRO X Superlight</h2>
      ${plate(mouseCells)}
      <p class="hint dev-hint">${ms.hint || ""}</p>
      <div class="dev-actions">
        <button class="btn" data-act="go" data-page="mouse" data-tab="profiles">Open mouse</button>
        <button class="btn quiet" data-act="go" data-page="mouse" data-tab="macros">Macros</button>
      </div>
    </section>
    <section class="dev">
      <div class="stage ${h.connected ? "" : "off"}" data-act="go" data-page="headset"><img class="tall" src="img/headset.webp" alt="White SteelSeries Arctis Nova 5" /></div>
      <h2 class="dev-name">Arctis Nova 5</h2>
      ${plate(headsetCells)}
      <p class="hint dev-hint">${hs.hint || ""}</p>
      <div class="dev-actions">
        <button class="btn" data-act="go" data-page="headset" data-tab="sound">Open headset</button>
        ${H.audio.output && !H.audio.headsetIsDefault ? `<button class="btn quiet" data-act="use-headset">Use as Windows default</button>` : `<button class="btn quiet" data-act="go" data-page="headset" data-tab="mic">Mic</button>`}
      </div>
    </section>
  </div>`;
}

// ---------- settings ----------

function settingsPage() {
  return `<div class="page fpage" data-key="settings"><div class="fpage-inner">
    <h1 class="h1">Settings</h1>
    ${updateRow()}
    <div class="row">
      <div><div class="rtitle">Launch at startup</div><p class="hint">Start kit in the tray when you sign in to Windows.</p></div>
      <label class="switch"><input type="checkbox" data-field="autostart" ${autostart ? "checked" : ""} aria-label="Launch at startup" /><span></span></label>
    </div>

    <h2 class="h2">Files</h2>
    <dl class="kv">
      <dt>Mouse settings</dt><dd class="path">${esc(M.configPath)}</dd>
      <dt>Headset settings</dt><dd class="path">${esc(H.configPath)}</dd>
    </dl>

    <h2 class="h2">Noise suppression log</h2>
    <p class="hint">What Windows' audio engine reported when it loaded kit's noise suppression.</p>
    <div style="margin-top:10px"><button class="btn quiet" data-act="show-log">${hv.log === null ? "Show log" : "Refresh log"}</button></div>
    ${hv.log !== null ? `<div class="logbox">${esc(hv.log || "Empty: the audio engine hasn't loaded noise suppression yet.")}</div>` : ""}

    <h2 class="h2">About</h2>
    <p class="hint">Closing this window keeps kit running in the tray; use Quit in the tray menu to exit. Keep Logitech G HUB and SteelSeries GG closed: they fight kit for the same devices.</p>
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
});

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

document.addEventListener("keyup", (e) => forwardKey(e, false));

document.addEventListener("keydown", (e) => {
  if (forwardKey(e, true)) return;
  if (captureHotkey(e)) return;
  if (e.key === "Enter" && e.target.matches("input")) e.target.blur();
});

// ---------- boot ----------

(async () => {
  [M, H, autostart, upd] = await Promise.all([invoke("mouse_state"), invoke("headset_state"), invoke("get_autostart"), invoke("update_state")]);
  mv.profileId = mcfg().active;
  render();

  // State pushed by the backend (tray clicks, hotkeys, side buttons, hardware) is a hardware change.
  const hw = (fn) => (e) => {
    fn(e.payload);
    hardwareChange = true;
    render();
  };
  await listen("mouse-state", hw(onMouseState));
  await listen("mouse", hw((p) => (M.device = p)));
  await listen("headset-state", hw(onHeadsetState));
  await listen("headset", hw((p) => (H.headset = p)));
  await listen("audio", hw((p) => (H.audio = p)));
  await listen("update", (e) => {
    upd = e.payload;
    render();
  });
})();
