# Architecture

How each part of kit talks to its device, and the reasoning behind the less obvious choices. The
[README](../README.md#how-it-works) has the overview diagram.

## Mouse

Source: `crates/hidpp`.

Logitech's HID++ 2.0 protocol through the Lightspeed receiver. kit switches the mouse to host mode so it can
change the polling rate, and re-applies settings whenever the mouse wakes.

## Input

Side buttons and macro recording use Windows low-level input hooks. Playback uses `SendInput` with scan codes,
so it works in games that ignore virtual-key input.

## NK65

Source: `crates/via`.

VIA's raw-HID protocol for the keymap, the onboard macro buffer and lighting values, on both the original PCB
(wilba's RGB backlight) and rev B (QMK RGB matrix). Per-key color needs RGB-matrix firmware; kit's is in
`firmware/nk65`. kit's own effects (`keyboard/anim.rs`) set the board to Solid and then stream one color and
brightness about 30 times a second without saving, so the EEPROM isn't worn. Key presses come from the input
hook (counted, never recorded) and the music level from the default output's peak meter. Lights out at the
lock screen, with the displays off or in sleep works the same way, unsaved, so a power cut brings the board back with its stored
lighting.

## PRO X TKL

Lighting through HID++ features 0x8071 (RGB effects) and 0x8081 (per-key lighting). kit takes the LEDs over
from the onboard profile while it runs, turns them off when the keyboard reports idle and hands them back on
Quit.

## Headset

Source: `crates/arctis`.

The base station's vendor HID interface, the same one GG uses. The headset's own settings (sidetone, mic volume,
mute light, auto-off, volume limiter) are stored on it. Its own EQ is kept flat: the EQ runs in kit (below).

The worker asks for the status once, when the base station appears and when the headset comes on, and then only
listens: the base station pushes battery, charging and on/off on a second interface (usage page 0xFF00), which a
thread of its own reads (seen on the user's headset: `b9 02` off, `b9 03` on, `bb 03` on battery, `b7 35` 53%). GG works
the same way, asking only when the headset comes on; asking every few seconds, as kit used to, was measured by another
Nova 5 tool to cause crackles.

The Nova 5 has no ChatMix dial, only a volume wheel the headset handles itself: the status reply's dial bytes read 0
and 0, and GG's ChatMix is a slider on screen.

## EQ

Source: `crates/eq`, `crates/apo`.

The same DLL as noise suppression holds a second APO, installed as the endpoint effect on the Nova 5's output,
after Windows mixes every app, the way Sonar ran its EQ but without a virtual device. The app writes the curve to the
registry and the APO picks it up live. Filters are designed at twice the rate and matched at 48 kHz, which is how
they come out the same as Sonar's (it runs its EQ at 96 kHz). Each preset has Sonar's Bass, Voice and Treble sliders,
measured as one peak each (docs/sonar-eq.md). Without the APO installed no EQ plays.

## Noise suppression

Source: `crates/apo`.

A Windows Audio Processing Object, a DLL the audio engine loads for the Nova 5 microphone. The DeepFilterNet 3
model runs on its own thread next to the engine's real-time thread and adds about 50 ms of mic latency.
Installing it, or updating it after a new kit version, takes one administrator prompt; it can be removed from
the Mic page.

## Keeping the audio effects working

Both APOs report what they are doing (streams open, the noise model loaded, the EQ running, or a fall back to
passthrough after an error) as a small JSON value under `HKLM\SOFTWARE\kit\Status`, which the installer makes
writable by LOCAL SERVICE, the account audiodg runs as. The Mic and Sound pages and Settings show it. Repair in
Settings takes the effects off endpoints they no longer belong on (a driver update recreates the headset's
endpoints under new ids), replaces a missing or older DLL, puts the effects back on the headset and restarts
Windows audio. Removing an effect takes it off every endpoint that lists it, connected or not, and kit's
uninstaller removes both (`kit.exe --remove-audio-effects`, run from `app/src-tauri/windows/hooks.nsh`).

## Taskbar battery

Source: `src/taskbar.rs`.

The headset's and mouse's battery next to the notification area, from the same readings as the tray tooltip. Windows
has no API for this, so kit makes a small window of its own a child of the taskbar (`Shell_TrayWnd`), right-aligned
against `TrayNotifyWnd`, and draws it with per-pixel alpha through `UpdateLayeredWindow`. Each run of text is drawn
white on black with GDI's grayscale antialiasing and laid on in its color with that coverage as alpha. Glyphs come
from Segoe Fluent Icons. Layered child windows need the app to declare Windows 8 or later, which
`kit.manifest.xml` does. The child window ties its thread's input to explorer's, so that thread only draws; every two
seconds it follows the notification area and recreates the window after explorer restarts.

## Other modules

- `crates/sysaudio` lists audio endpoints, sets default devices and speaker formats, records and plays
  streams, and installs or removes the APO.
- `src/session.rs` watches for the session locking, the displays turning off and Windows going to sleep.
- Settings live in `%APPDATA%\com.anthonyradke.kit\` (`app.json`, `mouse.json`, `keyboard.json`,
  `headset.json`). Every write goes through a temporary file. A file kit can't read is copied aside
  (`mouse.json.<time>.bak`) instead of being replaced, and Settings says so. An import first saves the current
  settings as `before-import-<time>.json`; the last five are kept.
- `src/logging.rs` writes `%LOCALAPPDATA%\com.anthonyradke.kit\logs\kit.log` (up to 1 MB, then `kit.1.log`),
  panics included. `src/diagnostics.rs` saves it, every device's status, Windows' audio endpoints and the
  settings as one text file from Settings.
