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
lock screen or in sleep works the same way, unsaved, so a power cut brings the board back with its stored
lighting.

## PRO X TKL

Lighting through HID++ features 0x8071 (RGB effects) and 0x8081 (per-key lighting). kit takes the LEDs over
from the onboard profile while it runs, turns them off when the keyboard reports idle and hands them back on
Quit.

## Headset

Source: `crates/arctis`.

The base station's vendor HID interface, the same one GG uses. Settings, including the EQ, are stored on the
headset.

## EQ on the PC

Source: `crates/eq`, `crates/apo`.

The same DLL as noise suppression holds a second APO, installed as the endpoint effect on the Nova 5's output,
after Windows mixes every app. The app writes the curve to the registry and the APO picks it up live. Filters
are designed at twice the rate and matched at 48 kHz, which is how they come out the same as Sonar's (it runs
its EQ at 96 kHz).

## Noise suppression

Source: `crates/apo`.

A Windows Audio Processing Object, a DLL the audio engine loads for the Nova 5 microphone. The DeepFilterNet 3
model runs on its own thread next to the engine's real-time thread and adds about 50 ms of mic latency.
Installing it, or updating it after a new kit version, takes one administrator prompt; it can be removed from
the Mic page.

## Surround

Source: `crates/spatial`, `src/headset/surround.rs`.

Kit sets VB-CABLE's "CABLE Input" to 7.1 at 48 kHz and makes it the default device for everything but calls,
then records it with WASAPI loopback. Each channel is convolved (uniformly partitioned FFT convolution,
256-sample blocks) with a Neumann KU100 dummy head's response from that speaker's direction, plus the early
reflections of a measured listening room from the nearest measured direction and a shared late reverb; the LFE
channel is low passed into both ears. A lookahead limiter keeps the sum from clipping. The cable and the
headset run on separate clocks, so the stereo result is resampled by a small, continuously adjusted ratio (at
most ±0.2 %) that holds the headset's buffer at 20 ms. The renderer uses about 1.5 % of one CPU core; the
whole path adds about 40 ms. Quitting kit makes the headset the default again.

## Other modules

- `crates/sysaudio` lists audio endpoints, sets default devices and speaker formats, records and plays
  streams, and installs or removes the APO.
- `src/session.rs` watches for the session locking, the displays turning off and Windows going to sleep.
- Settings live in `%APPDATA%\com.anthonyradke.kit\` (`app.json`, `mouse.json`, `keyboard.json`,
  `headset.json`).
