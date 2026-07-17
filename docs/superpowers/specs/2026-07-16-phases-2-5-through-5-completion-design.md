# Phases 2.5 Through 5 and Gameplay Completion Design

**Status:** Original design approved on 2026-07-16; gameplay amendment approved
on 2026-07-17
**Canonical base:** `d8e469979a0ec6c4798bb2ffc1dc45d3a9891eeb` (`phase2-textures`)
**Scope:** Phase 2.5, live chunk publication, Phase 2.7, all of Phase 3,
Phase 4.3 through 4.5, all of Phase 5, and the bounded in-game controls,
camera, and settings slice defined here

## Objective

Finish the requested client slices from the canonical Phase 2 lineage while
reusing integrated work, preserving archival worktrees, parallelizing only
independent work, and closing every deterministic, live-server, visual-parity,
performance, and verification gate recorded by this design and `plan.md`.

The 2026-07-17 amendment makes ordinary vanilla entity combat, held-item and
action presentation, three perspective modes, held-jump repetition, semantic
gameplay controls, and an in-game settings menu explicit completion
requirements. Combat is strictly vanilla client behaviour. Cinnabar neither
implements Lunar Reach nor negotiates, advertises, randomizes, or extends
attack distance for Lunar; any Lunar-side modification remains outside this
client.

Completion is strict. An unavailable reference capture, missing live witness,
unexplained diagnostic, or unproven performance claim remains a blocker rather
than becoming a silent deferral.

## Current-State Audit

The repository contained many linked worktrees whose apparent progress was
misleading because their local base refs were stale. The canonical remote tip
was refreshed and audited before this design was approved.

- The old local `phase2-textures` worktree is dirty and 318 commits behind the
  canonical base. It is archival and must not be updated, rebased, or merged.
- `phase25-biome-blend` is superseded. Its substantive biome-blending patch is
  already integrated.
- `phase27-atmosphere-parity` is superseded. Its medium-fog patch is already
  integrated.
- The old Phase 3 movement and physics worktrees are superseded. Equivalent or
  reviewed successor implementations of Phase 3.1 through 3.3 are integrated.
  Their uncommitted request-mode residency tests also exist in the canonical
  architecture.
- `phase4-actor-ingest` is superseded. Its sole feature patch is integrated,
  and the canonical branch also contains Phase 4.2 and later actor work.
- Phase 5 has no implementation branch or `crates/ui`; only its roadmap exists.
- Legacy normalization, inactive-subchunk, acceptance, focus, and runtime
  safety branches are patch-equivalent to integrated work or otherwise stale.

No stale branch may be merged wholesale. Valuable dirty files remain untouched
until their owner explicitly chooses to archive or remove them.

## Existing Foundations to Reuse

### Phase 2.5

The client already decodes palette-native biome columns, resolves live biome
definitions, classifies grass/foliage/water tint, applies tint without widening
the eight-byte cube quad, and implements a bounded radius-one 3x3 linear-colour
blend across chunk boundaries. The remaining work is native Bedrock kernel
adjudication plus live visual and performance proof.

### Chunk publication and Phase 2.7

The canonical code already contains bounded request/retry correlation, modular
client-world streaming, adaptive publication budgets, sparse block/sky light,
light metadata, light-aware meshing, GPU light consumption, world time and
weather state, sun/moon/cloud assets, camera-medium fog, and extensive coherent
publication diagnostics.

The client currently advertises Bedrock client blob caching as disabled and
rejects cache-backed LevelChunk and SubChunk payloads. Lunar supports this
route while Zeqa is expected to use ordinary inline/request payloads. Phase 2
therefore also includes a bounded content-addressed client blob cache: xxHash64
payload verification, exact hit/miss acknowledgements, FIFO pending-transaction
resolution, reconstruction of LevelChunk and SubChunk payloads, bounded memory,
and session-reset semantics. Lunar must prove the enabled cache route; Zeqa
remains the non-cache comparison. Cache support is independent of any measured
cohort, meshing, or publication correction and cannot be used to relabel those
stalls.

The deterministic radius-16 publication workload completed in 1,369 ms, but
the latest retained live evidence took 8,596 ms for a forced full-view remesh
and roughly 48 seconds for the binding teleport. Phase 2 therefore remains
open. The previously reported missing Lunar spawn chunks must be reproduced on
the canonical code before any new fix is selected.

### Phase 3

Phase 3.1 through 3.3 provide exact `PlayerAuthInput` encoding, a bounded 20 Hz
scheduler, free-camera authority isolation, a fail-closed fixed-tick simulator,
basic collision and correction replay, and app-side local physics integration.
Production still keeps physics network authority disabled. Remaining movement
strata, authoritative collision metadata, robust prediction and replay, and
live server verification are open.

The existing input path is hard-coded to keyboard and mouse. Space is retained
as held input, but the physics controller converts it into a jump request only
on the initial press edge, so holding Space cannot jump again after landing.
There is no semantic binding layer, controller gameplay input, perspective
state, third-person camera, or camera collision. Horizontal FOV is fixed at
120 degrees.

### Phase 4

Phase 4.1 and 4.2 provide bounded actor lifecycle ingestion, classic skin
retention, actor-specific coordinate normalization, three-tick network
convergence, adjacent-frame render interpolation, and a bounded shared biped
renderer. Phase 4.3 already has a deterministic `MCBEENT3` catalog and compiled
geometry/bone/cube payloads. Animation clips, Molang/controller evaluation,
runtime rigs, skeletal posing, GPU consumption, and animated evidence remain.
Phase 4.4 retains the binding live witness.

Vendored held-item, equipment, animation, and dropped-item packets exist, but
authored normalization and stores do not retain them. The renderer has no item
visual compiler, hand attachment, first-person viewmodel, third-person held
item, dropped-item path, or swing/use pose. These are implementation gaps, not
completed infrastructure.

### Phase 5

Phase 5.1 through 5.7 remain unimplemented. Vendored packet definitions do not
count as protocol normalization, state ownership, rendering, interaction, or
acceptance evidence.

Phase 5.5 names interaction and inventory but did not previously specify
entity attacks. The current plan places general settings in Phase 6, outside
the requested goal. This design deliberately pulls only the fully backed
in-game pause, controls, video, and persistence slice into Phase 5.8; server
browser, account, Realms, friends, resource-pack management, and other online
product surfaces remain Phase 6.

## Considered Execution Approaches

### Strict serial order

Complete Phase 2, then Phase 3, then Phase 4, then Phase 5. This minimizes merge
conflicts but wastes available concurrency and delays isolated compiler, UI, and
simulation work.

### Broad all-at-once parallelism

Start every roadmap item simultaneously. This maximizes nominal activity but
creates unsafe overlap in protocol exports, input authority, app lifecycle,
asset startup, render scheduling, and lockfiles. Integration risk outweighs
the apparent speed.

### Dependency-gated parallelism

Run independent lanes concurrently, freeze shared interfaces at explicit
checkpoints, and serialize shared-hotspot integration. This is the approved
approach.

## Branch and Worktree Architecture

1. Create a fresh integration branch from the canonical base.
2. Create fresh Phase 2, Phase 3, Phase 4, and Phase 5 worktrees from the
   current integration tip. Never repurpose an archival worktree.
3. Run at most three implementation lanes plus one integration/review lane at
   the same time.
4. Merge small reviewed tranches frequently. After a checkpoint, rebase or
   recreate each active lane from the integration tip before starting its next
   tranche.
5. Shared-hotspot edits land through the integration lane after lane-specific
   tests pass.

Primary ownership is:

- Phase 2: biome blending, client-world streaming/publication, lighting,
  atmosphere, and live performance.
- Phase 3: `crates/sim`, collision metadata, prediction, correction replay,
  movement networking, semantic gameplay input, and camera modes.
- Phase 4: entity asset compiler/carrier, Molang, runtime rigs, and actor
  rendering, shared item visuals, equipment, and action poses.
- Phase 5: `crates/ui`, UI state, UI rendering, interaction, inventory, and
  forms, vanilla combat, in-game menus, controls, and persisted settings.

Shared hotspots include protocol exports, app scheduling, app input, render
plugins, asset startup, architecture policy, `Cargo.toml`, and `Cargo.lock`.
Two lanes must not edit the same hotspot concurrently without an explicitly
frozen interface and integration owner.

## Delivery Sequence

### Checkpoint 0: establish current truth

- Build and test a fresh canonical worktree.
- Reproduce current Lunar and Zeqa behaviour on the canonical code.
- Record stage-level publication and presentation evidence before changing
  scheduling or protocol behaviour.
- Establish deterministic baselines for simulation, actor assets, and UI asset
  inputs.

### Parallel tranche A

- Phase 2 closes the 2.5 reference kernel, diagnoses live publication, fixes
  the measured bottleneck, implements and proves the Lunar client-blob-cache
  route, and advances remaining 2.7 visual defects.
- Phase 3 adds authoritative collision metadata and remaining deterministic
  movement strata while network authority remains disabled. In parallel
  within that lane, it defines the semantic action snapshot and correct
  held-jump contract without yet changing shared app wiring.
- Phase 4 compiles animation clips and implements the bounded Molang/controller
  evaluator. It also compiles canonical shared item visual identities behind
  an interface that does not depend on inventory UI.
- Phase 5 implements the isolated UI/font/layout/input foundation in 5.1 and
  the versioned settings schema without yet taking gameplay input authority.

### Integration checkpoint 1

- Merge only reviewed, deterministic-green tranches.
- Freeze publication, semantic movement/action input, collision metadata,
  entity-carrier, item identity, actor-pose, camera-pose, UI-action,
  settings, and UI-render interfaces.
- Run the full locked workspace and architecture checks.

### Parallel tranche B

- Phase 3 completes correction replay, physics authority, outbound movement,
  render interpolation, repeated held jumping, camera collision, all three
  perspective modes, and live verification.
- Phase 4 implements runtime rig selection, skeletal posing, shared GPU
  consumption, remote equipment/action state, third-person held items, and
  animated evidence.
- Phase 5 implements receive-side HUD/chat/scoreboard/bossbar work in 5.2
  through 5.4 plus the isolated in-game settings screens in 5.8.
- Run a non-binding 4.4 baseline to validate capture and diagnostic tooling.

### Parallel tranche C

- Complete Phase 4.5 local viewmodel, dropped-item, and action presentation
  after 4.3 rigs and Phase 5.5 selected-item state stabilize.
- Record the binding 4.4 witness after 4.3 and the actor-facing portion of 4.5
  stabilize.
- Implement 5.5 and 5.6 after Phase 3 input authority is stable, including
  strictly vanilla entity combat and inventory reconciliation.
- Integrate live settings, rebinding, persistence, and menu input authority,
  then close Phase 5 parity and performance through 5.7 and 5.8.
- Run the complete integrated server, native-reference, performance, and
  workspace verification matrix.

## Phase 2.5 Design

The existing radius-one 3x3 blend remains provisional until native evidence
confirms its radius and weights.

1. Build an exact abrupt-biome-boundary fixture whose biome identities,
   geometry, camera, time, weather, resource pack, and render settings can be
   reproduced in the matching native Bedrock client.
2. Capture multiple boundary patterns rather than fitting a kernel to one
   image. Include straight, corner, island, and alternating samples for grass,
   generic foliage, special foliage, and water.
3. Compare in linear colour and account for the native display/render path.
4. Infer the smallest kernel that explains every retained reference. If
   multiple kernels remain possible, gather discriminating evidence instead
   of choosing the easiest implementation.
5. Preserve palette-native immutable neighbourhood snapshots, uniform fast
   paths, source-identity validation, missing-neighbour behaviour, special
   foliage rules, and custom-biome fallback.
6. Prove abrupt boundaries, cross-chunk corners, missing neighbours,
   replacement, eviction, teleport, stale completion rejection, GPU output,
   memory bounds, and frame cost.

Phase 2.5 closes only when the native kernel is fixed or confirmed and the live
visual/performance gate passes.

## Chunk Loading and Publication Design

### Measurement before change

The first canonical Lunar run must classify the previously observed `0/0`
spawn result. Evidence must distinguish:

- request construction and outbound ordering;
- transport completion;
- response result codes and omitted entries;
- retry scheduling and exhaustion;
- decode admission, queue wait, and worker duration;
- light-halo readiness and solves;
- mesh queue wait and worker duration;
- main-world application;
- GPU upload, extraction, submission, and presentation.

The same instrumentation runs on Zeqa only after Lunar satisfies the gate.

### Scheduling requirements

- The player column and nearest visible columns outrank farther prefetch work.
- Semantic retries cannot be starved behind an unbounded stream of new outer
  columns.
- One failed vertical batch cannot amplify into uncontrolled retry traffic.
- Dispatch and publication throughput are elapsed-work and pressure aware, not
  artificially limited by low frame rate.
- All queues retain explicit item and byte ceilings.
- Known-air and packed-empty work remains semantically complete without
  consuming non-empty mesh/upload budgets.
- Successful, all-air, unavailable, malformed, stale, and timed-out results
  remain independently attributable.
- Adaptive budgets may respond to genuine frame pressure but cannot enter a
  low-FPS feedback loop that prevents recovery.

### Acceptance requirements

- The current player column and surrounding visible spawn region publish
  before far prefetch can exhaust their retry budget.
- Lunar and Zeqa show no persistent current-position holes.
- Initial radius-16 publication shows no visible stalls.
- Binding full-view remesh completes within two seconds on the release path.
- The gate must use normal FIFO/presentation behaviour, not a debug-only direct
  draw collapse.
- Required publisher-disk, resident, allocation, visible, submitted, and
  GPU-presented identities remain coherent and uncontaminated.

## Phase 2.7 Design

The existing face/directional shading is retained as the material response.
Solved block light, sky light, ambient occlusion, daylight, fog, and atmosphere
modulate that base; the task does not intentionally flatten the current look to
match a weaker visual result.

Remaining work includes:

- complete the identical-scene FIFO/Immediate motion-artifact comparison and
  remove the proven cause of the reported TV-static/void bands;
- satisfy the publication and full-view remesh requirements above;
- prove decoded sun/moon border and filter-edge behaviour against bright and
  dark skies across every moon phase;
- calibrate air, water, and lava fog against matching native references;
- implement and validate required weather/precipitation behaviour;
- correct finite-cloud source identity, mesh size, quality/distance controls,
  density, scale, thickness, silhouette, material response, weather colour,
  motion, fog, and seams against the matching native client;
- verify clouds from above, below, within, and grazing angles;
- retain bounded GPU resources, identity-cached pipelines, and no per-frame or
  per-subchunk resource churn.

Unknown light boundaries remain explicit. Stale light or mesh generations are
rejected and losslessly requeued. Malformed asset carriers or server inputs fail
closed with bounded, attributable diagnostics.

## Phase 3 Design

### Collision metadata

Compile authoritative, versioned collision metadata for every reachable
runtime state from pinned reviewed sources. Support compound boxes, friction,
climbable/liquid/special-block facts, and state-dependent behaviour. Missing,
contradictory, out-of-range, or stale metadata fails closed.

### Simulation

Extend the fixed 20 Hz simulator from the existing walk/sprint/jump/sneak slice
to the complete Phase 3 contract, including:

- edge-safe sneaking and stepping;
- item-use slowdown;
- ladders and other climbable states;
- liquids and swimming;
- cobwebs and other movement modifiers;
- slime/bed response and special block surfaces;
- effects, attributes, knockback, and dynamic player bounds;
- teleport/correction cases;
- gliding and game-mode differences required by the target servers.
- repeated grounded jumping while jump remains held, with native-compatible
  delay and release behaviour distinct from physical press/release flags.

Each predicted tick retains the immutable world/collision identity it queried.
Unloaded or replaced chunks invalidate replay rather than allowing guessed
terrain.

### Prediction, networking, and rendering

- Keep bounded tick-indexed input and state history.
- On server correction, restore authoritative state and replay only inputs
  whose world identities remain valid; otherwise snap and re-anchor safely.
- Render between adjacent simulation states independently of network
  convergence.
- Preserve exact feet/eye coordinate conversions, movement flags, deltas,
  client tick sequencing, and bounded network backpressure.
- Freecam remains network-silent and cannot authorize movement packets.
- Physics network authority remains disabled until deterministic and live
  gates pass.

### 3.4 Semantic controls and camera perspectives

Create one bounded semantic input router for gameplay and UI. Keyboard, mouse,
controller, and touch mappings produce a frame snapshot containing
pressed, held, and released actions plus finite bounded axes. The initial
action catalog includes movement, look, jump, sneak, sprint, attack, use,
perspective, menu/back, hotbar selection, and UI navigation. Raw device input
must not bypass this router after integration.

UI focus, window focus loss, controller disconnect, session replacement, and
authority changes emit semantic releases before the next fixed tick. A held
button cannot create repeated one-shot actions such as perspective changes,
but held jump remains available to the simulator so it can jump again after a
valid landing. Network key-edge flags and physical repeated hopping remain
separate facts and must follow native evidence.

Add `FirstPerson`, `ThirdPersonBack`, and `ThirdPersonFront` camera modes,
cycled once per semantic perspective press. Every mode derives from the same
interpolated local physics pose:

- first person uses the reviewed eye transform;
- rear view uses a bounded boom behind the view direction;
- front view uses a bounded boom ahead of the view direction and looks back at
  the local avatar;
- camera collision sweeps or clamps the boom against authoritative collision
  geometry and fails closed toward the eye at unloaded or unknown boundaries;
- perspective changes never mutate simulation, prediction history, or
  outbound movement.

The local avatar is hidden in first person and rendered exactly once in either
third-person mode using the Phase 4 rig. Interaction targeting always starts
at the local player's vanilla eye/look ray, never at the displaced
third-person camera position.

### Phase 3 evidence

- expanded pinned bedsim traces;
- terrain, step, edge, liquid, climb, effect, and correction fixtures;
- property/adversarial tests for bounds and replay identity;
- live vanilla parkour and target-server movement;
- Lunar movement without persistent rubber-banding;
- stable render interpolation and explicit freecam-silence evidence.
- held-jump traces covering land/rejump, release-before-landing, UI focus,
  catch-up limits, and correct network flags;
- semantic keyboard/controller equivalence, binding conflicts, disconnect
  releases, and input-authority transitions;
- deterministic perspective cycling, wall/corner/ceiling camera collision,
  teleport/dimension resets, multi-frame-rate stability, and local-avatar
  first-/third-person visibility.

## Phase 4.3 Design

### Asset compilation

Extend `MCBEENT3` with bounded animation-clip payloads while preserving exact
source-manifest provenance, symbol selection, legacy inheritance, geometry,
material, and texture contracts.

Compile the reviewed Molang subset into a deterministic typed representation.
Enforce expression depth, operation, collection, controller-transition, and
per-frame evaluation budgets. Unsupported optional expressions retain an
attributable static fallback; malformed required contracts reject the affected
rig rather than executing arbitrary behaviour.

### Runtime rigs and poses

- Resolve entity definitions and render controllers into a selected geometry,
  bone hierarchy, materials, textures, animation clips, and controllers.
- Drive reviewed variables from bounded actor metadata, attributes, pose flags,
  velocity, ground state, and 20 Hz actor history.
- Evaluate simulation poses at actor ticks and interpolate adjacent completed
  poses only in rendering.
- Reset controller/pose history on replacement, dimension change, incompatible
  metadata change, or teleport as defined by the controller contract.
- Retain shared geometry and texture storage.
- Upload compact bone transforms to bounded shared arenas; never create
  per-actor Bevy meshes or materials.
- Missing assets use explicit fallback/no-draw behaviour with attributable
  diagnostics.
- Normalize and retain bounded `AddPlayer` held items, `MobEquipment`,
  `Animate`, and the reviewed `AnimateEntity` subset with session, dimension,
  actor-lifetime, source-tick, and FIFO identity.
- Compile one canonical item visual identity used by remote equipment, dropped
  items, the local viewmodel, hotbar/inventory icons, and outbound selected
  stacks; presentation layers may add transforms but may not invent separate
  item identities.
- Attach third-person equipment to reviewed rig bones and drive remote
  swing/use poses from retained action state without per-actor meshes or
  materials.

### Phase 4.3 evidence

Test compiler determinism, inheritance, animation sampling, controller
transitions, Molang limits, rig selection, pose blending, replacement/teleport
resets, frame interpolation, GPU arena addressing, malformed packs, bounded
per-frame work, and native animated-player/mob output.
The evidence also covers equipment replacement, action deduplication/restart,
hand-bone attachment, item identity, and shared-resource bounds.

## Phase 4.4 Design

Run a diagnostic LBSG capture early to validate tooling. Record the binding
witness only after Phase 4.3 and the actor-facing portion of Phase 4.5 are
stable.

The final authenticated `play.lbsg.net:19132` witness must observe at least one
remote player's spawn, ordinary movement, rotation, and teleport and prove:

- AddPlayer and MovePlayer origins normalize exactly once;
- retained ground state and source ticks are coherent;
- three-tick network convergence remains distinct from adjacent-frame render
  interpolation;
- feet stay on the same ground plane without a 1.6-block jump;
- visual standing eye height `1.62` remains distinct from network offset
  `1.62001`;
- packet, actor-store, pose, draw, and native visual evidence share bounded
  identities.

## Phase 4.5 Held Items, Actions, and Viewmodel Design

Phase 4.5 integrates the shared item and rig foundations across remote actors,
the local player, and dropped item actors. It is a cross-phase integration
tranche: remote equipment can land after Phase 4.3, while the local viewmodel
waits for Phase 5.5's authoritative selected stack.

- Normalize and retain `AddItemEntity` lifecycle and render dropped items from
  the same canonical item visual catalog.
- Render remote held items and action poses through Phase 4 rigs and bounded
  shared GPU arenas.
- Render a separate first-person paper-doll arm and held-item viewmodel using
  the authoritative selected stack, native-referenced hand/item transforms,
  FOV behaviour, depth ordering, and use states.
- Drive local attack, break, placement, and item-use presentation from one
  bounded action timeline. Local swing/use prediction is visual only and must
  cancel or reconcile when server or inventory authority rejects the action.
- Support empty hand, melee tools, placeable blocks, consumables, and reviewed
  duration-based uses. Unsupported valid items receive an attributable bounded
  fallback instead of a stale prior item.
- Keep first-person and third-person transforms distinct while sharing item
  identity, source assets, action phase, and rig semantics.

Deterministic gates cover equipment and selected-slot replacement, actor and
session resets, swing restart/deduplication, use duration/cancellation,
left/right handedness where the protocol proves it, item fallback, viewmodel
depth, shared arena addressing, and absence of per-action resource allocation.
A controlled two-client BDS witness cycles all nine hotbar slots and correlates
local viewmodel, remote equipment, action packets, dropped items, inventory UI,
and presented frames against matching native references.

## Phase 5 Design

### 5.1 UI foundation

Create `crates/ui` as a renderer-independent retained UI model with
deterministic layout, safe areas, UI scaling, focus/navigation, and semantic
keyboard/mouse/controller/touch actions. Compile Bedrock bitmap fonts and glyph
metrics through the provenance-checked asset pipeline.

Use one shared bounded UI draw pipeline and texture resources. Cache text
layout by content, style, scale, font, and asset identity. Avoid per-glyph
meshes, materials, bind groups, or unbounded frame allocations.

### 5.2 Receive-only text and HUD

Normalize bounded Text, title/actionbar, toast, player-status, health, hunger,
armor, air, and related lifecycle packets into vendor-neutral stores. Render
chat history, title/actionbar, and survival HUD. Session replacement clears UI
stores atomically.

### 5.3 Interactive chat

Implement bounded UTF-8 editing, focus/history, autocomplete, Bedrock
formatting, clipboard behaviour, and ordered session-aware sends with spam-safe
rate limits. UI focus consumes input before movement and releases held actions
without stuck keys or accidental packets.

### 5.4 Scoreboard and boss bars

Maintain independent bounded objective/display/score and boss-event lifecycle
stores. Render sidebar, list, below-name, score ordering, boss style/health,
stacking, replacement, removal, and coexistence with titles and actionbar.

### 5.5 Interaction, hotbar, and inventory

Interaction and inventory remain server-authoritative. Predicted break cracks,
placement, and item-use visuals are provisional. Reconcile item stack request
IDs, network IDs, slots, containers, selected hotbar state, creative/survival
inventory, chest/furnace/crafting state, and rejected responses. Rollback must
restore the last authoritative state without item duplication or stale UI.

Entity combat is part of this tranche and is strictly vanilla:

- sample one immutable local eye/look pose, selected stack, actor snapshot,
  and collision/world identity for each attack decision;
- ray-test against reviewed combat bounding boxes, choose the nearest valid
  intercept deterministically, and reject an entity when a nearer solid block
  occludes the path;
- apply native game-mode reach and interaction rules established by matching
  Bedrock evidence; never add reach fluctuation, extended distance, target
  enlargement, automatic targeting, or Lunar-specific behaviour;
- on a valid attack, emit the protocol-1001
  `InventoryTransaction`/`UseItemOnEntityActionAttack` contract with the exact
  target runtime ID, selected slot/item, player position, session identity, and
  FIFO ordering required by the server;
- on a miss, present only the native missed-swing behaviour and do not invent a
  target transaction;
- keep damage, death, knockback, durability, cooldown, inventory mutation, and
  target validity server-authoritative while allowing bounded provisional
  swing/hit presentation where native behaviour does;
- rate-limit only malformed, duplicated, or queue-overflowing input; do not
  turn one physical click into an auto-clicker or impose non-native combat
  timing.

Combat fixtures cover nearest-hit ordering, overlapping boxes, inside-box
starts, pose-dependent bounds, block occlusion, unloaded boundaries, stale or
removed runtime IDs, server-declared non-attackable state, game-mode reach,
miss behaviour, selected-item changes, session replacement,
backpressure, and exact attack encoding. Local BDS and authenticated
third-party witnesses correlate click, ray snapshot, target decision,
transaction, server response, actor/inventory revisions, swing/hurt pose, and
presented frame. No Lunar module is enabled, queried, or required for these
gates.

### 5.6 Server forms

Parse modal/menu/custom JSON forms with strict byte, depth, collection, option,
and text limits. Support keyboard/controller/touch navigation, validation,
cancellation, and session-aware responses. Unknown optional extensions are
ignorable; malformed required structure produces a safe cancellation response.

### 5.7 UI parity and performance

Compare matching native views at supported scales and aspect ratios. Exercise
keyboard, mouse, controller, and touch focus transitions. Prove bounded retained
memory and stable frame time with chat, scoreboard, boss bars, inventory, and
forms active together.

The active-state performance witness also includes the hotbar, first-person
viewmodel, multiple equipped remote actors, combat/action animations, and the
in-game settings overlay with no per-action mesh, material, texture, or bind
group allocation.

### 5.8 In-game menu, controls, video settings, and persistence

Add an in-game menu with resume, settings, disconnect, and quit actions. The
settings surface includes only options whose runtime behaviour is implemented
and verified in this goal:

- controls: complete keyboard and controller bindings for the semantic action
  catalog, mouse/controller sensitivity, inversion, controller deadzones,
  binding conflict resolution, and the perspective binding;
- video: horizontal FOV, fullscreen/display mode, frame cap, VSync, UI scale,
  render/chunk distance where server constraints permit, brightness/gamma,
  and the implemented cloud/weather quality controls;
- gameplay/UI: supported perspective default and other implemented UI/input
  preferences.

Changing a setting updates the live runtime through validated typed state.
FOV remains finite, aspect-correct, and native-calibrated in first person and
both third-person modes; combat reach never originates at the camera boom.
Bindings cannot strand held actions when changed while active.

Persist a versioned per-user settings document outside the repository using
bounded counts and strings, finite/range validation, deterministic conflict
rules, schema migration, safe defaults on corruption, and atomic replacement.
Never store credentials or tokens in this document. CLI acceptance overrides
remain separate, explicit, and deterministic rather than silently rewriting
user preferences.

Settings gates cover live application, cancel/apply/default flows, round-trip,
migration, malformed/truncated/oversized files, write interruption, binding
conflicts, active-input rebinding, focus transitions, controller disconnect,
restart persistence, every supported aspect ratio, and native visual/input
comparison. Audio, account, server browser, Realms, friends, and resource-pack
management remain outside this bounded Phase 5.8 slice.

## Cross-Cutting Input and State Rules

- UI focus has priority over gameplay input.
- A focus transition emits semantic releases for held movement/actions before
  another authority consumes input.
- Session and dimension identities qualify retained network state.
- Stale async work cannot mutate a replacement session, world, actor, or UI.
- All retained collections have explicit item and byte limits.
- All externally sourced floats are finite and bounded before use.
- Combat decisions bind local pose, selected item, target actor, collision
  world, session, and FIFO identities; stale identities cannot attack.
- Third-person camera displacement never changes the eye-origin interaction
  ray or movement authority.
- Local item presentation, remote equipment, inventory UI, and outbound
  transactions share one authoritative item identity.
- User settings and acceptance overrides remain separate typed authorities.
- All asset carriers retain source and schema identities and fail closed when
  stale or malformed.
- Backpressure is lossless for required ordered state and explicitly
  coalescing only where the protocol contract permits it.

## Error Handling and Diagnostics

Each subsystem exposes independent counters and structured reasons for
malformed input, unsupported-but-valid input, capacity rejection, stale work,
retry, timeout, fallback, and permanent failure. Counters must distinguish an
expected safe fallback from data loss.

The release client must not hide correctness failures behind debug-only paths.
Live gates record build profile, backend, adapter, driver, present mode, server,
session cohort, and asset identities without retaining credentials or private
token contents.

## Verification Contract

### Requirement ledger

Maintain a ledger mapping every open `plan.md` checkbox, every sub-gate, and
every explicit requirement added by this amendment to its proving test, live
run, capture, metric, reviewed artifact, and commit. Absence of a detected
failure is not proof. The first implementation-plan tranche adds matching
roadmap entries for 3.4, 4.5, expanded 5.5, and 5.8 so the two ledgers cannot
silently diverge.

### Per-lane gates

Each tranche runs:

1. focused red/green tests;
2. affected crate suites;
3. strict warnings-denied Clippy;
4. formatting and architecture policy;
5. deterministic carrier/fixture identity checks;
6. independent review before integration.

### Integration gates

Every integration checkpoint reruns protocol fixtures, client-world sequencing,
lighting/meshing, rendering, simulation, semantic input, camera modes, item
identity/presentation, vanilla combat, inventory rollback, UI focus, settings
persistence, startup asset identity, Go core tests, and the locked workspace
suite. A failed integration is fixed or reverted before another lane merges.

### Live matrix

- `pvp.lunarbedrock.com:19134`: spawn/current chunks, publication, lighting,
  movement, HUD, vanilla interaction/combat, perspectives, and held items.
- `zeqa.net:19132`: authenticated transfer path, chunk streaming, movement,
  chat, forms, UI, controls, settings, and vanilla interaction/combat.
- `play.lbsg.net:19132`: Phase 4.4 actor ground-contact and interpolation.
- Local BDS: deterministic biome, lighting, movement, two-client actor,
  held/dropped item, combat, inventory, form, settings, camera, and visual
  galleries.
- Matching native Bedrock: biome kernel, atmosphere, clouds, celestial edges,
  UI parity, movement/input behaviour, perspective cameras, held-item/action
  presentation, combat targeting/transactions, FOV, and settings behaviour.

### Performance and resource gates

- full-view remesh completes in at most two seconds;
- no persistent spawn/current-position holes or visible streaming stalls;
- frame-time gates in the authoritative acceptance manifests pass;
- combined steady-state RSS is at most 650 MB on the reference class;
- steady-state combined CPU is at most 15 percent on the reference class;
- join and teleport bursts settle within the binding gate;
- queues, arenas, histories, actor stores, and UI stores remain within their
  explicit ceilings;
- hotbar, viewmodel, equipped actors, combat animations, and settings overlay
  retain stable frame time and allocate no per-action GPU resources;
- no debug-only draw, backend, or scheduling path is used as release evidence.

### Final completion gate

The goal completes only when:

- every scoped requirement-ledger entry has authoritative evidence;
- every scoped `plan.md` checkbox and amendment-ledger entry is closed without
  an unexplained deferral;
- required GPU paths produce two consecutive exact presented witnesses;
- all live server and native-reference gates pass;
- decode, normalization, stale, capacity, retry, and fallback counters contain
  no unexplained failures;
- freecam produces no movement packets;
- combat remains vanilla and produces no extended-reach or automated attacks;
- first-/third-person modes, held jumping, controls, FOV, and persisted
  settings satisfy their deterministic and live gates;
- local and remote held items, swing/use actions, and dropped items share
  authoritative identities and pass native visual comparison;
- shutdown is clean;
- the final integrated branch is review-clean and the full verification matrix
  passes from a clean worktree.

## Goal Invocation

The durable goal should remain concise and reference this document rather than
copying the entire design into command text:

> Finish the approved amended Phases 2.5-through-5 and gameplay-completion
> program in
> `docs/superpowers/specs/2026-07-16-phases-2-5-through-5-completion-design.md`
> from canonical base `d8e4699`, preserving archival worktrees, parallelizing
> independent lanes, and satisfying every recorded deterministic, live,
> visual-parity, performance, and verification gate.
