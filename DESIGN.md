---
name: kit
description: One small desktop window for one mouse and one headset, read off their own rating plates.
colors:
  graphite-ground: "#141517"
  chrome-graphite: "#0f1012"
  raised-graphite: "#1a1b1e"
  raised-graphite-2: "#212327"
  hairline: "#2a2c30"
  hairline-2: "#36383d"
  hairline-3: "#4a4c51"
  label-ink: "#ece8df"
  label-ink-2: "#bdb9b0"
  caption-ink: "#8f8c85"
  faint-ink: "#66645f"
  signal-green: "#4cc96f"
  safety-yellow: "#e8b931"
  fault-red: "#e5574e"
  record-red: "#ef4b4b"
typography:
  wordmark:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 800
    letterSpacing: "-0.03em"
    fontVariation: "\"wdth\" 112"
  display:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "27px"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "-0.012em"
    fontVariation: "\"wdth\" 88"
  headline:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "25px"
    fontWeight: 700
    letterSpacing: "-0.012em"
    fontVariation: "\"wdth\" 88"
  plate-value:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "21px"
    fontWeight: 500
    lineHeight: 1.2
    fontFeature: "\"tnum\""
    fontVariation: "\"wdth\" 80"
  plate-value-text:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 500
    fontVariation: "\"wdth\" 86"
  title:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
  body:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.45
    fontFeature: "\"tnum\""
  caption:
    fontFamily: "Archivo, system-ui, sans-serif"
    fontSize: "10.5px"
    fontWeight: 600
    letterSpacing: "0.08em"
    fontVariation: "\"wdth\" 72"
rounded:
  hair: "2px"
  plate: "3px"
  rail: "4px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "22px"
  page: "26px"
  rail-width: "92px"
  titlebar-height: "40px"
components:
  plate:
    backgroundColor: "{colors.graphite-ground}"
    rounded: "{rounded.plate}"
  plate-cell:
    textColor: "{colors.label-ink}"
    typography: "{typography.plate-value}"
    padding: "8px 12px 9px"
  plate-cell-live:
    textColor: "{colors.signal-green}"
  button-primary:
    backgroundColor: "{colors.label-ink}"
    textColor: "{colors.graphite-ground}"
    rounded: "{rounded.plate}"
    padding: "0 14px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "#ffffff"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.label-ink}"
    rounded: "{rounded.plate}"
    padding: "0 14px"
    height: "32px"
  button-quiet-hover:
    backgroundColor: "{colors.raised-graphite-2}"
  button-text:
    textColor: "{colors.label-ink-2}"
    rounded: "{rounded.plate}"
    padding: "0 9px"
    height: "30px"
  segmented-on:
    backgroundColor: "{colors.label-ink}"
    textColor: "{colors.graphite-ground}"
    padding: "6px 13px"
  input-field:
    backgroundColor: "{colors.chrome-graphite}"
    textColor: "{colors.label-ink}"
    rounded: "{rounded.plate}"
    padding: "0 9px"
    height: "32px"
  rail-item:
    textColor: "{colors.caption-ink}"
    rounded: "{rounded.rail}"
    width: "72px"
  rail-item-on:
    backgroundColor: "{colors.raised-graphite-2}"
    textColor: "{colors.label-ink}"
  side-panel:
    backgroundColor: "{colors.raised-graphite}"
    width: "372px"
---

# Design System: kit

## Overview

**Creative North Star: "The Rating Plate"**

Every device reads its live state off its own rating plate, the printed spec label under real hardware: ruled cells, tiny condensed captions, firm values. The window is warm graphite with silver-white label ink, hairline rules and 3px corners. Nothing glows. The two white devices stand as product photos on a soft floor light, and everything you can know about them sits beneath them in ruled cells that never move; when the hardware changes a value, the value changes in place.

Density is that of a working tool, not a showroom: a 92px rail of device thumbnails, page tabs in the title bar, a canvas with a plate across its top, and a raised side panel of settings sections divided by hairlines. The whole system runs on one variable typeface, Archivo, with the width axis doing the work: condensed caps for captions, semi-condensed tabular figures for values, normal width for reading text.

Colour is almost entirely neutral. Signal green appears only where something is connected or on right now, so a glance at the window answers "what is actually running" without reading; yellow asks for attention and red marks a fault. The vendor default this replaces (near-black with violet-to-blue gradients, glowing tiles, tracked uppercase labels everywhere) is explicitly rejected.

**Key Characteristics:**
- Warm graphite ground, silver-white label ink, three quieter inks.
- Rating plates: ruled cells, condensed caps caption over a firm tabular value.
- Hairline rules and 3px corners; no glows, no gradients except the floor light under product photos.
- Signal green means one thing: connected or on right now.
- Selection is shown in ink (inverted cell), never in a signal colour.
- One variable face (Archivo), differentiated by width rather than by family.

## Colors

A near-monochrome warm graphite world with one signal colour and three state colours.

### Primary
- **Signal Green** (`signal-green`): the live signal and nothing else. The active profile dot, the preset that is on the headset, the connected status mark, the live DPI stage tag, and the rail pip of a connected device.

### Neutral
- **Graphite Ground** (`graphite-ground`): the page background and the ink-on-ink text colour inside inverted (selected) cells and primary buttons.
- **Chrome Graphite** (`chrome-graphite`): title bar, device rail, and the inset fill of text fields and selects; slightly darker than the ground so the chrome recedes.
- **Raised Graphite** (`raised-graphite`, `raised-graphite-2`): the side panel and macro list; the second step is the hover fill for rows, tabs and buttons and the fill of the active rail item.
- **Hairlines** (`hairline`, `hairline-2`, `hairline-3`): structural dividers, inner plate rules, and outer plate border / quiet button outline / control outlines, in that order of strength.
- **Label Ink** (`label-ink`): primary text and values, the selected fill of segmented controls, band cells and default DPI stage, the primary button face, the EQ curve.
- **Label Ink 2** (`label-ink-2`): secondary text, unselected control text, meter and slider fills.
- **Caption Ink** (`caption-ink`): plate captions, inactive tabs, hints, icons at rest.
- **Faint Ink** (`faint-ink`): axis labels, step indexes, values that are fixed or switched off.

### State
- **Safety Yellow** (`safety-yellow`): attention without failure. Warning state lines, the Sonar defaults notice, a device that is asleep, off or being searched for (status mark and rail pip), an available update, plus the favorite star, the text caret and the in-place flash of a plate value the hardware just changed.
- **Fault Red** (`fault-red`): a device that can't be found (status mark and rail pip), low battery, destructive buttons and confirmations, delete hovers.
- **Record Red** (`record-red`): the recording dot, bar and clock of macros and the mic test only.

### Themes
Graphite above is the default. Settings offers five more that keep the same structure and only swap the tokens (`app/ui/style.css`, `[data-theme]`): **Field** (olive drab, stencil cream), **Phosphor** (oscilloscope green ink), **Blueprint** (white on Prussian blue), and plain **Dark** and **Light**. Each theme tunes its own shades, but live is always a green, warn a yellow and bad a red, so the colours mean the same thing in every theme. Selection, floor light and state text shades are mixed from the tokens, so a new theme only sets the base list.

### Named Rules
**The One Meaning Rule.** Signal green means "connected or on right now" and is never used for selection, emphasis, hover, focus, links or decoration. If a green element would still be green with the device unplugged, it is wrong.

**The Ink Selects Rule.** Selection and the chosen option are shown by inverting a cell to label ink with graphite text (segmented controls, EQ band cells, the default DPI stage, the selected EQ node). Live state and selection can coexist on one element without colliding.

## Typography

**Display Font:** Archivo variable (weight 100-900, width 62-125%), self-hosted, with system-ui fallback
**Body Font:** Archivo at normal width
**Label/Mono Font:** Archivo condensed (`wdth` 72) for captions; Cascadia Mono / Consolas only for file paths and the log box

**Character:** One grotesque stretched and compressed like the printing on an equipment label: condensed caps for what a value is, semi-condensed figures for the value, normal width for sentences. All figures are tabular so changing values never shift their neighbours.

### Hierarchy
- **Wordmark** (800, 18px, `wdth` 112, -0.03em): "kit", lowercase, in the title bar only.
- **Display** (700, 27px, `wdth` 88, line-height 1.1): device names on the home screen.
- **Headline** (700, 25px, `wdth` 88): profile, preset and page names; the profile/preset name is an editable field set in this style.
- **Plate value** (500, 21px, `wdth` 80, tabular): numeric plate values; units in a 12px caption-ink `small`. Text values drop to 16px at `wdth` 86. EQ band values use 17px, DPI stage inputs 19px.
- **Title** (600, 14px): side panel section titles and form row titles; a trailing 13px/400 caption-ink aside may follow ("default 800 DPI").
- **Body** (400, 13.5px, line-height 1.45): everything else. Hints are 12.5px caption ink, max 68ch.
- **Caption** (600, 10.5px, `wdth` 72, 0.08em, uppercase): plate captions, field labels above inputs, rail labels, mouse callout captions.

### Named Rules
**The Width Axis Rule.** Hierarchy comes from width and weight on one family, not from a second typeface. Captions are condensed, values semi-condensed, reading text normal width.

**The Caption Belongs To A Value Rule.** Condensed caps sit directly above or beside the value or control they name (plate cell, field, callout, rail item). They are never used as a free-standing eyebrow over a heading.

## Layout

A fixed-chrome desktop frame: a 40px title bar (wordmark cell the width of the rail, device name, page tabs, window controls) over a 92px device rail and the page. The window is 1240x800 by default, minimum 1000x660, frameless.

- **Home:** two equal columns separated by a hairline, each a stage (product photo on its floor light, absolutely placed so it shrinks with the window), the device name, a four-column rating plate of two rows, then a primary and one quiet action. Column padding 28px 36px 30px.
- **Device pages:** a 44px strip of profile or preset tabs, then a work area of a flexible canvas (18px 26px padding) and a 372px raised side panel. The canvas carries the headline, a four-column plate, then the mouse callout drawing or the EQ graph and its ten-cell band plate.
- **Form pages** (mic, device, settings): a single column, max 780px, rows separated by top hairlines with 15px vertical padding.
- **Macros:** a 250px raised list beside a scrolling editor.
- **Rhythm:** a 4px base; 8 / 12 / 16 / 22 / 26px are the working steps.
- **Narrow windows** (below 1150px): the side panel drops to 320px and padding tightens; nothing reflows into a different layout.

### Named Rules
**The No Reflow Rule.** Plate cells sit on a fixed grid and long values truncate, so a hardware change rewrites a value without moving anything around it.

## Elevation & Depth

Flat and tonal. Depth comes from three graphite steps (chrome darker than ground, panels raised lighter) and hairline borders, not from shadows. The only shadows are physical: a soft drop shadow grounding each product photo, a small shadow under slider and DPI knobs, and one diffuse shadow under the toast so it reads above the page.

### Shadow Vocabulary
- **Photo ground** (`filter: drop-shadow(0 28px 24px rgba(0,0,0,.55))`): product photos on the home stage; rail thumbnails use `0 4px 5px rgba(0,0,0,.5)`.
- **Knob** (`box-shadow: 0 1px 3px rgba(0,0,0,.6)`): slider thumb and the selected DPI knob.
- **Toast** (`box-shadow: 0 10px 30px rgba(0,0,0,.55)`): the transient status toast.

### Named Rules
**The Nothing Glows Rule.** No coloured shadows, no outer glows, no gradient fills. The single permitted gradient is the faint radial floor light (label ink at 7% alpha) under each home product photo.

## Shapes

Small, firm corners: 3px on plates, buttons, fields, segmented controls and the toast; 2px on knobs, switch thumbs and small icon buttons; 4px on rail items. Circles are reserved for status: the live dot, the off ring, the rail pip, radio marks, the record dot and EQ nodes. Borders are 1px hairlines; capture targets for shortcuts use a 1px dashed hairline that turns solid while listening.

The mouse drawing's callout leaders run at 0 and 90 degrees only, ending in a small ringed dot on the button they name.

## Components

### Rating Plate (signature)
The core component. A grid of ruled cells (`--cols`, usually 4; 10 for EQ bands) inside a 1px hairline-3 border with 3px corners. Inner rules are hairline-2, drawn once (right and bottom edges are dropped on the last column and row). Each cell is a condensed caps caption over a firm tabular value, optionally with a 3px meter bar (label-ink-2 fill, fault red when low). Values can be dim (faint ink), warn, bad or live (green). Status values use a 7px filled dot in green when connected, or a 7px ring in yellow while waiting and red on a fault.
- **Signature motion:** when a backend event changes a cell's value, the cell flashes its new value in safety yellow for 900ms in place, then eases back over 0.5s. Local edits do not flash.
- **Selectable plates** (EQ bands, DPI stages): hover fills raised-graphite-2; the selected cell inverts to label ink with graphite text. A live DPI stage appends " · live" in green to its caption.

### Keyboard Drawing
Keyboards are drawn from their real layouts as ruled key cells: raised graphite, 1px hairline-2 inset outline, 3px corners, a semi-condensed 12px legend and an optional condensed caps sub-label (layer function, macro name). Keys bound to a kit macro take an ink-3 outline. The selected key inverts to label ink, like every other selection. On the Lighting page each key shows its LED color as a 24% tint plus a 3px bar of the full color along its lower edge; the color is the content, so it is not a glow. Animated effects are previewed with a slow hue or opacity cycle that stops under reduced motion.

### Setup
Three steps, shown as a three-cell plate whose current cell inverts to ink. Step one is three product cards in ruled columns (photo on a floor light, name, an on/off switch, a "Plugged in" live mark when detected, model and color segmented controls); an unpicked card dims its photo.

### Buttons
- **Shape:** gently squared (3px), 32px tall, 14px horizontal padding, 13px/600 label, optional 14px stroke icon.
- **Primary:** label-ink face with graphite text; hover brightens to white. One per action group ("Open mouse").
- **Quiet:** transparent with a 1px hairline-3 inset outline; hover fills raised-graphite-2.
- **Danger:** fault red with white text, for confirmed destructive actions.
- **Text button:** 30px, label-ink-2 text, no outline; hover raises ink and fills raised-graphite-2. Used for Duplicate, Delete, New profile.

### Segmented Control
A row of plate cells: 1px hairline-3 outer border, hairline-2 dividers, 3px corners. Unselected cells label-ink-2; the chosen cell inverts to label ink. Used for report rate and side-button selection.

### Radio List
Two-column list of options, each with a 13px ring mark (hairline-3). The chosen option fills to a ringed ink dot and its text goes to label ink.

### Inputs / Fields
- **Style:** 32px, chrome-graphite inset fill, 1px hairline-2 border, 3px corners, 13.5px text.
- **Hover / Focus:** border steps to hairline-3 on hover and label-ink-2 on focus. No glow ring.
- **Disabled:** 40% opacity.
- **Select:** the same field with a caption-ink chevron.
- **Switch:** a squared track (3px) with a 2px-cornered thumb; on fills the track with label ink and the thumb with graphite.
- **Keycap** (`kbd`): 22px raised-graphite-2 chip with a hairline outline and a 2px bottom lip.

### Navigation
- **Rail:** 72px items on chrome graphite, a 20px icon or a 56x44 device thumbnail over a condensed caps label, battery figure beneath. Hover fills raised graphite; the current item fills raised-graphite-2 with a hairline-2 inset outline. A 6px pip in the corner: green when connected, yellow while asleep, off or searching, red when not found; yellow on Settings when an update is ready.
- **Title bar tabs and strip tabs:** caption-ink text, label ink on hover and when selected; the selected tab gets a 2px label-ink underline sitting on the bottom hairline. Strip tabs carry a 6px status dot: green when live on the device, caption ink when stored, a faint ring otherwise.

### Mouse Callout Drawing
The top-down mouse photo inside an SVG with captioned callouts: condensed caps caption in caption ink, value in label-ink-2, hairline-3 leader to a ringed dot. Rebindable side buttons are clickable; the selected one turns caption, value, leader and button to label ink.

### EQ Graph
Grid lines barely above the ground, a hairline-3 zero line, the summed curve in 2.25px label ink over a 6% ink fill, the selected band's own curve dashed in caption ink. Nodes are numbered rings that invert to ink when selected. The ten-cell band plate below mirrors the nodes.

### Mic Strip
A 58px chrome-graphite well in a hairline-2 border, with a hairline midline and 2px vertical bars growing from it. Each bar is the loudest moment of a fixed slice of time, on a dB scale so the noise floor shows, and bars never move once drawn, so a short sample simply stops partway across. Bars are signal green while sound is coming off the mic, label-ink-2 once they are a stored sample, and during playback they turn label ink as the sample plays, ahead of it faint ink.

## Do's and Don'ts

### Do:
- **Do** put live device state on a rating plate: condensed caps caption over a firm tabular value, in ruled cells that never reflow.
- **Do** reserve signal green for state that is connected or on right now, and flash a plate value yellow in place when the hardware changes it.
- **Do** show selection by inverting to label ink with graphite text.
- **Do** keep corners at 3px (2px for knobs and small icon buttons, 4px for rail items) and dividers at 1px hairlines.
- **Do** differentiate type by Archivo's width axis and weight, with tabular figures everywhere values change.
- **Do** keep depth tonal: chrome darker than the ground, panels one step lighter.

### Don't:
- **Don't** use signal green for selection, hover, focus, links, headings or decoration.
- **Don't** add glows, coloured shadows, or gradient fills; the floor light under product photos is the only gradient.
- **Don't** bring back the old violet-to-blue gradient, near-black glowing tiles, or tracked uppercase labels used as decoration.
- **Don't** use condensed caps as a standalone eyebrow over a heading; a caption always names a value or control.
- **Don't** introduce a second typeface for display or labels.
- **Don't** run mouse callout leaders at any angle other than 0 or 90 degrees.
