---
name: Cinnabar Launcher
description: The player at the edge of a cinnabar island cluster, with one lit Play block and a dock of what's new.
colors:
  cinnabar: "#e2483a"
  cinnabar-hi: "#f27a63"
  cinnabar-fill: "#d23b2e"
  cinnabar-ink: "#fff7f3"
  ink: "#f5f0ea"
  ink-2: "#c9c2bb"
  ink-3: "#aba59f"
  solid: "#11131d"
  glass: "rgb(14 16 28 / 0.66)"
  glass-strong: "rgb(12 14 24 / 0.8)"
  glass-edge: "rgb(255 255 255 / 0.07)"
  raise: "rgb(255 255 255 / 0.06)"
  raise-2: "rgb(255 255 255 / 0.1)"
  line: "rgb(255 255 255 / 0.08)"
  ok: "#6fd39f"
  warn: "#f2b550"
  bad: "#ff7a8a"
  focus: "#ffd0c4"
typography:
  hero:
    fontFamily: "Inter Variable"
    fontSize: "26px"
    fontWeight: 650
    lineHeight: 1.1
    letterSpacing: "-0.028em"
  page:
    fontFamily: "Inter Variable"
    fontSize: "30px"
    fontWeight: 700
    lineHeight: 1.05
    letterSpacing: "-0.028em"
  play:
    fontFamily: "Inter Variable"
    fontSize: "20px"
    fontWeight: 680
    lineHeight: 1.05
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Inter Variable"
    fontSize: "16px"
    fontWeight: 620
    lineHeight: 1.25
    letterSpacing: "-0.012em"
  body:
    fontFamily: "Inter Variable"
    fontSize: "14px"
    fontWeight: 430
    lineHeight: 1.5
    fontFeature: "'tnum' 1"
  label:
    fontFamily: "Inter Variable"
    fontSize: "13px"
    fontWeight: 560
    lineHeight: 1.3
  meta:
    fontFamily: "Inter Variable"
    fontSize: "12px"
    fontWeight: 480
    lineHeight: 1.35
rounded:
  sm: "8px"
  md: "12px"
  lg: "18px"
  pill: "999px"
spacing:
  s2: "8px"
  s3: "12px"
  s4: "16px"
  s5: "24px"
  s6: "32px"
components:
  play-ready:
    backgroundColor: "{colors.cinnabar-fill}"
    textColor: "{colors.cinnabar-ink}"
    typography: "{typography.play}"
    rounded: "14px"
    height: "62px"
    width: "300px"
  play-busy:
    backgroundColor: "{colors.glass-strong}"
    textColor: "{colors.cinnabar-ink}"
    rounded: "14px"
    height: "62px"
  join:
    backgroundColor: "{colors.raise-2}"
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    height: "34px"
  join-hover:
    backgroundColor: "{colors.cinnabar-fill}"
    textColor: "{colors.cinnabar-ink}"
  dock-card:
    backgroundColor: "{colors.glass}"
    rounded: "{rounded.lg}"
    height: "196px"
  settings-group:
    backgroundColor: "{colors.glass-strong}"
    rounded: "{rounded.lg}"
  segmented-thumb:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.solid}"
    rounded: "{rounded.pill}"
---

# Design System: Cinnabar Launcher

## Overview

The window is a world, not a page. A real-time voxel outcrop of slate and dolomite, split by a glowing cinnabar vein, floats among satellite islands and cloud banks in a sky lit for the player's local time of day. The player stands on a stone in the foreground, anchored to a fixed screen position at every window size, and turns their head toward the pointer. The interface floats over it: the player's identity, one lit Play block with a rejoin beside it, and a dock of glass cards. Everything except the scene is quiet, exact and fast. Spectacle is spent once.

The vein is the signature move. It is the product state made visible: it refines from dark ore to glowing cinnabar as an update downloads, goes dark when the installed build cannot start, and re-ignites on the first launch of a new version, the "what's new" moment.

Anti-references: ad rails and promo carousels (Lunar, Dawn), dense rails of unbuilt features, grey dashboards with one accent, flat isometric clip art.

## Colors

- **Cinnabar** (`#e2483a`, fill `#d23b2e`, highlight `#f27a63`) comes from the brand mark's faces. It marks the one primary action, the vein, and the current location. It never decorates.
- **Ink** is warm dolomite white in three steps. `ink-3` is the floor for text on glass and holds 4.6:1 over the brightest part of the dusk sky.
- **Glass** is smoked and tinted toward the night sky, never neutral grey. It appears only where content sits over the world: the shelf, notices, the account chip and settings groups.
- State colours are reserved for state: `ok` for live and good ping, `warn` for recoverable trouble and stale data, `bad` for a build that cannot start. Each always pairs with an icon or words.
- Scene palettes (sky, light and ore glow per time of day) live in `src/lib/diorama/mood.ts`, not in CSS.

## Typography

One family, Inter (OFL), at normal width throughout. Its optical-size axis gives large text display cuts, and tracking tightens as size grows. Display roles sit at weight 650 to 700 rather than heavy black weights, and nothing is expanded or stretched. Numerals are tabular. Everything is sentence case: no all-caps labels, no eyebrows.

## Layout

- The title bar is 56px and transparent: wordmark, three text tabs with a sliding cinnabar bar, and the account chip at the right. On macOS the left 88px belong to the traffic lights. Windows and Linux get drawn window controls.
- Home has two rows. The hero (skin head and name, or the brand mark for signed-out players; channel and version; Play; Rejoin; one status line) sits bottom-left. A 196px dock runs along the foot: what's new in the installed build, up to two news cards, then recent servers, falling back to featured servers, falling back to a compact first-run hint. Nothing renders as an empty box.
- The camera frames the island right of centre. The player's feet are anchored just right of the launch column, on the dock's top edge.
- Secondary pages veil the world: it blurs, dims and stops rendering. Settings caps at 720px, Servers at 1240px.
- The window's default is 1180×720 and its minimum 900×560. At the minimum, the dock shows one news card and the scene keeps its anchors.

## Elevation & Depth

Depth comes from the scene: an atmospheric sky, fog, soft shadows, baked voxel ambient occlusion, a cool rim light, bloom on the ore, drifting rock chunks that parallax with the camera, and a left scrim that shapes light behind the text. Interface surfaces use glass with a 1px inner edge and no outer shadow. The only cast shadow belongs to the Play block, a soft cinnabar glow offset below it.

## Shapes

Radii step by hierarchy: 8px for small controls, 12px for rows and tiles, 16px for Play, 18px for cards and groups. Pills are reserved for chips, the join button, search, filters and segmented controls.

## Components

- **Hero labels**: Play, Update & play, Downloading, Verifying, Installing (or Opening installer on Windows), Starting, In game. A crash after the last session is one status line with View log, never a label change on Play.
- **Status line**: anything that doesn't block play (check results, a failed check, a failed download that left the build intact, a crash last time) is one muted line with at most one action. Notices are reserved for a build that can't start and for a deliberate downgrade.
- **Play** is the state machine. Ready is a lit block face: a top-down gradient, an inner top highlight and a hover lift with one band of light. Busy is glass with a cinnabar fill and a light sweep confined to the filled part. Running shows a live dot. Labels cross-fade and rise between states.
- **Server cards** show typographic poster art (a hue field, a fixed-scale block lattice and the initial) whenever a server ships no image. Hover or focus reveals the MOTD, tags and Play.
- **Segmented control**: a dark track with an ink thumb that slides, operated with arrow keys.
- **Notice**: glass with a tone-coloured inner edge and icon. Its action is the recovery step. The `info` tone carries deliberate choices, such as a downgrade.
- **Dev builds** show a small "Dev build" tag beside the version, and Settings explains why updates are off.
- **Profile**: the account chip opens a popover with the player and a friends slot that states plainly that friends arrive with Cinnabar accounts.
- **News cards** use the post's image or, without one, a typographic poster: the release version, or the post's tag.
- **Update outcomes**: a downgrade never takes over Play. It is a quiet notice and a Settings row that open the platform installer. Installing names its outcome: on Windows, "Opening installer" (the launcher closes); elsewhere, "Installing" (the launcher restarts into the game).
- **First run**: until the game has signed in once, the server list explains why it is empty and offers Play.

## Do's and Don'ts

- Do spend colour on Play, the vein and the current location only.
- Do keep the world rendering only on Home and while the game is not running. Reduced motion and slow GPUs get a still frame. Without WebGL, the engine's own pre-rendered still (`src/assets/world-still.jpg`) stands in.
- Don't add destinations, chips or badges for features that don't exist.
- Don't use middle-dot meta strings, all-caps labels or eyebrows.
- Don't use game assets, names or trademarks. Every texture, skin and poster is generated in `src/lib/diorama/` and `src/lib/poster.ts`.
