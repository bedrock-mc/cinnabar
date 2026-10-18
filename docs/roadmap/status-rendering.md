# Rendering and artwork status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Custom block and item texture inheritance

Custom terrain aliases resolve their selected filenames against server packs and the
installed carrier, retaining destination tint and UV sampling. Inventory thumbnails and
cube sheets can read borrowed base-page materials. Standalone custom item icons can inherit
base sprite files without creating replacements for ordinary vanilla items. The icon
carrier retains raw source-filename bindings independently of processed thumbnails,
including nonzero atlas variants and original mask alpha; older carriers require `make assets`.
Isolated item texels admit ambient-occlusion and face-dimming flags; baked thumbnails
honor disabled face dimming. GUI cubes that cannot retain this flag keep their unshaded
thumbnail while their carried face sheet remains available. Grass-tinted thumbnails use the winning carried-grass
overlay colour from the installed pack and server stack; other unresolved tint operations
remain rejected. Each thumbnail prepares its material texels once and shares processed
pixels between model faces.
Block inventory geometry and materials use `minecraft:item_visual` descriptions when
present, independently of placed-state transforms and world palette offsets. Compound
placement traits with enabled-state lists retain complete state identities.

Incomplete: full native comparison, additional carried tint families, non-cube custom items held as authored
three-dimensional models, and hardware performance qualification. These rendering fixes
do not close a parity gate.


## Incremental persona artwork pages

Persona animation images retain bounded single-layer page slots across empty views.
New images share their source pixels and update only their own slots; a full cache
reuses offscreen slots. Session and base-art changes invalidate residency. Grouped
admission remains available when the exact page layout exceeds the existing budgets.
Regression coverage compares pixel bytes and attributable hashing and uploads with the
grouped path. Incomplete: these changes have not been compiled or tested; matched
rendered captures and hardware performance measurements remain pending. No parity or
performance gate closes here.


## Far terrain (LOD)

**Disabled, incomplete:** far terrain and the full-detail radius are off, with no Video settings,
until their streaming bugs are fixed; values older builds saved are ignored. The design below is
kept for re-enabling.

`far-terrain` summarises each column the server sent: the height of every cell's highest
visible block, its average texture colour tinted by biome, and any water depth, with water
blended over its floor. World workers summarise a column when all of its sections have arrived,
including a resend, and again as it retires, so local edits are kept. Summaries run below
mesh, decode and lighting on the background workers. One process-wide admission owner bounds
copied job inputs and undelivered results across live and retired streams; retained world
snapshots and immutable asset registries remain separate source memory. Large sources advance
through bounded copies of the relevant layers and biomes. A shared asset identity prepares
its colours before any dependent summary runs. Retired inboxes keep deferred sources alive
and advance them during draining, within the existing bounded exit wait. Shutdown gives all
retiring inboxes a bounded drain turn before waiting for activity in any of them, so one
session's ready results can release capacity for another. Disabling live far terrain closes
its previous result receiver: queued results are discarded and running copies release their
reservations when they finish, independently of enabled polling. Focused regression
fixtures are written; execution, transfer timing and output qualification remain unverified.
Summaries hash their content, so an identical resend leaves the cache unchanged. The in-memory cache keeps them
compressed under a byte budget and drops the least recently used first. Cache bounds live in
`far_terrain::config`; copied worker admission lives in the chunk pipeline.

Summaries persist under the user data root in 32×32-column region files, namespaced by a
hashed server identity, the dimension and a hash of StartGame's seed, level ID and level name.
Local worlds also use the launcher's world ID instead of their shared endpoint. Without a
level ID or local world ID, each visit starts fresh. Before reusing a namespace, every initial
non-empty probe must exist and match in it (or every probe received within 10 s, for sparse
worlds); missing or different probes start a new epoch. Only live columns show before that.
Delivered empty columns hide cached terrain immediately, including beyond the full-detail
radius, while their empty summaries are prepared in the background. Air coverage does not
extend the near geometry bound used to choose far tile detail.
Afterwards the first cached columns replaced by live ones are compared too: when enough
differ, as for another lobby instance under the same identity, the visit drops the cached map
and takes a new epoch. A disk read never replaces a newer column, and a retired session's late
summaries never replace a later visit's. Every session, including a transfer to the same
address, and every committed dimension change starts a fresh visit, including a round trip
between rendered frames. Dimension changes disconnect pending summary delivery before the
destination can publish terrain. An IO thread writes each region in
batches and evicts the least recently used regions past the per-server (64 MiB) and global
(512 MiB) caps.

The far pass draws the cached terrain beyond near terrain as blocky tiles of 64×64 cells,
1 to 16 blocks wide, finer toward the near edge. It never draws within the full-detail radius
(or, without one, what the server sends), which loads in as without far terrain; past it, far
terrain hides over the columns near terrain has drawn, such as those handing over to it:
finest quads whole, coarser ones per fragment. Only finest tiles reach the near edge. Tile
meshes merge equal-height tops and runs of walls on worker threads, skirt their edges down to
the lowest neighbouring surface, rebuild at most every 250 ms when their columns change, and
keep a replaced tile drawn until its replacements are ready, never two over one area. The GPU holds only
the selected tiles and covering fallbacks during bounded replacements; a still camera uploads
and rebuilds nothing. Growth copies existing GPU data without replaying resident uploads.
Near column admission and the far mask publish together before draw queues run. Delivered-air
columns remain in the publication target outside the mesh radius. First delivery publishes
zero-byte empty evidence through the same bounded queue at either radius, so its receipts
hide cached terrain without requiring geometry work. Empty-section receipts survive feature
toggles within their stream and dimension visit, stay bounded by retained sections, refresh
on every section-membership change, and expire on replacement or
withdrawal. Valid decoded summaries prove coverage per column; missing, invalid or partial
summaries cannot prove an entire tile empty or retire its visible fallback. Resource geometry
and material tables publish together after pending removals and their fences allow the swap;
blocked candidates stay staged. The same admission is rechecked before replacement allocation
components reach draw queues. Focused lifecycle regressions pass, including 15 app far-terrain
cases after transfer and resource resets. Hardware performance and visual acceptance remain
open. Encoded summaries share
immutable bytes across caches and tile snapshots. Gathering includes skirt neighbours and
resumes within column and byte limits. Cache completions and live or retiring summary
admission share a per-frame budget; partial disk reads keep the highest queued version. Cells are lit as sky-lit
terrain faces and fogged with the terrain fog, which is unchanged. The render distance is how
far the player sees: far terrain fills the gap between what the server sends and the render
distance, within a budget set by the device tier (`render_api::far_terrain`). Video › Far
Terrain (LOD) is Auto (the default), On or Off. Auto runs it only when the render distance
exceeds the tier's automatic full-detail radius; below that nothing of it runs. An earlier
saved on/off reads as On/Auto.

With far terrain on, only columns within the full-detail radius are lit, meshed and uploaded;
the rest of what the server sends keeps its block data and is drawn by far terrain from its
summary. The radius is Auto by device tier (24, 32 or 48 chunks) or a full-detail distance
(16–64), capped at the render distance, so large render distances cost far tiles rather than
meshes. A column is promoted (relit and remeshed) inside the radius and demoted two chunks past
it. Both hand-overs are two-phase: a promoted column passes to near terrain only once its
meshes are published, and a demoted one keeps its meshes until its fresh summary is delivered
and its far tile is built. Block changes re-summarise their column, and turning far terrain off
keeps it shown until near terrain draws every loaded column, so no change opens a hole. Entities keep their 12-chunk render distance, inside the smallest
radius.

Incomplete: the far-terrain look (cell colours, height aggregation and water blending) is held
for owner review. Enhanced rendering lights far terrain with the vanilla path,
and block light, clouds and weather do not reach it. No parity gate closes here.


## Local terrain reads

Local mods with the `terrain` grant poll small bounded regions as palette-indexed
layers with canonical states, resolved collision/selection geometry and physics facts
([docs/mod-terrain-api.md](../../docs/mod-terrain-api.md)); unchanged samples return only a stamp.
Unknown cells and unavailable contextual geometry remain explicit. Existing mods receive no
new permission by default.

Incomplete: live acceptance on Windows has not been completed. No movement or interaction
parity gate closes here.


## Runtime terrain bindings

Optional terrain catalogs retain each material's source key, state variant, tint, and
independent weighted choices. Raster-only layers refresh selector representatives while
preserving base variation ranges and leaf weights. Retained choice indices distinguish
repeated raster paths for flipbook updates. Block texture bindings
repoint only plain full-cube world faces with one-to-one source mappings and prepare
separate carried cube icons and held-item sheets. Carried-only texture changes also
prepare art for eligible cubes. The material sidecar retains source facts and cube
rebinding eligibility after packing. Carrier fallback resolves raster paths across keys,
retains baked atlas tint, and applies a new multiplier only to untinted sources. Pack
and carrier images share filename resolution, tint, UV grids, and frame admission. Cube
bindings derive UV rotation from the replacement binding shape and retained pillar mapping.
Animation and selector caches use resolved state indices. Material admission reserves the
base table, failed routes release unpublished allocations, and held sheets survive without
thumbnails. Retained block bindings are ordered by visual ID.
Repeated catalog states and cube bindings keep base references when their rasters are absent;
changed choices still use carrier fallback. Specialized material sources retain their selected
catalog keys and variant indices.
Raster reuse follows the resolved image filename while preserving tint, grid and flipbook
policies. Full arrays check indexed aliases for reuse before reading any new image payload.
Block icon baking runs only when the icon subscriber recompiles. Mesh reuse also requires
each material to retain its positional-versus-static variation mode.

Incomplete: Block texture rebinding excludes pottable plants, directional or rotated
blocks without a pillar axis, model and container blocks, tinted or overlay-color carried textures,
leaves, walls, fences, and other inventory-state models. These keep their existing routes.
Carrier fallback for animated donors is also incomplete: their baked timelines do not
retain physical source-frame identities. Such donors are excluded from fallback; bundled
images still use the destination route's flipbook metadata.
CI must compile and run the regression fixtures. Rendered state variants, position-selected
texture output, pack reload/removal, and performance acceptance still need headless
verification. No parity gate closes here.


## Lightning rod terrain geometry

Lightning rods compile their complete shaft and tip for all six facings, eight
oxidation/wax variants and both powered states. Surfaces retain the authored sprite
regions and powered texture selector. Original-art regressions cover geometry,
material selection, carrier roundtrips and malformed states. Offline Metal frames
cover every state beside supporting terrain; broader world-join and performance
gates remain open.


## Respawn anchor artwork

Respawn anchors compile six cube faces for charge states zero through four. Terrain
arrays and animated top materials retain the selected charge, while the shared
carried-face sheet preserves pack overrides for inventory and held rendering.
Original-art regressions cover carrier roundtrips, short arrays, scalar overrides,
transparent faces, partial animations and malformed states. Headless captures with
checked world and icon carriers verify the held cube and all five charge textures
through placement and charging input. Server-pack compilation, broader block and
animation parity, and performance acceptance remain incomplete.


## End rod terrain geometry

End rods compile a separate shaft and mounting plate for all six integer-facing states.
Each surface samples its authored sprite rectangle, with independent face orientation,
alpha testing and no full-face occlusion. The owning regression covers active registry
identities, carrier roundtrips, bounds, UVs and unsupported selectors. Offline GPU
acceptance uses original fixture art and production terrain meshing and shaders;
broader residual-family, world-join and performance gates remain open.


## Grindstone terrain geometry

Grindstones compile separate supports, pivots and wheel for floor, ceiling and
wall mounts in all four directions, including the legacy multiple selector's
floor pose. Each part retains its pack-resolved material and sprite rectangle.
Original-art regressions cover every registered state, carrier roundtrips,
malformed selectors and replacement sprites. Matched headless frames cover all
16 registered states, and real input places a new grindstone through a local
server. Acceptance uses externally compiled carriers with production terrain
consumers; runtime server-pack compilation and reported server-version placement
remain unverified. No broader parity gate closes.


## Redstone dust item sprite

Redstone dust uses the pinned item-atlas sprite for its inventory icon and held
item rather than the placed wire geometry. The reviewed default binding is
checked against the pinned atlas and retail inventory. Original-sprite regressions
cover entity/icon carrier roundtrips and preserve the redstone block's model.
Other geometry reports and broader item-rendering parity remain incomplete.


## Command-block terrain materials

Command blocks select front, back and conditional side materials from their facing
and conditional state, with full opaque boundary faces and oriented arrow UVs.
Original-art compiler regressions cover all registered variants, side directions,
cap orientation, neighbour culling, atlas-bound animation strips and carrier
roundtrips. Local BDS rendered panels, arrows and animation phases pass against
the official Android reference. Render admission remains provisional for broader
pack and platform parity; hardware performance gates stay open. The command-block
editor remains unimplemented.


## Lever terrain geometry

Levers compile separate mounting bases and power-selected pitched handles for
wall, floor and ceiling states, retaining independent pack materials. Original
16/32-pixel fixtures cover all registered mounts, power motion, sprite regions,
face winding, network-ID resolution, malformed selectors and neighboring barrel/
trapdoor controls. The base uses the cobblestone-wall top source independently of
the handle sprite. Owning regressions and the focused crate check pass; the
missing-part regression fails on the previous envelope model.
Rendered/input acceptance, native-server placement and broader parity remain incomplete.


## Bubble column terrain geometry

Bubble columns compile separate outer and inner side shells with directional
sprites and positional variants. The surface sprite appears only below primary
air; water keeps its independent liquid contributor. Original-art regressions
cover texture ownership, carrier groups, malformed direction states, and layered
meshing across sub-chunk boundaries. Matched headless frames with original art
cover both directions and preserve the water layer. A native 1.26.52.3 server
session confirms normal-pack rendering, column motion, and real glass placement
and breaking above the surface. Final bubble tint, native-client frame comparison,
and enhanced-graphics parity remain incomplete; broader rendering gates stay open.


## Sea pickle terrain geometry

Sea pickles compile one to four separate bodies and crossed live tips selected by
the typed count and dead states. Bodies retain their side, top and inset cap sprite regions;
replacement packs supply the sprite, and water remains a separate contributor.
Original-art regressions cover all eight registry states, carrier roundtrips,
material ownership and malformed selectors. Geometry uses the carrier's existing
position and UV precision. Rendered/input acceptance and reported server-version
placement remain incomplete; broader parity gates stay open.


## Transparent water draws at base vertex zero

Unsorted transparent water draws at base vertex zero: each liquid record names its own
chunk origin slot, and the origin carries the sub-chunk's liquid and liquid-lighting
bases. The previous base-vertex encoding drew flat water a block off on Honeykrisp,
which misreports the builtin vertex index under nonzero base vertices.


## Source-water retention during block changes

Primary block changes preserve source water in the secondary layer when the
pinned block property permits it. Server updates and local predictions share
this transition, including ordered explicit secondary writes and water removal.
The block registry derives containment per state from the shared library rather
than an identifier list. Regressions cover both session ID formats, replacement
controls, and malformed updates. Flowing/falling water retention, server-defined
block capabilities, broader liquid parity, and hardware performance gates remain
incomplete.


## Flowing water texels

Replicated flipbooks compile the frame's upper-left `1 / replicate` square at full
resolution, matching the native flipbook blit, which draws a frame `replicate` times the
tile's size from its upper-left corner. Liquid faces then span the whole tile. The
native liquid tessellator's UV extent for rotated flow is unverified: diagonal flow
shrinks until its corners meet the tile edges, an approximation that keeps parity open.


## Embedded-block overlay

Eyes inside scaffolding keep a clear view instead of receiving the solid-block
overlay. Submerged eyes still select the liquid medium before the scaffolding
exception, and ordinary solid blocks retain their overlay. Eligibility for other
nonopaque blocks, scaffolding interior-face rendering, and complete native
screen-effect parity remain incomplete.


## Small-surface render safety

- Explicit render scissors and viewports intersect their physical attachments; empty
  intersections skip drawing. UI pipeline selection follows current resource preparation,
  including retained layers and model depth during the first frame and resize.
- Deterministic bounds witnesses cover oversized layouts, 254×124, 1×1, zero extents and
  differing DPI. Native offscreen validation covers nonzero small attachments.
- The reported unfocused macOS startup panic remains incomplete until a matching
  headless startup capture reproduces it. No native parity or frame-budget gate closes.
- Frame waits read the current focus policy after frame work, so focus restoration
  removes the background limit before that frame waits. Background and occlusion
  limits stay unchanged. Deterministic regression coverage exercises both rendering
  paths; native focus-transition latency and the reported resize lag remain unverified.


## Anvil models

- Intact, chipped and damaged anvils use four stacked pieces with their damage artwork and
  cardinal orientation. The base, rim, stem and head replace the provisional single cuboid.
- Exact texture rotation/crop parity remains incomplete; the models retain provisional support
  until a matching native directional gallery verifies texel orientation. No parity gate closes.


## Authored terrain materials

- Optional authored maps decode texture sets, MER/MERS and explicit LabPBR 1.3.
  Joint material mip generation preserves cutout coverage and filters normals and roughness.
  Local packs can reuse a content-addressed decode cache. GPU uploads retain carrier
  resources and publish complete authored maps after bounded transfers.
- Enhanced remains hard-disabled, so normal sessions do not load these packs or change
  rendered output. Renderer integration and visual/performance acceptance remain incomplete.
- Incomplete: Java LabPBR files pair with Bedrock textures through a provisional name
  table (`crates/pack-compiler/src/pbr/sources/names.rs`, baselined in the architecture gate).
  Replace it with a generated Java/Bedrock mapping shared with shader-pack block IDs.
  Independent authored animation timing remains incomplete; frames use the carrier clock.
  State and positional variants outside the variant-zero source retain carrier artwork
  until authored variant resolution is implemented.


## Frame attribution and unchanged GPU uploads

- With VSync, macOS 14 and later reserve a third native drawable for display-link
  prefetch; older systems and other backends keep one queued frame. The native
  Metal backend delivers drawables through the display link on supported AppKit
  surfaces at the configured latency, follows the hosting screen's refresh rate,
  discards updates past their submission deadline and cancels before
  reconfiguration. Smooth 120 Hz acceptance remains open.

- Metal completion markers feed bounded GPU queue predictions into input admission,
  delaying the next input sample until CPU work can feed the GPU without a frame
  backlog. Completed host timestamps reanchor predictions; missing or stale samples
  retain CPU pacing. Synchronized surfaces skip GPU extrapolation and reset stale
  history on mode changes; display-dependent gaps must not become predicted work.
  Incomplete: paired hardware responsiveness and frame-budget
  acceptance remain open.
- Conservative actor and block-entity occlusion projects each box's angular bounds
  once, retaining complete slab coverage, rounding margins and bounded fail-open
  probes. Geometry tests cover interior holes across axes and translated coordinates.
  Incomplete: hardware frame-time and streaming hitch acceptance remain open.
- Sorted transparency records its contiguous colour-space passes into one command
  buffer, removing worker waits around draws serialized by the phase's shared lock.
  Regression tests cover draw order, submission counts and empty viewports.
- Block-model uploads compare bounded vertex pages and upload disjoint changed
  ranges, retaining complete shifted tails and refilling grown buffers. Bytewise
  comparisons preserve signed zero and NaN payloads. Sparse-update and native
  staging-allocation regressions are written but unrun.
- Empty retained UI pixels preserve scene colour exactly; opaque pixels skip the
  scene read, while translucent pixels retain premultiplied gamma blending. Native
  pixel tests cover colour, alpha and HDR values. Incomplete: fullscreen presentation,
  paired hardware frame-time improvement and the 120 Hz acceptance gate remain open.

- Simulation reads the current admitted frame clock. Deterministic FIFO tests at 60 and
  120 Hz and an uncapped test bound tick flush delay and 20 Hz send jitter to 3 ms;
  manual recording clocks keep their requested steps. Live movement acceptance remains open.

- Actor catch-up uses `simulation_clock::MAX_TICKS_PER_FRAME`. Prediction and live per-tick actor
  advancement share one clock step, including server-defined interaction boxes after a stall.
  Returning actors warm their previous tick in full-cap prefix waves on the existing pool,
  with bounded helper launches, ordered actual charges and a serial partial-budget tail.
  Differential and lifecycle regressions pass in the focused scheduling suite (32 tests).
  Native crowd-turn timing,
  first-visible-frame acceptance and measured gains remain incomplete.
  Deterministic regression coverage does not close live animation or movement acceptance.

- Apple pipelined rendering keeps surface creation on the UI thread while render
  submission stays on the render thread. Main and extraction schedules retain
  single-thread execution; runtime startup checks do not close frame-budget gates.
- Focused synchronized macOS windows request their reported display refresh through
  a window-bound display link, bounded by FPS limits. Background and fixed-clock
  runs release this request. ProMotion presentation cadence and live smoothness
  remain unverified; this does not close the 120 Hz display gate.
- Idle native metadata publication no longer dispatches through the main thread.
  macOS frame workers use interactive scheduling; bounded-load diagnostics show
  more render-work headroom. Unloaded tail results remain adverse and unexplained:
  the hitch and 120 Hz display gates remain incomplete. See
  [frame-pacing evidence](../../docs/evidence/zeqa-frame-pacing.md).
- The opt-in `alloc-stats` feature counts requested allocations and bytes per frame
  and timed CPU stage, with either System or mimalloc. Frame and schedule-span totals
  cover all threads, including concurrent background work; RAII timers include nested work
  on their calling thread. Labelled GPU-write counters
  split retained staging and queue staging, including partial uploads. Tracy plots and
  slow-frame fields report these diagnostics; hardware performance acceptance remains open.
- Opt-in Tracy spans cover Bevy and owned streaming/render work; Metal pass
  durations are delayed plots. macOS zones alone cannot separate preemption from waits.
- Per-packet ingress admission preserves queued events when consumer fan-out fills
  headroom. Bounded lookahead lets light events pass stalled terrain while the shared
  conflict tracker keeps dependent mutations and barriers ordered. Latency echoes flush
  when their control fence clears, preserving earlier movement ordering. Deterministic
  regressions cover these paths; hardware join and transfer latency remain unverified.
- Rolling frame recordings retain recent completed spans and received counters, carry input
  identities into render submission, and queue every hard-hitch window with completeness
  metadata. Slow reports format outside recorder locks on the console thread. Full exports move
  ring storage to a worker and stream events individually. Delayed GPU samples still lack causal
  CPU identities and presentation feedback remains unavailable.
- Named schedule traces and bounded frame recordings separate main work, render
  handoff, drawable acquisition, command submission and presentation. Metal
  timestamp queries use owned render passes and leave uncovered stages absent.
- Ordinary direct terrain draws group retained CPU-selected units into indexed instances,
  retain unchanged section descriptors in stable GPU slots, and upload ordered
  visibility references separately after current occlusion results. Both regions
  share one storage binding. Changed sections update only their descriptor spans;
  streams exceeding that binding's capacity keep complete ordered page draws.
  The per-section fallback and transparency paths remain available. Incomplete: Metal pixel identity and paired hardware captures
  must qualify the change before closing rendering or frame-budget acceptance.
- Unchanged hand and cloud uniforms, inactive portals and empty item scenes skip
  redundant staging work. Regression tests assert allocations, writes and retained
  buffers; hardware captures measure elapsed time separately.
- Hidden clouds skip geometry preparation. Visible cloud windows use bounded worker
  prefetch and retained GPU storage while keeping the existing admission threshold
  and cap selection. Cold views and missed prefetches still wait for exact geometry.
  Incomplete: paired hardware captures and frame-time acceptance remain open.
- UI publication re-emits only the nodes a frame changes into its retained draw list,
  and HUD rebinds run only the view bindings a change reaches. The paper doll and the
  inventory model turn as geometry without rebuilding a texture page. The pack worker
  resolves server HUD screens and reads the pack textures they name, and large session
  icon sets pack on a worker. Regression tests assert redrawn nodes, view runs, and
  on-frame resolves, pack reads and icon packs.
- Incomplete: the first in-world frame after joining (4.9 s), first server form opens
  and chat-open freezes from live sessions did not reproduce offline at that size, and
  large server art still decodes on the frame within its budget. A tree-shape change,
  such as blinking hearts, builds the frame in full, and the first two HUD rebinds after
  a cold bind rebuild most controls. Release-build hardware captures remain open.
- Actor publication retains native skin pixels and unchanged GPU artwork. Bounded worker
  batches prepare custom models while replacements retain the last complete profile.
  Ready appearances publish within a frame budget, with the local player first; normal
  ready joins remain visible in their arrival frame. Distant block-entity rescans rotate
  within their budget, while nearby edits remain immediate.
  Prepared meshes retain validation and vertex fingerprints across catalog publication.
  Deterministic tests cover admission, reuse, stale completions and indexed lookup work;
  see [actor burst evidence](../../docs/evidence/actor-burst-preparation.md).
- Incomplete actor-join acceptance: readiness-gated first appearance remains provisional.
  Models exceeding the preparation mesh budget use the default rig until their source
  changes or the session resets; exact appearance under that limit remains an open gate.
  First GPU uploads, pipeline compilation, UI publication and exact page comparisons
  remain cold costs. Synthetic crowds and headless screenshots do not
  establish native first-appearance timing, join latency, no-pop-in acceptance or release
  frame budgets; those gates remain open.
- Recurring hand, UI viewport, particle, actor, nametag, block-model, shadow, dropped-item,
  atmosphere and animation-clock writes share bounded retained GPU
  staging. Terrain arena writes use a separate pool that starts at three 120 Hz
  frames of the publication target rate and grows after missed frames, at most
  2 MiB per frame, to a 32 MiB ceiling. Abutting records share one copy, batches
  stage what fits and queue only the overflow, and a growth slice encodes staged
  writes before it copies. Allocation regressions cover real Metal owners; lifecycle
  and byte-order tests cover asynchronous reuse, partial staging and fallback.
  Attach-only native samples confirm staging-allocation mutex contention, drawable
  waits and scheduler delays.
- Particle atlas references and per-identifier emitter/particle totals update at mutation
  boundaries. Tile aliases retain their append order with precomputed names. The fixed
  frame staging shape derives its capacity from the particle cap and GPU instance size,
  with room for other frame writes and four frames awaiting completion; unavailable
  mappings and excess writes retain fallback. This is a partial particle bookkeeping
  optimization: front eviction and draw-list reuse are unchanged. Focused regression
  coverage is written but unrun; native burst timings and fallback measurements remain
  incomplete. No particle parity or performance gate is closed by this change.
- Incomplete: one 32 blocks/s flight capture sized the terrain pool's growth; its
  staged share after growth, the 128 blocks/s route and joins remain unmeasured.
- Incomplete: the synthetic fixed-camera fixture is not the populated-lobby/flight
  release replay; hidden frames do not establish displayed FPS. Three paired runs
  at each resolution reduce measured maxima but do not consistently improve p99.
  A separate pooled-upload run still exceeds 100 ms with no staging fallback.
  Drawable/present dependencies, remaining lock owners, retirement costs and
  complete long-stall attribution remain open. Shared-pass GPU categories, exact
  per-item costs and native GPU regression validation also remain unqualified.
  See [pooled-upload evidence](../../docs/evidence/render-stall-pooling.md),
  [frame breakdown evidence](../../docs/evidence/frame-breakdown.md) and
  [Tracy attribution](../../docs/evidence/frame-breakdown-tracy.md).
- Block entities whose reach lies only in sections the current cave result never reached
  skip model and text preparation, under the actor camera and graph validity rule; lids,
  bell rings and map requests still advance, and beacon beams are exempt. Regression tests
  count prepared candidates and bound authored model reach. Incomplete: no hardware capture
  yet measures the gain.
- Nametag atlas admission counts misses and reserves cells before rasterizing. Pressure
  reclaims unused shelf cells in place; remaining misses get a bounded fresh packing
  attempt at the next frame boundary. Selection retention checks each cached actor
  identity once. The overall nametag performance package remains
  partial: this covers its atlas and selection portion. Regression tests are unrun;
  paired captures, pixel acceptance and hardware frame-time measurements remain open.


## Always-passing entity depth materials

- Server-pack entity materials with `depthFunc: Always` (through-wall markers)
  compile to an always-passing depth test. `LessEqual` restores normal testing,
  and children inherit the setting. These spans draw in the sorted pass after
  opaque terrain, so terrain cannot overwrite them. Every admitted raster state
  prewarms both normal and always-passing depth variants, including blending and
  disabled depth writes.
- Sorted instances retain separate positions. A sorted dissolve mask carries its
  own color layer into one draw item in that pass, so another actor cannot split
  the dependent passes. Ordinary pairs finish before transparent terrain.
  An explicit always-passing test overrides the color pass's default.
  Rigs with always-passing depth layers bypass terrain occlusion while retaining
  the candidate distance and frustum bounds. Admission checks the same sampled
  frame layers that the draw consumes, including frame-conditioned controllers.
  A capability cached from all compiled layers keeps ordinary occluded rigs from
  consuming the shared frame-evaluation budget.
- Incomplete: `depthBias`, other depth functions, `InvertCulling` and stencil
  states are ignored. The sorted-pass order approximates native entity order and
  is unverified against a native capture.


## Worn server-pack armour textures

- Item bindings retain owner-specific attachables and evaluate their item conditions
  against the wearer. A stand-only chestplate or pair of leggings leaves players
  using their own definition. Custom armour bones retain their authored pose, and
  controller-selected geometry and visibility reach the worn draw. Skin visibility
  masks are applied after equipment attachment so hiding a torso does not hide its armour.
- Worn armour from server packs runs its attachable scripts with the wearer as
  `context.owning_entity` and draws the texture its render controller selects,
  such as owner-driven team variants. Each worn slot keeps its own retained state
  and native model binding name. Owner item-use timing and frame delta are passed
  to the worn scripts. Equipment, properties and local-player identity also
  remain available to owner queries when the body pose is static.
  Attachable timing is independent of owner tick batching, and elytra scripts
  receive the actual render delta.
- Cape composition retains the chest route used by the equipment draw. First-person
  hand placement and drawing share one admitted definition, and preview atlas fallback
  retains its startup choice and category. Predicate cache warming cannot change a
  routing decision within those compositions. The routing table includes budget
  exhaustion and armour-stand helmet and chest slots.
- Incomplete: matching body bones still use the owner's pose without the attachable's
  own animation channels. Worn bone-binding expressions, multiple armour material
  groups, trim artwork, modern controller materials, and precedence among overlapping
  owner mappings remain incomplete. Native humanoid models absent from the
  resource pack still use the existing static player-model fallback.


## Distant grass sides

- Grass sides' tint-mask tiles take vanilla's atlas mips (byte-space box averages of the
  original tile), so the dirt stored under alpha zero no longer turns black at distance.
- The user confirmed distant grass sides in a live macOS build.
- Incomplete: a server pack that replaces an overlay-masked texture still gets the runtime block
  overlay's alpha-weighted mips.


## Entity shadows

- Entity facts are compiled by `registryc` into a hash-checked carrier and installed by
  `make assets`. Actor admission caches the advertised native base for shadows, offsets,
  mount classification and native effects; unknown bases remain unresolved.
- Incomplete parity: compatibility interaction, placement, equipment and native class rules
  remain provisional. Chest-boat network origin and complete living-mount coverage remain
  unverified. The pinned actor-name inventory still comes from sample metadata.

- Vanilla blob shadows: a 13-sided volume under each caster darkens the opaque surface inside it
  by the encoded-colour multiplier (0.7 grey with a sky tint), once however many overlap. One
  instanced draw after opaque geometry; casters and parameters upload only when they change.
- Caster rules (radius table, babies, slimes, projectiles, burning, invisible, dead, submerged,
  riders, ghast drops) follow [the vanilla rules](../../docs/reference/entity-shadows.md).
- Width-based radii share collision scaling and its minimum size, including player fallback bounds.
- Rigged casters follow the actor frame's drawn bodies. Incomplete parity: sign shadows are not
  drawn; the breathing point, item and local volume culling remain provisional.
- Volumes containing the camera draw no shadow. Camera-inside and ordinary outside
  footprints have GPU regression coverage. Exact near-plane clipping and depth-bias
  edges remain incomplete parity checks.


## Terrain occlusion on direct-draw devices

- Metal (any direct-draw device with compute) never draws indirectly. While the view holds
  still and a verdict could change, solid terrain draws in its own pass ahead of other opaque
  draws; a Hi-Z of that depth tests every resident slot and the occluded bits come back
  through a small readback ring that drops frames rather than wait. Opaque streams of a slot
  are skipped once two consecutive verdicts agree under the same eye, orientation,
  projection, viewport, depth size and geometry; any translation or turn (the near plane swings and can
  clip a near occluder), sub-chunk removal or replacement, cave or tint change voids them.
  `CINNABAR_CPU_CULLING=1` turns it off.
- Offscreen tests: kernel bits match a CPU Hi-Z reference; replayed paths (flick, strafe past
  a pillar, wall edit, a turn that clips a wall just beyond the near plane) never skip a
  sub-chunk that shows pixels while a stale-verdict policy does; the walled scene stops
  submitting what it hides.
- Incomplete live visual acceptance: an in-game pass on the M3 Pro (no pop-in, no wrongly
  hidden terrain) and live `gpu_opaque` and render CPU stage captures are pending.

- An opt-in same-frame path (`CINNABAR_TERRAIN_VISIBILITY=1`) draws a bounded near-first
  solid depth subset, builds conservative Hi-Z verdicts, and writes unit visibility flags
  on the GPU. Ordinary color draws keep their original order; strict depth rejection keeps
  ties and current-frame depth handles disocclusion. Missing depth or pipelines clears the
  flags. Metal indirect draws remain disabled. Native raster fixtures cover page tails,
  camera changes and one-sample/2× MSAA output equality. Performance and presentation
  acceptance remain incomplete.
- Opt-in Metal admission (`CINNABAR_TERRAIN_VISIBILITY=adaptive`) probes current-frame
  visibility for views with at least 64 candidates. An inactive view probes once per 64 frames and remains active while completed feedback
  hides at least half its candidates. Settled still views keep the cheaper readback path.
  Feedback only chooses whether to do fresh work;
  current depth supplies every visibility mask, including during motion. Falling back
  clears masks before direct draws. The default and `CINNABAR_TERRAIN_VISIBILITY=0` keep
  the legacy path; `1` forces the current-frame path. The admission thresholds are
  provisional and do not close the hardware performance gate.


## Video quality and repeated renderer work

- Smooth Lighting now chooses corner interpolation/AO or one light sample per face.
  Transparent Leaves now chooses exposed cutout leaves or opaque leaves with internal
  shared faces removed. Runtime changes rebuild resident meshes, cancel old work and
  invalidate resource-pack snapshots even after an A/B/A toggle. Existing defaults keep
  full-quality rendering.
- Native lightmap bytes are prepared once per upload. Sky fan lookup uses a bounded
  search while preserving rim-edge rounding and the existing draw order. Uniform sky
  colour conversions run at vertices. Actor readiness polls retained unique pipelines, and retained UI
  layers reuse composite bindings across both scene ping-pong views.
- SMAA writes its filtered result directly into every shared scene sample, removing the
  restoration copy or draw. Depth edge detection rejects pixels below both edge thresholds
  before reading the slope neighbours. Deferred nametags do not split otherwise compatible
  transparent passes; their sharp text draws share one later pass in the same order.
  The main scene transfer still preserves encoded material blending and post-processing
  colour ownership. Native regression fixtures compare direct and restored LDR/HDR colour
  with partial-sample overlays. World-projected UI shares compatible depth attachments across
  material modes, with unchanged authored order and a separate-pass fallback while pipelines
  compile. Native comparisons preserve LDR/HDR pixels at one and four samples.
- Incomplete: these bounded native fixtures and work-count checks do not establish the
  MacBook Neo frame-rate gain or close the live frame, join, streaming or settings parity
  gates. Simple Graphics precipitation and stock Fancy Skies behavior remain unverified.


## Bounded lighting CPU work

- Retained-support early exits and section packing preserve light/provenance output;
  [remote CPU evidence](../../docs/evidence/lighting-cpu-work.md) records workload, allocation
  and queue measurements alongside independent regression oracles.
- Incomplete: these synthetic solver diagnostics do not close native vanilla parity,
  release frame, join or streaming gates; target-hardware captures remain required.


## GPU terrain culling with Hi-Z occlusion

- On Vulkan/DX12 with native multi-draw-indirect-count, opaque terrain (solid runs, cutout,
  models, depth-writing liquid) is culled by a compute pass over persistent per-slot records:
  frustum, cave visibility, facing runs and a two-phase depth-pyramid test, drawn from
  compacted slot-ordered args. Metal reads occlusion back instead (above); GL and probe
  frames keep the CPU path.
- Offscreen GPU tests match Bevy's visible sets, the CPU reference args, a conservative Hi-Z
  against rendered ids, and the CPU path's pixels from a stale history.
- Live visual pass: on Vulkan and DX12, GPU-culled and CPU-planned frames at the aeris.land
  spawn match at four headings apart from moving entities, nametags and chat. GPU pass times
  come from `CINNABAR_GPU_NODES=1` (below).


## Render frame time

- Culled terrain commands carry no builtin-visible offsets: each command's base vertex
  addresses its own entries in a draw-offset vertex buffer the cull kernel writes, so DX12 uses
  count draws like Vulkan. The fixed-count path, which submitted every slot's worst case and
  had wgpu validate and patch each command, is now the fallback for devices without count
  draws. The GPU test draws both submissions pixel-identical to the CPU path on DX12 and
  Vulkan.
- CPU-planned draws bind the arena's identity offset buffer, so both paths share one pipeline
  per terrain family. DX12 compiles shaders with FXC at every launch, about 0.5 to 1.7 s per
  pipeline here, so a culled variant per family would lengthen the loading screen.
- The Hi-Z pyramid builds only texels that cover a depth pixel, `ceil(depth / 2^(level + 1))`
  per axis, passed per level in a bounds uniform; the cull reads no other texel.
- The cull kernels skip wgpu's workgroup zero-fill. Each kernel writes its workgroup slots
  before reading them, and the fill made FXC spend about 7.5 s per kernel, which held DX12
  launch-to-HUD at about 23 s; it is now about 4 s.
- The opaque, late-cull and transparent passes record and encode on worker tasks; each
  transparent colour-space range is its own task.
- Sorted and direct water and model draws share one transparent pipeline
  (`transparent_terrain.wgsl`, composing the liquid and model modules over
  `chunk_bindings.wgsl`); every water draw's first instance carries
  `TRANSPARENT_WATER_DRAW_FLAG`, and `liquid_draw_ref` tells sorted refs from direct records.
  On an RX 6600 XT the program switch between the two families cost more than the drawing. An
  offscreen GPU test matches the shared pipeline's pixels, for sorted refs and direct records,
  against both programs through their own pipelines.
- `CINNABAR_GPU_NODES=1` plots per-node and per-section GPU times under Tracy, and Tracy
  builds expose wgpu's encode and submit zones; see the live-testing guide.
- Metal reports `gpu_frame` from the GPU host times of two empty marker command buffers that
  open and close the render graph's submission, read back without waiting and counted against
  the GPU budget. It runs from the later of submission and the end of earlier queued work to
  the end of the frame's work; submissions outside the graph are not included.
  `gpu_frame_completion_latency` stays a separate informational CPU interval that never counts
  towards slow frames. Incomplete: shared per-draw categories, because Apple GPUs sample
  timestamps only at pass boundaries.
- Evidence: [render frame time](../../docs/evidence/render-frame-time.md), measured on DX12 before
  Windows defaulted to Vulkan; `WGPU_BACKEND=dx12` selects that backend.


## Solid terrain back-face culling

- Single-sided opaque cube quads draw in per-face runs with back-face culling and no
  fragment discard; only runs that can face the camera are submitted. Alpha-tested and
  two-sided quads keep the cull-none discard pipeline.
- An offscreen GPU test matches the culled path pixel-for-pixel against the discard path.
- Incomplete live visual acceptance: a rendered-frame pass on the target platforms is pending.


## Transparent water order across camera turns

- The water sort covers every resident sub-chunk whose faces need an order, not the
  frustum's, so turning never re-sorts; the frustum only picks which sorted groups draw.
- Water whose faces share one plane and facing, one cell each (flat oceans and lakes),
  blends the same in any order and draws from its records through the sorted path's
  vertex program. Views that displace water (Enhanced) still sort it.
- A staged sort that is still readable commits before the next request, so streaming
  that changes residents every frame cannot starve it.
- Each sorted sub-chunk owns a range of the committed slot. A new, changed or removed
  sub-chunk patches only its own range, so one new shore uploads only its refs; the slot
  is repacked only when fragmented. Removed water is released once no committed or
  staged snapshot reads it.
- The resident index keeps running ref totals, and the sort inputs are rebuilt only for
  residents that changed. A pack reload seeds them, sorted for the view's water mode.
- Visible water keeps last frame's key order and merges in only new entries; a key
  visible under two entities draws the upload the resident index holds.
- Past the 2,097,152-ref ceiling the nearest water is sorted. Farther water that needs an
  order, and water awaiting its first sort, draws unsorted and is logged, never skipped.
- Regression tests cover a turn, a rotation sweep, resident churn, streaming, the ceiling
  through the live prepare step, and an ocean past the ceiling. An offscreen GPU test
  matches flat water drawn from its records pixel-for-pixel against any sorted order.
- Incomplete: live RD 255 acceptance is pending; order-dependent water beyond the ceiling
  draws unsorted; transparent models still sort only the visible set.


## Held sprite edges

- Held sprites exclude alpha 0 and 1 from their edge mask, preserving side faces next to
  nearly transparent pack pixels. Dropped sprites share this mask without copying the raster.
- Mesh and placement fixtures cover 16, 32, 64 and 128 pixel sprites, holes, partial alpha,
  atlas texel centers, fixed thickness and both animation modes in first and third person.
- Incomplete: matched in-game pack comparisons, authored sword display transforms and
  first-person face-shading acceptance remain open.


## Scaled name tags

- Name-tag offsets (box height plus head clearance) follow the actor's Scale, a published zero
  box height counts, and a falling block's network offset follows its scaled box, so
  falling-block text hosts sit where their server puts them. Provisional: the user's vanilla
  observation and Dragonfly's zero-offset text entity are the witnesses, not a client trace.


## Flower-pot floor and lily-pad atlas tint

- Pot contents retain their block-state selectors and use separate terrain-textured models.
- Flowers, saplings and roots use unextruded planes; cactus, bamboo and azalea retain their
  separate parts and pot-specific sprite regions. Ordinary terrain models remain unchanged.
- Incomplete: mangrove-propagule geometry, eyeblossom geometry/emission and biome tint need separate
  acceptance; native-server placement parity and the full pot body remain open.
- Flower pots now add the dirt surface four pixels above the block base, below the rim.
- Pot bodies and lantern surfaces use double-sided alpha testing instead of fallback blending and rear-face culling.
- The pot body remains provisional fallback geometry; full current-version visual parity is incomplete.
- Literal lily-pad atlas tint survives raster-only resource-pack replacement. A higher catalog can replace or clear it.
- The current vanilla pack supplies a fixed green atlas multiplier, rather than biome tint.
- Regression tests reproduce the missing floor and ignored pack tint. Installed Windows visual acceptance is pending.


## Terrain particle texture repair (2026-10-01)

Incomplete parity work on `fix/break-particles`: particle level events need the
same wire-to-internal block palette remap as chunk data. The ordinary destruction
texture comes from the resolved down face; biome tint is a separate block policy.
Pack definition: `particles/block_destruct.json`.

The exact particle parity gate stays open for destruction texture/count overrides,
weighted texture variations, non-cube crack AABBs, mining hit cadence, seasonal tint
and native ambient lighting. The October 4 correction below replaces the separate
particle brightness approximation with the native RGB lightmap composition.
Landing and sprint dust are not wired by the current particle adapter. Rain splash
uses the static particle sprite sheet, as the pinned `particles/rain_splash.json`
defines. Offline tests or previews do not close the target-platform visual gate;
no live server connection is authorized for this work.

### Dark item/particle correction (2026-10-04, live accepted)

User-authorized offline BDS testing exposed an extra sRGB encoding of dropped-item
lighting and a separate scalar particle brightness floor. Items now compose gamma
texture/tint/overlay and the native byte-quantized `/16` RGB lookup before the final
Bevy linear-output conversion. Lit particles consume that same world lightmap;
unlit effects keep their bypass. Sources and remaining item shade/AABB, particle
solid-neighbor and fog boundaries are recorded in
[item-particle-lighting.md](../../docs/reference/item-particle-lighting.md).

The user accepted dropped items, particles and survival mining on macOS/Metal,
Retina 2× with the rebuilt client. During the earlier test, survival was
incorrectly taking the creative mining route because Instabuild overrode game mode.
Vanilla selects creative destruction from the creative game mode, not that
ability; the narrow capability correction and transition regression are recorded in
[game-mode-updates.md](../../docs/reference/game-mode-updates.md). This does not close
broader ability-layer refresh or historical replay parity. The focused particle
tests (59), client-world tests (198), and native GPU color regression (60 draws in
one test) passed. The user explicitly requested stopping the queued verification,
skipping further checks and pushing directly to remote `dev`; the full pre-push
gate and PR/CI merge gate were waived, not completed. Complete item/particle
parity remains open.

### Zeqa correction audit (2026-10-01, incomplete)

The October 1 trace contains 34 committed corrections. The audit in
`docs/evidence/2026-10-01-zeqa-movement.md` compares each authoritative position
with the originally transmitted input, not a prediction already changed by replay.
These fixes follow current vanilla behaviour: player corrections preserve look;
PosDelta carries end-of-tick velocity; zero-stamped SetActorMotion changes live
velocity without a replay overlay; Jumping follows held processed input and
StartJumping follows actual initiation; keyboard raw diagonal movement is normalized and analogue axes stay zero;
MovePlayer teleports acknowledge without an opt-in; teleport snaps preserve raw
button history and jump cooldown; player collision boxes retain their full width.
Latency replies preserve native flags and timestamp conversion and follow committed
motion through the outbound FIFO. Tagged motion remains on the replay timeline.

This does not close movement parity. The capture omits collision volumes/revisions,
most inbound correction velocity/ground fields, and some motion events. The first
burst's floor-contact discrepancy and the last burst's exact replay failure need a
fresh capture. Future/missing correction-frame behavior remains unverified; existing fallback snaps, collision identity policy and teleport
expiry remain provisional. The simulator still uses its existing f64 arithmetic.
No live server connection was made. Use CINNABAR_MOVEMENT_TRACE=1 for outbound PAI
and the new unthrottled inbound movement and latency-fence records. Normal MovePlayer acknowledgement
no longer needs CINNABAR_TELEPORT_ACK; that opt-in still enables unverified extra routes.


# Optional cloud texture safety

Optional resource-pack cloud masks outside the current mesher's fixed dimensions
retain the startup cloud texture instead of panicking during live application.
Other supported pack textures still apply. High-resolution pack clouds remain
incomplete; this fallback does not close the native cloud parity gate.

### Ordinary water rendering continuation (incomplete parity acceptance)

Current-client liquid tessellation does not use terrain ambient occlusion;
its side and bottom faces repeat one outward light sample. The top still uses our
existing sample admission and maximum until vanilla’s independent brightness-admission property can be carried: native smooth top lighting rounds
four samples from the above plane, not terrain's maximum or solid-render gate.

Ordinary transparent alpha distance now derives independently of profile fog and
cloud fade from the current camera and uniform producers.
The above-water formula uses the builder's adjusted render-distance input. Native
optional platform-cap admission and the underwater/no-FrameBuilder branch remain
incomplete; these changes do not close the overall water visual parity gate.

Ordinary blended terrain now preserves the native gamma/UNORM framebuffer blend
(RendererSettings, format mapping 0x57, RenderChunk Transparent Metal)
without reordering the shared transparent phase. Native liquid inward winding and
selective reverse-face admission follow vanilla: original
faces use CW, exposed tops and primary-air sides admit the flagged reverse face,
and bottoms do not. Production GPU geometry, material and six-face raster tests
pass. Mixed terrain's segment cap now uses the existing bounded reference budget
instead of falling back at 4,096 segments in an ordinary ocean view.

Live user checks accept flowing water over ice and its previous flicker fix.
Nighttime ocean visibility is reported correct; daytime submerged scenery is
still too dark. Both clients use 50% brightness. The visual gate remains open
while tracing native client skylight mode/heightmaps and submerged receiver light;
no speculative global opacity or brightness adjustment closes this gate.

The follow-up traced the actual current WATER draw, not just camera fields:
MeshContext.x is zero, and the uploaded
FogAndDistanceControl.w comes from the camera distance scalar minus seven.
Confirmed chunk radius is stored on the native Player with one extra chunk; after the native camera margin, a confirmed ten-chunk radius uses
160 blocks, not 144. Classic water side/bottom contacts are suppressed whenever
the neighbour's primary block is non-Air, including non-solid plants
and transparent cubes. Focused camera, contact and liquid raster tests pass.

Daytime-depth investigation found a separate registry mismatch in the final
native registrations, which override constructor defaults. Current concrete
BaseGameVersion >= native compatibility gate 1.21.130 sets still water's filter
to one, while flowing water stays at two. Ice and
all frosted ages finish at three, not their constructor zero.
The current light projection and rebuilt local world carrier now use those
values. Shipped-carrier regressions verify every water depth/falling state and
the full eight-deep ocean column: still water retains sky seven at the floor's
outward sample instead of zero. Ordinary Fancy still seeds from the normal
water-including heightmap; no shader brightness workaround
was added. The Go and Rust focused tests pass, and the user has accepted the
rebuilt daytime ocean visibility. The live StartGame version is `*`: current
parser marks byte seven as wildcard, and final registration
jumps directly to filter one for that wildcard. Only concrete older versions
retain filter two; dynamic compatibility selection for those remains incomplete.
Ice/water edge appearance and lily-pad rendering are new open visual gates;
the accepted daylight lighting values remain unchanged while tracing them.

### Ocean rendering checkpoint (2026-10-04; incomplete ice acceptance)

The user has accepted lily pads in the live macOS Metal ocean world. Current
native tessellation supplies two opposite planes at 1/64 block, with
the position-hashed rotation and pack-authored fixed tint retained. The copied
material does not recolour shared atlas images; the underside applies native
15/255 shading continuously in the shader. Focused compiler, meshing, shader
and Metal rotation tests pass. The rebuilt carrier and canonical Rust client
were exercised on the original BDS seed -7289507175626565880 on UDP 19132.

Native transparent cubes now reverse U on opposing faces and V on the bottom. Native transparent sorting packs the emitted-vertex centroid in
chunk-local space to ten bits at 1/32-block precision.
The ordering metric now reproduces that packing without altering geometry.
Four regressions failed before the change and all eight focused metric tests
pass afterward. This is a vanilla ordering correction, not evidence
that the user's ice-edge artifact is resolved.

The latest paired live screenshots still show extra bright upright ice faces
through the foreground ice next to the ocean. A separate user witness shows
angle-dependent dark bands on opaque blocks at straight-on views. Both remain
open and under investigation; ordinary ice opacity, accepted water lighting,
and accepted lily-pad appearance have not been adjusted to conceal them.
The user explicitly requests publishing this checkpoint directly to dev before
continuing those fixes. This checkpoint does not close full rendering parity.

### Fish visibility and animation correction (2026-10-04; incomplete full parity)

Native Cube setup offsets the drawable fin UV rectangles into the
texture even when their unused unfolded box origin is negative. Actor artwork
admission now accounts for the collapsed axis and cube/bone inflation, allowing
cod, salmon, pufferfish and both tropical body geometries into the carrier.
Native tropical variable update supplies the Base/Pattern selection
from streamed Int variant metadata before pack evaluation. Behavior and
remaining limits are in [fish-rendering.md](../../docs/reference/fish-rendering.md).

The shared liquid probe now samples native shrunken body bounds against material
cells, preserving the last valid sample when world data is unavailable. This
removes false dry transitions that triggered a 90-degree land-flop roll in water.
Native fish tick and variable updater now supply the retained
current/previous phase that the authored swimming body/tail channels require.
Spawn/motion vectors and interpolation follow the native velocity
path, separate from displacement-derived movement queries.

UV, pinned carrier, tropical selection, body-probe and fish-phase regressions
pass. The complete client-world suite passed with 214 tests and eight ignored;
the separate pinned pose check passed for all four fish families. The production
client built successfully, and the user confirmed that fish look correct in the
fresh macOS Metal client with ordinary controls and vanilla rendering.
At the user's request, remaining verification and tests after integrating latest
dev are skipped, and this correction is published directly to dev.
Tropical two-sampler color composition, fractional-alpha pattern art, native
intermediate-frame query sampling and full animation/material parity remain
incomplete; this does not close those gates.

### Placed player skull lighting (Zeno visual acceptance)

Vanilla's skull renderer reads light at the placed skull's integer
BlockPos, samples the RGB lightmap at /16 coordinates,
and submits the `mob_head.skinning` material. Player skulls now retain those
coordinates, emit rotated world normals for both layers, and use the shared
actor lighting and gamma-domain composition. This replaces the scalar terrain
light/face coefficients that could turn the skull completely black.
Vanilla rules and the focused mesh/Metal witnesses are recorded in
`docs/reference/player-skull-lighting.md`.

The first corrected live build still failed the user's check. Further investigation
also found that the existing carrier rendered current head names as terrain
fallback models, and description required the legacy SkullType NBT instead of
selecting by the backing block.
Current head identities now share one mapping, compile without terrain geometry,
and select their block-actor model even with missing/stale SkullType.

Zeno's affected heads are custom blocks. Their network light component is the
compound `{lightLevel: 0}`. The old decoder discarded that zero and the overlay
defaulted to filter 15, removing the skylight sampled by their inset geometry.
The native decoder uses the compound field. Dampening now
retains `lightLevel`; emission retains its distinct `emission` field, with
network-NBT and explicit-zero overlay regressions.

The user confirmed that skulls render perfectly on `zenomc.org:19132` Zeno
Practice in the freshly rebuilt Rust client on macOS/Metal with ordinary
controls. The initially selected `:19197` external BDS and freecam run do not
count as acceptance. Seven focused Metal shader/readback tests
passed, including day/night/torch/darkness, rotation, alpha cutoff and the legacy
scalar path. The final client build passed. The user explicitly requested stopping
all task background processes, skipping the remaining verification and pushing
directly to remote dev; the affected gate and new unit regressions were not run.
Native hat geometry, other block-entity materials and full version-matched
rendering parity remain open gates.


## Worn elytra glint acceptance

Actor glint uses the vanilla raster, two centered UV rotations and independently
wrapping scrolls. Exact 1.26.50 glint pixel comparison and enhanced graphics glint
remain incomplete and do not close the rendering parity gate.


## Entity-only held item geometry

Block items without a cube sheet retain their compiled icon in both player-preview
hands, matching the existing world equipment fallback. Inventory banners retain their
colored model icon. Exact native 3D held-banner geometry and patterns remain incomplete;
the fallback availability regression is fixed, but it does not close that parity gate.


## Image clarity and multisampling

Desktop rendering removes FXAA and defaults to two MSAA coverage samples when supported,
matching the inspected Windows raster setting. Retail, mobile and console defaults remain
unverified. Video's Anti-Aliasing slider exposes only supported sample counts.
Terrain uses point-filtered texels, linear mip interpolation and byte-space atlas mips.
See [the Vanilla rules](../../docs/reference/rendering.md) for established behavior and evidence
limits. These changes do not close the cross-platform rendering parity gate.

Opaque, cutout, transparent, sky, world text and hand geometry now retain their original color
samples in one shared attachment. Compatible views preserve encoded blending without scene
copies. The last ordinary hand-rig draw resolves and discards the samples; an end-of-main-pass
resolve covers views without that draw. Shadows use per-sample depth and an overlap stencil,
and Hi-Z directly reduces the original depth samples. Single-sample post consumers resolve
only when needed. Real GPU coverage fixtures distinguish this path from resolved-color
reconstruction, including partially covered silhouettes and overlapping shadows.

Enhanced remains disabled by its existing GPU-fault kill switch. Its MSAA attachment and
post-chain changes receive compile and shader checks only; no live Enhanced acceptance is
claimed, and this work does not remove that switch.

Matched native screenshots, material-specific actor/item mip policies, custom-pack mip-level
limits, console/mobile defaults and texel anti-aliasing remain unverified. Mac headless
captures and focused GPU tests cannot qualify Intel-integrated performance, other native
platforms, release streaming or hitch budgets. Those acceptance gates remain open.


## Device render-distance defaults

Ordinary Video settings now choose their default from installed RAM and the
selected GPU's dedicated memory. Explicit saved values win, and Video reset
restores the device recommendation. Slider stops start at 5 and extend in
one-chunk steps to the owner-chosen 255 maximum. See
[the rules table](../../docs/reference/render-distance-default.md).

Incomplete parity: experimental low-memory overrides, advanced graphics presets,
and native VRAM probes on unsupported backends are not verified. These remain
open and do not close a parity gate.

Video settings offer owner-requested Performance, Balanced (the default), and
Quality presets. Balanced restores existing defaults, including the device
render-distance recommendation. The selection is derived from current values;
manual graphics edits show Custom, and personal and accessibility options are
preserved. These presets are a custom feature and do not close a parity gate.
CI verification and owner visual acceptance remain pending.

### Authored particle lifecycle fields

The core block-membership, expiration-expression, and optional parametric-field
contracts have been checked for exact namespaced identifiers. Matched headless
GPU fixtures cover expiry, direction, and an unchanged control. Alias and unresolved
authored-name handling, end-to-end world-query binding, and emitter-frame integration
remain incomplete. These fixtures do not close the full parity gate.

### Crack surfaces after resource reloads

Crack surfaces now follow the admitted runtime asset identity as well as block
identity, transform, and column displacement. Resource reload replaces cached
surfaces in the current session; unchanged assets keep shared geometry.

### Creative search by displayed name

Creative search uses the same component and localized item-name projection as
item presentation. Retained matches follow component and language replacements
as well as catalog, registry, tab, and query changes. Unchanged filters keep their
allocation-free retained index view.

Incomplete parity: formatted-name cleanup, Unicode case folding and alternate
name search modes remain unverified. Display-name projection and cache freshness
do not close those search acceptance gates.

### Recent status effects

Status-effect retention and HUD icon/row lookup now share a versioned descriptor
registry. The pinned pack's effects through ID 37 are covered, including the
nonpersistent effects without HUD icons. A hash-checked independent effect
inventory checks descriptor coverage when the named local fixture is available.
The required HUD carrier includes the added icons; rebuild it with `make assets`.

### Compiled equipment dye capability

Compiled equipment dye capabilities remain incomplete for material contracts the compiler cannot
resolve. The equipment carrier stores Dye, NoMask or Unresolved; unknown parent chains no longer
acquire dye from a material name. The existing undyed colour and legacy armour material defaults
are retained. Complete native shader coverage and session item-component dye overrides remain
open; this descriptor change does not close those parity gates.

### Compiled ambient material facts

The world carrier retains solid/liquid material dispositions for every state. Ambient leaf sampling reads these facts without identifier lookup or geometry inference. The partial plant catalog is joined to pinned state identities; unknown states and custom overlays remain unresolved. Air admission is explicitly provisional.

Incomplete: the complete native material/state inventory, air binding, liquid and solid classifications, legacy protocols and server material component overlays remain unverified. This partial catalog does not close the material parity gate.

### Block-family routing coverage

Crossed-plane routing now comes from the shared family catalog and is retained
in current registry records. Historical records resolve that catalog at
admission. Broader native material and movement descriptors remain incomplete;
this routing cleanup does not close their parity gates.

### Session block highlights

Mod block identifiers resolve to every admitted state in the active session, in
sequential and hashed ID modes. Replacing the custom palette or starting another
session invalidates highlight discovery. Unresolved identifiers are reported when
that mapping is refreshed. This closes the custom-name lookup gap; wider selection
shape and frame performance acceptance remain incomplete.

### Typed Molang query contracts (incomplete query behavior)

Admitted query symbols bind to typed handlers before evaluation. The query manifest owns
argument forms, output kinds and implementation support. Queries marked unimplemented
retain their explicit idle behavior; provisional queries still need matched behavior
verification. Typed dispatch does not close the actor animation parity gate.


## Inherited block overlay geometry

Block overlays use the shared pack compiler to resolve legacy geometry inheritance before projecting faces. Parents may come from other model files or pack layers; missing parents, cycles and chains beyond the depth limit are rejected. Budget-limited reads admit higher-priority definitions first.

Incomplete parity: bone mirror, inflation and visibility projection, modern geometry composition and broader block geometry behavior remain open.


## Hollow terrain containers

Composters retain inward walls and a level-dependent floor. Cauldrons retain their rim, inner floor, walls and feet, with separate water, lava and powder-snow surfaces at each fill level. These models do not occlude full neighbor faces.

Incomplete parity: cauldron block-actor potion and dyed-water colours require runtime block-actor data; their state-only visual remains provisional. Enhanced lighting and performance acceptance remain open.

Compiled pack and admitted server use modifiers share one simulation-clock
duration conversion: seconds use component float precision and
discard fractional ticks. Equipment carriers require a rebuild for this change.
Use-kind, appetite, movement and cooldown admission is compiled against an
independent item inventory; its complete native coverage remains incomplete. Component-version precedence and malformed or extreme-duration
parity remain unverified.

The simulation clock values and finite component-duration conversion share the
`simulation-clock` crate. Protocol consumers no longer depend on the world model
for timing. Complete native use-kind, appetite, movement and cooldown coverage
remains incomplete.

Mount equipment admission is incomplete. Each compiled entity symbol carries a
typed unresolved, empty, supported or provisional equipment disposition. The
existing eleven mount layouts are retained as provisional facts in one
hash-checked source; they do not prove active component groups or the full native
inventory. Actor admission caches these facts, including unresolved pack
replacements. Unknown mounts no longer inherit horse equipment cells in JSON-UI,
fallback drawing, pointer handling and quick-move routing. Active equippable
components, accepted items, conditional activation, advertised custom bases and
later mount layouts remain incomplete and do not close the entity parity gate.
Entity carrier 23 requires a `make assets` rebuild.

### Compiled furnace fuel admission (incomplete)

Furnace quick-move fuel decisions use numeric registry facts and validated compiled inputs.
Retained retail compatibility eligibility and log ingredient preference remain provisional;
the complete native fuel inventory and durations are unverified. Missing, malformed and
unsupported component declarations stay unresolved. Unknown and explicitly nonfuel items
are deferred to the player inventory rather than guessed into a furnace cell. Committed furnace
recipe inputs are bound by numeric IDs, tags and metadata during admission.
Session duration conversion and complete native recipe precedence remain unverified.
Other container slot rules remain incomplete; this change
does not close those gates.

Block destroy facts are compiled into world carrier 15 and read by numeric runtime
identity during mining. Every input state is checked against the independent pinned
registry; changed or custom identities remain unresolved. The existing hardness and
tool source is retained as explicitly provisional prediction data. Full native tool,
state-dependent hardness, legacy protocol and session destroy-component coverage is
incomplete; this does not close the complete destroy parity gate. `make assets`
regenerates the carrier and stale envelopes fail admission. Held-item tool and
creative-break classification still use item names and need separate admission
facts before the complete mining path can be considered name-free.

Movement descriptor coverage remains incomplete. The physics carrier records a reviewed
ordinary, special or unresolved disposition for every admitted state. The movement census in
the registry policy is bound to the digest of the identifier set it reviewed, so a new
identifier fails generation until it is classified. Doors, trapdoors and fence gates (one-way
collision while overlapping), shulker boxes and moving blocks (block-actor collision) are
special but unmodeled and keep ordinary numbers; soul soil stays unresolved. Climbable and
liquid membership follows the declared rules and is not independently established. Custom
blocks take surface friction from their base or last matching permutation's
`minecraft:friction` value (one minus the value, default 0.6) with neutral speed and fluid
facts, independent of any vanilla carrier record. The unmodeled and unresolved behaviour and
all provisional rule values still require verification before this gate can close.
Rig placement and target-rotation frames are compiled and carried with the selected
rig. Their compatibility source preserves existing absolute-frame routes, while
other identities remain explicitly unresolved and retain the body-yaw fallback.
Presentation and target-rotation queries consume cached frames. Complete native
class coverage, custom advertised-base admission, active component overrides and
ordinary-body frame classification remain incomplete. Camera-query presence does
not establish a placement frame.

Item-use descriptor admission is incomplete. The carrier records explicit unresolved items against the independently collected item-visual inventory. Gameplay, animation durations, consume audio, first-person consumption and cooldown publication read numerical admissions with session component overlays. A duration alone grants no use kind or movement penalty. The stock compatibility catalog, complete native use-kind census, authored cooldown timing, special appetite modes and conflicting component/class semantics remain provisional. Authored and session facts share whole-tick conversion. Cooldown quantization and component-version precedence remain provisional.



## Brewing stand models

Brewing stands compile a central post, three feet and three bottle-selected radial panels
from pack textures. All eight registered bottle combinations retain their own sprite
regions. Exact server placement, native rendered comparison and broader lighting and
performance parity remain incomplete.


## Controller particle attachments

Controller effects retain typed locator references and compiled initialization
programs. Locator attachments follow the sampled actor placement and inherited
skeleton; initialization reads the emitter's variables and, for bound effects, the actor's
numeric query context before any particle is emitted. Bound effects retire with their
actor lifetime. Temporary zero scale retains attachments and pending state entries
retry placement when scale returns. Particle spawning and attachment updates share
one lazy pose sample per actor per frame. The annotation coverage guard reads the
pinned controller files.

Incomplete parity: function-style actor queries outside the particle expression
grammar, skin-authored locators, bound child-event offsets under rotation, fallback
placement without a rig, and the full animated locator gallery remain unverified.
Frame and streaming performance acceptance remains open. No parity gate closes.

### Particle depth ranges

Translucent particles enter the shared terrain phase at individual camera depths from the
current extracted frame. Adjacent contiguous instances of the same material combine after
phase preparation, while intervening terrain preserves the range boundaries. Cutout particles
keep their depth-writing pass. The expression of depth matches the view sorting direction.

An offline fixture verifies alpha and additive depth composition through the production shader
and measures the GPU pass cost. Exact native composition across material buckets and full-frame
performance acceptance remain incomplete.


## Cached beacon beam sections

Beacon columns invalidate typed color sections when blocks, NBT, dimension bounds or block assets change. Admitted beacons populate the cache lazily after distance and submission checks. Top-layer beacons extend to open sky without sampling blocks. A block-owned eligibility table recognizes glass and hardened glass; sections carry their height and mixed tint to the renderer.

Incomplete parity: the full beacon geometry, animation, visibility and GPU performance gallery remains open.


## Compiled beacon tint eligibility

The block registry carries a tint or an explicit non-tinting disposition for each
canonical state. Hardened stained glass and panes use the same palette colours
as ordinary stained glass. Frame submission reads numeric descriptors, and
unrecognised custom identities remain unresolved. Stale active registries require
`make assets`.

Incomplete: this descriptor does not establish beam sections, obstruction,
activation, world height, custom block material overlays or end-to-end colour
transfer. These remain separate acceptance gates.


## Active block-entity artwork

The resource-pack transaction now builds the block-entity atlas on its worker and publishes it with the accepted pack result on join and reload. Missing or invalid overrides retain the validated startup tile. Texture and UV publication advance together.

Incomplete parity: full pack-reload galleries and streaming frame budgets remain open.

Live pack changes also prepare held terrain models and their artwork on the resource worker.
Publishing a replacement retires cached block meshes, including non-cube models, and removal
restores startup resources. Compatible authored carried sheets remain available without
replacing shaped terrain models. Startup and reload use one held-block generation builder;
pack UV grids and source-sized mips use the same face sampler as pack icon preparation.
Far-terrain reloads discard old summary jobs and resummarize
resident columns when the carrier or terrain cache identity changes, including when the carrier
is reused. Disk colors stay separate for each set of terrain resource inputs while
sharing one destination's disk quota. Regression tests cover warmed models, both hands,
authored sheets, byte-identical fresh and reloaded sheets, UV-grid cropping, restoration,
mesh-slot reuse, resource generations, and shared disk quotas.
Rendered acceptance and the full pack-reload gallery remain incomplete.


## Entity terrain occlusion and retained block-entity view culling

- Remote player bodies containing the camera are rejected using interpolated, server-scaled
  body bounds before model sampling and skin, cape, armor or glint preparation. The local
  avatar keeps its perspective-owned visibility. This guard does not use the larger authored
  visibility box. Native overlap behavior, rendered-frame and FPS acceptance remain incomplete.
- Actor bodies and their shadow bounds use bounded, current-terrain exposed-face proofs after
  ordinary view admission. Name tags retain their independent publication. Unknown,
  dirty, unpublished, clipped, transparent or budget-exhausted terrain always draws.
  Internal voxel faces cannot certify depth. Near-clip safety is independent of rotation.
  Actors with unbounded attachments, extra layers or no authored bounds also draw.
  Controllers with any replacement geometry bypass base-model terrain proofs, including
  selections between ticks; controllers that always draw the base model remain eligible.
- Block entities reject conservative bounds outside the current camera before scene geometry
  and GPU preparation, then use the same terrain proof. Hidden entities keep advancing lids,
  books and ring cues, requesting maps and retaining text canvases. Unknown bounds and global
  beams bypass preparation culling. Retained vertex runs are also culled at draw submission;
  camera turns publish geometry when admission changes.
- Each kind has a fixed plane/cell probe budget. Preparation counters record candidates,
  hidden-by-occlusion and submitted; block entities count hidden-by-visibility to include
  frustum rejection. Draw counters record skipped/submitted vertices.
- Performance and rendered-frame acceptance remain incomplete until comparable captures
  verify walls, gaps, rapid motion, shadows, name tags and unchanged animation state.

Server pack priority: admission now converts highest-priority-first server handoffs
into the low-to-high layer order used by resource reads and merge subscribers.
Regression coverage checks overlapping files and glyph sheets, missing-file fallback,
and original rejection indexes. Version-matched vanilla captures remain incomplete;
this change does not close a parity gate. Locale font fallback and built-in named
font alias behavior remain unchanged and need independent verification.

### Model texture-array selector coverage

Model texture-array selector coverage is incomplete. Compilation retains typed
selectors and output ranges for the existing explicit model bindings. Unrecognized
model arrays draw their first entry, as vanilla draws legacy arrays no state selects
from (stone brick stairs, firefly bushes, stone buttons). The generic terrain
accessor, complete native binding inventory and custom selector expressions still
need their own admission and coverage work.

## Overlay texture sharding across device-limited arrays

The block overlay uses up to four texture-array pages after the base carrier.
Each page respects the device layer limit, and the total retains the format's
8,192-layer cap. Each shard uses the tile size of its largest source. Source
pixels and uploaded mip chains keep their separate shared byte budgets.
Admission includes the diagnostic layer and any increase in retained layers'
cost before adding a new frame run. Rejected routes retain diagnostic visuals
without discarding the admitted overlay.

Texture references use a three-bit page field. World carrier version 19
separates this encoding from version 18. Chunk, model and liquid shaders bind
all supported pages, with diagnostic textures for unused slots. Shared frame
caches and transactional terrain bindings retain each run's page and restore
shard costs when a route is rolled back. Runtime pages own each shard once.
Complete frame runs stay together when they fit; otherwise frames span shards
in timeline order before truncating at layer or byte limits. Placement compares
actual growth costs, and each layer's source dimensions and UV grid validate
against its owning shard.

Incomplete: CI compilation, regression tests, and rendered-frame validation.
This does not close a parity gate.



## Overlay vanilla-texture fallback

Implemented: custom blocks naming vanilla textures absent from the server
stack can borrow a static page-0 texture from the matching base carrier.
The resolver uses the retained atlas and UV indices to select variant zero,
independent of material ID order. Donors must have the catalog's requested
tint already baked into their pixels; missing or different tints decline
fallback. Server material flags remain overlay-local,
and borrowing consumes no texture layer or byte budget. Server rasters take
precedence when they decode; capacity and budget refusals remain refusals.
Borrowed layers are validated against the base page, and vanilla material
overrides continue to require overlay textures. Changed catalog routes whose
pack pixels cannot be decoded stay diagnostic for custom blocks; they never
borrow the original key's artwork.

Regression coverage includes sidecar roundtrips with reversed variant order,
missing or ineligible variant-zero materials, custom-block publication,
server-raster precedence, diagnostic fallback, and admission refusals. Live
visual verification remains pending. Block-item sheets and inventory icons
still use placeholders for borrowed faces, and animated vanilla art remains
limited to base blocks.


## Cubeless server geometry draws nothing

Server packs pair a cubeless geometry with a null texture to hide a block
state (furniture anchors, empty vessels). The overlay compiler renders
cubeless geometry invisible with no gap counted. Geometry emptied by
rejected cubes keeps the diagnostic visual and its skipped-cube count.
Fully hidden valid cubes still return invisible, even if another cube was
rejected. Regressions cover all three cases in hashed and palette sessions,
including visual support, gap counts, and session application.


## Compiled pack render admission

Pack compilation reads shared render identities, tint domains, routing and icon flags.
Attachable categories use admitted armour slots and shield/glider facts; native held
items use the item trait and tier owners. Atmosphere source specifications are shared.
Existing state, collision and artwork checks still guard native model compilation.

Incomplete: render rules remain provisional. Native artwork associations, texture
aliases and broader custom content admission still need owning metadata and rendered
verification; this migration does not close their parity or performance gates.


## Actor GPU allocation recovery

Actor page and buffer allocations now check out-of-memory errors before publishing
resources. A failed page replacement retains a compatible previous page; a fresh
page retries progressively smaller normalized-UV rasters down to 1×1. Failed
geometry, skin or frame-buffer preparation skips that publication's draw. Unused
previous pages are released before replacement uploads. The existing actor pixel
ceiling remains unchanged.

Incomplete parity: retaining current resources on failed replacement and coarse
fallback through 1×1 are verified recovery behavior. Per-texture mip demand,
residency and idle demotion remain unimplemented. Windows-specific texture memory
allowances, initial allocation policy and adapter pressure thresholds remain
unverified. This recovery change does not close the streaming or hardware frame
budget acceptance gates.


## Cutout cube contact depth

Two-sided cutout cubes use a small receding slope depth offset so their back
faces do not fight coplanar opaque stair/slab faces seen through alpha holes.
Ordinary and retained-unit draws share the policy. Opaque cubes, model geometry,
blended terrain and liquids retain their existing depth state.

Incomplete parity: the installed native cutout material supports this offset,
but exact-current seasonal material selection remains unverified. Mixed model
materials are outside this change. The raster and descriptor regressions are
written but unrun; this does not close a renderer parity or frame-budget gate.


## Session custom item models and legacy declarations

Legacy registry items admit declared stack limits and retain scalar durability,
intrinsic shine and use-duration components. Custom block items share their
authored inventory geometry and material sheets across camera hands, player
equipment, inventory previews and dropped items. Resolved carried grass tint
is baked into the sheet while alpha remains unchanged. Unsupported UV wrapping,
material counts and unresolved tints retain the existing sprite fallback.

Custom wearables referring to recognized native external armor models select
a drawable model through the startup catalog's owner-specific armor definitions.
Unrecognized missing models remain unresolved. The original custom texture and
wearable slot continue to own the worn draw.

Original synthetic regressions cover legacy facts, both hands, artwork replacement,
preview and dropped geometry, tint, external model selection and unknown models.
Broader custom-content parity and performance gates remain open; these changes
do not close them.

### Cache status aggregation and simulation opportunities (2026-10-18, incomplete)

Cache classifications now enter one session outbox shared with the detached writer.
Simulation opportunities select at most one packet with misses first and the existing
combined hash limit. A strict five-millisecond exclusion advances even for an empty
opportunity. Ordinary render flushes do not select status. Idle and terrain-blocked
frames advance the session clock independently of movement output.

Ready world ingress and admission events no longer wait for status writes. Intake
pauses at the outbox byte budget, with one bounded candidate allowed to cross it,
while ready ingress and bounded raw read-ahead keep their existing owners. Selected
whole batches survive cancellation. Epoch rotation discards only status that has not
entered the writer FIFO; accepted batches finish once and ordinary packets survive.
Fast-transfer arming alone does not rotate the outbox.

Regression coverage is written but unrun in this worktree. Compilation, deterministic
red/green verification, review, complete cache lifecycle parity, unusual pause and
step timing, and complete current-version callback parity remain open.

### Retained mod drawing

Drawing permissions and connection fences cover legacy post effects, block highlights and
unfiltered world observations. The retained drawing ABI is versioned separately. Retained
indexed meshes, polylines, shared instances and callback groups are admitted transactionally
with per-owner quotas. The shared unlit renderer reuses meshes and updates view-relative
instances. Geometry admission validates each vertex once and caches box and sphere bounds,
payload bytes and expanded vertex counts. Instance validation uses only cached facts;
nonuniform scales use conservative extent bounds. Callbacks without resource changes
preserve snapshots without allocating; separate geometry, material and instance revisions
and snapshot identity determine retained publication. Entity root anchors use exact
owner-issued spawn references and the actor renderer publication. Missing roots hide,
while confirmed despawns release their instances.

Procedural materials add fixed per-instance parameters, a wrapped real-time session clock,
SDK presets and owner-scoped fragment-only WGSL with compile and cost bounds. World
emission participates in enabled HDR bloom; render-mode-independent halos require the
separate mask path. Deterministic lifecycle, shader and animation capture tests cover these
contracts. Bone anchors, in-world native output captures and hardware budgets remain
incomplete; no rendering parity or performance gate is closed by these contracts.

Actor masks retain owner-scoped, session-fenced union selections with bounded entity and
vertex counts. Masks draw admitted body and equipment meshes with the scene's pose and
material coverage. Depth-tested coverage is filtered before post effects; always-visible
coverage needs separate through-wall authority. Unfiltered target lookup requires explicit
observation and through-wall consent until a visibility-filtered target source exists.
Bounded host outlines and shared downsampled halos compose display-space colours after grading,
independently of world bloom. Loading waits for the shared blur and both scene-format composites,
even before a selection is published. Styles retain bindings for both scene textures until
their coverage, halo, sampler or scene inputs change. Custom post effects declare only their
owner's masks and need separate post authority. Incomplete: in-world Enhanced bloom on/off,
hand/UI ordering acceptance, resolution-transition captures and hardware budget measurements.
Offscreen tests do not close those live or performance gates.

The Rust SDK builds retained wireframe boxes, circles, three-ring spheres, arrows,
polylines, distance-based color gradients, grids and chunk borders directly in the
generated WIT records. Entity boxes use caller-supplied authorized bounds. Instance
defaults preserve depth testing. Legacy fixed primitives keep their existing
paths because the unlit contract does not preserve their procedural coverage and
placement. Their retained compatibility presets and a generated TypeScript author
package remain incomplete.
