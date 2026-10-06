# Hive connection and dimension transfer

An authenticated Hive hub join exposed failures in pack admission, terrain
decoding and dimension transfer. All 24
required packs carried display labels as selected subpack names, although their
manifests declared no subpacks. Admission rejected the entire required stack.
After admitting root resources, persistent block palettes decoded as air. After
resolving those palettes, terrain appeared 64 blocks below the server's actors
and player because the named dimension definition was ignored.

The hub then supported walking, jumping and the server's game-selector menu.
Selecting SkyWars changed dimension to a staging position at Y 4000. Prediction
continued falling and no dimension acknowledgement was sent; Hive disconnected
with "You didn't finish joining." The destination handshake now pauses prediction
and retains the loading-screen identifier.

Captured persistent identities also exposed missing custom floor blocks. The
session used sequential wire IDs, so the custom overlay omitted its canonical
hash table. Persistent entries therefore could not resolve advertised blocks
such as `hive:cream_brick` and `hive:stone_herring_bone_bricks`; their existing
solid collision shapes never reached the world. Custom overlays now retain
complete identities in either wire mode, preserving visual and collision slots
when an individual definition cannot provide a complete identity.

Offline replay additionally exposed early decode snapshots taken before the
asynchronous custom artwork finished. The session world registry now installs
custom identities before terrain admission; immutable worker snapshots retain
them independently of artwork publication.

The captured definition named `minecraft:overworld`, with minimum Y 0, height
256 and numeric dimension type 3. StartGame and LevelChunk used dimension 0.
The definition arrived before StartGame and remained ahead of chunks through
the login handoff. Pack archives, packet captures and rendered frames remain
outside git.

| Vanilla rule | Implementation |
| --- | --- |
| An unavailable selected subpack falls back to root resources. | Validate the root manifest before selecting a declared folder; retain the wire label as metadata. |
| Persistent palettes identify blocks by name and typed states. | Decode network NBT, qualify vanilla short names and resolve the canonical identity in the active registry. Unknown entries use air. |
| Persistent custom identities are independent of the session's wire ID mode. | Keep the custom hash lookup in sequential overlays too; incomplete identities skip only their own slots. |
| Advertised block identity is available before resource artwork. | Install canonical identities in the session world registry before terrain decoding and retain them in immutable worker snapshots. |
| Legacy cube textures come from `blocks.json` when explicit visual components are absent. | Apply scalar, three-face or six-face bindings in stack order before caching the effective visual. |
| A custom block without a collision component retains a full-block shape; disabled collision is empty. | Resolve the advertised block before applying its existing collision policy. |
| Named builtin definitions override their dimension's default bounds. | Retain the definition name; the overworld key selects dimension 0 independently of the definition's numeric type. |
| Definitions retain their first registration; instantiated dimensions retain their height. | Admit definitions before later decode snapshots and freeze a dimension's effective range when terrain first uses it. |
| Terrain consumers share one dimension range. | Decoding, requests, residency, collision queries, block entities and sky ceilings read the session owner. |
| Dimension acknowledgement belongs to the session, including sentinel actor IDs. | Admit server action 14 without filtering its actor ID. |
| Transfer completion waits for the server acknowledgement and a loaded destination area. | Wait for the acknowledgement, or a timeout strictly over ten seconds followed by another tick, and check inclusive position ±16 bounds. |
| Readiness follows the current player position and skips sections outside the dimension height. | Accepted server teleports update the anchor; authoritative air columns count as present. Out-of-range Y uses the dimension's loading fallback (0, or 50 in the End). |
| Loading notifications retain the same optional identifier. | Queue LoadingStart, the local action-14 acknowledgement, and LoadingEnd in order; retry only writes not already admitted. |

Regression coverage includes required pack stacks, NBT stream boundaries,
every pinned vanilla typed state, both runtime ID modes, early login definitions,
inline placement, request origins, pending decode ordering and invalid ranges.
Transfer regressions cover sentinel acknowledgements, metadata and packet
roundtrips, readiness boundaries, authoritative air, current-position changes,
prediction holds with continuous stationary input ticks, timeout ordering,
queue backpressure and production app wiring.
Custom identity regressions cover named states, missing visual resources,
incomplete definitions beside valid neighbors, admitted ranges, offset overflow,
asset precedence and snapshots retained across registry replacement.
Visual regressions cover legacy face bindings, per-block cache identity, valid
lower bindings beneath malformed overrides, geometry-specialized animation
instances, large artwork page sets and immutable geometry aliases.

The live checks used macOS Metal on Apple M3 Pro, a 1280×752 logical window at
2× display scale, vanilla render mode and a debug build. Hive hub walking,
jumping and the compass game-selector menu worked. SkyWars transferred to the
destination and supported movement after the acknowledgement fix, but later
disconnected. A subsequent movement-only run ended with an "Unfair Advantage"
ban showing expiry `6d 23h`. Live connections stopped. That opaque server verdict
does not establish its cause; final live acceptance after the custom identity
correction was blocked until the user reported the account unbanned.

The final offline run used build `8d32996a`, eight captured terrain columns, one
synthetic air neighbor and a local teleport anchor. All 1,581 advertised custom
states were registered before artwork with zero skipped definitions. Walking
reached the captured `hive:cream_brick` at [-11, 39, 0]; the eye rested at Y
41.62001, jumped to 42.87221 and returned to the same floor. Fresh rendered frames
showed the restored custom surfaces and working input. The local bridge omitted
encrypted server artwork and ignored gameplay requests: diagnostic textures were
expected, and this run establishes neither server acceptance nor visual or
performance parity. Its complete replay report ended on the local client's exit.

Persistent legacy-state upgrades, default-state reconciliation and unknown
property handling remain incomplete in `plan.md`. This work does not close the
broader terrain or server-pack visual parity gates. The local readiness delay
currently uses a later app frame; the vanilla readiness updater's exact scheduling
clock remains unverified.

On 2026-10-05, current dev was integrated after the user reported the account
unbanned. Transfer ownership was consolidated into the app coordinator, retaining
the server acknowledgement and loaded-area gates. Input ticks continue while
prediction is held; LoadingEnd now waits for the JSON-UI loading presentation and
a fresh destination frame. Three reproduced regressions cover named overworld
probe heights, raised custom air columns and synced-block range freezing.
The renewed hub join admitted all 24 packs and registered all 1,581 custom states,
but live frames showed diagnostic custom terrain and missing custom actors. Many
blocks use legacy `blocks.json` texture bindings that the visual compiler omitted;
one cached diagnostic visual concealed hundreds of missing bindings. The entity
bundle separately failed the compiled animation-table bound. After correcting
that bound, the captured stack compiled 1,014 artwork bindings and 1,777 textures
without dropping source files. Publication initially rejected 636 bindings at the
artwork page ceiling and rejected the combined vanilla/server vertex catalog.
CPU artwork page identifiers now retain all 551 pages with zero rejected bindings.
Exact immutable vertex payloads share storage while keeping their independent rig
metadata and routes; the combined catalog publishes 548,922 vertices within its
existing bound. The captured-stack publication regression passes, all 25
block-overlay checks pass with the encrypted fixture, and all 585 renderer checks
pass. These offline checks retain original artwork and geometry.

The following live Metal run restored custom floors and NPC models. User frames
still exposed opaque title backs, black hologram panels, floating sheep, dark
flowers, incomplete climbing vines and lamps, and diagnostic hay. The hub stayed
connected for about 23 minutes before an opaque server Disconnect message ended
the session. Its cause is unresolved. Subsequent client startup found the core
endpoint absent; no new successful join is claimed from that attempt.

Offline inspection reproduced a 32-face truncation on the 33-face climbing vine
and 40-face large lamp, omitted legacy light filters and authored material states,
rejected 41-choice geometry selectors, and stale absolute registry-ID checks for
hay. Focused regressions now pass for those fixes, all 41 captured hologram
variants, authored GPU material states and targeted-entity F3 diagnostics.
Lantern bodies, caps and crossed handles replace the collision-box fallback;
their sprite sampling survives carrier publication. Geometry measurements use a
near-version witness and remain fallback support pending exact-version evidence.
NPC controller particle bindings and alpha-first hex tint decoding also pass
their owner and app route regressions. The following inspection build joined Hive
with no missing textures, missing geometry, truncated models or unevaluated
permutations in its block overlay. User frames show readable game titles, restored
plants and visible NPC particles, while title backgrounds remain too pale.
The user identified the floating-text hosts as sheep with server scale zero.
The client now preserves finite scale zero and suppresses body/equipment draws
without removing their independent name tags. Its spawn/update regression passes.
This inspection session ended after about nine minutes with another opaque
server kick and zero decode errors. Reconnection is not session acceptance.
Full visual acceptance and the unresolved disconnect remain open in `plan.md`.

The next local inspection build joined Hive on macOS/Metal with ordinary rendering
at a 1280×752 logical window and 2× display scaling. A fresh frame shows the gold
logo, readable game titles on dark panels and no floating sheep. The panel GPU
regression verifies encoded destination blending with the production shader and
attachment format. Enhanced/HDR/MSAA actor blending and exact cross-family order
remain incomplete. Actor pipelines now prewarm behind the loading gate; authored
NPC emitters survive ten minutes until their state/actor ends. Their regressions
pass. These changes and dev integration are locally committed, not pushed; the
running inspection remains separate from full session/performance acceptance.

Current dev's GPU culling and VSync controls were then integrated and rebuilt.
The repeated Metal pass shows the same corrected titles and hidden text hosts.
All 889 renderer checks pass, including the encoded-panel raster and new culling
checks. The app suite passed 1,054 checks; its remaining equipment-capacity fixture
now passes a focused rerun after receiving distinct, valid equipment geometry.
Cache reuse and corruption recovery preserve catalog/artwork and material inputs.
Formatting and the architecture gate pass. The test client remains open for the
user's gameplay check; no release performance or disconnect gate is closed.

The following gameplay check reported arm swings without a selector response,
selected-item text crossing status rows, and chat returning to the top-left.
The actor-store regressions reproduced omitted-dimension rejection and ignored
server scale. The gameplay regression reproduced a missing NPC pick and retains
the picked runtime ID and world-space hit point in its attack transaction.
The HUD regressions reproduced namespace-wide withdrawal from a partial server
style edit: chat moved from the bottom to the top, and selected-item text lost
its built-in clearance. Layering now retains the built-in HUD below ordered server
overlays, with explicit server positioning still winning. F3 shows effective
hitbox dimensions and attack logs report the target, packet kinds and transport
result. The next manual Hive check reached game selection and a destination lobby.
The attack log picked the BedWars and Murder Mystery selectors with their server
scales of 1.85 and 1.9 and queued their attack transactions. The user confirmed
interaction and the bottom chat placement, then reported command suggestions at
the top and an unexpected dimension-loading animation. Those presentation checks
remain open. Actor post-fix tests are deferred at the user's request; the HUD
composition and corrected native-chat fixture checks passed. Zero-scale minimum
box clamping, definition-specific boxes and native item-name offsets remain incomplete.

The command list inherited a nearly full-height grid, so its first row appeared
near the screen top. Its height now follows the native collection count and row
template, retaining the bottom anchor and indexed selection. A distinct dimension
loading stage suppresses the join animation and retains the destination backdrop,
title and terrain message. The ordinary no-bar policy is supported by current
handler initialization and a near-version native dispatch witness; exact current
dispatch remains an open parity gate. The chat/loading regressions are authored
but unrun at the user's request. These inspection changes remain uncommitted.

The next report identified flicker in the Murder Mystery NPC's magnifying glass.
Its lens has two coincident opposing faces with different UVs and authored partial
alpha. The controller assigns the body material through `*`, then overrides
`magnifyingGlass` with `slime_outer`; the compiler previously discarded that bone
rule. Vanilla applies the last matching material rule to each bone independently.
The near-version vanilla material enables blending and back-face culling while
retaining depth writes; the authored highlight already has its own offset.
No additional depth bias is supported by this evidence. General pattern semantics
and the renderer's inclusive default depth comparison remain open parity gaps.
The compiler now partitions each controller by the resulting per-bone material,
preserving authored visibility and alternate/inherited geometry. The glass material
is admitted with its blended, culled, depth-writing state. Existing carrier admission
and geometry remain unchanged. Three regression checks are authored but unrun at
the user's request. The rebuilt Metal client joined Hive, but the user's manual
check still showed whole-lens flicker during movement and title flicker during updates.

Latest dev was merged locally through `3fa4d3145` in merge `7006ac138`, preserving
the inspection fixes, then through `4d0814084` in merge `c6021f566` after a fresh
fetch. Nothing was pushed. Both the Go core and Rust client rebuilt successfully.

Title-facing animation used a tick-owned camera position and left controller-selected
geometry poses unchanged between ticks. Frame evaluation now samples the current
camera position for the body and each distinct selected geometry, with shared poses
and atomic fallback on budget exhaustion. Two regressions are authored but unrun at
the user's request. The fresh Metal inspection build joined Hive at a 1280×752 logical
window; the user still reported flicker in both the lens and titles. Their visual
acceptance remains open; camera sampling and bone material routing alone are insufficient.

A captured Metal frame has the correct body/glass masks, shared transforms and
separated lens/highlight planes. The observed NPC receives no post-spawn actor
updates; title updates only change player counts. Actor queueing used the preceding
frame's span indices before resource preparation replaced the spans and buffers.
Preparation now precedes queueing, and newly invalidated bindings are rebuilt before
drawing rather than preventing queue admission. This fixes a frame-ownership defect;
two behavioral regressions cover first-frame admission and changed span ownership,
but remain unrun at the user's request. The user confirmed that movement flicker is
resolved in the rebuilt Metal client on Hive.
Developer state queries now
expose bounded published identities, materials, geometry, camera and bone matrices.
Latest dev through `41c72fb3d` was merged locally in `0bc711d08`; both binaries rebuilt
successfully. The rebuilt client was launched for the user's check; nothing was pushed.

The next native/client comparison exposed a truncated top edge on the Bridge title
panel. Its authored face extends beyond the geometry's texture dimensions. Vanilla
clamps normalized cube UV corners before interpolation; retaining overflowing
corners stretched the transparent edge texel over the panel's right side. Cube mesh
generation now clips finite UV corners while preserving signed flips and geometry.
Three focused regressions are authored but unrun at the user's request. The Rust
client rebuilt successfully. After the requested relaunch, the user confirmed the
panel correction works. It remains uncommitted; nothing was pushed.

The upgrade NPC comparison then exposed missing bold styling and faint name-tag
plates. Server formatting reaches the parser, but the name-tag atlas drew each
glyph once and shared layout omitted bold advances. Bitmap bold uses a second
glyph one design pixel right and adds one pixel to positive measured advances,
including spaces. The open-font path now shares that offset across measurement,
UI drawing and name-tag rasterization, retaining the shifted ink bounds.
Ordinary SDR name tags also join encoded-color transparency, preserving their
0.25-black plate, depth rules and record order. Near-version shaders corroborate
encoded output; exact current final attachment dispatch, native Unicode font
adaptation, HDR and MSAA remain incomplete. Regressions are authored but unrun
at the user's request. The changes remain local and await a fresh manual check.

The Rust inspection build completed successfully and was launched on Hive using
the retained Go core. A fresh Metal frame at a 1280×752 logical window confirms
the world, actors and name tags render; runtime logs contain no shader or render
validation errors. The upgrade NPC comparison remains for the user's manual
check. No avatar movement or purchase/form action was performed. Changes are
uncommitted, tests remain deferred, and nothing was pushed.

The user's next upgrade NPC check exposed split strokes in bold letters. Both
font providers store exclusive glyph rectangle ends, but name-tag rasterization
added one to each source extent. This sampled transparent padding and reduced
two-texel stems to one, leaving a gap when the bold copy was drawn. Rasterization
now follows the same exclusive bounds as layout and skips zero-area sources.
A private pixel diagnostic using the loaded font reproduces the broken title
before the change and continuous strokes afterward. Regressions cover ordinary,
bold and scaled stems plus empty runtime glyphs; they remain unrun at the user's
request. The fresh in-game comparison remains pending.

The corrected Rust client rebuilt successfully and was opened on Hive. A fresh
Metal frame at a 1280×752 logical window renders the current game lobby, with no
logged shader validation errors. No game input was automated. The title's close
comparison remains for the user; changes are uncommitted and nothing was pushed.

The next manual game-lobby check exposed the level sidebar at the right middle.
Hive replaces the outer scoreboard with a full-screen panel and anchors its
entries at the top right. That replacement retained the built-in HUD's asymmetric
outer anchors, adding half the viewport height. The built-in panel now uses
vanilla's symmetric outer anchors; its inner placement preserves the accepted
compact sidebar layout. Server priority and bottom chat remain intact. Regression
cases cover server placement at two viewport sizes and compact row counts; they
are authored but unrun at the user's request. Live acceptance remains pending.

The next SkyWars report exposed an independent movement gap: local metadata
retained the primary flags, but movement projection ignored immobility. The
version-matched vanilla movement rules suppress travel and clear all velocity
axes and active jumping, without changing position, ground state or collision
flags. Camera turn and ordinary input intent remain available. The separate
input-lock and dimension-loading paths do not define this flag's behavior.

Local-player facts now retain committed immobility before physics authorization
and across spatial anchors. Fixed simulation input captures it for prediction
and correction replay; explicit clears release it and session replacement resets
it. Immobile ticks suppress depenetration, gravity and physical jump arcs while
continuing input reporting. Terrain-unavailable pose selection retains its prior
inferred mode; exact native fallback remains incomplete. The collision registry
must remain identical when a retimed freeze changes the tick's terrain read set.
F3 and developer queries expose immobility, and transitions are logged.

Regressions cover metadata targeting/reset, airborne and embedded freezes,
missing terrain, input/look preservation, release, delayed edits and replay.
They remain unrun at the user's request. The Rust client builds locally. Live
freeze/release acceptance remains pending; no Hive reconnect was attempted
after the ban report, and the available evidence does not establish ban causality.

The user accepts the updated selection-outline visibility but reports climbing
vines retaining one orientation. The captured definition supplies four facing
states whose conditions use `query.block_property`; the running client logged
3,644 unevaluated block permutations. Vanilla resolves that spelling and
`query.block_state` through the same named-state lookup. Restoring alias admission
allows the authored facing transformations to reach model compilation. The Rust
inspection build passes. Regressions cover typed named-state reads and four
distinct runtime model orientations for sequential and hashed identities; they
remain unrun at the user's request. Live vine acceptance remains incomplete.
The existing game was left open; changes are uncommitted and nothing was pushed.

CubeCraft's captured actor definitions exposed valid engine queries rejected by
the animation compiler. A rejected query discarded the whole pre-animation
script: floating models retained their initial zero scale and the player rig
never updated its visibility weight. The compiler and actor context now admit
camera-distance range interpolation, declared-property existence, armor-slot
occupancy and default base swing duration. Camera-dependent scripts also refresh
their sampled poses when the camera or visual frame fraction changes, while
retaining completed pose storage for unchanged inputs.

The player definition also inherits ordinary player textures that its server
pack does not contain. Actor texture collection now searches the installed
vanilla pack after all server texture formats, retaining server overrides. The
selector uses collection panels with conditional control-id arrays and no
collection name; those factories now expand after their view bindings settle
and use the selected roles rather than the outer form's template id.

Regressions cover query contracts, retained scripts, camera sampling, inherited
texture precedence, conditional factory routing and nested factory settlement.
They are authored but unrun at the owner's request. Live comparison of cards,
labels, icons, scrolling, button actions, floating models, player animation and
the empty hand remains pending. Custom item swing-duration overrides and the
guard for extremely narrow camera-distance ranges remain incomplete. Changes
are local and uncommitted; nothing was pushed.

The integrated Rust inspection build completed successfully at the canonical
executable path. The existing diagnostic client was left open; the updated
binary awaits the user's requested restart and manual CubeCraft comparison.

After the user requested a relaunch, the updated client was opened. A fresh
Metal capture confirms its main menu renders at a 1280×752 logical window.
CubeCraft joining and the four live comparisons remain for the user's check;
no server connection or gameplay input was automated.

The next CubeCraft comparison shows floating models and the empty hand, while
reporting a distance-dependent opaque appearance on the central cube and a
missing VIP scoreboard icon. The compiled cube's three layers contain no authored
material state, and its unlit reflection controller's illumination multiplier
is dropped. Material behavior and the winning glyph remain incomplete.

Private opt-in capture now includes admitted material definitions and font
rasters; developer state inspection adds bounded sidebar rows while preserving
formatting and private-use codepoints. These diagnostics require a fresh manual
join. Their regressions are authored but unrun at the owner's request. No visual
gate is closed by the diagnostic changes; all work remains local and uncommitted.

Controller illumination multipliers now survive compilation, evaluate with a
default of one, and multiply final RGB lighting even on unlit draws. Camera and
frame-dependent expressions refresh with presentation. Compiler, carrier,
runtime, GPU transport and raster regressions are authored but unrun. The
canonical Rust diagnostic build passes; cube overlay and glyph live acceptance
remain open, pending the admitted material and exact scoreboard character.

After the user's diagnostic-launch approval, the new client opened at its main
menu with private material/font capture and read-only sidebar inspection enabled.
A fresh Metal window capture and state query confirm startup. The next CubeCraft
join remains manual; no connection or gameplay input was automated.

The user's next manual join supplied the reflection material and exact sidebar
character. The reflection requests inherited emissive shading with One/One
additive blending, but its unresolved parent previously left the scene-art
texture drawn opaque over the orange body. The compiler now retains emissive
and additive contracts, the shader respects emissive alpha-test and lighting,
and the renderer applies the authored blend factors. Four additive raster
contracts are warmed; shader-only flags reuse existing pipelines. Sampler
addressing, sheen mask behavior, other factor pairs and exact matching-version
built-in registration remain incomplete.

The VIP character U+E250 has a valid colored raster in the admitted glyph sheet.
Codepoint-order packing exhausted eight atlas pages before reaching it. Exact
RGBA raster reuse and height-first placement preserve independent scalar metrics
and private-use priority. A private reproduction with the captured 768 cells
retains every cell, including all 416 nonblank glyphs, within the existing eight
pages; the previous packing dropped 113 nonblank glyphs, including VIP.
Regressions for material inheritance, emissive pixels, blend factors, bounded
pipeline warming and mixed-size glyph packing are authored but unrun at the
owner's request. Live visual acceptance remains open; no restart or push occurred.

The integrated client build passes at the canonical executable path with
developer inspection enabled. The existing diagnostic session remains open;
the new build awaits a requested restart and manual CubeCraft comparison.
All feature edits remain local and uncommitted, with no push.

After the user's requested relaunch, the canonical inspection build opened and
the user manually rejoined CubeCraft. A fresh client capture shows the colored
VIP icon; user acceptance of the cube's materials and glyphs remains pending.
No server connection or gameplay input was automated.

The black patch above XP reproduces with CubeCraft's captured HUD: eight unused
fixed boss-text slots draw overlapping backgrounds. Vanilla supplies an empty
name and false registration for absent boss entries. The HUD controller now
supplies those defaults, allowing the authored visibility predicates to hide
unused slots. The captured-pack offline draw removes all eight backgrounds
while retaining the XP number and outline. A retained-binding regression covers
empty, unrelated, matching and cleared names; it is authored but unrun at the
owner's request. The canonical client build passes. Live acceptance remains
pending; the current game stays open and the updated build is ready. Changes
are local and uncommitted; nothing was pushed.

At the user's request, the empty-slot fix build was relaunched. A fresh Metal
capture confirms its main menu at a 1280×752 logical window. CubeCraft rejoining
and HUD acceptance remain manual; no server connection or gameplay was automated.

The Free For All form's missing borders and captions reproduce as a variable
child-key parsing error: `instance@namespace.template` was retained as a plain
name, so the inherited button definition and input were absent. Vanilla splits
the substituted key before applying inline overrides. The resolver now follows
that contract. The minimal reproduction previously produced an untyped child
without descendants; the updated production library produces the named button
with its border and caption. Captured form definitions with representative menu
contents emit four borders, captions and click regions. A regression covers
named inheritance and inline overrides, authored but unrun at the owner's
request. The canonical client build passes. Actual-form live visual and input
acceptance remains open; the build is ready for manual inspection. No restart,
server input or push occurred. Changes remain local and uncommitted.

At the user's request, the form-button fix build was launched. A fresh Metal
window capture confirms the client rendering at a 1280×752 logical window.
Actual-form acceptance remains manual; no server input was automated.

The owner accepted the final live macOS Metal result and approved landing the
compatibility changes. The variable-named form controls now retain their borders,
captions and button ownership. This manual acceptance does not establish broader
native parity or performance qualification.

For landing, formatting and architecture checks passed. Affected-crate test
compilation exposed stale fixture fields, an ambiguous numeric type and a test
app setup error; those were corrected. The owner explicitly waived further local
tests and waiting for CI, so final test validation remains incomplete.
