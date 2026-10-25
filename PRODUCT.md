# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users
The owner, a PC gamer on Windows 11 with a white Logitech PRO X Superlight (original, Lightspeed receiver), a white SteelSeries Arctis Nova 5 headset and an NK65 Milkshake Edition keyboard (VIA firmware); and the owner's brother, with the black mouse and headset and a Logitech PRO X TKL LIGHTSPEED keyboard. A first-run setup picks which devices (and colors) are on the PC. Opens the app in a mix of ways: occasional set-up sessions (profiles, macros, EQ presets), and frequent quick checks of battery or a quick switch. Day-to-day switching mostly happens from global hotkeys, a side button or the tray, without the window.

## Product Purpose
kit replaces Logitech G HUB, SteelSeries GG / Sonar and the VIA web app, which were unreliable or clumsy (G HUB's per-app switching never worked; Sonar's virtual audio devices broke inputs and outputs). It talks to both devices directly and stays tiny when the window is closed. Success: the devices just work, settings survive sleep, wake and reboots, and the app never gets in the way.

## Positioning
A personal, single-purpose replacement for two vendors' bloated suites, covering exactly this mouse and this headset. No virtual audio devices, no accounts, no popups. The headset EQ runs on the headset's own onboard DSP, so it keeps working even when kit is not running.

## Operating Context
- Tauri 2 desktop app, frameless window drawing its own title bar; plain HTML/CSS/JS (no bundler, no Node). Window ~1240x800, min 1000x660, not maximizable.
- Closing the window destroys the webview; the app lives in the system tray. Only tray Quit exits.
- Used at a gaming desk, often at night, often while a game is running.

## Capabilities and Constraints
- Mouse: unlimited user profiles (manual switching only: tray, per-profile hotkey, next-profile hotkey, side button; no per-app switching). Up to 5 DPI stages per profile (100-25600, step 50) with a default stage; report rate 125/250/500/1000 Hz. Only the two side buttons (Back/Forward) are rebindable: default, disabled, key combination, macro, DPI shift (held), cycle DPI, next profile, switch to profile. Macros: record keys and clicks with timing, edit steps, play once (with repeat) / while held / toggle. Status: connected / asleep / receiver not found, battery, charging.
- Headset: 10-band parametric onboard EQ (peak, low/high shelf, low/high pass; ±10 dB; Q 0.2-10) with named presets, live drag preview. Mic noise suppression (RNNoise system-effect APO on the real mic endpoint, needs one UAC prompt to install) with strength %. Mic volume 0-15, sidetone 0-10, mute LED off/low/medium/high, auto-off timer, volume limiter. Keeps the headset as the Windows default device. Warns when SteelSeries Sonar holds the defaults. Single output only (no Game/Chat/Media split).
- Keyboard (NK65): remap every layer, onboard macros, lighting (effect, colors, brightness, speed, indicators) with saved presets; kit macros on keys either stored on the board or run by kit. (PRO X TKL): zone effects and per-key paint, battery, idle lights-off, kit-handled key bindings.
- App: first-run setup (devices, colors, vendor-app check, startup), launch at startup, config file paths, noise suppression log.
- No on-screen popups or notifications.

## Brand Commitments
- Name: **kit** (lowercase).
- Dark theme (confirmed by the user).
- Kept from the old apps by the user's choice: the top-down mouse drawing with labelled callout lines to each button; the drag-to-edit EQ graph with its band strip.
- Navigation: G HUB-style home screen with photos of both devices, plus a slim rail of device thumbnails for one-click switching.
- The old lightswitch/headroom look (near-black, violet-to-blue gradient with glows, tracked uppercase labels) felt generic to the user and is being replaced.

## Evidence on Hand
- Real device data comes live from the hardware (battery, DPI, rate, EQ). The user's own profiles, macros (e.g. "678 - valheim") and presets ("Main", "opus5.5", "Footsteps", "Voice", ...) live in their config.
- Product photos: white and black PRO X Superlight and Arctis Nova 5, black PRO X TKL (manufacturers' sites), NK65 Milkshake (Oblotzky, background removed); provenance next to each in app/ui/img.

## Product Principles
1. Out of the way: tiny when closed, no popups, nothing to babysit.
2. Hardware truth first: show what the device is actually doing (live DPI, battery, what is stored on the headset).
3. Every setting one click from the device it belongs to.
4. Reliability over features: skip anything the vendors did badly rather than half-doing it.
