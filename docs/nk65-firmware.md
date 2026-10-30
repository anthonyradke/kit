# kit's NK65 firmware

The NK65 Milkshake Edition I have came with old firmware (VIA protocol 9) running wilba's RGB backlight code. That
code only lights the whole board one way at a time, so effects that start at the key you press weren't possible.
Current QMK has moved the original NK65 (USB id 8968:4E4B) to RGB matrix, which can light every key on its own and
already includes key-triggered effects (Splash, Multi-splash, the Reactive ones). kit's firmware is that, plus a few
effects of my own.

## What's in it

`firmware/nk65/keymaps/kit` is a QMK keymap, built on `novelkeys/nk65/v1_4`, that runs on both NK65 PCBs:

- `keymap.c`: QMK's default NK65 layout with VIA. Fn+\ is the bootloader key; the Fn layer has lighting keys
  (Fn + Q/W/E effect, A/S/D/F hue and saturation, Z/X/C/V brightness and speed). It also answers two values on
  VIA's custom channel: `0x6B` (get) returns the firmware version, the number of kit effects, the first kit
  effect number and (from version 2) which PCB's pins have seen a key press; `0x62` (set) restarts the board into
  its bootloader; `0x63` (get/set, from version 4) is the second color's hue and saturation and the background
  level, and VIA's save on the custom channel stores it in the user EEPROM word. From version 3 it flags Esc, Backspace, Enter, the modifiers, the arrows and the right-hand column
  as modifiers: QMK's NK65 layout flags every key the same, which left Alphas / mods with one color.
- `rgb_matrix_user.inc`: kit's effects, appended after QMK's own, so the stock effect numbers don't move. Ripple,
  Prism ripple, Ink and Afterglow react to key presses; Aurora, Lava, Tide and Fireflies are ambient; Gradient (from
  version 4) is still. They do their math in floats (the STM32F303 has an FPU). From version 4 they use two colors
  (QMK's own and a second one), a background level that defaults to off, brightness and speed. Prism ripple is
  still in the firmware so the numbers after it don't move, but kit no longer offers it.
- Version 4 also puts every effect's brightness through a CIE 1931 lightness curve (`rgb_matrix_hsv_to_rgb` in
  `keymap.c`), so the brightness setting looks even and fades glide; unlike QMK's own table, its lowest steps round
  down to off, so fades don't end on a faint glow.
- `config.h`: remembers 16 key presses for ripples (QMK's default is 8) and starts on Ripple after a flash.
- `matrix.c`: the key matrix. The two PCB revisions differ only in the pins of columns 7-9 (7 8 9, U I O, J K L,
  N M , and right Alt): C13 C14 C15 on v1, B1 B0 A7 on v1.4. Columns are inputs with pull-ups, so it reads both sets
  and leaves out any pin that is already low before a row is selected. Version 1 of the firmware came as two files,
  one per PCB, and the wrong one left those keys dead.

## Building

`firmware/nk65/build.sh <qmk_firmware checkout> [out dir]` builds `kit-nk65.bin`, with
the qmk CLI if it's installed or QMK's Docker image if not. The release workflow builds it at the QMK commit in
`firmware/nk65/QMK_VERSION` and attaches it to every GitHub release.

## How kit handles it

- On connect kit asks the board for RGB matrix brightness (channel 3). If it answers, kit treats it as an RGB matrix
  board even though the USB id says wilba. If it also answers `0x6B`, the kit effects show up on the Lighting page.
- The board holds one keymap, the active profile's. kit keeps every profile in `keyboard.json`, and switching needs
  no firmware support: kit writes the keycodes that differ over VIA, once every key is up (a key remapped while held
  would be released as its new keycode and stick). On connect, the board's keymap replaces kit's copy of the profile it
  holds, so edits made in VIA are kept. A switch that was cut short is finished from kit's copy instead. Keys that
  switch profiles are spare F13-F24 codes kit catches on release, so they need kit running.
- kit keeps a copy of the keymap and the raw macro buffer in `nk65-backup.json` next to its config. When the board
  comes back on different firmware (another VIA protocol or kit firmware version), the Device page offers to put them
  back. Keycodes from before protocol 12 are translated (`crates/via/src/keycodes.rs`), in every saved profile too; a key with
  no equivalent keeps the new default. Bump `KIT_FIRMWARE_VERSION` for every firmware release, because a new build wipes the EEPROM and
  the version is what tells kit to offer the restore.
- QMK takes one color (hue and saturation) and a speed for every effect, and each effect uses them its own way, so
  kit translates: rainbow effects get full saturation, Alphas / mods gets its second color as a hue step in the speed
  value, and the Lighting page shows only the controls an effect uses. The catalog and a port of each effect's math
  (QMK's and kit's) are in `app/ui/lighting.js`, which also draws the preview; keep it in step with the firmware.
- QMK's Splash, Solid splash and the wide, cross and nexus reactive effects come in pairs: one draws only the latest
  press, its multi version every remembered press. kit shows each pair as one effect with a switch. kit offers only
  the smoother effects; the others stay in the firmware and show on the Lighting page only while a preset uses one.
- "Restart into flash mode" on the Device page sends `0x62` on kit's firmware, or VIA's old bootloader command on
  older firmware (it may not exist there; then Fn+\ or Esc held while plugging in).

## Ideas for later

- A streaming mode where kit draws per-key frames on the PC and sends them over raw HID (about 8 reports per frame),
  for effects that need the PC: sound spectrum across the rows, notifications.
- Per-layer key colors (light the keys that do something while Fn is held).
