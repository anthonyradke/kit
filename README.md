# headroom

A small, fast replacement for SteelSeries GG and Sonar, built for the **Arctis Nova 5** on Windows.

- **EQ on the headset**: 10-band parametric EQ with a draggable curve. It runs on the headset itself, so it applies to
  everything you hear, with no virtual audio devices and nothing to break when headroom isn't running
- **Mic noise suppression** (RNNoise) installed straight onto the real "Headset Microphone": every app gets the clean mic
- **Keeps the headset as your default device** whenever it reconnects
- Headset settings: mic volume, sidetone, mute light, auto-off, volume limiter, battery
- Presets switchable from the tray. Tiny footprint when the window is closed

> Quit (or uninstall) SteelSeries GG: Sonar takes over your default devices and GG rewrites the headset's settings.

## How it works

- `crates/arctis` talks to the base station over HID (the vendor interface used by GG). Settings, including the EQ,
  are stored on the headset.
- `crates/apo` is a Windows Audio Processing Object: a DLL the Windows audio engine loads for the Nova 5 microphone.
  Installing it needs one administrator prompt; it can be removed from the Mic page.
- `crates/sysaudio` lists audio endpoints, sets the default device, and installs/removes the APO.

## Build from source

Requires Rust (MSVC toolchain) and the Tauri CLI (`cargo install tauri-cli`).

```
cargo build -p headroom && target\debug\headroom.exe     # run the app (debug)
cargo build --release -p headroom-apo
cd app/src-tauri && cargo tauri build                      # release exe + installer
cargo run -p probe                                         # hardware test: status, EQ, settings
cargo run -p probe -- devices                              # audio endpoints and defaults
```

Protocol notes are based on [HeadsetControl](https://github.com/Sapd/HeadsetControl).
