# kit

One small, fast app that replaces Logitech G HUB and SteelSeries GG / Sonar on Windows, built for a
**Logitech PRO X Superlight** mouse and a **SteelSeries Arctis Nova 5** headset.

![kit home screen](docs/home.png)

**Mouse**
- **Unlimited profiles**: DPI stages, polling rate and side-button bindings per profile
- **Instant switching** from the tray menu, a global shortcut per profile, a "next profile" shortcut, or a side button
- **Macros**: record keys and clicks with their timing, edit the steps, and play them once, while held, or as a toggle
- **Side buttons** can be a key combination, a macro, a DPI shift (sniper), DPI cycling, or a profile switch

**Headset**
- **EQ on the headset**: 10-band parametric EQ with a draggable curve. It runs on the headset itself, so it applies to
  everything you hear, with no virtual audio devices and nothing to break when kit isn't running
- **Mic noise suppression** (RNNoise) installed straight onto the real "Headset Microphone": every app gets the clean mic
- **Keeps the headset as your default device** whenever it reconnects
- Mic volume, sidetone, mute light, auto-off, volume limiter, battery

**Both**: one tray icon for profiles, presets and noise suppression; about 6 MB of memory when the window is closed.

> Quit (or uninstall) **G HUB** and **SteelSeries GG**. G HUB talks to the mouse over the same channel; Sonar takes
> over your default audio devices and GG rewrites the headset's settings.

## Install

Download `kit_x.y.z_x64-setup.exe` from [Releases](https://github.com/anthonyradke/kit/releases) and run it.
Closing the window keeps kit running in the tray. Use **Quit** from the tray menu to exit.
Settings from lightswitch and headroom (the two apps kit replaced) are copied over the first time it runs.

## How it works

- `crates/hidpp` controls the mouse over Logitech's HID++ 2.0 protocol through the Lightspeed receiver. kit puts the
  mouse in *host mode* so it can change the polling rate, and re-applies your settings whenever the mouse wakes.
- Side buttons and macro recording use Windows low-level input hooks. Playback uses `SendInput` with scan codes,
  so it works in games that ignore virtual-key input.
- `crates/arctis` talks to the headset's base station over HID (the vendor interface GG uses). Settings, including the
  EQ, are stored on the headset.
- `crates/apo` is a Windows Audio Processing Object: a DLL the Windows audio engine loads for the Nova 5 microphone.
  Installing it needs one administrator prompt; it can be removed from the Mic page.
- `crates/sysaudio` lists audio endpoints, sets the default device, and installs/removes the APO.
- Settings live in `%APPDATA%\com.anthonyradke.kit\` (`mouse.json`, `headset.json`).

## Build from source

Requires Rust (MSVC toolchain) and the Tauri CLI (`cargo install tauri-cli`).

```
cargo build --release -p kit-apo            # once: the installer bundles this DLL
cargo build -p kit && target\debug\kit.exe  # run the app (debug)
cd app/src-tauri && cargo tauri build       # release exe + installer in target/release/bundle
cargo run -p mouse-probe                    # mouse hardware test: device info
cargo run -p mouse-probe -- dpi 800         # set DPI directly
cargo run -p headset-probe                  # headset hardware test: status, EQ, settings
cargo run -p headset-probe -- devices       # audio endpoints and defaults
```

## Layout

- `crates/hidpp`, `crates/mouse-probe`: mouse protocol and its CLI test tool
- `crates/arctis`, `crates/apo`, `crates/sysaudio`, `crates/headset-probe`: headset protocol, mic APO, Windows audio, CLI
- `app/src-tauri`: the app. `src/mouse` (device worker, input hooks, macro player, hotkeys), `src/headset`
  (HID worker, audio endpoint watcher, APO installer), `src/tray.rs`, `src/main.rs`
- `app/ui`: the window (plain HTML/CSS/JS, no build step)
- `scripts`: dev helpers for screenshots, synthetic input and the app icon

Headset protocol notes are based on [HeadsetControl](https://github.com/Sapd/HeadsetControl). Product photos are
Logitech's and SteelSeries' own. The Archivo typeface is under the SIL Open Font License (`app/ui/fonts/OFL.txt`).
