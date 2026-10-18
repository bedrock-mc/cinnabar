# Mods, experiences and cosmetics status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Catalog cosmetics

The wardrobe uses a fetched, ETag-cached catalog with namespaced item IDs and independent
head, back, face, body, pet and aura slots. Geometry, textures and optional joint motion
are loaded from bounded, SHA-256-checked assets. The catalog includes original bunny
ears, the crown, rigid red devil horns, animated back-slot dragon wings and a pet-slot
shoulder parrot, plus studio headphones and a halo (head), pixel shades and a kitsune
mask (face), morpho wings and an ember jetpack (back), a fox tail and a knit scarf (body),
a bee buddy (pet) and an amethyst orbit (aura). Editable artwork and export conformance
checks live in cinnabar-api; the client keeps stable regression fixtures and an optional
compatibility check against an API checkout.
The parrot stays in Cosmetics, with its existing pet slot exposed by the category filter.
Each attachment follows its declared bone and offset. World and HUD share local motion.
Bounded transform clips and continuous span flex animate wings alongside independent hats.
An optional intro and delayed repeating clip sequence animate the parrot flying in once
and changing shoulders, using shared immutable geometry and caller-owned time.
Unknown, unsupported and unowned items render nothing.
Items flagged `render_flags.emissive` draw their opaque texels fullbright in the world through
the emissive-alpha actor material; previews keep the authored texture. Animation files may
declare up to eight particle emitters on authored bones. Their effects are registered under
client-owned identifiers and may draw original 16x16 sprites cut from the item's own texture.
They follow the posed bone and stop when the item is unequipped, hidden or despawned.
Emitters run only in the world, with at most 128 alive at once; HUD and wardrobe previews show
geometry without particles.

Visible catalog tiles and menu character/paper-doll previews play authored repeating joint
motion without a joined world. Preview playback starts after an optional world fly-in intro;
the HUD continues to share the local actor's evaluated motion. Static poses and original
texture pages stay cached. Tiles center the isolated cosmetic at a stable scale, and
wardrobe fitting uses a worker-prepared, sampled motion envelope. Exact envelope coverage
for arbitrary downloaded clips, live player-preview visual acceptance and hardware performance
qualification remain incomplete.
Visible thumbnail carriers take priority over optional gallery animation textures, and
cosmetic atlas borders retain their original point-sampled edge texels.
Equipped character geometry is admitted before optional tile meshes consume the frame budget.
Each admitted rig revision prepares its motion bounds on the artwork worker, including texture-cache hits.

The Cosmetics gallery offers All, Head, Face, Back, Body, Pet and Aura choices, one per schema
slot, through a filter icon beside
Remove all. Its menu closes on selection, outside click or Escape. Filtering preserves equipped
items, retains separate scroll positions and limits thumbnails and keyboard navigation
to the selected category.

Saved hat choices migrate into the head slot. Gallery requests include visible thumbnails
and prioritize equipped textures. Cosmetic motion retains the hand rasters, and unchanged
gallery requests compare the catalog generation without cloning its items. Attachments use
their declared preview bone and a separate range of render layers.

Authenticated nearby players exchange revisioned session loadouts independently of leased
emotes. The highest account revision wins across sockets. Disconnect, visibility opt-out
and nearby departure withdraw the shared selection. The API validates slot placement and
ownership before publication; all current catalog items are free.

Deploy the API and original R2 assets before the client. Payments, additional catalog items,
moderation, live authenticated multi-account acceptance and hardware qualification remain
incomplete. No Bedrock parity or performance gate advances.


## Restore and share custom emote playback

The articulated emote snapshot now carries the matching prepared body and clothing meshes.
The account API exchanges nearby watches, starts, phases and stops; authenticated identities
resolve only to current actor lifetimes. Shared poses override native/Java body animation for
their lifetime and also drive clothing, capes and equipment. Visibility opt-out and bounded
leases retire playback without depending on a game server's emote relay. Local clips no longer
send their owned identifiers as Bedrock emote packets.
Regression coverage includes mesh/pose agreement, cached mesh reuse, remote pose isolation,
start/stop, same-clock restarts, lease expiry, API identity, subscriptions and visibility.
Linux Vulkan headless local-world frames show bent knees and planted level soles through the
hip pulse. Sharing requires deployment of the API extension. Exact native/Lunar parity, Windows visual
acceptance and hardware performance qualification remain incomplete.


## Mod surface presentation

`mod-ui` paints the surfaces mods and server experiences own: trusted consent chrome, the client
part's modal, player-mod screens over containers, the personal panel and HUD editor, personal HUD
cards and label, package templates and the cosmetic cursor. It depends on `ui-engine` and lower
crates only. `client-ui` keeps authority, item-icon and container projection, scene order and
publication, and adapts runtime state into paint frames and typed inputs. This move has not been
compiled or executed and closes no parity gate.


## Mod surface motion

Personal-panel and HUD-editor surfaces may declare `transitions` that ease numeric bindings
from their displayed value toward each new value, with JSON-UI curves or cubic-bezier points,
an optional opening value and an optional exit value that personal-panel surfaces ease to
when they close (`docs/modding-spike.md`). Surface transitions, exits and `motion: true`
feedback follow the Screen animations setting. Controls may declare `cinnabar_transform`
to scale about an origin and rotate their drawing and hits without changing layout,
bindable through `#cinnabar_scale`, `#cinnabar_rotation` and `#cinnabar_origin`; rotated
subtrees cannot clip. `cinnabar_glass` panels show the world behind them blurred and
filtered like CSS `backdrop-filter`, and the reserved `#world_blur`, `#world_brightness` and
`#world_saturation` bindings blur and dim the world behind an open panel; one dual-Kawase
pyramid of the post-processed scene serves every backdrop in a frame, built only while one
is published. `cinnabar_filter` blurs a control's subtree as one image through an offscreen
group, animatable through `#cinnabar_blur`. These are Cinnabar extensions for mod
surfaces; server-pack JSON-UI is unchanged.

Incomplete toward expressive mod menus:
- The shared scene blur and element blur are unmeasured on the reference hardware tiers.
- Backdrops blur only the 3-D scene: UI beneath a glass panel, including the HUD and nested
  glass, stays sharp.
- Exits between routes: replacing an open panel's surface swaps it at once without playing
  the old surface's exit values.


## Cinnabar API

- The account core keeps an optional WebSocket to the Cinnabar API (`core/cinnabarapi`). It dials
  before minting and reuses unexpired key-bound credentials with fresh challenge proofs. Negotiated
  limits, retry minima, reconnect jitter and bounded frame-rejection retries control its traffic.
- Player lookups prioritize new joiners, drain coalesced refresh rounds, expire visible positives and discard
  older-session replies. Release notices include manifest builds and schedule jittered checks with
  bounded retries for older signed manifests. Packaging rejects placeholder origins and announces
  only changed signed manifests.
- Tab-list and name-tag badges share roster answers and the default-on General display toggle.
  Name tags reuse the original gem in a retained atlas cell, widen the name plate and apply the
  existing text visibility, depth and opacity passes.
- Go tests cover the handshake, lookups, release identities, retry policy and schema conformance.
  Rust tests cover roster-cache lifetimes, session changes and release-check scheduling.
- Incomplete: an authenticated live run against the deployed API and
  owner visual acceptance remain outstanding. No vanilla parity gate closes for this optional feature.


## Always Sprint keyboard/mouse extension

- Optional custom setting, off by default, persisted with the existing settings registry.
- Forward movement requests normal sprint; sneak, hunger and other restrictions still apply.
- Auth-input sprint flags remain derived from the completed physics state.
- Windows official install: Keyboard & Mouse rendered at a 1280×720 client area; label and toggle are legible, aligned and unclipped. Enabled preference persisted during user interaction. Live user movement acceptance remains pending.


## Cinnabar extension: server experiences (incomplete; not vanilla parity)

- Discovery, signed session negotiation, scoped consent, hashed bundle delivery,
  capability hosting and synchronized media are an opt-in Cinnabar extension.
- This branch is code-only. Local compilation and regression validation are now
  authorized; no vanilla, visual, performance or containment gate is closed.
- Production remote execution must remain unavailable until restricted helpers,
  compiler limits and media decoding pass independent cross-platform validation.
- See `docs/server-experiences.md` for the client implementation and remaining gates.
- Provisional, labeled incomplete: server WIT 0.4 focus snapshots stop counting when the
  player is farther than `provisionalFocusRange` (`tools/localserver/experience/limits.go`,
  Dragonfly's survival block reach). Vanilla closes a block container screen beyond the
  player's pick range (per input mode, survival or creative), measured squared from the
  player's eyes to the block centre; the range constants are not yet known. Replace the
  constant with those values, per game mode, once they are known.

- Implemented client preview: admitted marker, signed session challenge, scoped trust
  JSON-UI, HTTPS/hash cache, bounded ordered ScriptMessage records, versioned WIT,
  transactional developer helpers, and off-thread WebM/media output primitives.
- Incomplete: restricted production helpers/compiler limits; optional-pack provenance;
  full JSON-UI screens and input focus; scene/material adapters; live native media
  routing, shared surface binding, device clock, applied drift correction and fast
  seeking/looping. Production never sends readiness or starts a bundle. Developer
  readiness grants only UI labels and typed messaging.
- The MP4/H.264/AAC platform decoder is an unavailable trait stub. Native AV1/Opus
  decoding is an optional compiled feature, but workers remain unavailable until
  a helper enforces a process memory ceiling. No SDK or server-side integration
  was written. The existing Cargo.lock passes the locked workspace check.

### Review hardening and local validation

- ZIP bundles use only the bounded final directory and validated local entries;
  streaming decompression cannot retry earlier directories. Unsupported compression,
  extra metadata, ZIP64 and streaming data descriptors are rejected before decoding.
- Native media workers stay unavailable until an enforced process memory ceiling
  exists. Sticky reader faults prevent download or integrity errors becoming EOF.
- Host staging reserves the complete serialized transaction, including its owner,
  epoch, wrapper and command separators. Identifier, channel-field, initial bundle
  generation and widget text limits use shared policy constants.
- Regression tests cover an oversized earlier ZIP64 directory behind a malformed
  final AES entry, extra metadata, oversized EBML declarations, demuxer I/O failure
  at an element boundary, exact transaction/IPC limits and channel field limits.
- All Cargo validation below ran locally through the owner's `cslot` limiter.
  No dependency or lockfile update was needed. These checks passed:
  - `cargo check --workspace --all-targets --locked`
  - `cargo test -p server-experience -p mod-host --locked` (31 and 13 tests)
  - `cargo test -p server-experience -p mod-host --features server-experience/developer-media --locked`
    (34 and 13 tests)
  - `cargo test -p cinnabar --locked` (unit, integration and documentation tests)
  - `cargo clippy --workspace --all-targets --features server-experience/developer-media --locked -- -D warnings`
  - `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml`
- `cargo fmt --all`, the formatting check and `git diff --check` passed. Production
  containment, media integration and vanilla parity gates remain open.

### PR #34 review fixes and local validation (2026-10-02)

- Fixed all ten review findings: consent clears raw mouse-button messages through
  dismissal; the controller follows the committed UI drain; initializers share
  callback slices, retain bounded sends and publish readiness before those sends;
  reliable events wait for idle helpers and available callback fuel.
- Media grants now bind the verified archive digest. Applied controls retain their
  accepted timestamp frontier, clock probes survive delayed or unsolicited replies,
  and the PCM presentation queue enforces the decoder packet frame ceiling.
- Added regressions for next-frame input replay, schedule ordering, same-frame
  dimension revocation, staggered initialization, four-component startup, burst
  delivery, signed revision substitution, timestamp reversal, delayed probes and
  maximum-sized PCM blocks. Reconciled the preview documentation with the branch's
  recorded validation and existing lockfile additions.
- Local checks passed through the owner's `cslot` limiter:
  - `cargo check --workspace --all-targets --locked`
  - `cargo test -p server-experience -p cinnabar --locked` (35 server-experience
    tests; 1,879 client unit tests passed and 16 were ignored; client integration
    and documentation tests passed)
  - `cargo test -p server-experience --features developer-media --locked` (38 tests)
  - `cargo clippy --workspace --all-targets --features server-experience/developer-media --locked -- -D warnings`
  - `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml`
- `cargo fmt --all`, the formatting check and `git diff --check` passed. No new
  dependency or lockfile update was needed. Production containment, live media,
  visual, performance and vanilla parity gates remain open. No remote build or
  live client/server session was used.
### Movement audit continuation (2026-10-01, incomplete)

`fix/zeqa-corrections` merged `origin/dev-sonnet` at `473cec0e`. Ordinary steering
now follows current-client f32 sin/cos products. Historical Go fixtures remain comparison data,
not a bit-exact vanilla oracle. Position, collision and other travel arithmetic
still retain f64; the D01 parity gate remains open. Prediction-sync payload
sources, historical-world replay, custom dimensions, fluid currents, special
block effects and the missing gameplay/input scenarios remain incomplete.
Grounded liquids now select liquid acceleration/ascent; soul sand uses native
acceleration friction, with Soul Speed removing that penalty. Sneak edge clipping
runs to supported motion or zero. Prediction sync waits 200 fresh ticks, and live
and replay packets use actual jump initiation. Server flight-off is authoritative.
Web slowdown applies once and honors Weaving. Client ContainerClose sends type -9 while the ledger retains its real type.
No live or visual acceptance gate is closed by these changes.

2026-10-02 RM-06 continuation (incomplete): atmosphere carriers retain initial
fog and transition timing. The current 27-position biome layer blends distance,
RGB and transition fields with missing-entry coverage. Water transitions blend
initial color/start/end using the minimum-clamped two-stage timeline, replacing
the fixed endpoint multiplier. Server-directed layers, frame smoothing and depth
adjustments remain incomplete. Pack definition: `fogs/default_fog_setting.json`.

2026-10-02 GEO-01 continuation (incomplete): nested weighted texture paths are
kept separate from state arrays and carried to cube, model and liquid shaders.
Selection uses wrapping absolute block coordinates; weighted cube faces cannot
merge across cells. Carrier material records now retain selector ranges and
normalized weights; old world carriers require `make assets`. Server material
replacement clears the replaced selector. Variant flags must match the selector's
rendering path; string and object paths retain their distinct default weights.
Variant tint/UV extension metadata
is rejected explicitly pending a matching material route, so full pack-semantic
and native visual parity remain open. Pinned terrain_texture.json
ordinary arrays (including repeater/comparator) remain state selectors.

### Numeric continuation (2026-10-02, incomplete D01)

Walking vectors, collision arithmetic, AABB centers, jump impulses, gravity and drag
now round at f32 operations. Motion remains independent of the rounded final position. Sprint jumps use native float
indices and table initialization by `sinf(index / 10430.378f)`. Exact angle and distant-position
witnesses cover these changes. D01 remains incomplete: the non-walking travel models
and exhaustive Windows-versus-host sinf bit equivalence still need validation.
Water acceleration now multiplies the effective Depth Strider level before dividing by
its maximum.
Collision flags use the native float epsilon, with exact boundary witnesses.
Processed control limits, block-height contact, support offsets, climb requests and
fallback travel drag now round within the same f32 simulation boundaries. New
precision regressions await CI; these changes do not close D01.

### Registry collision continuation (2026-10-02)

Doors now resolve facing/open from the lower half and hinge from the upper half;
missing pairs use the native default plane. Current planes are 0.1825 blocks thick. Stair collision reads the
current registry corner state; the older
neighbor-derived algorithm is not substituted for it.
Scaffold support uses the stable registry unit cube and the native pre-move top/contact
conditions. Registry
coverage includes both runtime-ID modes and all stair corners/halves. Scaffold movement
coefficients, powder-snow equipment behavior and broader interaction parity remain open.

### Historical replay continuation (2026-10-02, incomplete INT-10)

Palette prediction frames now retain immutable block pages, load state, registry data
and collision revisions. Replays use each frame's world even after live edits or unloads.
Controller frames retain mode intent, input edges, requested controls, mode state and
environment; corrected ticks re-evaluate pose and repeated jumps and preserve retimed
server overrides. Tests cover changed ceilings, changed correction anchors and repeated
replays. Vanilla uses history-based component replay; these tests verify our implementation, not complete native parity.
Anchor depenetration remains provisional (INT-10), and full component coverage and the
memory/performance cost of retained world metadata still need validation.

### Liquid contact continuation (2026-10-02, incomplete D08–D11)

Liquid contact now uses the current native water/lava shrink vectors, including low-pose
center clamping and material-cell tests independent of fluid surface height. Contact boundary witnesses
and the complete simulator suite pass. Currents, complete swimming travel/drag, liquid
attributes and exits remain open. The 49-scenario, 1,112-tick Go differential changes
from 22 to 19 scenarios above 1e-5 or with flag differences, and from 49 to 41 scenarios
with any exact difference. This comparison is not a native parity acceptance gate.

Prediction-sync packets now use the wire defaults for undefined attributes instead of withholding
a due packet. Known values and transport-admission retries are retained. Completed local
sneak/sprint flags and the standard unscaled walking pose height reach a due sync before
a server echo. Other models and poses retain their supplied sizes. Full correction-state,
custom/scaled and horizontal-pose size prediction, locomotion and packet-order parity remain
incomplete.
Alternate locomotion now uses the same ground-contact rule as ordinary movement: a vertical
collision grounds only a downward request; without that collision, prior ground survives only
an exact zero vertical request. Tiny positive or negative requests clear it. Complete locomotion
and packet-timing parity remain incomplete.

### Inventory batching continuation (2026-10-02, incomplete serverbound parity)

Ready ledger requests now share one ItemStackRequest packet, retaining each request's
ID, ordered actions and text-filter origin. Transport refusal leaves the entire batch
unsent; successful admission advances all included requests together. Empty batches
emit no packet. The existing
window-control priority is retained. Native tick/flush phase, cross-family packet batching,
vehicle prediction, interaction models and emote/spin/flight input ownership remain open.
### Mesh streaming follow-up (incomplete native acceptance)

The offline load/teleport and burst fixtures exercise separate decode, light and mesh
queues, bounded frame service and coalesced invalidations. Pool sizes, service shares
and OS priority mappings are Cinnabar implementation choices; exact current-client
scheduling parity remains incomplete. Native release frame and network-latency
acceptance remains open. See `docs/reviews/mesh-stall-followup.md` for the references
and local regression measurements.

### Sub-chunk request batching (incomplete serverbound parity)

Sub-chunk dispatch now groups the stable admitted intent prefix into one request.
It samples the current published render camera when its session, stream and dimension
owners match, otherwise the current actor position. Horizontal coordinates floor to
16-block cells; the vertical origin stays zero. Absolute member positions keep their
admitted order, including sparse negative heights and multiple columns. Each axis
admits offsets from -127 through 127, and one packet holds at most 8,192 members.
The per-frame budget still counts column intents and bounds the flattening work.
Wrong-dimension and unencodable demands retire without declaring terrain loaded.
Transport refusal restores current intents in order; successful writes confirm one
immutable packet membership and arm response deadlines only for its current demands.

This implements the bounded 26.30 request origin, ordering and grouping contract.
The existing timeout, retry, recovery and queue capacities remain local policy and
are explicitly incomplete for native parity. Global request discovery, the complete
camera writer phase, broader native retry policy, later client versions, and live
streaming and join acceptance remain open. Regression coverage is authored;
final verification and independent review are pending.

### Between-frames world servicing (incomplete native acceptance)

The production client lends `WorldStream` to a `stream-service` thread after each frame's
last schedule and reclaims it before the next frame's first, so decode commits, light and
mesh scheduling and retention evictions overlap extraction and pacing. The frame keeps a
floor poll and still commits the chunk data that a ready block change, retention change
or barrier waits behind. Resident membership, keyed stream state and terrain lookups use
hashed indexes, and mesh halo and neighbour checks read each index once. Offline synthetic
replays (radius 12, 120 Hz) cut frame-thread stream time several-fold and converge to the
same presented meshes ([offline evidence](../../docs/evidence/async-chunk-servicing.md)); joins
mesh the faced view sooner and rear sub-chunks of nearby columns later. Release captures on
target hardware, live join and streaming budgets, and real-pack meshing and lighting load
remain open.

### Inventory/HUD correction continuation (2026-10-02, incomplete general parity)

Selected-item text now positions its spawned Java-look factory root above the
hotbar, retaining the inherited Bedrock label and animation. Bare block stacks
can split/restack without rejecting nonzero block identity; ingredient plainness
no longer requires zero aux/block identity. Supported recipe shapes are retained
independently of discovery metadata, including high-bit result block identities.
Personal/workbench closes explicitly return crafting inputs, and all storage
closes return cursor items to player inventory, dropping only overflow, with
sparse dependency preservation and rejected-return recovery. References and
scoped acceptance are in
`docs/reference/selected-item-hud-label.md`, `inventory-block-restacking.md`,
`inventory-recipe-admission.md` and `inventory-crafting-close.md`.

Scoped offline BDS acceptance exercises split/restack, manual 2×2 and workbench
crafts, input/cursor returns and repeated reopen with server-verified counts.
Rendered survival/creative selected-name geometry is inspected at the owner's
Retina scale. See `docs/reviews/inventory-hud-crafting-fixes.md` for exact builds,
local evidence paths and verification state.

Recipe unlock packets now have a retained authority foundation: exact names and
contexts survive recipe admission, additions resolve against the ordered catalog,
and explicit membership survives ordinary catalog clears while a new session
resets it. Unsupported crafting shapes and furnace-family identities remain
addressable. Duplicate names across eligible tags are conservatively unresolved;
exact tag priority and synthesized multi-recipe identities remain incomplete.
Startup and later crafting rules now reach the committed inventory frontier.
Manual results check exact recipe discovery when both recipe unlocking and
limited crafting are enabled. The book distinguishes absent discovery state
from an empty component, and nonempty search can show locked recipes
when limited crafting is off. Their selection remains guarded. Book actions recheck the current recipe handle
and discovery; rule and unlock changes invalidate retained output previews.
Book rows aggregate ingredient availability across same-output variants separately
from the candidate's discovery permission, and selection rechecks that permission.
Initial discovery-state absence is provisional; player creation, respawn and
listener ordering still require verification. Category masks, toast presentation,
complete book variant ranking and stable live acceptance remain incomplete.

Full structural-NBT merge parity, descriptor-dependent capacity/variant rules,
complete recipe-book discovery state,
arbitrary container return flags and exact native close/flush timing remain open.
These corrections do not close the overall Phase 5 inventory parity gate.

Storage-close regressions cover empty/full player inventories, pending/accepted
takes, both close-acknowledgement orders, inactive personal inputs, and later inventory gestures. The
close-with-cursor lock is reproduced in the owning ledger. Live kit-server
acceptance and an overflow-only trigger remain unverified.

Incremental recipe replacements now retire obsolete station and multi-recipe
views. Station inputs outside the supported slot counts, or with quantities
other than one, are explicitly counted and skipped. Support for those broader
station shapes remains incomplete.

### Zeqa regression follow-up (incomplete visual/performance acceptance)

Nametag phase traversal, omitted catalog plane backs, active player appearance
lifetime, and matrices cached across rig replacement have focused corrections.
The supplied offline witnesses do not close the live form layout/FPS, missing
hotbar icons, all nametag size/garbling symptoms, or RustMCBE stretched-limb gates.
The player-body report omits equipment and GPU execution. See
`docs/reference/zeqa-regression-investigation.md` for source boundaries, vanilla
references, PNG evidence and the limitations of the capture.

### JSON-UI review follow-up (incomplete live form acceptance)

JSON-UI names have a categorized `json_ui::vanilla` contract checked against a
names-only vocabulary from the pinned sample pack. Screen routing and core form
and context consumers use it. Remaining controller migrations and corrections
to stale names are incomplete; this contract closes no live visual gate.

Review corrections cover untrusted animation graphs, expanded widget component
bags, native form titles, screen cancellation, Drop remapping, recipe icons and
perspective settings. An open form also remeasures when its session font changes.
The captured Spirit Bundle witness exercises late pack installation and texture
residency, but the black rectangle, floating labels and live FPS loss remain
unproven. No live visual/performance gate is closed; see
`docs/reference/jsonui-review-fixes.md`.

Tiny JSON-UI disabled frames preserve authored source slices when opposing
insets overlap, including reversed center UVs. Individual oversized insets stay
inside the texture region as an atlas safety fallback; exact beyond-image native
sampling remains incomplete. Original raster fixtures cover
one-texel borders through the atlas at GUI scales 3 and 6. Exact-version authored
frontend comparison and the broader UI visual parity gate remain incomplete.


## Freelook extension

Freelook is a requested Cinnabar extension, not vanilla behavior. Hold its configurable
Keyboard & Mouse binding (default F) to orbit a collision-resolved third-person camera
while retaining gameplay facing, movement and interaction direction. Release, UI focus
or window focus loss returns to the prior perspective. Windows/DX12 1280x720 hidden
capture verifies the Freelook/F settings row. Routed tests cover independent rotation,
release/focus restoration, persistence and existing-F migration. A manual in-world
orbit acceptance pass remains incomplete.
