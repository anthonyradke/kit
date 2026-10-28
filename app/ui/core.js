"use strict";
// Shared helpers for every page: DOM patching, saving, the rating plate, icons.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const newId = () => Date.now().toString(16) + Math.random().toString(16).slice(2, 8);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
// Whole numbers only: the backend rejects fractions, and one bad field would fail every save.
const clampInt = (v, lo, hi) => Math.min(hi, Math.max(lo, Math.round(Number(v) || 0)));

// Click handlers by data-act, and input/change handlers by data-field, filled in by each page script.
const handlers = {};
const inputFields = {};
const changeFields = {};

const icons = {
  home: '<svg viewBox="0 0 24 24"><path d="M4 11l8-7 8 7M6 9.5V20h12V9.5"/></svg>',
  gear: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M21.5 12h-3M5.5 12h-3M18.7 5.3l-2.1 2.1M7.4 16.6l-2.1 2.1M18.7 18.7l-2.1-2.1M7.4 7.4L5.3 5.3"/></svg>',
  up: '<svg viewBox="0 0 24 24"><path d="M12 19V5M5 12l7-7 7 7"/></svg>',
  down: '<svg viewBox="0 0 24 24"><path d="M12 5v14M19 12l-7 7-7-7"/></svg>',
  x: '<svg viewBox="0 0 24 24"><path d="M18 6L6 18M6 6l12 12"/></svg>',
  stop: '<svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor" stroke="none"/></svg>',
  clock: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
  mouse: '<svg viewBox="0 0 24 24"><rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/></svg>',
  copy: '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="1.5"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>',
  play: '<svg viewBox="0 0 24 24"><path d="M7 5l12 7-12 7z" fill="currentColor"/></svg>',
  key: '<svg viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M8 14h8"/></svg>',
  reset: '<svg viewBox="0 0 24 24"><path d="M4 12a8 8 0 1 0 2.3-5.6M4 4v4h4"/></svg>',
  shield: '<svg viewBox="0 0 24 24"><path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  brush: '<svg viewBox="0 0 24 24"><path d="M14.5 4.5l5 5-8 8-5-5zM6.5 12.5L4 20l7.5-2.5"/></svg>',
  eraser: '<svg viewBox="0 0 24 24"><path d="M8 20h12M5 15l9-9 5 5-9 9H8z"/></svg>',
};

// ---------- products ----------
// Photos follow the color picked in setup (A is the app config, loaded in app.js).

const PRODUCTS = {
  mouse: { name: "PRO X Superlight", colors: ["white", "black"] },
  headset: { name: "Arctis Nova 5", colors: ["white", "black"] },
  nk65: { name: "NK65", long: "NK65 Milkshake Edition" },
  proxtkl: { name: "PRO X TKL", long: "PRO X TKL LIGHTSPEED" },
};

function productImg(kind, view, colorOverride) {
  if (kind === "keyboard") return `img/keyboard-${colorOverride || A.keyboard?.model || "nk65"}.webp`;
  const color = colorOverride || A[kind]?.color || "white";
  const dark = color === "black" ? "-black" : "";
  if (kind === "mouse") return `img/mouse-${view || "side"}${dark}.webp`;
  return `img/headset${dark}.webp`;
}

function keyboardName(long = false) {
  const p = PRODUCTS[A.keyboard?.model || "nk65"];
  return long ? p.long : p.name;
}

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

// ---------- saving ----------
// Each device saves its whole config, debounced. `unsaved()` guards incoming
// state events from wiping edits the backend doesn't have yet.

function makeSaver(command, getConfig, onSaved) {
  let timer = null;
  let inFlight = 0;
  const saver = {
    save(delay = 250) {
      clearTimeout(timer);
      timer = setTimeout(saver.flush, delay);
    },
    unsaved: () => timer !== null || inFlight > 0,
    async flush() {
      if (timer === null) return;
      clearTimeout(timer);
      timer = null;
      inFlight++;
      try {
        onSaved(await invoke(command, { config: getConfig() }));
      } catch (err) {
        toast(`Couldn't save: ${err}`);
      } finally {
        inFlight--;
      }
      render();
    },
  };
  return saver;
}

function toast(text, ms = 2600) {
  const t = $("#toast");
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), ms);
}

// Two-step delete: the first click arms the button for 3 seconds.
let confirmDelete = null;
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

// ---------- the rating plate ----------
// A plate is rows of ruled cells: a condensed caption over a value. Cells are
// laid out on a fixed grid so values change in place without reflowing.

// cell(key, caption, valueHtml, { span, cls, meter, low })
function cell(key, cap, val, o = {}) {
  const cls = ["cell", o.span === 2 ? "span2" : "", o.cellCls || ""].join(" ");
  const meter = o.meter != null ? `<div class="meter ${o.low ? "low" : ""}"><i style="width:${clamp(o.meter, 0, 100)}%"></i></div>` : "";
  return { span: o.span || 1, html: (end) => `<div class="${cls} ${end}" data-cell="${key}"><div class="cap">${cap}</div><div class="val ${o.cls || ""}">${val}</div>${meter}</div>` };
}

// Lay cells onto a grid of `cols` columns, marking the right and bottom edges
// so inner rules are drawn once.
function plate(cells, cols = 4, extra = "") {
  const rows = [];
  let row = [];
  let used = 0;
  for (const c of cells) {
    if (used + c.span > cols) {
      rows.push(row);
      row = [];
      used = 0;
    }
    row.push(c);
    used += c.span;
  }
  if (row.length) rows.push(row);
  const html = rows
    .map((r, ri) => {
      let col = 0;
      return r
        .map((c) => {
          col += c.span;
          const end = [col >= cols ? "r-end" : "", ri === rows.length - 1 ? "b-end" : ""].join(" ");
          return c.html(end);
        })
        .join("");
    })
    .join("");
  return `<div class="plate ${extra}" style="--cols:${cols}">${html}</div>`;
}

const battery = (key, b, charging) =>
  b == null ? cell(key, "Battery", "—", { cls: "dim" }) : cell(key, charging ? "Battery · charging" : "Battery", `${b}<small>%</small>`, { meter: b, low: b <= 15 });

// Signature: when the hardware reports a change, the changed plate cells flash
// their new value in yellow. Local edits don't flash (they're already seen).
const cellValues = new Map();
let hardwareChange = false;

function flashChangedCells() {
  const flash = hardwareChange;
  hardwareChange = false;
  document.querySelectorAll("[data-cell]").forEach((el) => {
    const id = `${el.closest("[data-key]")?.dataset.key || ""}/${el.dataset.cell}`;
    const v = el.querySelector(".val")?.textContent;
    const before = cellValues.get(id);
    cellValues.set(id, v);
    if (flash && before !== undefined && before !== v) {
      el.classList.add("tick");
      setTimeout(() => el.classList.remove("tick"), 900);
    }
  });
}

// ---------- keys ----------

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

function slider(field, value, max, disabled = false, label = "", min = 0, step = 1) {
  const pct = ((value - min) / (max - min)) * 100;
  return `<input class="slider" type="range" min="${min}" max="${max}" step="${step}" value="${value}" data-field="${field}" style="--p:${pct}%" aria-label="${esc(label)}" ${disabled ? "disabled" : ""} />`;
}
