# Phases 2-5 Completion Ledger

This ledger records acceptance evidence, separately from feature implementation. An
open gate does not mean its feature is absent. Older rows without evidence are not
a current implementation audit; check the owning code and later entries before
calling work unstarted. Test files listed below establish existing coverage, not
a fresh passing run or full gate acceptance.

## Browser host composition

The browser host uses verified memory carriers, the shared game frame and HUD, injected
session I/O and canvas input. Native owner checks and a wasm artifact pass. Headless Chrome with
Metal WebGPU boots, renders an offline world's terrain and HUD, and moves its player. WebSocket
transfer, disconnect and rejoin pass. Audio ranges feed the existing decoder and Web Audio mixer;
real nonzero playback, gesture activation, cached suspension/resume and single-close disposal
pass. Initial terrain latency, interactive UI controls, audio parity and hardware budgets remain
incomplete. This adds no completed parity or browser-release gate.
transfer, disconnect and rejoin pass. Initial terrain latency, interactive UI controls, audio
delivery/playback and hardware budgets remain incomplete. This adds no completed parity or
browser-release gate.
## Local actor presentation flags

The local rig retains animation flags separately from gameplay metadata. Visual
item use and locomotion poses no longer author correction-sync flag words.
Rendering consumers read the separate animation state. Accepted local held-use and
glide/swim/crawl projection, replay authority and send-phase parity remain open.

## Correction-sync posture retries

Correction syncs retain accepted server sneak/sprint updates newer than the last
completed movement sample, including walking height and retries without a new
tick. Sparse updates preserve the other posture flag; a completed local tick
replaces the retained update. Regression coverage includes transport refusal and
input-latch consumption. Other prediction-sync metadata, pose sizes, exact send
phase and live server acceptance remain incomplete.
## Browser release verification

Browser target checks and JavaScript edge suites are CI gates. Go and Rust consume
shared control fixtures, and devtool builds a staged static bundle with the locked
binding CLI and canonical carrier delivery index. Certificate renewal retains
relay sessions and rejects invalid replacements while keeping the last good pair.
The receive-buffer regression runs on the native test target.

Incomplete: valid shared-client join/pack/transfer/reconnect integration and the
real-browser acceptance matrix follow gameplay composition. No supported-browser,
release, parity, memory or hardware performance gate closes from these checks.
## Flowerbed stem tint

Wildflower and pink-petal stems retain biome grass tint independently of petal
colours and pack-selected sprite names. Every registered flowerbed state passes
the original-art compiler and carrier-roundtrip regression. The focused crate
check and rebuilt-carrier local BDS rendering pass against the official client.
Broader terrain parity and hardware performance gates remain open.

## Redstone dust item sprite

A reviewed default binding selects the item-atlas dust sprite, leaving placed
wire and redstone-block rendering separate. Owning regressions cover authored
sprite pixels, carrier roundtrips and absent-atlas fallback. Native and current
before frames confirm the wrong icon and invisible held item. Rebuilt carriers
show the correct dust sprite in the fixed hotbar and hand. All 308 owning tests
and focused checks pass; other geometry and item-parity gates remain open.

## Command-block terrain materials

Facing and conditional selectors now retain their front/back panels, oriented
side arrows and atlas-bound animation strips. Compiler regressions cover every
registered command variant, boundary culling and carrier roundtrips. Render-fact,
exact regeneration and focused compile checks pass. Local BDS frames and animation
phases pass against the official Android reference. Broader pack/platform parity
and hardware performance gates remain open; the command-block editor is unfinished.

## Source-water retention during block changes

Primary updates and predictions share source-water retention using generated
per-state liquid-containment facts. Owning regressions cover both ID formats,
ordered layer overrides, solid-block and bucket-removal controls, and malformed
cells. The registry and bound carriers regenerate together. Flowing/falling
water, custom block capabilities, broader parity and hardware performance remain
open gates. Local BDS checks pass for own/observed placements, fence breaking and
bucket removal against the official Android client. Focused checks pass; two
installed-creeper tests in the broader suite remain unattributed.

## Liquid surfaces beneath opaque roofs

Lowered water and lava tops remain visible below solid blocks, with shared side
insets. Owning meshing regressions and the focused crate check pass. Matched local
BDS frames reproduce the missing tops and verify the corrected surfaces against
the official Android client. Broader liquid parity and hardware performance gates
remain open.

## Catalog cosmetics

A domain catalog replaces the fixed hat list. Independent slot selections persist locally;
ETag catalog caching and SHA-256 asset admission precede geometry or texture decoding.
Original bunny ears use bounded authored joint motion. Every slot follows its declared
attachment bone and offset, and the HUD shares local motion with world presentation.
Saved hats migrate into the head slot without overriding current choices. Visible thumbnails
and equipped textures share the artwork budget; stable gallery frames and cosmetic motion
retain their catalog and raster caches. Cosmetic layers are separate from skins, flames and
render-controller layers, with boundaries derived from the supported slot count.

Session loadouts use authenticated mutual nearby subscriptions with account revision
ordering and server-side ownership validation. Loadouts have no emote lease. The current
catalog is free for every account. API and client implementation require API-first rollout;
live authenticated multi-account acceptance, payments, moderation and hardware qualification
remain incomplete. No broader parity or performance gate advances.

Settings mod cards now show permission summaries, explicit requested-grant consent
and a compact customization grid. Capability metadata supplies gameplay warnings;
batch grant edits preserve the existing registration format and enforcement.
Owner visual approval and native cross-platform acceptance remain incomplete.
This changes no broader parity or performance gate.

Single-cell palette edits retain usage counts and patch canonical storage when existing
indices and width stay valid. Structural changes still rebuild, and equality ignores only
derived bookkeeping. Regression tests and benchmark fixtures have not been compiled or run;
broader lazy compaction, output verification and measured performance remain incomplete.
Single-cell palette edits retain bounded physical slots and live counts while preserving
canonical palette order. Last-use removal and width shrink defer word packing until an exact
word view is requested; snapshot-local caches and exact mutation results avoid eager equality
materialization. Untouched decoded representations, no-ops and snapshot isolation are preserved.
Memory admission accounts for physical storage, historical slots and possible canonical caches;
existing process-wide capacity limits are unchanged.
Regression tests and benchmark fixtures have not been compiled or run. Output verification,
consumer-cadence, retained-memory and measured performance evidence remain incomplete.
This change advances no phase completion, parity or hardware performance gate.

Verified chunk blobs now persist through a bounded background store. Restart, corruption,
miss reporting and memory pressure coverage belong to `client-runtime` and `protocol`;
this storage change advances no rendered, live-join or broader parity completion gate.

Startup carrier selection, bounded decoding, validation and fallbacks now belong to
`assets::carriers::startup`. App adapters retain renderer and pack publication, Bevy resources,
load timing and fatal startup handling. Carrier regressions move with their owner;
app composition coverage remains in `app`. This move adds no compilation, execution
or live evidence, and all behavior and performance gates retain their status.

End-rod terrain coverage now includes the six facing states, separate shaft/base sprite
mapping and an original-art GPU fixture. This bounded geometry correction leaves the
exhaustive residual-family, world-join and hardware performance gates open.

Common farmland crops now retain four lowered rows with stage-specific artwork.
Fruit stems retain growth height, stage colour and facing-oriented connectors;
pitcher crops retain pods and separate flower halves; torchflowers retain their
stage artwork at the plant width. Berry bushes retain their growth-stage artwork
and narrower sprite geometry. Exact native-client crop comparison,
sub-unit geometry and atlas-edge UV calibration remain incomplete.
Compiler regressions cover all eight growth states and both network-ID modes;
full crop presentation parity and the broader phase gates remain open.

Trapdoor compiler regressions cover edge sprite rows, facing/top-half artwork,
opened cap texture ownership and malformed selectors across registered poses
with 16/32 pixel textures. Rendered/input acceptance and broader graphics parity
remain open.

Lever compiler fixtures cover separate base/handle geometry, eight mounts, both
power states, 16/32-pixel sprite regions and neighboring controls. The mounting
base retains the cobblestone-wall top material independently of the handle. These
tests and the focused crate check pass, with a missing-part failure on the previous
model. Rendered/native interaction acceptance remains incomplete.
No completion or performance gate advances.

Cape rotation limits now prevent upward flips in both player animation modes. Remote resets
discard Java chase lag, and remote Bedrock flap uses completed tick displacement instead of
retained packet speed. Local flap keeps per-tick velocity through catch-up frames. The Bedrock
direction bound preserves hanging authored rotations, including yaw beyond a quarter turn.
Flight, remote movement/reset, local catch-up and authored-rotation regressions have not been
compiled or executed. Rendered-frame validation and the exact Bedrock flap calculation remain
incomplete. This change closes no parity or performance gate.

Far-summary extraction now has global copied-work admission, a background lane and an
explicit shared colour-preparation dependency. Retired inboxes retain and advance deferred
sources. Shutdown advances all retiring inboxes before waiting, and cancelled live delivery
releases completed results while far terrain is off. Regression fixtures are written but unrun; output, transfer timing and hardware
performance qualification remain incomplete. No phase or performance gate closes here.

## P2.5-NATIVE-BIOME

| Field | Evidence |
|---|---|
| Owning plan/task | Packed biome storage and tint preparation exist in `crates/world/src/biome.rs` and `crates/meshing/src/biome.rs`; full native qualification remains open. |
| Deterministic tests | Existing shader coverage includes `crates/render/tests/it/biome_shader.rs`; full-gate run results are not recorded in this row. |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

## P2-CHUNK-PUBLICATION

| Field | Evidence |
|---|---|
| Owning plan/task | Bounded sub-chunk request origin, FIFO grouping, transport membership and deferred-mesh promotion are implemented; broader publication acceptance remains open. See the sub-chunk request batching continuation in `plan.md`. |
| Deterministic tests | Protocol, stream, transport and app regressions are authored. Deferred-mesh regressions cover settled dependencies, stale light, priority and bounded work. Full publication qualification is pending. |
| Review commit | Independent review is pending. |
| Live/native witness | Full camera writer phase, global request discovery, native retry policy and later client versions remain open. |
| Performance/resource witness | Live join, streaming and hardware budgets remain open. |
| Final status | Open |

## P2.7-ATMOSPHERE

| Field | Evidence |
|---|---|
| Owning plan/task | Numbered raster pack skyboxes and optional native vertical placement in `plan.md`; broader atmosphere work remains open. |
| Deterministic tests | Pack admission and reload dependency regressions in `pack-runtime`, with publication regressions in `app`, cover priority, malformed and oversized winners, missing faces and removal. GPU tests cover projection, filtering, transparency, depth, visibility and stable colours; texture-cache tests cover reloads and dimension changes. |
| Review commit | Skybox tranche tracked in [#632](https://github.com/cinnabar-mc/cinnabar/pull/632); full phase review remains open. |
| Live/native witness | Retained-pack offline GPU views and both Video setting states were inspected. Current-server full-client acceptance remains open. |
| Performance/resource witness | Existing raster byte, dimension and allocation bounds remain; texture-cache regressions cover stable frames. Live frame and join budgets remain open. |
| Final status | Open — numbered PNG/TGA/JPEG skyboxes are implemented; HDR/KTX, enhanced rendering and broader atmosphere acceptance remain open. |

## P3-MOVEMENT

Riptide releases now carry a level-dependent launch and spin lifetime through fixed ticks,
correction replay, input start/stop flags and local actor presentation. Charge and admission
regressions cover wetness, queue rejection, captured ordinary and assisted release facing,
snap/replay lifetime and cancellation with a discarded release. Ordinary and assisted releases retain
release-edge eligibility before waiting for interaction sampling or facing input, including movement
FIFO backpressure, and retain use slowdown through that tick. An accepted use keeps its facing
fence while pending inventory gestures hide the selected stack. The first release observation
retains its selected item, eligibility and effective action facing for the input and launch,
without replacing later physical look outside that action fence. Release
sprint blocking expires on the same fixed tick independently of render-frame grouping. Admitted
launches survive a snap or replay fallback until their movement input is written, including late
socket receipts. Cancellation replay rebuilds the pose eye and interpolation, retained aim and
liquid attachment heights from retained eye state. Locomotion selection, travel and replay share
the preceding spin's compact liquid probe so water sensing also gates glide entry. Respawn search and ready anchors discard old
releases, temporary use modifiers
and pending launches, including unreleased item-use charges and reserved use input/facing. Item-use admission waits for
respawn completion without resetting inventory or cooldown policy. Suppressed travel uses the compact spin body for block contacts. Unsupported
impulses are retired at the
collision-query budget, a known safety divergence for oversized levels. Exact positional rain eligibility, actor-hit
termination, server spin metadata, audio and integrated acceptance remain incomplete; this
change closes no movement parity or performance gate.

Remaining double intermediates in processed controls, block-height contact, support
offsets, climb requests and fallback travel drag now use f32 operations. Precision
regressions await CI; full numeric and trajectory parity remains open.

Spectator regressions cover forced flight without MayFly, solid and unloaded
terrain traversal, and embedded server anchors. The full native trajectory,
platform, and performance gates remain open.

Body-contact sampling and retryable query limits now cover fast flight exits.
The added simulator and controller regressions await CI; live movement and
performance acceptance remain open.

Server-defined vehicle seats have deterministic coverage for actor-origin conversion,
yaw, scale, link ordering, updates and removal. Dismount request coverage includes
vehicle controls, queue retries and session/input boundaries. Authored seat fallbacks,
special family placement, predictive exit placement and vehicle prediction remain
incomplete; this work closes no movement parity or performance gate.

Custom block surface friction follows each state's `minecraft:friction` component, with
protocol, permutation and registry regressions in both identity spaces. No live witness
exists. Every vanilla state now carries a reviewed ordinary, special or unresolved movement
disposition bound to its identifier set; one-way door collision, block-actor collision and
soul soil remain unmodeled or unresolved.

| Field | Evidence |
|---|---|
| Owning plan/task | `docs/superpowers/plans/2026-07-17-phase-3-movement-controls-camera.md`, Tasks 8-14 |
| Deterministic tests | Current integration tranche: 282/282 `cinnabar` library tests and strict all-target/all-feature Clippy green; bounded Phase 3 evidence validator 5/5 green. See `docs/evidence/phase-3-movement-controls-camera.md`. Render frame-clock tests also cover 20 Hz tick flush cadence at 60/120 Hz FIFO and uncapped, with manual recording clock preservation; live movement cadence remains unverified. |
| Review commit | Pending independent review of the final integration commit. |
| Live/native witness | Current tranche not run; local BDS, Lunar, Zeqa, LBSG, and matching native gates remain required. |
| Performance/resource witness | Current tranche not run; bounded live JSON records remain required. |
| Final status | Open -- deterministic integration advanced; normal Physics enable and binding live evidence remain gated. |

Runtime queue ownership, publication budgeting, world-service schedules, shutdown,
session helpers and pack geometry preparation now belong to `client-runtime`.
App adapters retain menu, movement, presentation and audio integration. Existing
regressions follow the moved owners or continue exercising app composition; this
extraction is incomplete and adds no compiled, executed or live parity evidence.

## P3.4-INPUT-CAMERA

`client-interaction` owns camera input adapters, movement resource wrappers and
hotbar sampling, plus item-use and placement observations. App-owned world,
transport and screen adapters still drive the remaining interactions and block
selection. This extraction is incomplete and has not been compiled or tested;
no movement, input or interaction acceptance gate is closed by the move.

Frame sets and shared player, settings and finalized-input resources now live in
`client-state`. `ClientShellPlugin` owns menu input, fullscreen and settings drivers
and publishes focus for gameplay and camera input. Desktop focus transitions live
in `semantic-input`; its Bevy wrapper stays in `client-state`. The shell also owns
launcher/local-world resources, appearance selection and Discord presence, using
shared session observations instead of sibling plugin dependencies. Session,
network and audio adapters remain in `app`. Menu tests moved with their owner;
combined session, inventory and present-mode checks remain in `app`. These moves
have not been compiled or executed and do not close this gate.

Settings now manages ordered local components, independent reloads and per-mod permissions.
The native mod host is always included; regular callback fuel and separate event/loading
lanes are bounded across the full frame. Deferred lifecycle state and bounded screen-scoped
edit text remain queued until delivery, while frame-local outputs reset. Revocation is distinct
from quarantine, so unrelated registration changes do not restart trapped guests.
These extension changes close no vanilla parity gate. Their native execution and visual
acceptance remain incomplete until CI and owner review, as recorded in `plan.md`.

`client-extensions` now owns personal component collection and callback merging,
permission grants, player snapshots, panel fonts, and server consent input and text.
Consent state and ordering live in `client-state`. The coupled session, gameplay,
menu, render and audio adapters remain in `app`; the extraction and its execution
validation are incomplete. The moved tests retain their assertions, and this move
closes no acceptance gate.

Spectator hand suppression and no-clip inside-block overlays have owning
behavioral regressions. Runtime skin controller visibility precedes equipment, and
authored opacity has a GPU regression. The spectator material fallback remains
provisional; version-matched stock materials, custom controller geometry, persona,
and exact native and platform comparison remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | `docs/superpowers/plans/2026-07-17-phase-3-movement-controls-camera.md`, Tasks 8-10 and 12-14 |
| Deterministic tests | Atomic interaction identity/reset, real eleven-stage production schedule membership/order, semantic device barriers, all perspectives/camera collision, and gameplay movement/jump/use/look touch targets are green in the 282-test client suite. |
| Review commit | Pending independent review of the final integration commit. |
| Live/native witness | Current tranche not run; touch, first/rear/front, collision, local-avatar, and native comparison gates remain required. |
| Performance/resource witness | Current tranche not run; 30/60/144 FPS and bounded resource artifacts remain required. |
| Final status | Open -- deterministic code and validator present; binding live/native evidence remains gated. |

## P4.3-RIGS

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: local custom models; static imports reuse actor preparation. |
| Deterministic tests | Focused custom-import, rollback, persistence, login/update encoding, preview-worker and shared first/third-person preparation tests pass. |
| Review commit | The PR description records independent review of static imports; full parity review remains open. |
| Live/native witness | Headless before/after preview and local-avatar frames, restart persistence and second-client login/update frames are attached to the PR; complete native parity remains open. |
| Performance/resource witness | Worker preparation is bounded; hardware frame budgets remain open. |
| Final status | Open |

## P4.4-LIVE-ACTOR

Sulfur-cube material states and content-pinned shell opacity have alias, carrier
roundtrip and custom-raster exclusion regressions. Absorbed-block attachment
placement remains open; no complete actor parity claim.

Held-sprite edge-mask fixtures include alpha 0/1/2 and 16–128 pixel rasters;
matched pack frames, authored sword display transforms and first-person face shading
remain open in `plan.md`.

Full-texture weapon planes use ordinary held-sprite extrusion and stage artwork.
Focused equipment regressions check contour walls and depth after first- and
third-person placement. Matched native frames remain open.

Stock wolf texture admission preserves controller-selected adult/baby variants
and dye-mask alpha bytes. Tamed collar colors follow byte dye metadata and live
updates. Wet shading, custom-material composition, and exact target-version stock
material/shader linkage remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | Not recorded in this row |
| Deterministic tests | Not recorded in this row |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

Actor rigs, Molang evaluation, controllers and poses live in Bevy-free
`actor-animation`, which reads actors through its `AnimatedActor` seam.
Tests that drive actor authority stay in `client-world`. Compilation and
execution of this move are unverified, and it adds no parity evidence.

## P4.5-ITEM-ACTIONS

Session custom-item regressions cover scalar legacy facts and stack limits,
shared held/preview/dropped block geometry, carried tint and owner-selected
external armor models. Broader item-action and custom-content parity stays open.

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: Totem activation; broader item actions remain open. |
| Deterministic tests | Local/remote event admission, equipped-item snapshot across delayed publication, equipment-only hands, consumption ordering, retrigger, expiry, session retirement, partial ticks, bounded stall time, activation before the frame clock update, full-viewport scissor and retained clipping, viewport/layer placement, admitted texture replacement/removal and authored particle routing/attachment. |
| Review commit | Totem implementation: `590c2039e`; broader item actions remain open. |
| Live/native witness | Totem camera event/icon behavior and local authored-emitter routing verified; full-client default/custom activation, main-hand fallback, retrigger and remote/empty exclusion pass. Broader item actions remain open. |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

## P5.1-UI

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: Compact font carriers and glyph residency; broader UI parity remains open. |
| Deterministic tests | Font schema, bounded decode, frame residency, atlas churn, and retained allocation regressions cover compact carriers. Original large-sheet UV, loop, reload and independent residency-budget regressions cover tiled UI images. |
| Review commit | Not recorded in this row |
| Live/native witness | Exact coverage/SDF GPU pixels and mixed-script offline frames; native Bedrock UI parity remains open. |
| Performance/resource witness | Font storage is bounded and development-profile first-menu startup is measured; release hitch and hardware-tier qualification remain open. |
| Final status | Open |

Sound engine, session, named playback, local cue and UI sound tests belong to
`client-audio`; camera publication and actor presentation belong to
`client-presentation`. App integration tests retain the composed frame-order and
camera-listener coverage. This ownership change adds no new parity evidence.

The `json_ui::vanilla` contract checks categorized names against the pinned UI
vocabulary. Screen routing and core form/context consumers use those names;
remaining controller migrations and stale-name corrections are incomplete.
This adds no live visual evidence and leaves the UI acceptance gate open.

Cube inventory thumbnails and alpha-tested GUI meshes share the canonical outward
faces. Regressions cover projected surface orientation, transparent front holes and
retained alpha/depth/UV behavior. Broader custom-model sidedness remains open.

## P5.2-HUD

| Field | Evidence |
|---|---|
| Owning plan/task | HUD state and publication exist in `crates/ui/src/hud.rs` and `crates/client-ui/src/ui_runtime/gameplay_hud.rs`; full acceptance remains open. |
| Deterministic tests | Existing HUD matrix coverage includes `crates/client-ui/src/ui_runtime/presentation/tests/hud_matrix_tests.rs`; full-gate run results are not recorded in this row. |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

Attack-category cooldown shading has owning timer and hotbar-publication regressions
for admission rollback, shared categories and expiry. A local spear jab reproduces
the missing overlay against the official client; broader HUD parity remains open.

## P5.3-CHAT

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: chat completion and quoted player names. |
| Deterministic tests | Mention filtering, name quoting, command permissions, insertion, Tab cycling, bounds and row-text regressions are written; compilation and execution await CI. |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

Plain chat offers `@here` and player-list names; command targets include `@n`.
Education selectors, selector-expansion permission and cheat gating, selector
argument completion, and live visual acceptance remain incomplete.

Server-authored message content keeps its parser bindings and hover controls inside
the client-owned chat placement. Regressions cover retained captions, HUD collection
rows, pointer entry/exit through pass-through panels, display scaling and zero-sized
tooltip controls. Native command-edit acceptance and broader JSON-UI parity remain open.

## P5.4-SCOREBOARD

| Field | Evidence |
|---|---|
| Owning plan/task | Scoreboard state and UI adaptation exist in `crates/ui/src/scoreboard.rs` and `crates/client-ui/src/ui_runtime/scoreboard_adapter.rs`; full acceptance remains open. |
| Deterministic tests | Existing pack rendering coverage includes `crates/client-ui/src/ui_runtime/presentation/forms/engine_tests/scoreboard_pack.rs`; full-gate run results are not recorded in this row. |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

## P5.5-INTERACTION-COMBAT-INVENTORY

Custom block mining retains component defaults, disabled values, ordered item
overrides and resolved state permutations in both runtime identity spaces.
Regression tests cover component decoding, progress rates and completion packets.
Complex item-tag scripts and live custom-block acceptance remain incomplete;
this does not advance the interaction completion gate.

Ordered desktop interactions now attempt each edge only in its input frame.
Refused presses retain no proposal or movement reservation. Gameplay owners retain
accepted mining and release work, while overload recovery suppresses current held
input until neutrality. Scrolling preserves per-control suppression, and accepted
item uses can end before neutrality without starting another use. Regression coverage
exercises per-frame admission, use resolution and pending controls. Live input
acceptance, overload timing and camera sampling remain incomplete;
no completion gate advances.

Selected transaction descriptor regressions cover optional stack-ID omission while
preserving item contents, inventory action IDs and legacy requests. The shared
embedded movement payload uses the same conversion. Admission, dynamic actions,
NBT rewriting and combined runtime acceptance remain incomplete.

Block and actor rays share float normalization. Block traversal and clipping use
rounded float segments; placement bounds and click coordinates stay in float
precision. CI must verify the new rounding regressions. Block tie/union rules,
camera-direction generation and runtime witnesses remain incomplete.

Remote actor snapping respects target-column terrain order. Block-use and painting
occlusion share rounded intercept distances. CI verification remains pending.

| Field | Evidence |
|---|---|
| Owning plan/task | Partial local placement coverage and compiled movement/selection ownership; see Local placement prediction and Block selection outlines and visual bounds in plan.md. |
| Deterministic tests | Gameplay direction, support, stacking and immediate-placement tables in both palette encodings; pipeline atomic pair, rollback and ordered publication tests. Item-use tests cover frame/tick slot changes, the native rearm boundary, category cooldowns, rejected-send rollback and hotbar cooldown publication. Inventory regressions cover packet routing, whole transaction rejection, owned response data, active close inputs and selected-item use after close; complete inventory parity remains open. |
| Review commit | Recorded by the placement follow-up PR. |
| Live/native witness | Headless stair comparison and door pair before server confirmation; broader family visual parity remains open. |
| Performance/resource witness | Paused-server stair recording: first visible/staged +3 frames, observed upload +5; bounded late worker service. Click-frame and full hardware budgets remain open. |
| Final status | Open: remaining placement families and the wider interaction/inventory gate are incomplete. Combat reach, float clipping, movement rounding and snapping, minimum collision dimensions, actor/latency commit ordering and fresh-press swings have regression coverage pending CI. Pose-dependent reach, per-entity pick radii, specialized actor motion, native-origin rounding, terrain availability during asynchronous ingestion, catch-up-frame attack positions and full batch/latency timing remain open. |

## P5.6-FORMS

| Field | Evidence |
|---|---|
| Owning plan/task | Server form admission and presentation exist in `crates/client-ui/src/ui_runtime/forms.rs` and `crates/client-ui/src/ui_runtime/presentation/forms.rs`; full acceptance remains open. |
| Deterministic tests | Existing form coverage includes `crates/client-ui/src/ui_runtime/presentation/tests/forms_tests.rs`; full-gate run results are not recorded in this row. |
| Review commit | Not recorded in this row |
| Live/native witness | Not recorded in this row |
| Performance/resource witness | Not recorded in this row |
| Final status | Open |

## P5.7-PARITY-PERF

Ingress uses bounded lookahead and the existing conflict tracker to admit light events
past stalled terrain. Dependent mutations, dimension changes and transfer barriers stay
ordered. Latency echoes flush when their movement/control fence clears. Deterministic
regressions cover these paths; paired hardware join and transfer timings remain open.

Cloud preparation skips hidden geometry, prefetches finite windows on workers and
retains GPU storage. Regression coverage checks admission frames, exact records,
bounded caching and buffer reuse. Cold and missed windows still wait for geometry;
hardware frame-time and capture acceptance remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | Bamboo geometry, column transforms, sampling and overlay consumers covered; wider parity and performance work remains open |
| Deterministic tests | All twelve bamboo states, offset admission in both ID spaces, source-pixel mip selection, opaque/cutout consumers and populated Metal rendering graph |
| Review commit | Not recorded in this row |
| Live/native witness | Fresh Vanilla and bounded Enhanced bamboo gallery, selection and timed breaking captures; screenshots accompany the change |
| Performance/resource witness | No displayed-frame performance acceptance from hidden debug captures; ordinary Enhanced remains disabled |
| Final status | Open |

Video quality now reaches terrain meshing, including generation changes for running jobs and
resource-pack rebuilds. Renderer work is reduced through uploaded native lightmap bytes,
bounded sky lookup, retained actor pipeline IDs and UI composite bindings, and direct
SMAA blending into the shared scene samples. Early depth rejection and shared passes across
deferred nametag gaps reduce filtering and attachment work without changing draw order.
Player label anchors use Java's standing height before crouching offsets; collision-height
transitions and Scale have regression coverage. Wider name-tag qualification stays open.
Native colour comparisons cover LDR/HDR filtering and partial-sample overlays. World UI
also shares passes across depth modes, preserving pixels and pipeline-readiness fallbacks.
Opt-in Metal visibility admission uses fresh current-frame masks.
Target-device frame-rate and full settings parity acceptance remain open; bounded fixtures
and deterministic work budgets do not close these gates.

## P5.8-SETTINGS

Minecraft import adds explicit consent, per-profile preview, schema-checked option
mapping, server merging, validated pack copying and a restore snapshot. Full option
and control-layout coverage, live checks of every supported data location and
Android scoped-storage import remain incomplete. This leaves the settings parity
and acceptance gates open.

The global Subscriptions tab and marketplace UI/services are removed from the
product. Settings retain the remaining category order and icon alignment;
Realms, Dressing Room and server resource packs remain supported. Navigation
and unused-packet regressions await CI. No rendered-frame witness was captured,
and this visible removal closes no settings, UI parity or performance gate.

| Field | Evidence |
|---|---|
| Owning plan/task | `launcher` settings models; `launcher-host` storage and persistence; `app` runtime adapters. Persisted **Bedrock Skybox Vertical Offset** Video option; broader settings completion remains open. |
| Deterministic tests | Launcher regression covers default-on, old saves, both saved values, subsystem projection and Video reset. Offline JSON-UI regression reaches and renders both toggle states. |
| Review commit | Skybox option tracked in [#632](https://github.com/cinnabar-mc/cinnabar/pull/632); full phase review remains open. |
| Live/native witness | Both offline Video control states were inspected; full settings and current-server acceptance remain open. |
| Performance/resource witness | Full settings performance witness remains open. |
| Final status | Open — the skybox option is implemented; the complete settings gate is not closed. |

The standalone setup window belongs to `first-run`. Its preparation tests move with it;
menu integration tests stay in `app`. Account, process and persistence tests belong to
`launcher-host`. These ownership changes do not close the settings parity gate.

## Xbox presence follow-up

The account core publishes menu, world-default, Realm and generic featured/Experience activity.
Exact heartbeat, platform configuration, server overrides, permission gates and
friends-visible acceptance remain incomplete. No existing completion entry is
advanced by this feature; see `docs/reference/xbox-presence.md` and `plan.md`.

## Session block highlight follow-up

Highlight lookup uses admitted custom state identities and the pinned base registry.
Regression coverage includes both ID modes, palette replacement and session reset.
The loaded-block highlight capture shows a custom block before and after admission.
Selection shape parity and displayed-frame performance gates remain open.

## Inherited block overlay geometry

Block overlays use the shared pack compiler to resolve legacy geometry inheritance before projecting faces. Parents may come from other model files or pack layers; missing parents, cycles and chains beyond the depth limit are rejected. Budget-limited reads admit higher-priority definitions first. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.

## Render ownership boundary

CPU block models, dropped items, actor rigs and viewmodel meshes live in
`render-model`. Bevy submission resources and presentation acknowledgements live
in `render-scene`; presentation and acceptance have no dependency on `render`.
GPU passes remain in `render`. Existing regression coverage is retained, but
compilation and execution of these moves are unverified. No parity or performance
gate is closed by the ownership change.

## Session biome climate and environment

Admitted live biome identifiers and climate now travel with the session tint registry. Weather, fog precipitation, camera temperature and environment routes read that registry, including custom biome routes. Registry replacement clears removed routes. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.


## Spawner display actors

Spawner displays now use client-owned actor lifetimes and the ordinary actor
evaluator, render controllers, sampled poses and active pack artwork. Configured
entities no longer depend on a fixed mob list. Displays retire when the block or
world changes, and pack replacement rebinds existing lifetimes.

Incomplete parity: display factory metadata defaults, exact native renderer bounds,
activation edge cases and the full entity gallery are unverified. Frame and GPU
budgets remain open; these changes do not close those acceptance gates.

## Protocol session layer

`protocol::login` runs the spawn sequence and play stream over the core's session endpoint, and
the vendored client session crate is removed. Startup ordering, terminal transfers and
disconnects, and the one-shot initialization keep their regression coverage through a scripted
core; those tests have not been executed locally. No existing completion entry is advanced.
## Controller particle attachments

Typed locator and initialization contracts are implemented. Independent pinned
controller coverage and focused emitter/placement regressions are added. Pending
placement retries after temporary zero scale; existing attachments retain their
actor lifetime. Particle placement samples each needed actor once per frame. The
unsupported query grammar, skin locator, child-event placement, fallback and
animated gallery checks in `plan.md` remain open. No completion entry advances.

## Particle terrain ordering

Individual depth ranges and adjacent-range batching are implemented. Renderer composition and
current-frame regressions pass, and an offline Metal fixture measures the GPU span. Native bucket
composition and full-frame acceptance remain incomplete.

## Cached beacon beam sections

Beacon columns invalidate typed color sections when blocks, NBT, dimension bounds or block assets change. Admitted beacons populate the cache lazily after distance and submission checks. Top-layer beacons extend to open sky without sampling blocks. A block-owned eligibility table recognizes glass and hardened glass; sections carry their height and mixed tint to the renderer. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.

## Active block-entity artwork

The resource-pack transaction now builds the block-entity atlas on its worker and publishes it with the accepted pack result on join and reload. Missing or invalid overrides retain the validated startup tile. Texture and UV publication advance together. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.

## Loom banner previews

JSON-UI banner controls draw ordered front-cloth layers from the active artwork. Loom pattern choices and results publish bounded pattern and dye bindings, including existing banner layers. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.

## Protocol codec swap

`protocol` uses protocolgen's vendored contract crate as its only codec; Valentine and its
normalization machinery are gone. Packet routing and UI budgets derive from the generated packet
table and field sites rather than hand-maintained ID lists. The ported tests have not been
executed locally. No existing completion entry is advanced.

## Graphics API startup recovery

The shared backend policy, driver blocklist, crash sentinel and restart-only Video override
are implemented. GL/GLES is absent from choices and candidates; older saved GL choices read
as Auto. Android requires Vulkan. Failed saved Video choices retry Auto for the current launch,
while WGPU_BACKEND stays strict. Each API is enumerated under its own sentinel before eligible adapters are
ranked by power preference, device type and vendor backend order, with unclassified hardware
ahead of software. Device failures advance through all candidates;
named GPU selection restricts every API. Exclusions are keyed only by backend and reset on a
client version change or Graphics API selection, including choosing Auto again. Exhausted candidates permit one retry
pass with exclusions cleared. Startup log boundaries synchronize only the file sink.
Candidate filtering requires base-vertex terrain draws, vertex and fragment storage, alternate
texture views, non-power-of-two mipmaps, sample-rate shading, unaligned buffer bindings and
sufficient storage limits. Native crash reports retain the running process's renderer context
independently of shared recovery files.
Window closure and handled SIGINT/SIGTERM cancel unfinished startup and termination prevents
later sentinel writes. Panics and native faults remain recoverable. Hardware crash reproduction,
Android lifecycle and rendered Video acceptance remain incomplete as recorded in `plan.md`.
No parity or performance completion entry advances.

## Generated block-entity routing

Registry generation now supplies the shared backing and renderer table used by
block-entity admission, spawner displays, portal scans and visual diagnostics.
Unsupported and provisional routes remain explicit. This changes no completion
status; the remaining renderer and live acceptance gates stay open in `plan.md`.

Retained mod drawing has authority and lifetime contracts. Geometry admission validates
vertices once and caches extent and quota facts for every shared instance and callback
group. Extent checks use conservative box and sphere bounds without revisiting vertices.
Unchanged callbacks preserve snapshot identity without allocations. The generic unlit
renderer is connected; actor root attachment uses the renderer publication and exact spawn
references. Bone attachment, native output captures and hardware budgets remain incomplete
and do not close any completion requirement.

Bounded procedural materials and SDK presets use a session animation phase.
Lifecycle, shader layout and GPU timing tests pass; native Metal fixtures cover
animated presets, line joins and scaled geometry. These fixtures do not close
in-world rendering or hardware budget requirements.

The Rust/WIT drawing SDK supplies deterministic shape builders and depth-tested
instance defaults. Legacy compatibility paths remain in place until retained
presets preserve their coverage and placement. TypeScript packaging, renderer
acceptance and hardware performance gates remain incomplete.

Host-composed outlines and halos use admitted actor masks. Custom effects declare their
own mask bindings. In-world ordering, resolution transitions and hardware budgets remain
incomplete in `plan.md`; these additions close no rendering or performance gate.
