# Phase 5 — Interaction, inventory, UI

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


Scope: block breaking (server-auth crack progress overlay), placement, item use via
`InventoryTransaction`/`ItemStackRequest`; hotbar + survival/creative inventory + containers
(chest/furnace/crafting UIs); forms (modal/menu/custom JSON forms — Lunar's ClickUI depends on
these); chat with Bedrock formatting codes; HUD (health/hunger/armor/air, bossbar, scoreboard,
title/actionbar); Bedrock bitmap font rendering from pack `font/` assets. Taste bar applies:
UI phases get fable-5/opus-4.8 review before merge.

Empty vanilla buckets use a liquid-enabled block-use pick for source water and lava,
with the selected item resolved through the negotiated registry. Mining and outline
picks keep their ordinary eligibility. Flowing liquids and solid occluders retain
their existing rules; inventory remains authoritative until the server updates it.
Full bucket prediction and component-defined liquid picking remain incomplete.
Source-water, source-lava, and powder-snow collection were observed against a
local Bedrock server. Waterlogged-source acceptance remains unverified; this does
not close the interaction parity gate.

Dynamic container registry cleanup retires only matching retained bundle contents
through the session inventory FIFO. Other containers and item stacks stay intact,
and cleanup frees capacity for subsequent dynamic containers. Focused wire and
ledger tests cover cleanup, ID reuse, retention pressure and session reset;
exact stable-client gameplay validation and full inventory parity remain incomplete.

Middle-click picking includes mobs and paintings, and asks the server for the
nearest eligible actor's item using its persistent ID. Mob spawn eggs remain
server-authoritative; walls, reach, and mounted-actor exclusion still constrain
the pick. This does not close the broader interaction parity gate.

**Phase 5 roadmap (kept current as work lands):**

**Approved gameplay-HUD presentation deviation (expanded 2026-07-19):** the
in-game HUD may use the pinned Java Edition 26.2 presentation for chat, hotbar,
scoreboard, hearts, hunger, armor, air, experience/level, and applicable
mount/offhand/effect/attack-indicator surfaces. Bedrock remains authoritative
for packets, attributes, equipment, inventory, game mode, combat timing, and
reconciliation; Java presentation must not invent state that Bedrock does not
expose. Menus, inventories, containers, forms, controls, and resource-pack JSON
UI remain Bedrock/resource-pack-driven. The current text/panel renderer is an
incomplete scaffold until the full state matrix and native/live comparison gates
below are green. See `AGENTS.md` for the repository-wide gameplay-HUD exception.

Zero-timestamp local attribute updates may follow specified server ticks without
ending the session or rewinding effect countdowns. The shared presentation clock
preserves the last nonzero packet fence; FIFO, session, local-time and specified
backward-tick rejection remain enforced. The captured CubeCraft failure sequence
replays through its final zero-tick damage update without ending the session. A
fresh live match remains pending; this bounded receive-path correction does not
close the full HUD parity gate.

Hunger shakes use renderer-local updates and neutral/upward offsets, independent of
food packet ticks. The complete native HUD motion gallery, including heart timing,
remains incomplete; this correction does not close the Phase 5.7 parity gate.

**Bounded native HUD tranche (2026-07-19):** the protocol-1001 carrier and
retained presentation now provide provenance-pinned health, hunger, armor, air,
hotbar, selected-slot, chat, and scoreboard data paths. Survival geometry is
bottom-centered across the tested Windows window sizes, uses the pinned
`hud_screen.json` cap alpha, and fails closed when the carrier is absent or the
viewport cannot contain the fixed-scale bar. This does not complete Phase 5:
automatic vanilla GUI-scale selection is still unimplemented (the owned
Windows classic profile remains pinned to logical scale 2), nonstandard
attribute maxima remain hidden until stacked/compressed-row authority is
implemented, held item stacks are not drawn in the hotbar, and scoreboard/boss
surfaces remain fail-closed until their full native authority is owned. The
normal/maximized native WGC comparison and live third-party checks remain part
of the 5.7 gate.

**Official sample authority and unified build closure (2026-07-19):** the
pinned Mojang `bedrock-samples-v1.26.30.32-preview-full.zip` contains the exact
25 HUD PNG byte sequences used by the bounded carrier plus the five reviewed UI
authority JSON files. `make client` now depends on the single `assets` umbrella:
it reacquires and verifies a missing extracted sample pack once, builds the
world, atmosphere, entity/animation, open-font, and HUD carriers and reports,
verifies physics assets, and launches only after every prerequisite succeeds.
The generated Mojang payloads remain ignored local data. The same audit confirms
that inventory/container/menu references, 1,612 compiled item visual routes,
entity rigs and animation payloads, particles, and most sound effects are
source-unblocked; their UI screens, held/viewmodel/dropped-item rendering,
animation evaluation, particle renderer, and audio runtime remain implementation
work and are not closed by asset availability. Base Latin Mojangles, 185 sound
binaries, and block-render model JSON remain absent from the official sample
archive.

**Approved Java-style gameplay HUD contract (2026-07-19):** the visual
reference is Minecraft Java Edition 26.2 with default resources on
Windows 11. Acceptance covers survival, creative, and spectator; normal,
damaged, absorption, poisoned, withered, and frozen hearts; normal/depleted
hunger; air; XP/level; armor present/absent; mount health/jump; main/offhand;
attack indicator; selected-item label; effects; and scoreboard/chat overlap.
The armor row appears above hearts only while authoritative equipped armor is
nonzero. Capture GUI scales 2, 3, 4, and Auto at 1280x720, 1920x1080, and
2560x1440 with 100% and 150% desktop scaling where applicable, then validate
equivalent logical layout and safe areas on supported macOS Retina output.

Delivered so far: Java-style scoreboard/chat presentation, centered hotbar
selection, local number-key/wheel/controller slot prediction with outbound
`MobEquipment`, experience attribute retention, and XP bar/level presentation.
Still incomplete: hotbar item icons/counts/durability, authoritative selected
stack, armor derivation and conditional row, nonstandard maxima, full state
matrix, GUI scaling/safe areas, and native/live comparison. Java chat fade-out
remains pending measured timing.

**Gameplay HUD through JSON-UI (2026-09-29, owner decision):** the HUD renders
`hud.hud_screen` and `hud_crosshair.hud_crosshair_screen` through the JSON-UI
engine over the session's pack stack, so server packs restyle it as on Bedrock.
The Java look ships as the built-in pack `assets/java-hud` under every server
pack. Partial server definitions overlay that layer; authored server replacements
and positioning take priority. The JSON-UI carrier is now a
required startup carrier. Native renderers (hearts, armor, hunger, bubbles,
mount hearts/jump, slot art, effects, crosshair) keep Java behavior at their
controls. Container, inventory, creative and book screens draw through the engine
(see `docs/tracking/vanilla-parity-gaps.md`). Still on the old path: the first-person
hands, the sleep overlay, toasts, the tab list, the open chat (editor and
history), nametags, and the debug overlay. Incomplete: no live rendered-frame
pass yet; `font_size` steps and the text-background option default are
unmeasured; a re-bind costs about 1.6 ms in the dev profile (steady frames about
0.3 ms), unmeasured in release; boss-bar progress and XP changes re-bind.

**Text on the device pixel grid (2026-10-09):** vanilla draws a font pixel over
exactly GUI-scale device pixels and truncates text origins to whole device pixels.
Text laid out at fractional display scales (Windows 125%, 150% and 175%) used to
measure in logical 1/64 pixels with a rounded scale, so glyphs drifted off the
device grid along a line. Nearest sampling then gave some letters' strokes one
device pixel more or less than their neighbours'. Text from the frame's text
metrics, and the Java-look book and window text, now measures in device pixels
whenever each font texel spans whole half device pixels, as Cinnangles Sans does
at every integer GUI scale. Display scales 1 and 2 lay out unchanged. Text at
fractional font scales (`font_scale_factor` 0.8, `small` at odd GUI scales) and
outline OreUI fonts keep logical-pixel layout. Incomplete: no live Windows
rendered-frame pass at a fractional display scale yet.

- [ ] **5.1 Bedrock UI foundation.** `P5.1-UI` Create `crates/ui`, ingest the pinned pack's bitmap
  fonts/glyph metrics, implement bounded formatting-code-aware text layout, UI scaling/safe
  areas, focus/navigation, mouse/touch/controller input, and a shared retained draw pipeline.
  Prove no per-glyph mesh/material churn and exact cross-platform DPI behavior.
- [ ] **5.2 Receive-only server text and HUD state.** `P5.2-HUD` Normalize bounded `Text`, title,
  actionbar, toast, player-status, health/hunger/armor/air, and related lifecycle packets into
  vendor-neutral stores. Render chat history, title/actionbar, and the survival HUD without
  permitting UI focus to leak movement input.
- [ ] **5.3 Interactive chat.** `P5.3-CHAT` Add chat focus/history/autocomplete, Bedrock formatting,
  bounded UTF-8 editing and clipboard behavior, then send through the Go core with session/FIFO
  identity and spam-safe rate limits. Verify third-party server receive/send and disconnect
  behavior.
- [ ] **5.4 Scoreboard and boss bars.** `P5.4-SCOREBOARD` Normalize objective/display/score and boss-event
  create/update/remove packets into independent bounded lifecycle stores. Render sidebar/list/
  below-name objectives, score ordering, boss health/style/count stacking, and title/actionbar
  coexistence with deterministic replacement/removal tests.
- [ ] **5.5 Interaction, hotbar, and inventory.** `P5.5-INTERACTION-COMBAT-INVENTORY` Implement server-authoritative break cracks,
  placement/use, selected slot, item stack/network-ID reconciliation, creative/survival
  inventory, and chest/furnace/crafting containers with rollback on rejected stack requests.

Block cracks retain one ordered entry for their speed and rendered progress.
Repeated starts keep the active crack, speed updates preserve progress (including
zero speed), and completion removes the entry before later events. Progress uses
the current speed and elapsed real time on each unpaused rendered frame; only
positive progress draws the ten-stage overlay. Native rendered comparison remains
open; this does not close the Phase 5.5 acceptance gate.
Mining prediction uses single-precision accumulation and rounds each effect's
multiplier separately.

**Throwable timing (2026-10-09):** the native 200 ms action delay remains
separate from item-category cooldowns. The opt-in Item Use Delay Fix permits
one use after a slot change even between frames sharing an upcoming tick.
Pearl, wind-charge and chorus-fruit cooldowns stay in their own categories;
chorus fruit starts its 20-tick cooldown when use begins. Deterministic tests
cover rapid slot changes, delay boundaries, rejected sends and hotbar cooldown
publication. Live gameplay
acceptance remains incomplete; this does not close the interaction gate.

Owner-designated inventory reference (2026-09-06): use Lunar's inventory-management
implementation when establishing these contracts. Pin the inspected source revision
and verify Cinnabar's end-to-end request/response behavior; the reference designation
does not itself close Cinnabar's inventory implementation or native acceptance gates.
The inspected Lunar revision is `f8cccf30a296c82e2af95161b856587937c3a0b6`:
`lunar/internal/inventory` owns packet/action-level prediction and reconciliation,
not physical mouse or drag input. The native ingress blocker and basic whole-stack
transfer/Swap workflow are verified by the bounded checkpoint above. Next add
half-stack and one-item transfer through the existing Cinnabar ledger; occupied
stack merging still needs item compatibility and capacity handling. Drag requires
multi-action requests. Neither backend conformance nor an input binding alone
closes the native inventory parity gate.

Bounded follow-up diagnosis (2026-09-07): occupied merges need a version-pinned
item-rules registry with exact per-item maximum counts and session/registry
generation handling. Current negotiated component data is retained only as a
digest, not usable stack-capacity rules. Unknown/custom items cannot inherit a
universal limit. Explicit-count requests should retain their checked amount;
any whole-stack selector computes available capacity before making the request.
Full compatibility also needs semantic item-data comparison, not equality of
counts or stack network IDs.
The independently reviewed capacity foundation `8839da37` is locally integrated:
1,485 exact retail identifiers have measured metadata-zero capacities from
public BDS 1.26.40.8, with deterministic generation and pinned provenance.
Its lookup returns no capacity for unknown identifiers or metadata variants;
it does not enable occupied merging or supply negotiated session authority.
The separately reviewed normalized-evidence tranche `c35a9f46` retains the exact
positive server component stack limit and canonical-empty-component marker,
preserves the original digest, and treats unsupported semantic evidence as absent.
That foundation added no runtime merge consumer. Fresh generator tests and vet
passed after integration. Combined foundation workspace tests passed with
3,499 tests, zero failures, and 13 ignored; formatting, strict workspace lint,
architecture validation, and the normal client build passed. Both exact source
heads received independent approval with no findings. These checks validate the
capacity foundations, not runtime merge acceptance.
The pushed foundation checkpoint `bd5c3354` passed all six hosted CI jobs in
run `34105977898`, including Windows acceptance.
The occupied-transfer implementation `343902db` is locally integrated after fresh
independent approval with no findings. Registry markers and inventory updates share
the bounded UI FIFO; session registry binding supplies negotiated or verified bare
retail capacities. Primary compatible clicks merge up to free capacity, secondary
compatible clicks place one, and incompatible primary swaps remain available.
The bounded compatibility subset is metadata-zero retail items with no retained
block identity, verified empty item data, and no meaningful response overlay.
Unknown or ambiguous same-item shapes do not merge; this is not full semantic
compatibility. Identity replacement and capacity-only updates have distinct
recovery behavior, and residual source/destination identities must remain distinct.
The exact implementation passed 929 client-library tests, 17 inventory integration
tests, focused merge/input/FIFO witnesses, strict lint, formatting, and architecture
checks. Combined post-integration workspace verification passed 3,531 tests with
zero failures and 13 ignored; formatting, strict workspace lint, architecture,
and the normal Windows build passed. Native acceptance then found that a 33-apple
source did not merge into a 60-apple destination; BDS retained 60 and 33 after the
source was placed in an empty cell. The captured login registry was not propagated
to runtime inventory authority. The corrected bootstrap fix `14393c06` is now
locally integrated after fresh independent approval with no findings: captured
registry data reaches the ledger before authority and gestures, malformed wire
fails startup, and semantic rejection remains nonfatal with a redacted warning.
Its routing tests passed 9/9 and client-library tests passed 942/942, with strict
lint, formatting, and architecture checks green. Combined verification passed
943 client-library tests and 20 inventory integration tests, strict app lint,
formatting, architecture, and the normal Windows build. Native BDS 1.26.40.8
then confirmed primary apples 33 + 60 became 64 + 29, empty buckets 3 + 15 became
16 + 2, and secondary apple placement moved exactly one (29 to 30, remainder 63).
All observed totals were conserved. Full-target no-ops, incompatible swaps, and
27/54-slot storage acceptance still require native checks; not pushed.
The 2026-09-07 native storage diagnostic used the existing normal Windows/DX12
build at `3eff46cf`, 1280x720 and GUI scale 2, against the isolated BDS 1.26.40.8
world. Test chests rendered, but normal gameplay right-click did not open storage.
Code inspection at that checkpoint found no production consumer of the click-block
packet builder. The diagnostic ended
with clean client/server shutdown and restored the original local server runtime.
A bounded empty-hand creative keyboard/mouse block-use implementation `614134e5`
is now locally integrated after fresh independent approval with no findings.
It attaches one initial Use edge to the completed movement tick, mutually exclusive
with mining, and revokes stale queued interactions without dropping movement.
Pending inventory recovery or hotbar changes suppress use; the server alone opens
storage. Independent focused checks passed: protocol use 3, movement fixtures 7,
runtime use 9, mining 23, and network revocation 1. Combined post-integration
checks passed as recorded above. Native empty-hand use opened a server-driven
inventory surface, but chest contents were not displayed: the personal layout
appeared without the player preview, then eventually closed. Container-content
identity/admission needs a live diagnostic witness before any broader routing
change. Native storage acceptance remains open; this tranche is not pushed.
A separately reviewed diagnostic `2f7a7535` is locally integrated to record a
bounded, opt-in prefix of storage-sized content identities and their ordering
relative to an open window. It does not change admission or log item payloads.
Its focused regression, strict app lint, formatting, and architecture checks
passed. The native witness showed 27-slot content carrying container-name code 0
after the matching generic-storage window opened; the projection left it
unrouted. Unrelated 54-slot traffic on window 124 used the same code and must
not become storage authority. A narrow contextual routing fix is in progress;
this evidence does not establish a global container-name alias. A real 54-slot
chest fixture is prepared, but its native content/transfer gate is still open.
This remains provisional functionality. The native click carrier, optional envelope,
reach, simultaneous-action priority, and repeat cadence are not yet established;
generated encoder fixtures do not close those reference gaps. Server acceptance
and a real chest interaction are required before shipping even this bounded path,
and full native interaction parity remains open afterward.
Generic server-container admission now retains nonempty inventories through the
one-byte stack-request slot address range instead of requiring vanilla chest row
counts. Storage artwork, hover-name capture, and JSON-UI collections use that same
bound, including high-index custom menu controls. Jaya's 200-slot market has a
native Android 1.26.50.4 witness: the menu stays open, while the earlier client
requested an immediate close. The current client now retains that live menu;
per-stack custom names also override localized item names in its collection data.
Live input now opens the market with its authored title, search/sales controls,
listing icons and pagination labels; the menu stays open until an explicit close.
Price/quantity text, panel art and exact layout still differ from native.
Incomplete: inventories beyond the retained address range, full custom binding and
visual parity, and platform/version coverage beyond this witnessed server route.
This does not close the storage or JSON-UI parity gates.
Missing block-item artwork is a separate route gap: compiled block visuals exist,
but the icon carrier emits only sprite routes and inventory publication reduces
the canonical route to an identifier. Preserve the visual route and design a
bounded thumbnail path from exact compiled geometry/materials; do not infer a
generic block icon from an item name. Projection, shading, all visible surfaces,
and native rendered acceptance remain open.
The same native run also exposed a separate sprite crosswalk gap: the modern
water-bucket identifier has no alias to its existing water sprite. A verified
modern-name-to-atlas mapping, compiler/resolver/icon tests, and native icon
acceptance remain open; this does not require a block-thumbnail renderer.
The crosswalk is now `crates/assets/data/legacy-icon-routes-26.30.tsv`: every
legacy icon assignment the 26.30 client makes during item initialization, plus
potion icons by aux. Still open: cooked foods and
other 1.10 JSON items (icon from resource-pack data), spawn eggs (per-entity
map), bow/crossbow draw frames, trimmed armor and broken elytra overrides, and
native icon acceptance.
Compasses and clocks animate: the icon carrier bakes every frame of the native
`compass_atlas`, `recovery_compass_atlas` and `watch_atlas` strips, and the local player's needles follow
the native sprite calculators once per tick (compass toward the world spawn,
clock by the celestial angle, both spinning outside the Overworld, with the
native 0.1 pull and 0.8 damping). Inventory, held and dropped dials share the
local player's frame, where the native client reads each holder. Recovery
compasses aim at the player's last death (actor data 127 to 129, kept across
dimension changes) in its own dimension and spin elsewhere. Lodestone compasses
and item-frame headings remain open.
Block items (provisional, incomplete): an item the retail client gives a legacy
icon, or one placing a differently named block, keeps its sprite over its block
route. Flat-shape blocks (cross plants, torches, rails, panes, ladders, lanterns,
candles, chains) draw vanilla's block-item icon: carried texture, else
texture, down face. Other non-cube shapes draw their isolated world template (a
wall item shows post plus east/west arms, a fence post plus arms); leaves draw
their carried cube. Retail block items missing from the Dragonfly route table
bind to their block's first canonical state. `assetc icon-assets` reports the
leftovers: block-entity items (chests, shulkers, beds, banners, pots, statues),
world-diagnostic cubes, grass (overlay tint), and seeds without a sprite source.
Shape classification, stair orientation and GUI shading are unverified against
the retail GUI tessellator. Head items (provisional, incomplete): every head draws
its placed skull model (player heads with the default skin; dragon and piglin heads
the pack geometry's head bone tree, shared with the placed renderer through
`assets::block_entity_geometry::heads`) turned to face south like other block items'
fronts, its uninflated bounds scaled to fill the cube frame; vanilla's skull GUI
transform and lighting are unmeasured.
Server item components (StartGame registry) drive display names, rarity and
hover colours, durability maxima, stack-merge capacity, held grip, wearable
slots and use durations. Item glint: stacks marked as glinting (an `ench` list,
the glint component, always-glinting items) sample the active pack's item glint
raster in HUD and JSON-UI item cells. Removing the pack restores the startup
UI carrier's image. Startup rejects stale carriers and missing or invalid base
glint artwork with the carrier path and `make assets` rebuild guidance.
UI UV scale, blend and scrolling periods remain provisional pending rendered
acceptance. Held items in both views draw vanilla's item glint:
the `enchanted_item_glint` raster, colour (0.38, 0.19, 0.608) scaled by strength,
1375/3750 ms scrolls, -20/80 degree layers, and a 0.5 UV scale across each atlas
sprite with the scroll divided by that scale, as the native item-in-hand renderer
sets them. Attachables take the item raster at unit scale. Dropped items draw
the same glint on every stacked copy. Exact pixel comparison remains open.
`minecraft:render_offsets` is not applied.
Initial negotiated item-registry binding for world item visuals is also separate
from the inventory-ledger bootstrap fix. The visual resolver still starts from
built-in mappings and only replaces them on a later registry event. Preserve
session/FIFO ordering when adding that initial binding; do not claim custom-item
visual authority from the merge fix alone.

Entity combat is part of this tranche and is strictly vanilla:

Pick ranges, box inflation and the strict attack/occlusion margin use the native
float limits. Personal reach requests can only shorten those limits. Fresh attack
presses may each swing before the next tick advances the animation counter.
The attack click is the world-space intercept of a float segment with the inflated
pick box. Remote movement steps multiply by the rounded reciprocal tick count.
Forced MoveActor updates below half a block interpolate in resident terrain;
missing target columns snap. Scaled collision dimensions retain a 0.005 minimum.
Latency probes share actor commit ordering, including parked movement and metadata.
Incomplete: full pose-dependent reach, per-entity pick radii, catch-up-frame attack
positions, specialized actor motion, and complete batch/latency timing still need
end-to-end verification. Terrain availability during asynchronous chunk ingestion
and native-origin rounding also remain unverified. These
changes do not close the combat parity gate.

Block rays now normalize, traverse and clip in f32 against rounded world endpoints.
Block-local clicks, placement obstruction and body-facing decisions round before
comparison. Actor picks share the normalization boundary. Traversal limits and
nearest-hit decisions use segment fractions so endpoint rounding cannot change
their units. Incomplete: native block tie/union rules, camera-direction generation,
and runtime witnesses still need verification; these precision fixes do not close
the interaction parity gate.

Remote actor snapping respects target-column terrain order. Block-use and painting
occlusion share rounded intercept distances. CI verification remains pending.

Selected transaction descriptors omit the optional stack network ID for actor use,
block use/destroy, air use, item-directed attack and release, including the shared
embedded movement payload. Serialization regressions preserve item contents and
inventory action identities. Admission, dynamic actions, NBT rewriting and combined
runtime acceptance remain incomplete; this bounded correction closes no parity gate.

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

- [ ] **5.6 Server forms.** `P5.6-FORMS` Implement modal/menu/custom JSON forms, validation, cancellation,
  keyboard/controller/touch navigation, and response routing. This is the prerequisite for
  Lunar ClickUI compatibility.
  Status: server resource-pack UI now reaches the JSON-UI form engine (per-layer indexed documents
  merge, `$screen_content` routing, expression, view-binding, and vanilla factory rules), with a resolve/layout cache and raw-edge pointer input. Checked only in
  tests against local pack fixtures, not against a live vanilla capture; the virtual-root scale
  constants, per-visual-line label alignment, and image aspect defaults remain unconfirmed.
  UI document paths are relative to the pack root and selected by the stack's merged indexes,
  including custom folders and extensions. The web editor uses the same selection for imports,
  previews, diagnostics, and exports, and shows complete paths in its file list.
  Form display fields now retain structured text for language and live-state resolution,
  including metadata, component precedence, nested text, and scalar display fallbacks.
  Custom forms include multiselect checkboxes, ordered index-array responses, and slider
  timeouts, scoped dropdown options, custom icons, and float slider edits with localized
  value labels. Custom controls and option lists use the document byte budget instead of
  a separate 256-item cap. Menu normalization prefers non-null buttons and ignores unused metadata.
  Unselected edit boxes leave focused controls available through global button mappings.
  A sanitized 346-button featured-menu capture guards complete selection indexes; that
  menu already renders after the earlier button-limit fix. Broader visual parity remains
  incomplete until matching native captures verify these controls at the supported scales.
- [ ] **5.7 UI parity and performance acceptance.** `P5.7-PARITY-PERF` Compare matching vanilla reference views at
  supported scales/aspect ratios, test keyboard/mouse/controller/touch focus transitions, and
  prove bounded retained memory plus stable frame time with chat, scoreboard, boss bars,
  inventory, and forms active together.

- [ ] **5.8 In-game menu, controls, video settings, and persistence.** `P5.8-SETTINGS`

  Desktop GUI scale modifier and fullscreen/F11 wiring are implemented,
  including preference persistence. The modifier range and scale rule use the
  vanilla desktop behaviour and the controls use the pinned vanilla JSON UI;
  see [desktop video settings evidence](../../docs/evidence/desktop-video-settings.md).
  **Incomplete parity:** the transferred behavior has not been compared with a
  version-matched native client. The settings context selects the pinned pack's
  compact spacing branch; Bedrock's service-controlled spacing treatment is
  not mirrored. Language-specific minimum-scale dialogs,
  safe-zone adjustments, and touch/console behavior remain unimplemented in
  this adapter. Linux rendered-frame and live-input verification validates the
  local wiring only; it does not close this or any broader UI parity gate.
