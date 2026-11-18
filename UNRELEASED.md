# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch. The notes show as a list in
Settings, so keep each line to one short sentence about what changed for the user.

- kit no longer asks the headset for its status every 2 seconds; it listens for the changes the headset reports, as GG does, which stops crackles in the audio.
- kit's limiter (preamp Off) no longer distorts loud bass: it now holds its gain through each bass note, as Windows' limiter did for Sonar.
- Presets have Sonar's Bass, Voice and Treble sliders, measured to match Sonar, in place of the Bass slider for all presets.
- The presets are now Flat, Tony Preset and Deep Bass, measured from Sonar; the older built-ins are removed unless edited or in use.
- The EQ always runs in kit's EQ on the headset's output, as Sonar's did; the option to run it on the headset is gone and the headset's own EQ stays flat.
- Removed virtual surround (it needed VB-CABLE) and ChatMix (the Nova 5 has no ChatMix dial).
