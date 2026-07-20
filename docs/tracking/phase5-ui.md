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
  score components read the retained scoreboard (reader sentinel included),
  selectors degrade to empty counted text (the vanilla server evaluates them
  before sending), and translation keys resolve through a new provenance-
  pinned localization carrier compiled from the pack's `texts/en_US.lang`
  (13,072 entries; `make lang-assets`, required fail-closed at startup like
  the HUD carrier). Formatting handles the %s/%n families; a key outside the
  catalog presents verbatim like the vanilla client. Item labels prefer the
  localized `item./tile.<path>.name` entries with a mechanical title-case
  fallback. No JSON is ever presented.
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
- [ ] Mount jump bar activation (riding input does not exist yet; the
  presentation path and carried strips are gated on a real jump-charge
  authority) and the attack indicator (Bedrock exposes no attack-cooldown
  state and the official sample pack carries no indicator art; deliberately
  not invented).
- [ ] Below-name and list scoreboard world/tab anchoring (nameplate and
  player-list surfaces do not exist yet; the store projections and per-owner
  lookups are ready).
- [ ] Native Bedrock and third-party live server acceptance, including live
  chat send/receive and disconnect validation from the stable executable
  paths.
- [ ] Release-profile frame-time and memory measurement against the plan.md
  budgets (the deterministic harness bounds structure, not wall clock).
- [ ] A green replacement CI run covering the architecture and Windows
  acceptance fixes.
- [ ] Independent review of this branch's complete base..head range.

## Historical references

The dated Phase 5 HUD/rawtext/scoreboard backup refs preserve prior experiments but are stale. Consult selectively; do not bulk merge them.
