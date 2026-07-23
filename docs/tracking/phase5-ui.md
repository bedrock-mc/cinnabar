# Phase 5 HUD, chat, scoreboard, and boss-bar tracker

Current audited progress on merged `main`: **53%** at commit `fe698f5`.

This estimate uses equal contract, runtime, production presentation, deterministic-verification/review, and live/native/performance gates. Unmerged PR work is referenced but not counted as landed.

Gate scores: contract 80%, runtime 75%, production presentation 45%, deterministic verification/review 65%, live/native/performance 0%; arithmetic mean 53%. No binding target-platform/native/live/performance witness has passed.

## Landed on merged `main`

- [x] Bounded text, title/actionbar, HUD attribute, objective/score, and boss-event normalization.
- [x] Retained chat, HUD, scoreboard, and boss-bar stores with lifecycle/reset limits.
- [x] Interactive chat editing, history, clipboard bounds, autocomplete, rate limiting, and outbound command/chat packet routing.
- [x] Provenance-pinned survival HUD carrier and partial health/hunger/air/hotbar presentation.
- [x] Scoreboard/boss state adapters and substantial deterministic UI/runtime tests.

## Current branch implementation (locally committed, not merged)

Implementation and deterministic verification for the tranche below are
complete on this branch; the native/live/performance gates in the next
section remain open, so no phase checkbox is closed by this delta.

- Protocol: MobEffect, MobArmorEquipment, SetPlayerGameType, and SetActorLink
  are dispatched into bounded vendor-neutral events (previously decoded but
  dropped); StartGame's unique local-player id rides `WorldBootstrap`; local
  SetEntityData metadata (air supply, max air, freezing strength) splits into
  committed UI events beside attributes; item user-data NBT exposes the
  vanilla `Damage` tag through a bounded fixed-LE walk.
- App state: a gameplay-HUD store retains hotbar/offhand mirrors (with a
  per-frame drain that also fixes the previously consumer-less inventory
  queue), authoritative armor stacks, bounded effects with soonest-expiry
  eviction, air/freezing, the local mount, damage-blink and selected-item
  identity clocks; armor points derive from resolved equipment identities via
  a pinned vanilla table; runtime game-mode changes apply mid-session.
- Carrier v4: 82 pinned official-sample textures (crosshair and the classic
  182x5 experience/mount-jump strips are cropped from `textures/gui/icons.png`
  under a pinned crop schema; damage/poison/wither/freeze/absorption hearts,
  hunger-effect recolors, mount hearts, bubble pop, effect backgrounds, and
  26 effect icons whose Bedrock ids are verifiable from cross-checked
  secondary sources). Newer effect ids without a verifiable pin are skipped
  and counted, never guessed.
- Presentation: the gameplay HUD lays out in Java GUI pixels under the Java
  auto-scale rule (fixed 1..4 preferences clamped, safe-area inset, fail
  closed below the 182px hotbar); centered 15x15 invert-blend crosshair
  (first-person only, hidden in spectator, still shown while chat is
  focused) through a second per-batch blend pipeline; stacked heart rows for
  nonstandard maxima with damage blink, poison/wither/frozen variants, and
  absorption overflow; conditional armor row; hunger with effect recolor;
  popping air bubbles; mount hearts replacing hunger while riding; the
  classic 182x5 XP bar with outlined level; top-right effect chips with
  ambient backgrounds and expiry blink; stacked tinted boss bars with titles
  and title/actionbar coexistence; hotbar counts, durability bars, offhand
  cell, and the fading selected-item label; Java chat fade (10 s + 1 s) with
  per-line contiguous backdrops.
- Rawtext and localization: typed documents now resolve instead of dropping —
  score components read the retained scoreboard (real player/entity owners
  through the stream's authoritative id-to-name map, plus the `*` reader
  sentinel), selectors resolve from retained authority (`@s`, the player
  list for `@a`) and otherwise degrade to empty counted text, and
  translation keys resolve through a provenance-pinned localization carrier
  compiled from the pack's `texts/en_US.lang` (13,072 entries;
  `make lang-assets`, required fail-closed at startup like the HUD carrier).
  Formatting handles the %s/%d, positional %N/`%1$s`, and fixed-precision
  %.Nf families; a key outside the catalog presents verbatim like the
  vanilla client. Item labels prefer the localized `item./tile.<path>.name`
  entries with a mechanical title-case fallback. No JSON is ever presented.
- Scoreboard: per-owner below-name and list lookups with lifecycle coverage
  beside the existing sidebar projection/presentation.
- Evidence harnesses: a saturated-surface witness pins retained-memory
  budgets (chat, scoreboard, boss, effects at their caps) and draw-list
  structural bounds with a settled steady-state text-layout cache.
- CI: the Windows acceptance failure (an un-stubbed pack sentinel firing the
  real vanilla fetch inside the atmosphere Makefile test, which then died on
  `Get-FileHash` resolution) is fixed hermetically with leak traps, both
  fetch scripts hash via .NET, and the three oversized modules were split
  under the architecture limits. A replacement green CI run has not yet
  executed; these fixes are unverified on the actual runners.
- Review-fix round (independent review of `4f57690..48774d8`): semantically
  odd but well-formed HUD values (inverted/non-finite attribute ranges,
  negative SetHealth, negative title durations, oversized chat rows, unknown
  game modes and effect ids) are skipped and counted through the gameplay
  diagnostics instead of ending the session; game modes never fabricate or
  discard stats, the world-default mode is retained and `SetDefaultGameType`
  dispatched; the crosshair centers exactly (no floor) with exact-equality
  witnesses; bound platform safe-area insets flow through geometry, retained
  layout, and render clipping with height validated fail-closed; finite
  effects expire on an estimated 20 tps session clock between packets; the
  copper tool/armor tier joined the item facts; the localization carrier
  (v2) pins the exact `texts/en_US.lang` byte identity end to end with a
  tamper witness; carrier recovery commands name exact custom paths for
  every failure case; rawtext resolves real player/entity score owners and
  bounded selectors (`@s`, `@a`) with per-child `with`-document arguments
  and `%.2f` precision; the mount jump bar, structural attack indicator,
  held-Tab player-list overlay, notched boss dividers, and the MobEquipment
  selected-stack echo landed with deterministic witnesses.

## Remaining gates and deferred work

- [ ] Real rendered-frame visual acceptance on the target platform for every
  surface above, including the crosshair against the pinned Java 26.2
  reference matrix (GUI scales 2/3/4/Auto at 1280x720, 1920x1080, 2560x1440,
  100%/150% desktop scaling, macOS Retina). No such pass has been performed
  on this branch; unit geometry/draw-list tests are not a substitute.
- [ ] Hotbar/offhand item icon pixels: no runtime artifact carries item
  sprite pixels yet (the entity carrier stores routes and hashes only), so
  icons are not drawn. Requires an item-sprite payload in the entity carrier
  or a dedicated reviewed carrier before the icons can render.
- [ ] Non-English locales (only the pinned `en_US` table is compiled; locale
  selection is future work).
- [ ] Below-name world anchoring: the tab player-list overlay renders (held
  PlayerList action), but the actor-nameplate surface does not exist, so
  below-name scores have projections and per-owner lookups without a
  world-anchored presentation. Requires world-to-screen projection plumbing
  from the camera authority.
- [ ] Native Bedrock and third-party live server acceptance, including live
  chat send/receive and disconnect validation from the stable executable
  paths.
- [ ] Release-profile frame-time and memory measurement against the plan.md
  budgets (the deterministic harness bounds structure, not wall clock).
- [ ] A green replacement CI run covering the architecture and Windows
  acceptance fixes.
- [ ] Independent re-review of the branch after this fix round (the full base..new-head range).

## Historical references

The dated Phase 5 HUD/rawtext/scoreboard backup refs preserve prior experiments but are stale. Consult selectively; do not bulk merge them.
