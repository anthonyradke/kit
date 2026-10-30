# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch.

- Mic noise suppression now uses DeepFilterNet 3 instead of RNNoise. Background noise no longer swells and fades with every word (the "wind" sound), an 80 Hz high-pass removes rumble, breath pops and desk thumps, and your voice comes through as clearly as before. Mic latency goes from 10 ms to about 50 ms. After updating kit, the Mic page asks to update noise suppression (one Windows prompt).
- Headset EQ presets trimmed to four: Flat, Main, Balanced and Bass. Built-ins you never edited and aren't using are removed; edited ones and the active one are kept.
- Main, Balanced and Bass are louder again: boosts go up to +6 dB (what SteelSeries' own presets use) while still keeping the Nova 5's own 80 Hz bass peak from turning boomy.
- The volume limiter's description now says what it does: it caps the headset at the EU hearing-safety level (about 85 dB). Turn it off if the headset isn't loud enough.
