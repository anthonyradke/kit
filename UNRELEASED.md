# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch. The notes show as a list in
Settings, so keep each line to one short sentence about what changed for the user.

- A settings file kit can't read is kept aside instead of being overwritten with defaults, and Settings says what happened.
- Each import keeps its own copy of the previous settings (the last five), instead of replacing the one before.
- A settings file with crafted profile or macro ids can no longer run code in kit's window.
