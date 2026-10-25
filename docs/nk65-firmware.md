# Per-key lighting on the NK65: custom firmware plan

kit works with the NK65's stock VIA firmware. That firmware only lets software change the effect, one or two
colors, brightness, speed and the indicator lights. It has no command for "make this key this color", so per-key
painting (which kit already does for the PRO X TKL) needs firmware of our own. This is the plan for that. Nothing
here has been done yet.

## Step 1: find out which PCB is in the board

There are two NK65 PCBs that the Milkshake Edition could have, and they need different firmware:

| | NK65 (original, v1 / v1.4) | NK65 rev B |
|---|---|---|
| USB id | 8968:4E4B | 8968:4E4F |
| Chip | STM32F303 | STM32F072 |
| LED driver | IS31FL3733 (I2C matrix) | WS2812 chain |
| Stock lighting code | wilba's RGB backlight | QMK RGB matrix |
| QMK target | `novelkeys/nk65/v1` or `novelkeys/nk65/v1_4` | `novelkeys/nk65b` |

kit's keyboard Device page shows which one it found ("Board: NK65" or "NK65 rev B"). For the original PCB the
revision (v1 vs v1.4) is printed on the PCB; that has to be checked with the board open, because flashing the wrong
one leaves the LEDs (or the whole board) not working until the right firmware goes on.

## Step 2: a "kit" keymap for QMK

Keep everything VIA and kit already use, and add a little on top:

- Build with RGB matrix on both PCBs (QMK has an IS31FL3733 matrix driver, so the original PCB can move off
  wilba's code).
- A custom RGB matrix effect, "kit paint", that shows a per-key color table.
- Store that table in the keyboard's EEPROM (QMK's keyboard data block), so the painted layout survives unplugging
  and works on any computer, like everything else on this board.
- Answer a few extra commands on VIA's raw HID channel, on a channel id VIA doesn't use: read the firmware's kit
  version, set colors for a run of keys, save. kit's `crates/via` already speaks this transport, so it is a small
  addition there.
- Optional, cheap once the above exists: per-layer colors (light the keys that do something on layer 1 while it is
  held) and a caps-lock color for chosen keys only.

The firmware would live in this repo (`firmware/nk65/`), built by a GitHub Actions job with QMK's Docker image, so
nobody needs a QMK toolchain on their PC.

## Step 3: kit side

- When the board answers the kit-version command, the NK65 Lighting page offers "Paint" under the effects (the
  same brush, eraser and swatches as the PRO X TKL).
- Painted colors go in the lighting preset, like they already do for the PRO X TKL, and are written to the board
  when a preset is switched.

## Step 4: flashing

1. kit already has the keymap and macros (Keys page, Device page); also export them from VIA once as a backup.
2. Put the board in bootloader mode: a key set to QMK's boot keycode (`QK_BOOT`, "Reset" in VIA), or the
   reset button / pads on the PCB.
3. Flash with QMK Toolbox (STM32 DFU). If anything goes wrong, the STM32's built-in bootloader can't be overwritten,
   so the stock firmware (`novelkeys/nk65:via` or `novelkeys/nk65b:via` from QMK) can always be put back the same way.
4. Re-apply the keymap and macros from kit if the flash cleared EEPROM.

Flashing from inside kit (dfu-util) could come later; QMK Toolbox is the safer first step.

## Open questions

- Which PCB is in the Milkshake Edition (step 1).
- Whether the original PCB's IS31FL3733 LED map in QMK matches this case's LEDs (it should; it is the same PCB).
