# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch.

- Retuned the headset EQ presets so bass stays clean instead of boomy: no preset boosts more than +3 dB (the headset has no preamp, so bigger boosts clip), and the bass presets now cut the Nova 5's own 80 Hz peak and add their low end under 60 Hz. Built-in presets you edited are kept as a "(yours)" copy.
- New Balanced preset: the headset's bass peak flattened into an even lift, for everyday listening.
- The Bass slider's shelf now starts at 60 Hz, below the headset's 80 Hz peak, so turning it up adds rumble instead of boom.
