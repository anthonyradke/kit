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
