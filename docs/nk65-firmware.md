# kit's NK65 firmware

The NK65 Milkshake Edition I have came with old firmware (VIA protocol 9) running wilba's RGB backlight code. That
code only lights the whole board one way at a time, so effects that start at the key you press weren't possible.
Current QMK has moved the original NK65 (USB id 8968:4E4B) to RGB matrix, which can light every key on its own and
already includes key-triggered effects (Splash, Multi-splash, the Reactive ones). kit's firmware is that, plus a few
effects of my own.

## What's in it

`firmware/nk65/keymaps/kit` is a QMK keymap for `novelkeys/nk65/v1` and `v1_4`:

- `keymap.c`: QMK's default NK65 layout with VIA. Fn+\ is the bootloader key; the Fn layer has lighting keys
  (Fn + Q/W/E effect, A/S/D/F hue and saturation, Z/X/C/V brightness and speed). It also answers two values on
  VIA's custom channel: `0x6B` (get) returns the firmware version, the number of kit effects and the first kit
  effect number, and `0x62` (set) restarts the board into its bootloader.
- `rgb_matrix_user.inc`: kit's effects, appended after QMK's own, so the stock effect numbers don't move. Ripple,
  Prism ripple, Ink and Afterglow react to key presses; Aurora, Lava, Tide and Fireflies are ambient. They use the
  color, brightness and speed set in kit, and do their math in floats (the STM32F303 has an FPU).
- `config.h`: remembers 16 key presses for ripples (QMK's default is 8) and starts on Ripple after a flash.

The two PCB revisions differ only in three column pins. Flashing the wrong one leaves U I O / J K L / M , . dead and
nothing else; Fn+\ still works, so the other file can go on the same way.

## Building

`firmware/nk65/build.sh <qmk_firmware checkout> [out dir]` builds `kit-nk65-v1.bin` and `kit-nk65-v1_4.bin`, with
the qmk CLI if it's installed or QMK's Docker image if not. The release workflow builds them at the QMK commit in
`firmware/nk65/QMK_VERSION` and attaches them to every GitHub release.

## How kit handles it

- On connect kit asks the board for RGB matrix brightness (channel 3). If it answers, kit treats it as an RGB matrix
  board even though the USB id says wilba. If it also answers `0x6B`, the kit effects show up on the Lighting page.
- kit keeps a copy of the keymap and the raw macro buffer in `nk65-backup.json` next to its config. When the board
  comes back on different firmware (another VIA protocol or kit firmware version), the Device page offers to put them
  back. Keycodes from before protocol 12 are translated (`crates/via/src/keycodes.rs`); a key with no equivalent keeps
  the new default. Bump `KIT_FIRMWARE_VERSION` for every firmware release, because a new build wipes the EEPROM and
  the version is what tells kit to offer the restore.
- "Restart into flash mode" on the Device page sends `0x62` on kit's firmware, or VIA's old bootloader command on
  older firmware (it may not exist there; then Fn+\ or Esc held while plugging in).

## Ideas for later

- A streaming mode where kit draws per-key frames on the PC and sends them over raw HID (about 8 reports per frame),
  for effects that need the PC: sound spectrum across the rows, notifications.
- Per-layer key colors (light the keys that do something while Fn is held).
