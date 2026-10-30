# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch.
- Fixed noise suppression breaking the mic: Windows' audio engine loads it as part of an object of its own (COM aggregation), which kit refused, so no app could open the mic. Installing now also checks that the mic still opens and removes noise suppression again by itself if it doesn't.
