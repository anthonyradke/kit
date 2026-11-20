# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch. The notes show as a list in
Settings, so keep each line to one short sentence about what changed for the user.

- The taskbar battery readout no longer leaks a little memory each time it changes, which over weeks could stop it drawing.
- A headset that was already on when kit started no longer shows as off when the first status request goes unanswered.
- The PRO X TKL comes back in kit after being off or out of range, instead of staying "not connected" until kit restarts.
