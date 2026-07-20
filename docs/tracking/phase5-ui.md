# Phase 5 HUD, chat, scoreboard, and boss-bar tracker

Current audited progress on merged `main`: **53%** at commit `fe698f5`.

This estimate uses equal contract, runtime, production presentation, deterministic-verification/review, and live/native/performance gates. Unmerged PR work is referenced but not counted as landed.

Gate scores: contract 80%, runtime 75%, production presentation 45%, deterministic verification/review 65%, live/native/performance 0%; arithmetic mean 53%. No binding target-platform/native/live/performance witness has passed.

## Landed

- [x] Bounded text, title/actionbar, HUD attribute, objective/score, and boss-event normalization.
- [x] Retained chat, HUD, scoreboard, and boss-bar stores with lifecycle/reset limits.
- [x] Interactive chat editing, history, clipboard bounds, autocomplete, rate limiting, and outbound command/chat packet routing.
- [x] Provenance-pinned survival HUD carrier and partial health/hunger/air/hotbar presentation.
- [x] Scoreboard/boss state adapters and substantial deterministic UI/runtime tests.

## Remaining features

- [ ] Render hotbar item icons, counts, durability, and authoritative selected-stack state.
- [ ] Finish the approved Java Edition-style gameplay HUD presentation for hotbar, scoreboard, chat, hearts, hunger, armor, air, experience, and level while preserving Bedrock protocol/server authority.
- [ ] Show the armor row above the hearts only while the authoritative equipped armor total is nonzero; hide the row entirely when no armor is equipped.
- [ ] Finish GUI scale selection, nonstandard maxima, authoritative armor derivation, clipping, and safe areas across supported resolutions and DPI.
- [ ] Pass the pinned Java-HUD state matrix: survival, creative, and spectator; normal, damaged, absorption, poisoned, withered, and frozen hearts; normal/depleted hunger; air; XP/level; armor present/absent; mount health/jump; main/offhand; attack indicator; selected-item label; effects; and scoreboard/chat overlap.
- [ ] Render complete sidebar/list/below-name scoreboards with correct ordering and lifecycle.
- [ ] Render boss-bar style, color, health, stacking, replacement, and coexistence with titles/actionbar.
- [ ] Resolve rawtext translation, score, selector, and localization documents without exposing JSON.
- [ ] Complete chat formatting, fade/focus behavior, live send/receive, and disconnect validation.
- [ ] Run matching Windows/macOS scale, DPI, aspect-ratio, native Bedrock, and third-party server acceptance.
- [ ] Prove bounded retained memory and stable frame time with all UI surfaces active.

## Current branch implementation

This draft PR also contains unmerged HUD/chat/scoreboard/hotbar/XP changes:

- Java-style chat backdrop changes within the approved chat-layout exception.
- Hotbar number-key, wheel, and controller selection plus outbound `MobEquipment` routing.
- Experience attribute retention and XP bar/level presentation.
- Scoreboard/background presentation changes using the approved Java Edition-style gameplay HUD direction.

The approved presentation deviation now covers the complete in-game gameplay HUD: hotbar, scoreboard, chat, hearts, hunger, armor, air, experience, level, and the applicable mount/offhand/effect/attack-indicator surfaces. Bedrock remains authoritative for packets, attributes, equipment, inventory, game mode, combat timing, and reconciliation. A Java visual has no authority to invent a state that Bedrock does not expose. Menus, inventories, containers, forms, and JSON UI behavior remain Bedrock/resource-pack-driven unless separately approved.

The clean-room presentation reference is **Minecraft Java Edition 26.2**, default resources, running on Windows 11. Capture GUI scales 2, 3, 4, and Auto at 1280x720, 1920x1080, and 2560x1440, including 100% and 150% desktop scaling where applicable; validate equivalent logical layout and safe areas on supported macOS Retina output. Each acceptance capture must identify version, game mode, HUD state, resolution, desktop scale, GUI scale, and relevant server-authoritative values. If a later Java version becomes the target, update this pin and recapture the affected matrix rather than silently mixing references.

Java Edition is a clean-room visual/behavior reference only. Decompiled proprietary Java source must not be copied, translated, vendored, or used as implementation code. Record observable geometry, visibility, ordering, timing, and state tables from a legally obtained running client or other approved references, then implement independently in Cinnabar.

No completed real rendered-frame visual acceptance pass or independent review is recorded. At implementation head `228ae70`, CI run `29713142467` failed Linux architecture enforcement because `app/src/asset_startup.rs` was 1,030 lines versus the 1,000-line production limit (unchanged from `main`), `app/src/ui_runtime.rs` was 1,006 versus 1,000, and `app/src/ui_runtime/tests.rs` was 1,204 versus the 1,200-line test limit. Windows acceptance failed `make_atmosphere_target_serializes_one_producer_for_missing_and_stale_pairs` because `fetch-vanilla-assets.ps1` could not resolve `Get-FileHash`. These failures must be resolved or proven unrelated on a replacement green run before any branch delta counts as landed.

## Historical references

The dated Phase 5 HUD/rawtext/scoreboard backup refs preserve prior experiments but are stale. Consult selectively; do not bulk merge them.
