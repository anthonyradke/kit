# Unreleased

Changes since the last release. Each fix or feature adds a line here; `scripts/release.sh` turns
this list into the release notes and empties it for the next batch.

- Retuned the headset EQ presets so bass stays clean instead of boomy: no preset boosts more than +3 dB (the headset has no preamp, so bigger boosts clip), and the bass presets now cut the Nova 5's own 80 Hz peak and add their low end under 60 Hz. Built-in presets you edited are kept as a "(yours)" copy.
- New Balanced preset: the headset's bass peak flattened into an even lift, for everyday listening.
- The Bass slider's shelf now starts at 60 Hz, below the headset's 80 Hz peak, so turning it up adds rumble instead of boom.
- An update button in the title bar checks GitHub on demand and turns into an Update button when a new version is waiting.
- Themes: pick one in Settings. Graphite (the original), Field, Phosphor and Blueprint, plus plain Dark and Light.
- NK65 lighting: effects are picture cards in groups (Typing, Still, Calm, Pattern, Rainbow, From kit), each with a description and only the color and speed controls it actually uses, labeled for what they do (for example Letter keys and Modifier keys, or Spread on the gradients).
- NK65 lighting: the preview runs the same effect math as the keyboard, with made-up typing, and you can click keys to press them.
- NK65 lighting: Splash, Solid splash and the wide, cross and nexus reactive effects keep every ripple by default instead of a new key press cutting off the last one; a switch brings back the one-at-a-time version.
- NK65 lighting: rainbow effects always show full color, instead of washing out to white when a white or pale color was picked.
- NK65 lighting: brightness is the brightest an effect gets on kit's whole-board effects too (Keystrike, Typing heat and the rest), like everywhere else, instead of the resting level; and a custom color no longer looks darker in kit than it does on the board.
- NK65 firmware 3: Alphas / mods colors the modifiers, arrows and right column in the second color (on earlier versions every key counted as a letter), and the second color is picked as a color instead of through the speed setting.
