# Cinnabar roadmap

Implementation status and acceptance requirements live here. Change history is
in [generated release notes and merged PRs](CHANGELOG.md). The
[completion ledger](docs/evidence/phases-2-5-completion-ledger.md) records acceptance
evidence; the [workspace map](docs/architecture/workspace.md) explains code ownership.

## Maintaining this plan

- Update the existing section when scope, remaining work or acceptance changes.
  Say what exists before describing a confirmed behavior gap or missing verification.
  Keep entries short; detailed behavior belongs with its code or existing reference docs.
- Keep provisional behavior and missing evidence explicitly incomplete. A merged PR,
  passing unit test or refactor does not by itself close a parity or performance gate.
- Keep each phase requirement ID consistent with the completion ledger. The architecture
  check requires each ID exactly once in its owning phase below.
- Write a brief imperative PR title and apply one category label for release history.
  Do not append a PR diary here or maintain a second handwritten changelog.
- Keep internal research and agent reports in the private maintainer checkout described
  in [AGENTS.md](AGENTS.md). Earlier plan revisions remain in Git history.

## Current priorities

1. Identify and resolve confirmed terrain, actor, movement and UI behavior gaps with version-matched evidence.
2. Qualify frame pacing, streaming, joining and retained memory on the reference hardware.
   Windows VSync queue depth uses coarse work estimates to try one queued frame and
   observed acquisition cadence to restore two. Failed probes back off exponentially;
   missing GPU samples leave the cadence policy running.
   Displayed cadence, input latency and surface-reconfiguration
   stalls still need hardware qualification; this closes no acceptance gate.
3. Verify complete session and input paths across supported platforms.
4. Simplify existing subsystem ownership without adding unnecessary crates or adapter layers.

**The unchecked boxes below describe full acceptance requirements, not absent features.**
Terrain rendering, movement, actors, chat, HUD, inventories and forms already have
implementations. These criteria come from the earlier plan and have not had a complete
status audit against current code. Check the implementation and later evidence before
treating an inherited entry as new work. Missing acceptance evidence means a feature
is not fully qualified; it does not mean the feature is unimplemented. This cleanup
closes no gate and provides no new runtime or performance evidence.

## Phase 0 — End-to-end acceptance

- [ ] Complete a sustained streamed-world acceptance run: join, move at speed, and
  break/place blocks to exercise live remeshing. Resolve outstanding command/protocol
  conformance findings, classify every decode error, and qualify frame and mutation
  latency against the current [live-testing requirements](docs/agents/live-testing.md).
  Use the current headless client workflow for scripted tests; keep captures local.
  Dimension definitions use an exclusive maximum Y and retain complete sub-chunks;
  live custom-dimension transfer acceptance remains open.

## Phase 1 — Core service

The Go core and local stream boundary are implemented. Keep authentication,
encryption and upstream transports with the core, and client behavior with Rust.
Changes still need current protocol conformance, bounded framing and lifecycle coverage.
The pinned target comes from [the target manifest](assets/bedrock-target.json).

## Phase 2 — World rendering

Implemented: [terrain meshing](crates/meshing/src/lib.rs),
[packed biome storage](crates/world/src/biome.rs),
[biome tint preparation](crates/meshing/src/biome.rs), and
[cloud](crates/render/src/cloud_render.rs) and [weather](crates/render/src/weather_render.rs)
rendering. The criteria below concern complete target coverage, native comparisons
and resource qualification; they do not imply these systems need to be written.

### Terrain, materials and biome acceptance

- [ ] Close the opaque texture slice with matched native views, full-view remesh
  qualification, and fresh combined memory and steady-CPU measurements.
- [ ] Close cutout-cube and leaf visual acceptance. Preserve independent geometry,
  occlusion and cave-connectivity semantics, packed quads and coverage-preserving mips.
- [ ] `P2.5-NATIVE-BIOME` — Qualify existing biome palettes and grass/foliage/water tinting
  against native output while preserving packed storage and the shared quad contract.
- [ ] Confirm or replace provisional biome blending using version-matched radius and
  kernel evidence. Cover abrupt boundaries, special foliage, custom-biome fallback,
  missing neighbours, replacement, teleport/eviction, GPU output and performance.

### Block-family and gallery acceptance

- [ ] Qualify static/non-cube models, blend/water and flipbooks, including native
  presentation and animation. Identify unsupported states before adding implementation.
- [ ] Qualify compact templates for slabs/stairs, thin attached faces, paired doors,
  trapdoors, connected panes/fences/gates and static chest/sign models. Check existing
  coverage before implementing a missing case. Keep conservative culling and connectivity
  until exact face-coverage behavior is verified separately.
- [ ] Close paired-door presentation and interaction acceptance, including lower/upper
  state resolution, cross-subchunk neighbours, missing pairs, highlights, cracks,
  directional replacement-pack artwork and native-server toggle/picking input.
- [ ] Close barrel/trapdoor visual acceptance, including facing/open states, grain,
  carried items, edge UVs, replacement materials and different texture resolutions.
- [ ] Close chiseled-bookshelf native-gallery acceptance with two consecutive exact
  GPU-completed model-stream witnesses.
- [ ] Close resin-clump native-gallery acceptance with two consecutive exact
  GPU-completed model-stream witnesses.
- [ ] Close selector-alias opaque-cube acceptance for axes, TNT and deprecated states,
  with matched views and two consecutive exact GPU-completed cube-stream witnesses.
- [ ] Close cactus overview, stacked, grazing and inset-top presentation acceptance,
  with stable exact model-stream witnesses and zero contamination counters.
- [ ] Close cake presentation at representative bite states, with stable exact
  model-stream witnesses and zero contamination counters.
- [ ] Close candle-cake flame-particle and native-world presentation acceptance;
  isolated composite geometry and texture fixtures do not establish the full result.
- [ ] Close farmland presentation at representative moisture states, with matched
  native views, stable exact model-stream witnesses and zero contamination counters.
- [ ] Close crop comparison, sub-unit sprite geometry, atlas-edge UV calibration and
  torchflower positional variation. Earlier bounded artwork fixes do not close this scope.
- [ ] Verify nether-wart geometry and selector acceptance independently of other crops.
- [ ] Close all fixed slab/stair gallery poses with exact GPU witnesses, matched native
  views and a clean mutation-to-visible/performance run. Isolate backend/presentation
  faults from geometry; do not mask them with speculative renderer changes.
- [ ] Close vine attachment-mask, neighbour, connectivity and texture/UV acceptance.
  Adjudicate glow lichen and sculk vein separately rather than assuming identical rules.
- [ ] Close the exhaustive residual-family inventory for the pinned target. Every
  non-air state needs an authoritative accepted visual; provisional fallback does not count.
  Require zero diagnostic/provisional-fallback counters and current resource-cost evidence.
- [ ] Close coral-fan native placement and rendered acceptance for floor/wall orientations
  and replacement-pack artwork.
- [ ] Close tripwire-hook native placement and rendered acceptance across facing,
  attachment and signal states.
- [ ] Close shelf native-gallery, pack-pixel fidelity and live replacement-pack acceptance.
- [ ] Close the static-sign gallery and native visual acceptance.
- [ ] Establish exact standing/hanging sign board, post and support geometry, UVs and
  GPU/native evidence. Provisional dimensions are not final geometry parity.
- [ ] Complete every exact-state GPU gallery page for the pinned inventory, with exact
  palette readback, two consecutive completed frames and matched native views. Derive
  page/state totals from the current inventory; support fixtures do not count as targets.
- [ ] Verify the separately versioned block-entity inventory and renderer manifest,
  including chunk NBT, live updates, required variants and explicit GPU/no-draw evidence.
- [ ] Close every registered block-entity renderer route, gallery builder, required
  NBT variant and strict-final witness without deferrals.
- [ ] Qualify the provisional block-entity carrier/render pass across its supported
  families, updates, animation and item contents; implementation alone closes no gate.
- [ ] Close protocol/content integration findings and required upstream fixes through
  the current PR, review and acceptance workflow. Follow the current branch policy.
- [ ] Verify invisible-block presentation, including conditional camera-facing marker
  sprites and their display/input conditions; invisible terrain alone does not close acceptance.

### Lighting, atmosphere and publication

- [ ] `P2.7-ATMOSPHERE` — Complete lighting, day/night, sky, fog, weather and cloud
  acceptance, together with the remaining terrain publication and remesh qualification.
- [ ] Classify and remove moving bands/static under camera motion using coherent
  resident, frustum, submitted and GPU-completed identities. Change culling only when
  evidence proves a missing draw.
- [ ] Run identical-scene release FIFO/Immediate presentation comparisons and fix the
  demonstrated source of the artifact.
- [ ] `P2-CHUNK-PUBLICATION` — Qualify initial publication, steady streaming and full-view
  remesh against current frame/join/streaming budgets. Attribute decode, light readiness,
  queue wait, worker time, upload, submission and present latency independently.
  Ordinary meshing now rechecks higher-priority deferred revisions within a bounded
  poll budget, with backoff while neighbours or light remain unavailable. Hardware
  dirty-to-visible latency qualification remains open.
  Sparse startup can complete without a publisher update after terrain decodes, stream
  and render work drain, and a later GPU frame completes. Empty-world initialization is
  verified; complete server-form and gameplay acceptance remains open.
- [ ] Close mesh-neighbour readiness acceptance with slow-delivery and seam coverage.
  Distinguish eligible horizontal columns from the provisional full-neighbour/quiet-time
  fallback; its timeout is not established native behavior.
- [ ] Close publisher membership/retention acceptance using FIFO-committed announcements
  and epoch identity. Keep raw-radius classification and confirmed player interest
  separate; no single enumerable Euclidean disk defines all server cohorts.
- [ ] Recheck reported sun/moon background rectangles and filter-edge artifacts using
  decoded pinned pixels and mip-edge tests across bright/dark skies and every moon
  phase. Fix any reproduced defects before closing their acceptance record.
- [ ] Close fresh release/native sun and moon comparisons, including horizon and
  filter-edge cases, before completing the visible-defect gate.
- [ ] Close precipitation geometry, wrapping, UV, occlusion, density and smoothing
  acceptance. Native wind/permutation, turbulence, block-light tint and view flags
  remain unqualified where the current implementation is provisional.
- [ ] Close volumetric cloud/layer acceptance from above, below, within and at grazing
  angles, preserving world anchoring, weather/fog fades and bounded GPU work.
- [ ] Recheck cloud scale, thickness, silhouette, shading, motion and seam differences
  with a fresh accepted native comparison gallery; resolve reproduced discrepancies.
  Deterministic mesher tests do not establish the rendered comparison.
- [ ] Confirm cloud texture scale, height, face shading, drift, day/weather/sunrise colour
  and distance-fade behavior for the pinned target. Historical profile approximations
  remain incomplete until calibrated against that target.
- [ ] Independently review and live-verify the final finite-cloud result.

The [family inventory](docs/phase-2-family-inventory.md) is a historical baseline;
regenerate target inventories from the current manifest before qualifying them.
See the [rendering reference](docs/reference/rendering.md) and
[chunk-pipeline architecture](docs/architecture/chunk-pipeline.md) for detailed contracts.

## Phase 3 — Movement and the local player

Implemented: [movement, collision and prediction](crates/sim/src/lib.rs),
[semantic input](crates/input/src/lib.rs) and the
[client interaction adapters](crates/client-interaction/src/lib.rs). Open acceptance
is a broader claim than the presence of these features.

- [ ] `P3-MOVEMENT` — Close movement, collision, prediction/correction and live-server
  acceptance for the pinned target. A server-compatible run is not full native parity.
- [ ] `P3.4-INPUT-CAMERA` — Close semantic input, camera and device-specific behavior.
  Touch remains open even when an owner-deprioritized touch witness does not gate a
  narrower scenario verdict.

Historical movement acceptance scope includes sprint/double-tap/toggle timing, forced
sneak/crawl, mode entry/exit, flight, swimming/gliding, liquid correction and scaffolding
descent. Reconcile each case with current code and evidence before calling it a missing
implementation.
Keep raw, analogue and processed input semantics distinct. Mounted controls need
attributable vehicle mapping, constants, state and actual prediction before claiming
client-predicted vehicle behavior. Riding alone does not establish that capability.

Correction-sync acceptance still includes predicted flag coverage, scaled/horizontal
poses and bounds, history loss, replay authority, send phase and live-server behavior.
Animation-only flags must remain separate from gameplay and correction authority.
Near-contact collision fixes do not establish complete collision-engine parity.

See [gameplay/session ownership](docs/architecture/gameplay-session.md),
[effective movement speed](docs/reference/effective-movement-speed.md) and
[camera behavior](docs/reference/camera.md).

## Phase 4 — Entities and equipment

Implemented: [actor rigs, Molang, controllers, attachables and pose evaluation](crates/actor-animation/src/lib.rs),
with [client presentation](crates/client-presentation/src/lib.rs) and
[rendering](crates/render/src/lib.rs). The open gates require complete behavioral
and rendered qualification, not initial implementation of those systems.
Frame sampling retains matching expression, layer and composition results and evaluates
remote body batches on the caller and available shared workers with ordered budget
reservations. Equipment placement reuses unchanged owner and attachment inputs. Rig plans
retain clip masks and name indices sized to each skeleton. Rig plans and prepared skins
share cape and cosmetic attachment bindings. Hardware crowd captures and full temporal/visual
qualification remain open.

Pure numeric Molang expressions use bound f32 lane programs with load fusion;
mixed values and effects retain the generic evaluator, including exact budget tails.
Full rig history and rendered qualification remain part of `P4.3-RIGS`.
Body and custom-skin composition use shared parent-first programs and caller-owned
output, including attachable root modes and legacy attacks. Alternate render
geometries and skin layers retain recursive composition. Rendered and live
performance qualification remains open.

- [ ] `P4.3-RIGS` — Close bounded asset admission, Molang/controllers, protocol-driven
  poses, skeletal rendering and distinct tick/frame interpolation with native evidence.
- [ ] `P4.4-LIVE-ACTOR` — Close spawn, movement, rotation and teleport ground-contact
  acceptance, including correct player-origin conversion and frame interpolation.
  Keep visual eye height and the protocol movement offset as separate owned facts.
- [ ] `P4.5-ITEM-ACTIONS` — Close held items, actions, dropped items and viewmodel behavior,
  including remote/local playback and session lifecycle.
- [ ] Close pickup sound and copied-item flight across immediate server removal with
  a version-matched rendered comparison.
- [ ] Close custom block-item texture resolution, non-cube shapes, tinting, animation
  and matched inventory/held-item presentation. Shared cube face sheets are bounded coverage.

Continuous cosmetic cloth uses shared vertices, fixed-step constraints, world-space
inertia and player-body contacts with standard-armor clearance. Vertical follow and
damping restrain jump and landing motion, and local cloth keeps advancing while
first-person visibility hides the body. View changes transfer the drape between
body pose spaces. Self-collision and world collision remain open.
Paid cosmetic selections use session ownership from the API database and withdraw
on disconnect or revocation; production rollout qualification remains open.

Equipped cosmetic emitters also run in visible menu and HUD paper dolls using the
shared particle engine and attachment poses. Catalog-wide, in-world interaction
and performance qualification for these previews remains open.

Player name tags use Java's standing player height before pose offsets, independent of
server collision-box changes. Other actors retain their published label height.

Remaining qualification includes attachables and custom item models, action/use timing,
projectile interpolation and shake, sleep/mount poses, name tags (including over
oversized server cosmetics), capes, shadows,
particles and equipment materials. Preserve existing accepted Java animation/cape/name-tag
and combat-visual deviations described in [AGENTS.md](AGENTS.md).

See [actor animation clocks](docs/reference/actor-animation-clocks.md),
[projectile rendering](docs/projectile-rendering.md) and
[presentation ownership](docs/architecture/client-presentation.md).

## Phase 5 — Interaction, inventory and UI

Implemented: [chat, HUD and scoreboard state](crates/ui/src/lib.rs),
[UI runtime and server forms](crates/client-ui/src/ui_runtime.rs),
[inventory prediction and crafting](crates/inventory/src/lib.rs), and
[menus and settings](crates/client-shell/src/menu.rs). The boxes track full acceptance
across these existing features; they are not a list of missing screens or systems.
Gameplay mouse capture resumes after focus or occlusion returns when it was active
before switching away. Screen ownership and focus-loss input clearing remain in effect;
OS Alt-Tab acceptance on macOS, Windows and Linux remains incomplete.

- [ ] `P5.1-UI` — Close pack-driven UI, bounded text/draw work, formatting, scaling,
  safe areas, focus/navigation and keyboard/mouse/controller/touch acceptance. Prove
  stable retained resources and correct supported DPI/aspect behavior. Form JSON
  container parsing and cleanup use heap stacks; structured text keeps its separate
  validation limits. Extreme structured-text and rendered-form acceptance remain open.
- [ ] `P5.2-HUD` — Close received text, titles, actionbar, toasts, survival/status HUD
  and lifecycle behavior without leaking UI-owned input into movement.
- [ ] `P5.3-CHAT` — Close history/autocomplete, formatting, bounded UTF-8 editing,
  clipboard, submission prefixes, command editing, rate limits and session/FIFO routing.
  Verify third-party receive/send and disconnect behavior.
- [ ] `P5.4-SCOREBOARD` — Close objective/score and boss-bar create/update/remove,
  ordering, display positions, styles and stacking alongside titles/actionbar.
- [ ] `P5.5-INTERACTION-COMBAT-INVENTORY` — Close targeting, mining/cracks, placement/use,
  combat, hotbar selection, stack/network-ID reconciliation and container/crafting
  prediction, acknowledgements, rollback and stale-session behavior.
  Native block requirements now cover every catalog identifier, including no-tool
  beds; recipe tags share the pinned native catalog. Sword/shears speed rules,
  effective item durability and live mining acceptance remain incomplete.
- [ ] `P5.6-FORMS` — Close modal/menu/custom form navigation, validation, cancellation
  and responses, including server-pack definitions, structured text, custom controls,
  scaling and image/layout behavior against matched native views. Server settings
  replies are cached until their tab is selected; embedding their controls within
  the settings panel instead of a separate form remains incomplete.
- [ ] `P5.7-PARITY-PERF` — Qualify combined chat, scoreboard, boss bars, inventory and
  forms at supported scales/aspects and devices, with bounded memory and stable frame work.
- [ ] `P5.8-SETTINGS` — Close menu, control, video and persistence acceptance. Desktop
  wiring alone does not establish native parity; language-specific scale dialogs,
  safe-zone handling, service-controlled spacing and touch/console behavior remain open.

The inherited interaction acceptance scope also includes full item identity and use phases,
replacement/deferred releases, mounted inventory, sign filtering/length/structured text,
native emote admission/cancellation/timing, overlapping dimension/respawn loading screens,
and server-authored chat hover/input behavior. Keep native input state independent of
custom emote playback and presentation-only changes.

Ordered desktop interactions still need live input and overload-timing acceptance,
global camera-sampling qualification, other input modes and device-specific chords.
The bounded burst/neutrality recovery policy remains provisional. Per-frame admission
regressions do not close the surrounding input, movement or interaction gates.

### Held block placement

Held placement still needs confirmation of direction/face/intercept retention, repeat
timing and reset behavior across slot/item changes, unavailable selection and deferred
presses. Preserve the explicit incomplete scope in the
[held-placement reference](docs/reference/held-block-placement.md).

See [inventory ownership](docs/architecture/inventory.md),
[launcher/UI ownership](docs/architecture/launcher-ui.md),
[OreUI](docs/oreui.md) and [desktop settings evidence](docs/evidence/desktop-video-settings.md).

## Phase 6 — Online product surface

Online services are implemented. Startup requires login facts and PlayerSpawn;
radius replies remain ordered play updates without holding startup. Complete
acceptance still covers authenticated
server/Realm/friend-world joins, transfers, reconnect/disconnect, account switching,
presence, invites and required-pack failures. Preserve transport/session identity
and keep actual account details out of captures and reports published with PRs.
Core transfer/disconnect terminals can abandon unfinished join preparation; ordinary
preparation and presentation readiness remain required for a successful join. Live
server latency impact and the complete transfer acceptance matrix remain unqualified.

Server pack acquisition, handoff and stack composition no longer impose fixed archive-count
or compressed-byte quotas. Saved server lists retain every imported entry. Declared sizes,
identity, digests and framing remain validated. Experience selections preserve service errors
and wait for the account core result. Complete featured-server acceptance remains open:
large-pack preparation can be slow or fail, and sparse startup terrain can leave loading open.
Server language loading retains every readable English and selected-locale layer without fixed
table, entry or process quotas, and folds inserted keys to ASCII lowercase. Unreadable sources
and allocation failures are diagnosed without discarding other definitions. Complete pack-content
acceptance remains open. Local imports, decompression, UI/text expansion and other admission limits
or compressed-byte quotas. Safe member paths have no added byte cutoff; server manifests
and default member reads retain complete declared content. Manifest subpack lists have no separate
row-count cutoff; server and local loading share the existing field validation and device-tier
selection. Native runtime and device acceptance for larger subpack lists remain open.
Explicit subscriber limits and large-content live and performance acceptance remain open.
Saved server lists retain every
imported entry. Declared sizes, identity, digests and framing remain validated. Experience
selections preserve service errors and wait for the account core result. Complete
featured-server, menu and gameplay qualification remains open, including large-pack
preparation and broader sparse-startup coverage.
Local imports, decompression, UI/text expansion, language memory and other admission limits
still need version-matched verification; the limit inventory closes no whole-client parity gate.

Cosmetics require live authenticated multi-account acceptance, catalog/motion envelope
coverage, supported slot/ownership enforcement, player-preview acceptance and hardware
qualification. Payments and moderation remain incomplete; rollout must respect API/client
compatibility. Existing catalog content is owned by the API repository.

## Phase 7 — Local worlds

Local worlds use the Go server integration. Qualify creation, offline play, reload,
import, persistence and hosting/session teardown. The accepted local-world scope does
not establish full vanilla world-generation parity.

See [local worlds](docs/local-worlds.md).

## Phase 8 — Audio, polish and packaging

Qualify audio admission, spatial playback, music and lifecycle behavior on supported
platforms. Missing source music/ambience requires lawful local input or the documented
fallback; do not bundle game assets to fill gaps.

Complete clean-install, first-run asset preparation, update/install recovery, startup
and crash diagnostics, packaging and platform acceptance. Required carriers must pass
validation; retain only the explicitly permitted diagnostic/optional fallbacks.

Windows runtime executables use `asInvoker` manifests and allow both normal and elevated
launches. Native Windows startup, update and crash-restart acceptance remains incomplete.

## Experimental hosts and extensions

- Browser/Android implementation and CI checks do not close their complete join, pack,
  transfer/reconnect, input/UI, audio, memory or hardware-performance acceptance matrices.
  Keep browser gesture activation and background work within the host's event-loop rules.
- Local mods and server experiences still need runtime/input isolation, reload/reorder/
  removal, consent, custom screen transitions and media qualification. Unmeasured shared
  blur, element blur, route exits and current-world visual integration remain incomplete.
- Native extension UI and HUD APIs (retained modules, profile fields, text plates,
  compass markers, totem annotations and bounded camera overrides) still need
  displayed-frame pacing and complete rendering acceptance.
- Enhanced rendering remains restricted until its GPU faults and freezes are resolved.
  Diagnostics or bounded fixtures do not establish safe general gameplay use. Java
  shader-pack source loading and configuration metadata are implemented in
  `shaderpack`, including options, dimensions, parsed properties, program discovery
  and enable rules. Translation, render-host integration and in-game acceptance
  remain incomplete.
  shader-pack source loading is implemented in `shaderpack`; configuration parsing,
  translation, render-host integration and in-game acceptance remain incomplete.
- `registryc --kind java-mapping` generates `crates/assets/data/java-mapping-v2193.bin` from
  the GeyserMC data pinned in `assets/geyser-mappings-source.json`: Java block states, block
  tags, biomes and their colours, entity types and items for every Bedrock id Geyser maps.
  States Geyser never produces take the closest same-block state and are flagged approximate;
  Bedrock-only biomes and blocks stay unmapped. Nothing reads the carrier yet; Java shader
  packs' ids, biome uniforms and sky colours are its first consumers.
- Runtime pack compatibility still includes custom texture inheritance/tints, authored
  geometry and block/item component behavior beyond the currently supported cases.

See [Android](docs/android.md), [mods](docs/modding-spike.md),
[server experiences](docs/server-experiences.md) and
[experience runtime](docs/experience-runtime.md) for existing implementation contracts.

## Acceptance and release qualification

Use the current [live-testing guide](docs/agents/live-testing.md) as the single owner of
reference hardware, frame/hitch, streaming, join and capture budgets. Capture the
rendered result on the relevant platform and scale; distinguish hidden-window pixel
coverage from displayed-frame pacing and OS input/focus evidence.

Compiled entities, blocks and icons share a compressed disk cache. Hits restore dependency
records; archive, session, startup artwork and device inputs invalidate affected entries.
Actor artwork prepares current demand immediately and prewarms unused pages within a
per-frame budget. Artwork arrays allocate exactly their source layer count. Skin slots
keep geometric capacity. Each artwork slot retains its own unchanged texture and bindings.
Worker rig-plan preparation and measured cold arrival latency remain open acceptance work.
Hardware join and frame acceptance still needs the full capture matrix below.

Check deterministic work in CI: bounded queues, invalidation, stale-work rejection,
retained memory, allocations, uploads and unnecessary rebuilds. Measure release
performance on hardware; existing source refactors and unmeasured optimizations
remain incomplete performance evidence. Preserve packed terrain storage and shared
render resources while measuring implementation choices under the architecture policy.

Release qualification starts from a clean install and covers sign-in, a server,
a Realm and a friend's world, survival movement/building/mining/containers/crafting,
chat/forms, resource packs, and an offline local world followed by reload. Required
platform, parity and performance evidence must be complete for the claimed release scope.
Keep the completion ledger consistent with this checklist; no historical green test
or isolated fix silently completes the surrounding gate.
