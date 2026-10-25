---
version: 1
slug: "app-ui-index-html"
primary_target: "app/ui/index.html"
related_targets: ["app/ui/app.js","app/ui/style.css"]
---

# kit desktop window (app/ui)

Scope: the whole kit window (home, mouse pages, headset pages, settings). Mode: Operate.
Audience and job: the owner, at a gaming desk, often at night; quick battery and state checks plus occasional set-up sessions (profiles, macros, EQ). Constraints: Tauri webview, no bundler, offline (self-hosted fonts), frameless window 1240x800, min 1000x660; G HUB-style home with product photos and a slim device rail (user's choice); dark theme, mouse callouts and drag EQ graph kept (user's choice).

## Direction contract

THESIS: Every device reads its live state off its own rating plate, the printed spec label under real hardware: ruled cells, tiny condensed captions, firm values. Refuses the vendor default of glowing tiles and gradient accents on near-black.

OWN-WORLD: Warm graphite ground (#141517) with a darker rail and title bar, silver-white label ink (#ece8df) and two quieter inks for captions. Hairline rules, 3px corners, no glows, no gradients except the soft floor light under each product photo. Safety yellow (#e8b931) means one thing only: live on the hardware right now. Archivo variable, width axis doing the work: condensed caps captions, semi-condensed tabular values, normal width body.

STORY: Open kit, see both white devices standing on their plates with battery and what is live, click one, change it, close the window.

FIRST VIEWPORT: Title bar with the kit wordmark and window controls. 92px rail: Home, mouse thumbnail, headset thumbnail, settings at the foot, battery under each thumbnail. Home: two equal columns, each a product photo on a floor light, the device name, a two-row rating plate (status, battery, two key values; profile or preset and one more), then Open plus one direct action.

FORM: Rating Plate (equipment spec label), candidate 6 of 7 on the ordered list; raises: fixed in-place value cells (split-flap), yellow reserved for live (orienteering), callout leaders at 0/45/90 degrees only (transit), denser device pages (high-density web). seed:818f6f41a0eaf6d4
Signature interaction: when the hardware reports a change (DPI shift from a side button, battery tick, preset switched from the tray) the changed plate cell flashes its value in yellow for a moment, in place, without reflow.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
