# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch. The notes show as a list in
Settings, so keep each line to one short sentence about what changed for the user.

- A settings file kit can't read is kept aside instead of being overwritten with defaults, and Settings says what happened.
- Each import keeps its own copy of the previous settings (the last five), instead of replacing the one before.
- A settings file with crafted profile or macro ids can no longer run code in kit's window.
- kit's EQ, the mic filter and surround no longer slow down after a stretch of silence.
- An EQ setting that can't be built (or a stray NaN from an app) passes audio through instead of making noise.
- kit's EQ at 96 kHz matches Sonar's exactly.
- EQ changes made quickly in a row (dragging a band) always reach kit's EQ; one could be missed before.
- Installing an update puts the headset back as the default device and the PRO X TKL back on its own lighting first, as Quit does.
- Quitting or updating while a macro or key combination is held lets go of its keys.
- An update check or download that stalls gives up instead of leaving Settings on "Checking…".
- A looping macro stops when its button or key is given another action.
- Importing settings with Add keeps a profile's macro binding when that macro is already here under another name.
- Leaving the macro page while recording stops the recording and keeps it, so side buttons work again; Add key is hidden while recording.
- When kit's EQ is added to or removed from the headset (including by a driver update), the EQ moves to match right away, so it is never missing or applied twice.
- A future preset update keeps playing your edited version of the active built-in preset.
- Updating kit's EQ no longer switches Run in kit on.
- Dragging an EQ band ends cleanly when the window loses focus, and a surround switch from the tray isn't undone by an edit still being saved.
- Removing noise suppression or kit's EQ takes it off the headset even while the headset is unplugged, and uninstalling kit removes both.
- Two installs started at once no longer get in each other's way.
- VIA macros copied into kit fit on the NK65 again: plain typing is stored as text instead of key presses.
- NK65 lighting changed with the Fn keys no longer stops kit from setting it, and lighting picked with a slider is stored on the keyboard.
- NK65 macro edits made while the keyboard is unplugged reach it when it's back, and a failed restore after flashing keeps the macro backup.
- The Mic and Sound pages show what noise suppression and kit's EQ are doing inside Windows' audio engine, including when one has fallen back to passing audio through.
- Settings has a Repair for noise suppression and kit's EQ: it puts them back after a driver update, replaces missing or old files and restarts Windows audio.
- kit keeps a log file, and Settings can save a diagnostics file (versions, device status, settings and the log) to send along with a problem.
