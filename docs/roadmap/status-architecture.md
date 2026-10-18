# Architecture cleanup status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Actor animation ownership

`actor-animation` owns actor rigs, Molang evaluation, controllers, poses,
attachables, custom skin models, Java 1.7 motion and the original emotes. It is
Bevy-free and sits below `client-world`. Actor authority supplies each actor through
the `AnimatedActor` trait and an `ActorTickContext` per tick, and drives the store
at spawns, swings, hurts, server animations and fixed ticks. Synced property
definitions and the wolf dye tint are animation inputs; `client-world` keeps the
property registry and no longer depends on `render-model`.

Tests that drive actor authority live in `client-world`'s `actor_store::animation_tests`,
using fixtures from the `test-support` feature. Animation unit tests use a plain actor
double. Compilation and execution of this move remain unverified, and no actor
animation parity gate closes.

Finalized catalog layouts retain bounded immutable rig templates and motion-dependency plans.
Actor-specific conditions still select geometry before lookup; variables, roots, controllers,
poses, skin drivers and history remain private copies. Exact catalog ownership and topology
keys prevent reuse across replacements. The shared cache admits at most 32 entries and 4 MiB
of accounted payload; oversized or unmatched plans use complete uncached construction.
Eight focused regressions pass. Per-actor copies, mutex contention and crowded-join gains
remain unmeasured; no performance or parity gate closes.


## Render geometry and scene ownership

- `render-model` owns CPU block-model and selection geometry, dropped-item meshes,
  actor rig construction and artwork, and validated viewmodel meshes.
- `render-scene` owns Bevy submission resources, chunk publication queues,
  camera settings, and renderer acknowledgements. `client-presentation` and
  `acceptance` consume these contracts without depending on `render`.
- `render` owns GPU passes, pipelines, uploads, and render-world system registration.
  The app composes the camera motion-blur plugin with the other GPU plugins.
- Existing behavior tests are retained. Compilation and execution of these moves
  remain unverified; behavior and performance acceptance gates stay open.

Actor commands collect receipts only after encoding draws and publish them against the
immutable queued batch once per command. Existing ordered-set membership, complete-span
coverage and GPU-completion acknowledgement stay in their current owners. The local receipt
vector adds transient allocation and copying; fragmented commands can still publish once per
span. Regression coverage is written but unrun, and lock/allocation measurements remain pending.
Indexed receipt storage and span reuse remain separate; no performance or parity gate closes.

Actor endpoint matrices retain bounded GPU slots independently of draw order. Exact byte
comparisons upload changed previous/current ranges while instance scalars still update.
The renderer projects matching instance and receipt offsets; CPU publications stay dense
for motion history. One CPU mirror is bounded by the existing endpoint arena, with no GPU
budget increase. Fragmented allocations compact without changing admission. Regression
tests pass in the focused actor-render suite (75 tests), and render crate clippy passes.
Rendered parity and measured performance remain pending. Dense CPU publication copies
and other matrix-cache work remain separate.


## Player interaction ownership

`client-interaction` owns the movement and interaction resource wrappers, item-use
presentation and selection queries, placement observations, camera input adapters,
and hotbar input. Its plugins register camera input, hotbar sampling and retained
block picks against the shared `client-state` frame contract. Callers import the
owning crate directly.

The extraction remains incomplete: world and transport systems for movement,
use/place/mine/melee/pick, block selection, world-dependent camera publication and
cursor focus remain in `app`. They still combine world, transport, menu and
extension state at the app boundary. Existing tests remain with their owning code;
compilation, test execution and the behavior/performance acceptance gates remain unverified.

Entity-hover reports use the retained primary actor pick shared with attacks. Local
ticks report a changed actor or held item before movement, and leaving the actor
sends a positionless clear. Unchanged targets do not generate repeated reports.
Remote actor motion shares completed local ticks and fractional progress during prediction;
otherwise it uses the client frame elapsed cap.


## Client runtime ownership

`client-runtime` owns the Bevy transport handle and frame flush, bounded ingress,
inventory ingress, publication budgeting, world-service schedules and resource,
process shutdown, shared session helpers, admitted-pack resource and pack geometry
preparation. `app` installs `ClientRuntimePlugin`; its frame adapters keep their
existing order through `client-state`.

The extraction remains incomplete: network bootstrap and world publication still
borrow interaction and presentation resources; join, transfer and teardown
still borrow the menu; pack reload publication still applies presentation and audio
state. These adapters remain in `app` until their shared contracts exist. The runtime
crate has no presentation, audio or other sibling-plugin dependency. Compilation and
execution of this move remain unverified, and no parity gate is closed by it.


## Client frame contract

- `client-state` owns the shared frame sets, player authority wrapper, settings revision
  wrapper, finalized input wrapper, and shell-published focus and control observations.
  It contains no systems or state transitions and has a 4,000-line source ceiling.
- `ui` owns retained settings revisions; `semantic-input` owns finalized input queries.
  The shell publishes screen ownership before semantic authority and refreshes it before
  gameplay. Interaction and camera input consume these observations directly.
- Focus and existing input/cursor regression coverage are retained. Compilation and execution
  of this ownership change remain unverified; movement and presentation parity gates stay open.


## Menu shell ownership

`client-shell` owns the menu runtime, launcher account and local-world drivers,
player appearance selection, GUI scale, fullscreen, settings persistence and the
Discord driver. `ClientShellPlugin` registers input and settings systems and
publishes screen focus through `client-state`. It has no dependencies on sibling
app plugin crates. Desktop focus transitions live in `semantic-input`; the
resource wrapper, consent state, session observations and ordering sets live in
`client-state`.

The app still adapts session/network state, audio settings and renderer capabilities
into the shell. Render-distance packet sending stays with the app's network
adapter. Optional language carrier loading belongs to `assets::carriers::startup`, and physical
memory discovery belongs to `launcher-host`. Existing menu and integration tests
remain enabled in their owning crates. Compilation, execution and live behavior
remain unverified for these moves; they do not close a parity gate.


## OreUI crate

`oreui` owns client-owned out-of-game screens, including disconnect reasons, join progress and
launcher popups, with their presentation state, artwork loading and shipped fonts. In-game
screens retain their existing renderer and server-pack behavior.
It depends on `ui-engine` and lower crates, never on `client-ui` or a sibling screen crate.
`client-ui` adapters project menu, loading and bed state into its typed inputs. Shared pieces
moved down: menu schemas and view projections to `launcher`, `LoadingStage` to `ui`, and
translation, caret spots, menu scroll views and the font fixture to `ui-engine`. Tests that
drive the presentation runtime stay in `client-ui`. This move has not been compiled or executed
and closes no parity gate.


## Vanilla screen crate

`ui-screens` owns the engine-drawn vanilla screens: containers with the recipe and furnace books
and their hit vocabulary, the HUD with its native renderers and catalog patches, chat, signs, NPC
dialogue, credits, loading, toasts, the roster, the emote wheel, and the menu shell's screen
bindings and settings binders. Screens read typed inputs; `client-ui` keeps their
runtime adapters, scene selection, the menu composition, the server-form fallback, the menu title
and caret state, the player preview and the Java-styled fallback. This move has not been compiled
or executed and closes no parity gate.


## Extension crate ownership

`client-extensions` owns local component loading and load-order merging, environment
permission grants, bounded player snapshots, optional panel fonts, and trusted server
experience consent text and input consumption. Its Bevy plugin uses the consent
observation and input boundary in `client-state`, with no sibling plugin dependencies.
The moved behavioral tests remain with their owners.

The extraction is incomplete. Local mod registration and frame adapters still use
app-owned network, world, movement, menu, and camera resources. Server experience
session driving, helper supervision, and media playback also remain in `app` until
shared session and presentation contracts are available. This ownership change has
not been compiled or executed and closes no behavior or performance acceptance gate.


## Launcher crate ownership

- `launcher` owns menu models and settings definitions. Control types come directly from
  `bridge`; shared skin data lives in `render-api`.
- `launcher-host` owns authentication and core processes, account and artwork workers,
  server-list storage, storage scans, persistent login IDs, video persistence and process lifecycle services.
  It also runs local worlds: the world store, the BDS download and the local server processes; the core
  only receives the open world's address over `local_world.v1`. This move is not yet compiled or run.
  `client-shell` supplies the menu resources and drivers; `app` adapts session and network state.
- `first-run` owns the standalone setup window and asset preparation flow. Shared OreUI styles
  live in `ui`, and panorama preparation lives in `render-model`. Both new crates are Bevy-free.
- `assets::carriers::startup` owns carrier path selection, bounded loading, validation, fallbacks,
  decoded sidecars and startup measurements. It remains Bevy-free. `app` binds the returned
  data to renderer, pack and Bevy state, records load timings and exits from `main` on failure.
  Carrier regressions follow their owner; app composition regressions remain in `app`.
  This ownership move has not been compiled or executed and closes no parity gate.


## Runtime pack ownership

`pack-runtime` owns server-pack admission, custom-block physics preparation, entity
compilation and its cache, language and texture preparation, and reload dependency
comparison. It has no Bevy dependency. `client-session` owns transport and join
bootstrap, receiving prepared block facts and an opaque presentation payload from
its caller; it has no dependency on `pack-compiler` or `pack-runtime`.

The app retains renderer, UI, audio, and world publication adapters and their
subscriber orchestration. Block visuals still use renderer-owned material flags,
and item, glyph, audio, and artwork payloads retain their existing owners. These
moves do not change the open behavior or performance acceptance gates.


## Diagnostics ownership

`diagnostics` holds Bevy-free metrics and marker contracts, with no `protocol`
dependency. The app owns the build-identity resource and passes its build profile
into the runtime-metadata and world-publication markers. Marker output is unchanged;
compilation and test execution of this move remain unverified until CI.


## Chat ownership and first-person shadows

- At the owner's request, built-in chat owns its screen, autocomplete and HUD
  history even when server packs replace their layouts. Chat-derived ordinary
  history factories are suppressed; other server HUD widgets retain pack priority.
- Local player shadows now follow the actor frame's drawn bodies, suppressing
  the caster in first person and retaining it in third person.
- Animated loading strips wait for settled artwork instead of drawing a
  downsampled preview. Oversized images use the original-resolution path
  described under Original UI image animation.
- The integrated Rust client and Go core builds pass. Incomplete acceptance:
  regressions are authored but unrun at the owner's request; live inspection
  is still pending.


## Go core simplification (2026-10-02)

Login client IDs persist as nonzero signed 64-bit values in launcher user-data storage.
Joins and transfers reuse the stored value, independently of account credentials,
device profiles and transport GUIDs. Invalid or zero files are replaced using full-width
OS entropy; storage failures return an explicit error. Broader network and platform
parity and destination-routing acceptance remain incomplete.

Client login claims use a supported memory tier and explicitly report editor capability
and connection intent for normal gameplay, so PowerNukkitX accepts valid skins.
The core-session and JWT login builders share the desktop-tier and editor-intent values.

The core's packet-decoding diagnostic observers for cache boundaries, loading order,
and form schemas are removed. The proxy still forwards packet batches and retains
resource-pack progress and admission status used by the client. Historical cache
boundary logs remain readable by the acceptance scripts. Current diagnostic runs
record missing boundary instrumentation as unavailable, with an explicit finding;
they do not satisfy an independent cache-route proof or a completed Lunar prerequisite.
Replacement live evidence is still needed before closing the cache-streaming parity gate.

Authentication and pack caches now trust the user's configuration directory, while
retaining atomic publication, file leases, credential binding and quota eviction.
New credentials remain private on Unix and Windows. Account methods reject calls
after close, and sign-out takes the same leases as token refreshes. The active
sign-in keeps a stable cache generation across refreshes; a replacement sign-in
ends the old account runtime before it can adopt the new credentials. The active
catalog exporter and native Windows/Linux BDS installer remain supported. Resource
packs still pass through the Go cache and retain their client progress reporting.
Session handoffs name pinned cache archives by absolute path, size and SHA-256.
The client reads them under its configured cache directory and rejects invalid
paths or bytes during the join. Cache misses retain the streamed archive fallback;
pack order, required flags, sub-packs and content keys remain unchanged. Live
performance acceptance of the cache-reference handoff remains incomplete.

Pack admission and join-stage logging declare static packet interests. Unrelated streaming
bodies skip observer copies when no unrestricted payload observer is installed. Stage timing,
offer admission and legacy callback ownership retain their existing contracts. Regression
fixtures are written but unrun; compilation and measured performance acceptance remain open.

The Rust `bedrock-replay` tool uses the protocol crate's session messages and keeps
the raw capture and report formats. Acceptance endpoint waiters use the session
endpoint. The real core-session test covers large binary frames in both directions.

### Astra UX follow-up (incomplete live performance/parity acceptance)

Real-carrier Bevy input now exercises all Add/Edit server fields, persistence and
queued endpoints; Inbox summaries stay within their cards. Startup accepts lit,
meshed, upload-acknowledged near terrain plus a later GPU frame without waiting
for distant replies. The optional OreUI static page now rebases loading fallbacks,
and unchanged GUI skins reuse their digest. See `docs/parity/server-info-input.md`,
`docs/core-join-startup.md`, `docs/parity/loading-textures.md` and
`docs/parity/menu-frame-cost.md` for references, tests and measured boundaries.

The supplied post-pack Zeqa page-grid corruption and 6 FPS, the owner's menu FPS,
and the minutes-long live BDS join did not reproduce offline. Native Metal frames
were rendered and inspected, but the native window capture integration returned
`cgWindowNotFound`. Release/live acceptance remains open; these local changes do
not close it. No live server connection was made.

### HUD paper doll and menu follow-up (incomplete parity acceptance)

The HUD now dispatches its pack-authored live player control through the shared
GUI model pipeline, with native trigger timers, settings and a full-body pose
when the camera is in first person. Home exposes its player and Profile control;
menu doll framing uses the native model origin. Play-button ribbon fields
survive the core feed, and Inbox has categories, dated sections, read state and
confirmed deletion. See `docs/reference/hud-paper-doll.md` for exact source
references, offline evidence and limitations.

HUD frame interpolation, full persona layers, vehicle rendering,
matched pause/inventory pixel captures and complete Inbox settings/rich-message
behavior remain incomplete. The owner's stretched-model bug has no reproduced
failing geometry witness. These changes do not close any overall visual or live
performance parity gate. No live server or remote machine was used.

2026-10-09 preview armor and inventory framing (implemented; parity incomplete): inventory,
pause and HUD previews resolve armor as the world player does and apply `customColor` only
through a leather material, so team-dyed diamond armor keeps its diamond art. Session pack
armor takes GUI model pages only for the worn textures, each placed on its own, so a server
stack with more attachable art than the model atlas no longer disables all of it. The live
inventory renderer centers the model-part origin instead of the eyes, as the HUD doll does.
Incomplete: pack attachables with their own geometry still draw on the humanoid armor boxes;
worn art longer than a 512-texel model page is box-reduced in the GUI although the world draws
it at full resolution; the inventory placement has no matched vanilla capture.
### Burning camera and HUD doll (accepted fix; overall parity incomplete)

The camera effect now uses vanilla's open fire cube, down-face sprite,
point sampling, tint/alpha and render order. The active pinned registry protocol
selects fire, and admitted pack animation frames share the terrain clock.
The HUD doll uses its separate native flame atlas, collision-box geometry,
per-draw animation and controller overlay, including the swimming translation
and extinguishing fade. References: `docs/reference/camera-fire.md` and
`docs/reference/hud-paper-doll.md`.

Focused fire, shader, UI adapter and UI renderer tests pass, including a physical
Metal GPU readback for cube orientation, open top, pixel edges and animation.
The active-protocol admission regression and architecture/fmt checks pass.
Affected production-library clippy passes with two existing unrelated terrain
lint categories exempted; the unmodified all-target baseline also has unrelated
terrain-test and source-inclusion warnings. No full workspace sweep was run.

On 2026-10-04, the user manually accepted the camera overlay and burning doll in
the rebuilt ordinary-mode Mac client (Apple M3 Pro/Metal, Retina display,
1280-by-720 logical content window). This closes the reported haze/missing-doll
defect. The installed native app is a near-version witness, not an exact-version
capture; broad rendering, persona and live HUD pack-refresh parity remain open.
World fire is tracked separately below.

### Burning world actors (fix; parity incomplete)

World mobs and third-person players now draw animated flame billboards when their
authoritative on-fire metadata is set. The layer follows interpolated feet and scaled
collision dimensions, faces the current camera, and disappears on extinguishing or
invisibility. Optional original flame art, geometry and draw-clock rules are shared
with the HUD doll. No client-side daylight or damage inference is introduced.

World billboard placement and playback reuse the HUD contract provisionally;
matched-version native world captures remain incomplete and no parity gate closes.
Headless Linux/Vulkan frames at 1280×720, scale 1 show front and side flames and
their removal after an authoritative extinguishing update. Four world-flame and
nine existing HUD flame regressions pass.

### Pack masks, End sky and title text (acceptance incomplete)

Active packs now prepare pumpkin and spyglass masks, End-sky pixels and layered
title splash lists during resource reload. Removing the optional resources
restores startup data. End-sky overrides retain their decoded dimensions; masks
use the existing 256-pixel scene layers. Synthetic regression tests cover
priority, removal, alpha, cache invalidation and stale-generation publication.
Compilation, regression execution and rendered parity acceptance remain open.

### World fire (accepted geometry and smoke; parity incomplete)

World fire now uses vanilla's eight supported sloped quads, their height,
both independently phased face textures, side
attachments and ceiling slopes. Signed world-position parity selects attached
texture/UV variants. The camera keeps its separate down-face binding. Native
terrain-layer routing proves double-sided cutout rendering with depth writes,
white vertices and sampled center-cell light without directional shade or AO.
Reference: `docs/reference/block-fire.md`.

Eleven focused compiler/admission/carrier tests and four meshing tests pass,
including malformed topology groups, all attachment masks, negative-position
UV parity and support across a sub-chunk boundary. Ordinary fire's native
ambient callback now emits runtime-pack smoke from the existing fixed-tick
sampler. Its six smoke tests and ten existing ambient tests pass. App unit-test
compilation required a temporary adapter/import correction in the pre-existing
movement owner tests; that unrelated file was restored byte-for-byte afterward.
The client and asset compiler build, updated local carrier compilation,
formatting and architecture checks pass. Affected production-library clippy
passes with the two previously recorded app lint-category exemptions.

On 2026-10-04, the rebuilt debug client was inspected on Apple M3 Pro/Metal,
ordinary vanilla mode, a Retina display and a 1280-by-720 logical content window.
Front, oblique and different animation frames show the tall eight-plane enclosure
on netherrack, transparent flame edges, original orange/white texels and rising
black/gray smoke. A separate live frame confirms blue soul fire on soul sand;
its lowered model now retains the independently proven native support fact.
These checks resolve the reported low crossed-plane block-fire defect.
The fixture fixes noon/clear weather, a flat world and camera eye positions
`(.5,-58.38,-3)` / `(3,-58.38,-2)` / `(3.5,-58.38,-3)`, with unchanged client FOV.
An ignored local loopback server supplies those states through the pinned
protocol; it is a test-data supplier, not a vanilla behavior reference.
Docker's console API stalled, so the saved BDS world was preserved and this
fixture supplied final captures. Keyboard focus/input parity was not closed.
Captures and fixture code remain local and outside git.

The complete support/flammability admission inventory remains incomplete.
Wool's nonzero catch component, glass and soul-sand support, leaf support rejection
and partial block orientation have identified references. Ordinary cube support still uses
a provisional native-default fallback; legacy wood/leaves catch components are
unadmitted. This does not close attached-fire parity. Fixed-point carrier geometry
quantizes native float coordinates to 1/256 block. Exact current shader cutoff,
matched-version live frames, soul-fire smoke and ambient crackle audio remain
open; none is silently treated as verified.

The user accepted block fire and requested a direct push to `dev` with further
verification skipped. Integration preserves `dev`'s world ownership and HUD
emote support; smoke sampling follows its Bevy-free particle owner. The checks
and live captures above precede that integration. Post-integration builds,
tests and the affected-verification command were skipped at the user's request.
