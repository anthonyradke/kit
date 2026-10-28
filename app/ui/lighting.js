// kit — lighting effects: what each one is, which controls it uses, and a
// simulation of the NK65's RGB matrix for the preview. The QMK effects are
// ported from QMK's own code (quantum/rgb_matrix/animations) and kit's from
// firmware/nk65/keymaps/kit/rgb_matrix_user.inc, so the preview moves and
// colors the way the board does. Loaded before keyboard.js.

// ---------- the catalog ----------
// Groups, in the order the effect picker shows them.
const FX_GROUPS = [
  ["typing", "Typing", "These react to the keys you press."],
  ["still", "Still", "Fixed colors that don't move."],
  ["calm", "Calm", "Slow movement in one color."],
  ["pattern", "Pattern", "Bands, spirals and pixels in one color."],
  ["rainbow", "Rainbow", "These run through every color on their own, so there's no color to pick."],
  ["kit", "From kit", "kit plays these by changing the whole board's color while it runs."],
];

// Color modes:
//   one      a color, used as is
//   base     a color the effect moves a little either side of
//   two      two colors (labels in c1/c2)
//   mods     letters color + modifiers color (QMK: hue offset kept in speed)
//   rainbow  no color: the effect makes its own
//   fixed    no color: always the same (Digital rain is green)
//   none     Off
// speed: the label of the speed slider, or null when the effect has no speed.
// multi: the id of the version that keeps every press (shown as one card).
const fx = (id, name, group, color, about, o = {}) => ({ id, name, group, color, about, speed: "Speed", ...o });

const RGBM_FX = [
  fx(0, "Off", "still", "none", "All lights off.", { speed: null }),
  fx(1, "Solid", "still", "one", "Every key in one color.", { speed: null }),
  fx(2, "Alphas / mods", "still", "mods", "Letter keys in one color, modifiers, arrows and the right column in another.", { speed: null, c1: "Letter keys", c2: "Modifier keys", fw: 3 }),
  fx(3, "Gradient up-down", "still", "one", "A rainbow from top to bottom, starting at this color. Spread sets how much of the rainbow it covers.", { speed: "Spread", c1: "Top color" }),
  fx(4, "Gradient left-right", "still", "one", "A rainbow from left to right, starting at this color. Spread sets how much of the rainbow it covers.", { speed: "Spread", c1: "Left color" }),
  fx(5, "Breathing", "calm", "one", "The whole board slowly fades in and out."),
  fx(6, "Band saturation", "pattern", "one", "A white band sweeps across the board."),
  fx(7, "Band brightness", "pattern", "one", "A bright band sweeps across a dark board."),
  fx(8, "Pinwheel saturation", "pattern", "one", "White spokes turn around the middle."),
  fx(9, "Pinwheel brightness", "pattern", "one", "Bright spokes turn around the middle."),
  fx(10, "Spiral saturation", "pattern", "one", "A white spiral turns around the middle."),
  fx(11, "Spiral brightness", "pattern", "one", "A bright spiral turns around the middle."),
  fx(12, "Cycle", "rainbow", "rainbow", "The whole board changes color together."),
  fx(13, "Cycle across", "rainbow", "rainbow", "A rainbow rolls from left to right."),
  fx(14, "Cycle down", "rainbow", "rainbow", "A rainbow rolls from top to bottom."),
  fx(15, "Rainbow chevron", "rainbow", "rainbow", "A rainbow in a V shape rolls across."),
  fx(16, "Cycle out-in", "rainbow", "rainbow", "Rainbow rings move in toward the middle."),
  fx(17, "Cycle out-in dual", "rainbow", "rainbow", "Two sets of rainbow rings, one on each half."),
  fx(18, "Cycle pinwheel", "rainbow", "rainbow", "A rainbow wheel turns around the middle."),
  fx(19, "Cycle spiral", "rainbow", "rainbow", "A rainbow spiral turns around the middle."),
  fx(20, "Dual beacon", "rainbow", "rainbow", "A rainbow that turns like a lighthouse beam."),
  fx(21, "Rainbow beacon", "rainbow", "rainbow", "A tighter rainbow that turns like a lighthouse beam."),
  fx(22, "Rainbow pinwheels", "rainbow", "rainbow", "Two rainbow wheels turning, one on each half."),
  fx(23, "Raindrops", "calm", "base", "Keys change one at a time between this color and two neighbors on the color wheel.", { speed: "Rate" }),
  fx(24, "Jellybean", "rainbow", "rainbow", "Keys change one at a time to random colors.", { speed: "Rate" }),
  fx(25, "Hue breathing", "calm", "base", "The whole board shifts a little either side of this color and back."),
  fx(26, "Hue pendulum", "calm", "base", "A small color shift that swings left and right."),
  fx(27, "Hue wave", "calm", "base", "A small color shift that rolls across the board."),
  fx(28, "Pixel rain", "rainbow", "rainbow", "Random keys light up in random colors or go dark.", { speed: "Rate" }),
  fx(29, "Pixel flow", "rainbow", "rainbow", "Random colors stream through the keys in a line.", { speed: "Rate" }),
  fx(30, "Pixel fractal", "pattern", "one", "Lit keys flow out from the middle to both sides, mirrored.", { speed: "Rate" }),
  fx(31, "Typing heatmap", "typing", "rainbow", "Keys you press often warm from blue to red, then cool off.", { speed: null }),
  fx(32, "Digital rain", "calm", "fixed", "Green drops falling down the board, like the film.", { speed: null }),
  fx(33, "Reactive", "typing", "one", "Only the key you press lights up, then fades out.", { speed: "Fade speed" }),
  fx(34, "Reactive fade", "typing", "one", "The board shows this color; a pressed key jumps a quarter of the way around the color wheel and fades back.", { speed: "Fade speed" }),
  fx(36, "Reactive wide", "typing", "one", "A glow spreads a little around each key you press.", { speed: "Fade speed", single: 35 }),
  fx(38, "Reactive cross", "typing", "one", "Each press lights its row and column.", { speed: "Fade speed", single: 37 }),
  fx(40, "Reactive nexus", "typing", "base", "Lines shoot out along the row and column of each press.", { speed: "Fade speed", single: 39 }),
  fx(42, "Splash", "typing", "rainbow", "Each press sends out a ring that runs through the rainbow.", { speed: "Fade speed", single: 41 }),
  fx(44, "Solid splash", "typing", "one", "Each press sends out a ring of this color across a dark board.", { speed: "Fade speed", single: 43 }),
];

// kit's firmware effects, in the firmware's order (numbers start at the first kit effect).
const KITFW_FX = [
  fx(0, "Ripple", "typing", "base", "Each press sends a ring across a dim board, tinted by where on the board it started."),
  fx(1, "Prism ripple", "typing", "rainbow", "Thin rings on a dark board that run through the rainbow as they spread."),
  fx(2, "Ink", "typing", "base", "Every press drops a blot of color that spreads and fades; overlapping blots mix. Each key has its own shade near this color."),
  fx(3, "Afterglow", "typing", "base", "Pressed keys light up hot and cool off slowly, glowing a little onto their neighbors."),
  fx(4, "Aurora", "calm", "base", "Slow curtains of light swaying across the board."),
  fx(5, "Lava", "calm", "base", "Blobs that drift, merge and split like a lava lamp."),
  fx(6, "Tide", "calm", "base", "Waves rolling across the board, with pale crests."),
  fx(7, "Fireflies", "calm", "base", "Keys glow up and fade on their own, each at its own pace."),
];

// Played by kit on either NK65 firmware: one color and brightness for the whole board, streamed.
const KIT_FX = [
  fx(200, "Keystrike", "kit", "two", "Every key press flashes the board to the flash color, then it fades back.", { c1: "Resting color", c2: "Flash color" }),
  fx(201, "Typing heat", "kit", "two", "Warms up and gets brighter the faster you type, then cools off.", { c1: "Cool color", c2: "Hot color" }),
  fx(202, "Hue step", "kit", "one", "Each key press nudges the color along the rainbow, starting from this color.", { c1: "Starting color" }),
  fx(203, "Music", "kit", "two", "Pulses with whatever is playing on the default output: louder is brighter and closer to the loud color.", { c1: "Quiet color", c2: "Loud color" }),
  fx(204, "Heartbeat", "kit", "two", "A slow double pulse. Speed sets the heart rate.", { c1: "Resting color", c2: "Beat color" }),
  fx(205, "Candle", "kit", "two", "Flickers like a flame; the dim moments lean to the ember color.", { c1: "Flame color", c2: "Ember color" }),
  fx(206, "Color drift", "kit", "two", "Drifts slowly back and forth between the two colors.", { c1: "First color", c2: "Second color" }),
];

// The NK65's stock firmware (wilba's RGB backlight) and the PRO X TKL.
const WILBA_FX = [
  fx(0, "Off", "still", "none", "All lights off.", { speed: null }),
  fx(1, "Solid", "still", "one", "Every key in one color.", { speed: null }),
  fx(2, "Alphas / mods", "still", "two", "Letter keys in one color, modifiers in the other.", { speed: null, c1: "Letter keys", c2: "Modifier keys" }),
  fx(3, "Gradient", "still", "two", "Fades from one color at the top to the other at the bottom.", { speed: null, c1: "Top color", c2: "Bottom color" }),
  fx(4, "Raindrops", "calm", "two", "Keys change one at a time between the two colors."),
  fx(5, "Cycle", "rainbow", "rainbow", "The whole board changes color together."),
  fx(6, "Cycle across", "rainbow", "rainbow", "A rainbow rolls from left to right."),
  fx(7, "Cycle down", "rainbow", "rainbow", "A rainbow rolls from top to bottom."),
  fx(8, "Jellybean", "rainbow", "rainbow", "Keys change to random colors."),
  fx(9, "Radial rainbow", "rainbow", "rainbow", "A rainbow wheel around the middle."),
  fx(10, "Radial", "calm", "one", "A wheel of this color around the middle."),
];
const LIGHTSYNC_FX = [
  fx(0, "Off", "still", "none", "All lights off.", { speed: null }),
  fx(1, "Static", "still", "one", "One color, or paint keys one by one.", { speed: null }),
  fx(10, "Breathe", "calm", "one", "The whole board slowly fades in and out."),
  fx(3, "Cycle", "rainbow", "rainbow", "The whole board changes color together."),
  fx(4, "Wave", "rainbow", "rainbow", "A rainbow rolls across the board."),
  fx(11, "Ripple", "typing", "one", "Each press sends out a ripple."),
];

// The card for `id` in `list` (a single-press variant finds its merged card).
const fxCard = (list, id) => list.find((f) => f.id === id || f.single === id);

// ---------- QMK's integer helpers ----------
const u8 = (x) => x & 255;
const i8 = (x) => ((x & 255) << 24) >> 24;
const scale8 = (i, s) => (i * (1 + s)) >> 8;
const scale16by8 = (i, s) => ((i & 0xffff) * (1 + s)) >> 8;
const qadd8 = (a, b) => Math.min(255, a + b);
const qsub8 = (a, b) => Math.max(0, a - b);
const abs8 = (x) => Math.min(127, Math.abs(i8(x)));
const sin8 = (t) => clamp(Math.round(Math.sin((u8(t) / 256) * 2 * Math.PI) * 128 + 128), 0, 255);
const cos8 = (t) => sin8(t + 64);
const atan2_8 = (dy, dx) => u8(Math.round((Math.atan2(dy, dx) / (2 * Math.PI)) * 256));
const sqrt16 = (x) => Math.floor(Math.sqrt(x));

// QMK's hsv_to_rgb (quantum/color.c), all values 0-255.
function qmkRgb(h, s, v) {
  h = u8(h);
  s = clamp(Math.round(s), 0, 255);
  v = clamp(Math.round(v), 0, 255);
  if (s === 0) return [v, v, v];
  const region = Math.floor((h * 6) / 255);
  const rem = u8((h * 2 - region * 85) * 3);
  const p = (v * (255 - s)) >> 8;
  const q = (v * (255 - ((s * rem) >> 8))) >> 8;
  const t = (v * (255 - ((s * (255 - rem)) >> 8))) >> 8;
  switch (region) {
    case 6:
    case 0: return [v, t, p];
    case 1: return [q, v, p];
    case 2: return [p, v, t];
    case 3: return [p, q, v];
    case 4: return [t, p, v];
    default: return [v, p, q];
  }
}

// "#rrggbb" to QMK hue and saturation, as kit sends it (config.rs to_hue_sat).
function hueSat(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  const s = max > 0 ? d / max : 0;
  let h = 0;
  if (d) h = max === r ? (((g - b) / d) % 6 + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [Math.round((h / 6) * 255) & 255, Math.round(s * 255)];
}
// The same color at full value: the board takes only hue and saturation, brightness is its own setting.
const fullValue = (hex) => {
  const [h, s] = hueSat(hex);
  return "#" + qmkRgb(h, s, 255).map((v) => v.toString(16).padStart(2, "0")).join("");
};

// A fixed pseudo-random number 0-1 (kit_hash in the firmware).
function hash01(i, salt) {
  let h = (Math.imul(i + 1, 2654435761) ^ Math.imul(salt + 1, 40503)) >>> 0;
  h = (h ^ (h >>> 15)) >>> 0;
  h = Math.imul(h, 2246822519) >>> 0;
  h = (h ^ (h >>> 13)) >>> 0;
  return (h & 0xffff) / 65535;
}

// ---------- the NK65's LEDs ----------
// QMK's novelkeys/nk65 rgb_matrix layout: [matrix "row,col", x, y], in LED order.
// x runs 0-224 and y 0-64; the middle is (112, 32).
const NK65_LEDS = [
  ["0,0", 8, 7], ["0,1", 22, 7], ["0,2", 35, 7], ["0,3", 49, 7], ["0,4", 63, 7], ["0,5", 77, 7], ["0,6", 91, 7], ["0,7", 105, 7], ["0,8", 118, 7], ["0,9", 132, 7], ["0,10", 146, 7], ["0,11", 160, 7], ["0,12", 174, 7], ["0,13", 195, 7], ["0,14", 215, 7],
  ["1,0", 11, 19], ["1,1", 28, 19], ["1,2", 42, 19], ["1,3", 56, 19], ["1,4", 70, 19], ["1,5", 84, 19], ["1,6", 98, 19], ["1,7", 112, 19], ["1,8", 125, 19], ["1,9", 139, 19], ["1,10", 153, 19], ["1,11", 167, 19], ["1,12", 181, 19], ["2,12", 198, 19], ["1,14", 215, 19],
  ["2,0", 13, 32], ["2,1", 32, 32], ["2,2", 46, 32], ["2,3", 60, 32], ["2,4", 73, 32], ["2,5", 87, 32], ["2,6", 101, 32], ["2,7", 115, 32], ["2,8", 129, 32], ["2,9", 143, 32], ["2,10", 156, 32], ["2,11", 170, 32], ["2,13", 193, 32], ["2,14", 215, 32],
  ["3,0", 16, 44], ["3,2", 39, 44], ["3,3", 53, 44], ["3,4", 67, 44], ["3,5", 80, 44], ["3,6", 94, 44], ["3,7", 108, 44], ["3,8", 122, 44], ["3,9", 136, 44], ["3,10", 150, 44], ["3,11", 163, 44], ["3,12", 182, 44], ["3,13", 201, 44], ["3,14", 215, 44],
  ["4,0", 9, 56], ["4,1", 27, 56], ["4,2", 44, 56], ["4,6", 96, 56], ["4,9", 146, 56], ["4,10", 160, 56], ["4,11", 174, 56], ["4,12", 188, 56], ["4,13", 201, 56], ["4,14", 215, 56],
].map(([id, x, y], i) => ({ id, i, x, y, r: Number(id.split(",")[0]), c: Number(id.split(",")[1]) }));
// Keys kit's firmware (version 3) flags as modifiers, for Alphas / mods.
const NK65_MODS = new Set(["0,0", "0,13", "0,14", "1,0", "1,14", "2,0", "2,13", "2,14", "3,0", "3,12", "3,13", "3,14", "4,0", "4,1", "4,2", "4,9", "4,10", "4,11", "4,12", "4,13", "4,14"]);
const LED_BY_ID = new Map(NK65_LEDS.map((l) => [l.id, l]));
const LED_AT = new Map(NK65_LEDS.map((l) => [`${l.r},${l.c}`, l]));
// Keys the simulated typist presses: mostly letters and space, now and then something else.
const TYPED = ["2,1", "2,2", "2,3", "2,4", "2,5", "2,6", "2,7", "2,8", "2,9", "1,1", "1,2", "1,3", "1,4", "1,5", "1,6", "1,7", "1,8", "1,9", "1,10", "3,3", "3,4", "3,5", "3,6", "3,7", "3,8", "4,6", "4,6", "4,6", "0,13", "2,13", "3,0"];

// ---------- one frame of an effect ----------
// ctx: { t (ms), h, s, v, speed (0-255), hits [{ x, y, i, tick }] oldest first, state, kitFw (version) }.
// Returns [r, g, b] 0-255 for LED `l`, as the firmware would drive it.

function splashRunner(ctx, l, single, math) {
  let h = ctx.h;
  let v = 0;
  const from = single ? Math.max(0, ctx.hits.length - 1) : 0;
  for (let j = from; j < ctx.hits.length; j++) {
    const hit = ctx.hits[j];
    const dx = l.x - hit.x;
    const dy = l.y - hit.y;
    const dist = u8(sqrt16(dx * dx + dy * dy));
    const tick = scale16by8(hit.tick, qadd8(ctx.speed, 1));
    [h, v] = math(h, v, dx, dy, dist, tick, ctx);
  }
  return qmkRgb(h, ctx.s, scale8(v, ctx.v));
}
const splashMath = (h, v, dx, dy, dist, tick) => {
  const e = Math.min(255, tick >= dist ? tick - dist : 255);
  return [h + e, qadd8(v, 255 - e)];
};
const solidSplashMath = (h, v, dx, dy, dist, tick) => {
  const e = Math.min(255, tick >= dist ? tick - dist : 255);
  return [h, qadd8(v, 255 - e)];
};
const wideMath = (h, v, dx, dy, dist, tick) => [h, qadd8(v, 255 - Math.min(255, tick + dist * 5))];
const crossMath = (h, v, dx, dy, dist, tick) => {
  const ax = Math.min(255, Math.abs(dx) * 16);
  const ay = Math.min(255, Math.abs(dy) * 16);
  return [h, qadd8(v, 255 - Math.min(255, tick + dist + Math.min(ax, ay)))];
};
const nexusMath = (h, v, dx, dy, dist, tick, ctx) => {
  let e = Math.min(255, tick >= dist ? tick - dist : 255);
  if (dist > 72) e = 255;
  if (Math.abs(dx) > 8 && Math.abs(dy) > 8) e = 255;
  return [ctx.h + Math.trunc(dy / 4), qadd8(v, 255 - e)];
};

// The most recent press of this very key, for Reactive and Reactive fade.
function reactiveOffset(ctx, l) {
  let tick = Math.floor(65535 / qadd8(ctx.speed, 1));
  for (let j = ctx.hits.length - 1; j >= 0; j--) {
    if (ctx.hits[j].i === l.i && ctx.hits[j].tick < tick) {
      tick = ctx.hits[j].tick;
      break;
    }
  }
  return Math.min(255, scale16by8(tick, qadd8(ctx.speed, 1)));
}

const timeI = (ctx) => u8(scale16by8(ctx.t, qadd8(ctx.speed >> 2, 1)));
const timeDxDy = (ctx) => u8(scale16by8(ctx.t, ctx.speed >> 1));
const CX = 112;
const CY = 32;

// Changes at random moments, one key at a time: which change key `l` is on now.
function randomSlot(ctx, l, perSecond) {
  const t = ctx.t / 1000;
  return Math.floor(t * perSecond + hash01(l.i, 7) * 13);
}

function beacon(ctx, l, k, pin) {
  const time = scale16by8(ctx.t, ctx.speed >> 2);
  const c = cos8(time) - 128;
  const s = sin8(time) - 128;
  // QMK passes (cos, sin) into parameters named (sin, cos).
  const [sn, cs] = [c, s];
  const x = pin ? 56 - abs8(l.x - CX) : l.x - CX;
  return qmkRgb(ctx.h + Math.trunc(((l.y - CY) * k * cs + x * k * sn) / 128), ctx.s, ctx.v);
}

const RGBM_DRAW = {
  0: () => [0, 0, 0],
  1: (ctx) => qmkRgb(ctx.h, ctx.s, ctx.v),
  2: (ctx, l) => qmkRgb(ctx.kitFw >= 3 && NK65_MODS.has(l.id) ? ctx.h + ctx.speed : ctx.h, ctx.s, ctx.v),
  3: (ctx, l) => qmkRgb(ctx.h + scale8(64, ctx.speed) * (l.y >> 4), ctx.s, ctx.v),
  4: (ctx, l) => qmkRgb(ctx.h + ((scale8(64, ctx.speed) * l.x) >> 5), ctx.s, ctx.v),
  5: (ctx) => qmkRgb(ctx.h, ctx.s, scale8(Math.min(255, abs8(sin8(timeI(ctx) >> 1) - 128) * 2), ctx.v)),
  6: (ctx, l) => qmkRgb(ctx.h, scale8(Math.max(0, ctx.s - Math.abs(scale8(l.x, 228) + 28 - timeI(ctx)) * 8), ctx.s), ctx.v),
  7: (ctx, l) => qmkRgb(ctx.h, ctx.s, scale8(Math.max(0, ctx.v - Math.abs(scale8(l.x, 228) + 28 - timeI(ctx)) * 8), ctx.v)),
  8: (ctx, l) => qmkRgb(ctx.h, scale8(u8(ctx.s - timeDxDy(ctx) - atan2_8(l.y - CY, l.x - CX) * 3), ctx.s), ctx.v),
  9: (ctx, l) => qmkRgb(ctx.h, ctx.s, scale8(u8(ctx.v - timeDxDy(ctx) - atan2_8(l.y - CY, l.x - CX) * 3), ctx.v)),
  10: (ctx, l) => qmkRgb(ctx.h, scale8(u8(ctx.s + dist(l) - timeDxDy(ctx) - atan2_8(l.y - CY, l.x - CX)), ctx.s), ctx.v),
  11: (ctx, l) => qmkRgb(ctx.h, ctx.s, scale8(u8(ctx.v + dist(l) - timeDxDy(ctx) - atan2_8(l.y - CY, l.x - CX)), ctx.v)),
  12: (ctx) => qmkRgb(timeI(ctx), ctx.s, ctx.v),
  13: (ctx, l) => qmkRgb(l.x - timeI(ctx), ctx.s, ctx.v),
  14: (ctx, l) => qmkRgb(l.y - timeI(ctx), ctx.s, ctx.v),
  15: (ctx, l) => qmkRgb(ctx.h + abs8(l.y - CY) + (l.x - timeI(ctx)), ctx.s, ctx.v),
  16: (ctx, l) => qmkRgb(Math.floor((3 * dist(l)) / 2) + timeDxDy(ctx), ctx.s, ctx.v),
  17: (ctx, l) => {
    const dx = CX / 2 - abs8(l.x - CX);
    const dy = l.y - CY;
    return qmkRgb(3 * u8(sqrt16(dx * dx + dy * dy)) + timeDxDy(ctx), ctx.s, ctx.v);
  },
  18: (ctx, l) => qmkRgb(atan2_8(l.y - CY, l.x - CX) + timeDxDy(ctx), ctx.s, ctx.v),
  19: (ctx, l) => qmkRgb(dist(l) - timeDxDy(ctx) - atan2_8(l.y - CY, l.x - CX), ctx.s, ctx.v),
  20: (ctx, l) => beacon(ctx, l, 1, false),
  21: (ctx, l) => beacon(ctx, l, 2, false),
  22: (ctx, l) => beacon(ctx, l, 3, true),
  23: (ctx, l) => {
    // Changes about (speed+16)/2.56 times a second across the board; hue steps of -32 (QMK's "shortest path").
    const n = randomSlot(ctx, l, (qadd8(ctx.speed, 16) / 256) * 100 / NK65_LEDS.length);
    return qmkRgb(ctx.h - 32 * Math.floor(hash01(l.i, n) * 3), ctx.s, ctx.v);
  },
  24: (ctx, l) => {
    const n = randomSlot(ctx, l, (qadd8(ctx.speed, 16) / 256) * 200 / NK65_LEDS.length);
    return qmkRgb(Math.floor(hash01(l.i, n) * 256), 127 + Math.floor(hash01(l.i, n + 999) * 128), ctx.v);
  },
  25: (ctx) => qmkRgb(ctx.h + scale8(Math.min(255, abs8(sin8(timeI(ctx) >> 1) - 128) * 2), 12), ctx.s, ctx.v),
  26: (ctx, l) => qmkRgb(ctx.h + scale8(Math.min(255, abs8(sin8(timeI(ctx)) + l.x - 128) * 2), 12), ctx.s, ctx.v),
  27: (ctx, l) => qmkRgb(ctx.h + scale8(abs8(l.x - timeI(ctx)), 24), ctx.s, ctx.v),
  28: (ctx, l) => {
    const every = 2048 - scale16by8(1792, ctx.speed);
    const n = randomSlot(ctx, l, 1000 / every / NK65_LEDS.length);
    if (n < 1 || hash01(l.i, n) < 0.5) return [0, 0, 0];
    return qmkRgb(Math.floor(hash01(l.i, n + 3) * 256), 127 + Math.floor(hash01(l.i, n + 5) * 128), ctx.v);
  },
  29: (ctx, l) => {
    const step = Math.floor(ctx.t / pixelInterval(ctx)) + l.i;
    if (hash01(step, 11) < 0.5) return [0, 0, 0];
    return qmkRgb(Math.floor(hash01(step, 12) * 256), 127 + Math.floor(hash01(step, 13) * 128), ctx.v);
  },
  30: (ctx, l) => {
    // Columns fill from the middle (col 6 and 8) outward, the left half mirrored on the right.
    const col = l.c < 7 ? l.c : 14 - l.c;
    if (l.c === 7) return [0, 0, 0];
    const born = Math.floor(ctx.t / pixelInterval(ctx)) - (6 - col);
    return hash01(l.r * 977 + born, 21) < 0.25 ? qmkRgb(ctx.h, ctx.s, ctx.v) : [0, 0, 0];
  },
  31: (ctx, l) => {
    const val = Math.round(ctx.state.heat?.[l.i] || 0);
    return qmkRgb(170 - qsub8(val, 85), ctx.s, scale8(Math.min(val, 85) * 3, ctx.v));
  },
  32: (ctx, l) => {
    const g = ctx.state.rain?.[l.i] || 0;
    const max = ctx.v;
    const pure = (max * 3) >> 2;
    const lvl = g * max;
    if (lvl > pure) {
      const boost = Math.round((((max * 3) >> 2) * (lvl - pure)) / Math.max(1, max - pure));
      return [boost, max, boost];
    }
    return [0, Math.round((max * lvl) / Math.max(1, pure)), 0];
  },
  33: (ctx, l) => qmkRgb(ctx.h, ctx.s, scale8(255 - reactiveOffset(ctx, l), ctx.v)),
  34: (ctx, l) => qmkRgb(ctx.h + scale8(255 - reactiveOffset(ctx, l), 64), ctx.s, ctx.v),
  35: (ctx, l) => splashRunner(ctx, l, true, wideMath),
  36: (ctx, l) => splashRunner(ctx, l, false, wideMath),
  37: (ctx, l) => splashRunner(ctx, l, true, crossMath),
  38: (ctx, l) => splashRunner(ctx, l, false, crossMath),
  39: (ctx, l) => splashRunner(ctx, l, true, nexusMath),
  40: (ctx, l) => splashRunner(ctx, l, false, nexusMath),
  41: (ctx, l) => splashRunner(ctx, l, true, splashMath),
  42: (ctx, l) => splashRunner(ctx, l, false, splashMath),
  43: (ctx, l) => splashRunner(ctx, l, true, solidSplashMath),
  44: (ctx, l) => splashRunner(ctx, l, false, solidSplashMath),
};
const dist = (l) => u8(sqrt16((l.x - CX) * (l.x - CX) + (l.y - CY) * (l.y - CY)));
const pixelInterval = (ctx) => 3000 / Math.max(1, scale16by8(qadd8(ctx.speed, 16), 16));

// kit's firmware effects (rgb_matrix_user.inc), in floats like the firmware.
const kitSpeed = (ctx) => 0.25 + (1.75 * ctx.speed) / 255;
const kitTime = (ctx) => ((ctx.t % 65536000) / 1000) * kitSpeed(ctx);
const c01 = (x) => (x < 0 ? 0 : x > 1 ? 1 : x);
const smooth = (x) => ((x = c01(x)), x * x * (3 - 2 * x));
const kitSet = (ctx, hue, sat, v) => qmkRgb(ctx.h + Math.trunc(hue), Math.floor(ctx.s * c01(sat)), Math.floor(ctx.v * c01(v)));
const kitDist = (l, x, y) => Math.hypot(l.x - x, l.y - y);
function kitRipple(ctx, l, prism) {
  const speed = 70 + 110 * kitSpeed(ctx);
  const width = prism ? 11 : 16;
  const reach = prism ? 200 : 160;
  let best = 0;
  let hue = 0;
  for (const hit of ctx.hits) {
    const r = (hit.tick / 1000) * speed;
    const fade = 1 - r / reach;
    if (fade <= 0) continue;
    const d = kitDist(l, hit.x, hit.y);
    const off = Math.abs(d - r);
    if (off >= width) continue;
    const a = smooth(1 - off / width) * fade * fade;
    if (a > best) {
      best = a;
      hue = prism ? d * 1.2 : hit.x * 0.3;
    }
  }
  const base = prism ? 0 : 0.12;
  return kitSet(ctx, hue, 1, base + (1 - base) * best);
}
const KITFW_DRAW = [
  (ctx, l) => kitRipple(ctx, l, false),
  (ctx, l) => kitRipple(ctx, l, true),
  (ctx, l) => {
    const sp = kitSpeed(ctx);
    let [r, g, b] = [0, 0, 0];
    for (const hit of ctx.hits) {
      const age = hit.tick / 1000;
      const amp = Math.exp((-age * sp) / 1.4);
      if (amp < 0.01) continue;
      const radius = 8 + 34 * (1 - Math.exp(-age * sp * 2.5));
      const a = smooth((radius - kitDist(l, hit.x, hit.y)) / 14 + 0.5) * amp;
      if (a <= 0) continue;
      const c = qmkRgb(ctx.h + u8((hit.i * 53) % 96) - 48, ctx.s, 255);
      r += c[0] * a;
      g += c[1] * a;
      b += c[2] * a;
    }
    const c = qmkRgb(ctx.h, ctx.s, 255);
    const v = ctx.v / 255;
    return [(r + c[0] * 0.05) * v, (g + c[1] * 0.05) * v, (b + c[2] * 0.05) * v].map((x) => Math.min(255, Math.floor(x)));
  },
  (ctx, l) => {
    const tau = 2 / kitSpeed(ctx);
    let best = 0;
    for (const hit of ctx.hits) {
      const amp = Math.exp(-hit.tick / 1000 / tau);
      if (amp < 0.01) continue;
      const near = c01(1 - (kitDist(l, hit.x, hit.y) - 4) / 18);
      best = Math.max(best, amp * near * near);
    }
    return kitSet(ctx, 40 * best, 1 - 0.35 * best * best, 0.07 + 0.93 * best);
  },
  (ctx, l) => {
    const t = kitTime(ctx);
    const curtain = 0.5 + 0.5 * Math.sin(l.x * 0.035 + t * 0.6 + 1.6 * Math.sin(l.y * 0.05 + t * 0.3));
    const swell = 0.5 + 0.5 * Math.sin(l.x * 0.018 - t * 0.45);
    return kitSet(ctx, 28 * Math.sin(l.x * 0.012 + t * 0.2), 1, 0.06 + 0.94 * curtain * curtain * (0.35 + 0.65 * swell));
  },
  (ctx, l) => {
    const t = kitTime(ctx) * 0.6;
    const cx = 112 + 70 * Math.sin(t * 0.37);
    const cy = 32 + 22 * Math.sin(t * 0.53);
    const { x, y } = l;
    const p = Math.sin(x * 0.045 + t) + Math.sin(y * 0.09 - t * 0.7) + Math.sin((x + y) * 0.03 + t * 0.5) + Math.sin(Math.hypot(x - cx, y - cy) * 0.06 - t);
    const n = 0.5 + p / 8;
    return kitSet(ctx, 36 * (n - 0.5), 1, 0.25 + 0.75 * smooth(n * 1.3 - 0.15));
  },
  (ctx, l) => {
    const t = kitTime(ctx) * 1.4;
    const wave = 0.5 + 0.5 * Math.sin(l.x * 0.04 - t + 0.8 * Math.sin(l.y * 0.08 + t * 0.5));
    const crest = wave * wave * wave;
    return kitSet(ctx, 18 * crest, 1 - 0.6 * crest * crest, 0.18 + 0.82 * crest);
  },
  (ctx, l) => {
    const t = kitTime(ctx);
    const e = Math.sin(t * (0.35 + 0.6 * hash01(l.i, 0)) + 6.2832 * hash01(l.i, 1));
    const glow = e > 0.55 ? smooth((e - 0.55) / 0.45) : 0;
    return kitSet(ctx, 32 * (hash01(l.i, 2) - 0.5), 1, 0.03 + 0.97 * glow);
  },
];

// kit's whole-board effects (app/src-tauri/src/keyboard/anim.rs): one color for every key.
const mixRgb = (a, b, t) => a.map((v, i) => v + (b[i] - v) * c01(t));
const hexRgb01 = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
function kitBoardFrame(st, p, dt, t, presses) {
  const s = p.speed / 100;
  const base = p.brightness / 100;
  const c1 = hexRgb01(p.color1);
  const c2 = hexRgb01(p.color2);
  const decay = (level, half) => {
    const l = level * Math.pow(0.5, dt / half);
    return l < 0.01 ? 0 : l;
  };
  const lift = (x) => base * (0.25 + 0.75 * x);
  st.level ??= p.effect === 205 ? 1 : 0;
  let bright = base;
  let rgb = c1;
  switch (p.effect) {
    case 200:
      st.level = presses ? 1 : decay(st.level, 0.6 - 0.52 * s);
      [bright, rgb] = [lift(st.level), mixRgb(c1, c2, st.level)];
      break;
    case 201: {
      const tau = 3 - 2.5 * s;
      st.level = Math.min(1, decay(st.level, tau * Math.LN2) + presses / (8 * tau));
      [bright, rgb] = [lift(st.level), mixRgb(c1, c2, st.level)];
      break;
    }
    case 202: {
      const [h0, s0] = hueSat(p.color1);
      st.hue ??= h0 / 255;
      st.target ??= st.hue;
      st.target += presses * (0.01 + 0.07 * s);
      st.hue += (st.target - st.hue) * (1 - Math.exp(-dt / 0.12));
      st.level = presses ? 1 : decay(st.level, 0.15);
      const [r, g, b] = qmkRgb(Math.round((st.hue % 1) * 255), s0 < 40 ? 255 : s0, 255);
      [bright, rgb] = [base * (0.75 + 0.25 * st.level), [r / 255, g / 255, b / 255]];
      break;
    }
    case 203: {
      // No sound in the preview: a made-up beat.
      const loud = Math.pow(Math.max(0, Math.sin(t * 7.3) * Math.sin(t * 2.1 + 1)), 2);
      st.level = Math.max(loud, decay(st.level, 0.35 - 0.28 * s));
      [bright, rgb] = [lift(st.level), mixRgb(c1, c2, st.level)];
      break;
    }
    case 204: {
      const beat = (t % (1.5 - 0.9 * s)) - 0.1;
      const pulse = (c) => Math.exp(-(((beat - c) / 0.07) ** 2) / 2);
      st.level = Math.min(1, pulse(0) + 0.6 * pulse(0.22));
      [bright, rgb] = [lift(st.level), mixRgb(c1, c2, st.level)];
      break;
    }
    case 205: {
      if (!(t < st.flickerAt)) {
        st.flickerTo = Math.random() < 0.08 ? 0.35 : 0.55 + 0.45 * Math.random();
        st.flickerAt = t + (0.25 - 0.18 * s) * (0.5 + Math.random());
      }
      st.level += (st.flickerTo - st.level) * (1 - Math.exp(-dt / 0.06));
      [bright, rgb] = [base * st.level, mixRgb(c2, c1, st.level)];
      break;
    }
    case 206: {
      const x = t / (30 - 26 * s);
      const blend = 0.5 + 0.5 * (0.6 * Math.sin(2 * Math.PI * x) + 0.4 * Math.sin(2 * Math.PI * x * 2.3 + 1.3));
      const swell = 0.7 + 0.3 * (0.5 + 0.5 * Math.sin(2 * Math.PI * x * 1.7 + 0.5));
      [bright, rgb] = [base * swell, mixRgb(c1, c2, blend)];
      break;
    }
  }
  // The board gets hue and saturation of that color at this brightness, on Solid.
  const hex = "#" + rgb.map((v) => Math.round(c01(v) * 255).toString(16).padStart(2, "0")).join("");
  const [h, sat] = hueSat(hex);
  return qmkRgb(h, sat, Math.round(c01(bright) * 255));
}

// ---------- the simulation ----------
// Which drawing an NK65 effect number uses on this board, or null.
function simDraw(effect, kitFirst) {
  if (effect >= 200) return "board";
  if (kitFirst != null && effect >= kitFirst && effect < kitFirst + KITFW_DRAW.length) return KITFW_DRAW[effect - kitFirst];
  return RGBM_DRAW[effect] || null;
}

class LightSim {
  constructor() {
    this.hits = [];
    this.state = {};
    this.t = 0;
    this.nextType = 600;
    this.burst = 0;
    this.presses = 0;
    this.effect = null;
  }
  // A press on the board: QMK keeps the last 16 (kit's firmware; 8 on stock).
  hit(id, t = this.t) {
    const l = LED_BY_ID.get(id) || LED_AT.get(id);
    if (!l) return;
    this.hits.push({ x: l.x, y: l.y, i: l.i, at: t });
    if (this.hits.length > 16) this.hits.shift();
    this.presses++;
    const heat = (this.state.heat ||= new Float32Array(NK65_LEDS.length));
    for (const o of NK65_LEDS) {
      const d = sqrt16((o.x - l.x) ** 2 + (o.y - l.y) ** 2);
      if (o.i === l.i) heat[o.i] = Math.min(255, heat[o.i] + 64);
      else if (d <= 40) heat[o.i] = Math.min(255, heat[o.i] + Math.min(16, 40 - d));
    }
  }
  // Someone typing: bursts of keys with pauses in between.
  autoType(dt) {
    this.nextType -= dt;
    if (this.nextType > 0) return;
    this.hit(TYPED[Math.floor(Math.random() * TYPED.length)]);
    if (--this.burst > 0) this.nextType = 110 + Math.random() * 220;
    else {
      this.burst = 3 + Math.floor(Math.random() * 6);
      this.nextType = 900 + Math.random() * 1400;
    }
  }
  // Advance by dt ms and return each LED's [r, g, b] (LED order).
  step(dt, p, kitFirst, kitFw, typing = true) {
    if (p.effect !== this.effect) {
      this.effect = p.effect;
      this.state = { heat: this.state.heat };
    }
    this.t += dt;
    if (typing) this.autoType(dt);
    // Typing heatmap cools by 1 every 25 ms.
    const heat = this.state.heat;
    if (heat) for (let i = 0; i < heat.length; i++) heat[i] = Math.max(0, heat[i] - dt / 25);
    if (p.effect === 32) this.rain(dt);
    const draw = simDraw(p.effect, kitFirst);
    if (draw === "board") {
      const presses = this.presses;
      this.presses = 0;
      const c = kitBoardFrame(this.state, p, dt / 1000, this.t / 1000, presses);
      return NK65_LEDS.map(() => c);
    }
    this.presses = 0;
    const [h, s] = hueSat(p.color1);
    const speed = Math.round((p.speed * 255) / 100);
    const ctx = {
      t: this.t,
      h,
      s: rainbowFx(p.effect, kitFirst) ? 255 : s,
      v: Math.round((p.brightness * 255) / 100),
      speed: p.effect === 2 ? u8(hueSat(p.color2)[0] - h) : speed,
      hits: this.hits.map((x) => ({ ...x, tick: Math.min(65535, this.t - x.at) })),
      state: this.state,
      kitFw,
    };
    return NK65_LEDS.map((l) => (draw ? draw(ctx, l) : [0, 0, 0]));
  }
  // Digital rain: QMK's drop and decay steps, at the firmware's ~60 frames a second.
  rain(dt) {
    const st = this.state;
    st.buf ||= Array.from({ length: 5 }, () => new Float32Array(15));
    st.acc = (st.acc || 0) + dt;
    st.drop ||= 0;
    while (st.acc >= 16) {
      st.acc -= 16;
      const b = st.buf;
      for (let c = 0; c < 15; c++) {
        if (st.drop === 0 && Math.random() < 1 / 24) b[0][c] = 1;
        for (let r = 0; r < 5; r++) if (b[r][c] > 0 && b[r][c] < 1) b[r][c] = Math.max(0, b[r][c] - 1 / 255);
      }
      if (++st.drop > 28) {
        st.drop = 0;
        for (let r = 4; r > 0; r--) {
          for (let c = 0; c < 15; c++) {
            if (r === 4 && b[r][c] >= 1) b[r][c] = 1 - 1 / 255;
            if (b[r - 1][c] >= 1) {
              b[r - 1][c] = 1 - 1 / 255;
              b[r][c] = 1;
            }
          }
        }
      }
    }
    st.rain = NK65_LEDS.map((l) => st.buf[l.r][l.c]);
  }
}

// Effects that only take saturation from the color get full color (device.rs rgb_matrix_rainbow).
const rainbowFx = (effect, kitFirst) => (effect >= 12 && effect <= 22) || effect === 31 || effect === 41 || effect === 42 || (kitFirst != null && effect === kitFirst + 1);

// An LED's drive level (0-255, linear light) as a screen color: the LED's
// light output follows its PWM level, a screen's follows sRGB.
const ledHex = (rgb) => "#" + rgb.map((v) => Math.round(255 * Math.pow(clamp(v, 0, 255) / 255, 1 / 2.2)).toString(16).padStart(2, "0")).join("");

// ---------- thumbnails for the effect picker ----------
// A small picture of the board: `colorOf(key)` gives each key's screen color (or null for dark).
const thumbCache = new Map();
function drawThumb(cacheKey, L, colorOf, w = 112) {
  if (thumbCache.has(cacheKey)) return thumbCache.get(cacheKey);
  const h = Math.round((w * L.h) / L.w);
  const cv = document.createElement("canvas");
  cv.width = w;
  cv.height = h;
  const g = cv.getContext("2d");
  g.fillStyle = "#0c0d0f";
  g.fillRect(0, 0, w, h);
  const kw = w / L.w;
  const kh = h / L.h;
  for (const k of L.keys) {
    g.fillStyle = colorOf(k) || "#000000";
    g.fillRect(Math.round(k.x * kw), Math.round(k.y * kh), Math.max(1, Math.round(k.w * kw) - 1), Math.max(1, Math.round(kh) - 1));
  }
  const url = cv.toDataURL();
  if (thumbCache.size > 400) thumbCache.clear();
  thumbCache.set(cacheKey, url);
  return url;
}

// The NK65 a moment into `effect`, a few presses in, as the simulation draws it.
function fxThumb(effect, p, kitFirst, kitFw) {
  const key = ["nk", effect, p.color1, p.color2, p.speed, kitFirst, kitFw].join("|");
  if (thumbCache.has(key)) return thumbCache.get(key);
  const q = { ...p, effect, brightness: 100 };
  let leds;
  if (effect >= 200) {
    // One color for the whole board at a time: show the colors it moves between, left to right.
    const [h1, s1] = hueSat(p.color1);
    const two = KIT_FX.find((f) => f.id === effect)?.color === "two";
    const cols = effect === 202 ? [0, 40, 80, 120, 160, 200].map((dh) => qmkRgb(h1 + dh, Math.max(s1, 200), 255)) : two ? [p.color1, p.color2].map((x) => qmkRgb(...hueSat(x), 255)) : [qmkRgb(h1, s1, 255)];
    leds = NK65_LEDS.map((l) => cols[Math.min(cols.length - 1, Math.floor((l.x / 225) * cols.length))]);
  } else {
    const sim = new LightSim();
    // Moving effects are shown a few seconds in, presses over the last 0.6 s.
    sim.step(4000, q, kitFirst, kitFw, false);
    for (const id of ["2,3", "1,8", "2,11", "3,5"]) {
      sim.hit(id);
      sim.step(170, q, kitFirst, kitFw, false);
    }
    if (effect === 32) for (let i = 0; i < 300; i++) sim.step(16, q, kitFirst, kitFw, false);
    leds = sim.step(effect === 31 ? 300 : 120, q, kitFirst, kitFw, false);
  }
  return drawThumb(key, LAYOUT.nk65, (k) => {
    const l = LED_BY_ID.get(k.id);
    return l ? ledHex(leds[l.i]) : null;
  });
}
