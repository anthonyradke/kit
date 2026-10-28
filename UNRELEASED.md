# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch.

- Retuned the headset EQ presets so bass stays clean instead of boomy: no preset boosts more than +3 dB (the headset has no preamp, so bigger boosts clip), and the bass presets now cut the Nova 5's own 80 Hz peak and add their low end under 60 Hz. Built-in presets you edited are kept as a "(yours)" copy.
- New Balanced preset: the headset's bass peak flattened into an even lift, for everyday listening.
- The Bass slider's shelf now starts at 60 Hz, below the headset's 80 Hz peak, so turning it up adds rumble instead of boom.
- An update button in the title bar checks GitHub on demand and turns into an Update button when a new version is waiting.
- Themes: pick one in Settings. Graphite (the original), Field, Phosphor and Blueprint, plus plain Dark and Light.
- NK65 lighting: effects are picture cards in groups (Typing, Calm, Still, Rainbow, From kit), trimmed to the smooth ones, with a star to add any of them to a Favorites group.
- NK65 lighting: each effect names what its colors are for (Ring and Fades to on Ripple, Water and Crests on Tide), and colors are picked with hue and saturation sliders plus swatches.
- NK65 lighting: the preview runs the same effect math as the keyboard, with made-up typing, and you can click keys to press them.
- NK65 lighting: Reactive wide keeps every glow by default instead of a new key press cutting off the last one; a switch brings back the one-at-a-time version.
- NK65 lighting: rainbow effects always show full color, instead of washing out to white when a white or pale color was picked.
- NK65 lighting: brightness is the brightest an effect gets on kit's whole-board effects too, instead of the resting level.
- NK65 firmware 4: kit's effects take a second color and a background level, and fade all the way to off instead of leaving a faint glow (set the background above 0% to keep one). Ripple, Ink, Aurora and Fireflies use your colors instead of shifting the hue on their own. New Gradient between two colors.
- NK65 firmware 4: brightness follows how the eye sees it, so 50% looks half as bright and fades are smoother.
- NK65 firmware 4: Alphas / mods colors the modifiers, arrows and right column in the second color (before, every key counted as a letter).
