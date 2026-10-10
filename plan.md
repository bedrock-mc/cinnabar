## Xbox Minecraft activity

- The Rust client reports menus and the committed world default, Realm and experience
  classification to the account core. Go publishes through the existing Xbox presence API;
  offline accounts do no service work and failures never block joins or gameplay.
- Fake-client tests cover joins, default-mode changes, leaving, cancellation and title
  cleanup. The control regression failed on the previous implementation. Rust state
  tests distinguish the world default from an individual player's mode.
- Named `COM_Experience_*` IDs are intentionally not sent. Featured servers and
  Experiences use `COM_Experience` without server-identity mappings.
- Exact current-version game-mode input, update trigger ordering, heartbeat cadence,
  platform-specific service configuration selection, server activity overrides and
  permission gates remain incomplete. The current heartbeat
  fallback is provisional. Signed-in friends-visible validation is incomplete, and no
  Xbox presence parity gate closes. Discord presence is unchanged.

## Small-surface render safety

- Explicit render scissors and viewports intersect their physical attachments; empty
  intersections skip drawing. UI pipeline selection follows current resource preparation,
  including retained layers and model depth during the first frame and resize.
- Deterministic bounds witnesses cover oversized layouts, 254×124, 1×1, zero extents and
  differing DPI. Native offscreen validation covers nonzero small attachments.
- The reported unfocused macOS startup panic remains incomplete until a matching
  headless startup capture reproduces it. No native parity or frame-budget gate closes.
## UI text and presentation failure recovery

- Wrapped presentation text keeps an indivisible glyph wider than its control, with
  existing line breaks and clipping. Newly shaped overflow layouts are counted and
  logged with bounded frequency. Strict measurement requests still reject oversized ink.
- Rejected runtime presentation frames count, log and skip publication without ending
  the network session; a later valid frame can publish. Startup asset validation remains
  fail-fast. Fitting status labels keep their width checks and ellipsis policy.
- Exact-version native overflow behavior remains incomplete pending a matching native
  control witness. The original reported text is unknown. No parity gate closes.

## Optional spatial anti-aliasing and cutout coverage

- Capability-aware MSAA remains the primary setting with its existing default. Spatial
  anti-aliasing is an independent Off/SMAA option, defaulting to Off.
- Spatial SMAA uses world-depth discontinuities and continuous-slope rejection before world
  text, hands and HUD drawing. It uses no temporal samples or colour edge detection.
- Multisampled cutouts integrate nearest alpha contours across a one-pixel band. Covered RGB stays
  nearest; newly covered transparent texels use the nearest covered neighbour's colour.
  Single-sample thresholds, opaque/translucent passes, shadows and entity depth passes are unchanged.
- Alpha-contour reconstruction is an extra visual policy, labelled incomplete for exact-version
  vanilla coverage parity. Matched hardware performance and native foliage comparison remain
  incomplete; no parity or performance gate closes from these changes.

## Optional camera motion blur

- Video offers Off (default), Low, Medium and High camera exposure. This is an owner-authorized
  visual extension; classic Bedrock has no motion blur. Gameplay and network state do not change.
- Depth reprojection blurs camera rotation/translation before nametags, projected UI, hands and HUD.
  Exposure uses elapsed frame time; explicit reanchors suppress the discontinuity frame.
- Per-object blur remains incomplete and needs motion vectors from every world renderer.
  Hardware GPU cost and frame-budget acceptance remain unmeasured; no performance gate closes.

## Anvil models

- Intact, chipped and damaged anvils use four stacked pieces with their damage artwork and
  cardinal orientation. The base, rim, stem and head replace the provisional single cuboid.
- Exact texture rotation/crop parity remains incomplete; the models retain provisional support
  until a matching native directional gallery verifies texel orientation. No parity gate closes.

## OreUI menus, appearance and motion

- Home, Pause, Play, Inbox, world creation, Accounts and confirmation dialogs use shared OreUI
  panels and controls. Home keeps the shipped title above Play and character cards, with compact
  utility actions and versions from their canonical sources. Commerce actions remain quiet.
- Persistent Dark Mode defaults off, updates immediately and strengthens backdrop dimming.
  Optional Screen animations controls quick hover/press feedback, selection, glimmers, text edits,
  screen changes, modal exits, pack accordions, scrolling and interpolated loading progress. Selected
  segmented choices remain lowered; hit targets remain stable and input applies immediately.
- Server views preserve service catalog groups, artwork, descriptions, activities and available
  counts. Exact ping is optional. Collapse state and drag-reordered sections persist; custom servers
  use the owner's requested label. Resource-pack controls and loading stages retain surrounding UI.
- Dimension travel names the destination, uses its block icon and the owner's private runtime
  background beneath a strong tint. Ordinary terrain loading retains its approved neutral treatment.
- Create New World uses native panels, installed preview/category artwork and independent scrolling.
  General/Advanced changes animate their contents. Unsupported categories, Hardcore and Realm
  creation remain disabled. Backend defaults to Dragonfly or offers BDS with Docker detection;
  generator selection independently offers Normal (Vanilla) and Flat. Template navigation works.
- Dragonfly Normal uses the pinned owner-requested vanilla-gen dependency with the saved signed
  seed for all three dimensions. New worlds use its spawn; reopening retains saved spawn/chunks.
  Normal supports saved overworld pre-generation and four chunk workers by default; see
  [generation measurements](docs/evidence/local-world-generation.md). Bedrock generation parity
  and join/streaming budgets remain incomplete.
- Dragonfly passive animal simulation and natural population remain unsupported and incomplete.
  Stored unsupported actors are preserved without becoming live entities. Mob parity and
  performance gates remain open.
- Dressing Room persists classic/slim and static custom-geometry skins, independent capes, imports and item edits. Home
  and Pause previews support rotation and pointer tracking. Cape attachment uses its own shoulders.
- Cropped cape imports pad the 46×22 layout at supported texture scales with transparent pixels,
  preserving texels and private source files. This custom import extension leaves native cape
  gallery parity incomplete.
- These layouts, appearance and custom motion are owner-authorized extensions. Matching-version
  native UI parity remains open for responsive controls, rich Inbox templates and unsupported world
  settings. Preview drag gain is provisional. The chosen normal generator targets Java-style
  terrain; Bedrock terrain, structures and mobs remain unverified. No performance gate closes.

- Back navigation retains an available prior control and validates Home focus against painted
  enabled controls. Keyboard/gamepad return and pointer outline admission have regression coverage.
  Exact native Home restoration policy and full authored frontend parity remain incomplete.

## Shipped OreUI fonts

- Body text uses Cinnangles Seven and headings use Cinnangles Ten, derived from the shipped
  Cinnangles Sans. Optional font carriers compile before startup and use the default font when
  missing or invalid. OreUI no longer reads installed game fonts or rasterizes them at startup.
- Compiled Unicode ranges supply Latin, symbols and bounded CJK coverage. The installed Noto
  chain and its worker cache have been removed. Full Unicode coverage, script substitution,
  bidirectional shaping and small-text raster parity remain incomplete.
- Game-bundle artwork remains a runtime input; this font change closes no native artwork,
  responsive-layout or font-parity gate.

## Prior OreUI verification

- The original Japanese-label failure, texture-budget rejection and colliding rendering flags were
  reproduced before their fixes. Regression checks cover language names, small-cap MOTDs, uncached
  glyph publication, cache recovery, bounds, texture ownership, control input, focus and navigation.
- Touched Rust crates compile with tests and developer-control enabled. Focused UI, font, input,
  loading, carrier and world checks pass. Core and local-server Go suites pass, including generator
  selection and saved-world reopening. Incoming cursor-return behavior is preserved.
- Owner reviews accepted Home, Pause, Inbox, world creation, server layouts and animation direction.
  Prior macOS/Metal captures verify menu previews at 2048×1152 and Retina 2560×1440.
- Final macOS/Metal pass at 2560×1440 physical, 1280×720 logical, DPI 2: primary and CJK language
  labels, styled server MOTDs, dark Home and Quit legibility, geometry, clipping, layering and colors
  were inspected. Mouse Cancel, Escape and keyboard Cancel work; focus remains inside the modal.
  The final build also verifies restored native Play/settings/status icons, server symbols and the
  latest merged input. User language, appearance and section preferences were preserved.
- The final integrated rebuild repeats the native language/Home/Quit review with readable Japanese,
  Korean and Chinese labels, intact artwork, working modal Cancel and confirmed shutdown.

## Movement and input audit fixes

- Input packets retain digital buttons and raw jump/sneak events separately from
  requested controls and resulting actor state. Opposing keys remain visible,
  brief taps survive tickless frames, and retries and rewinds preserve the input
  captured for each tick. Sprint admission includes direction, stall and
  seven-tick double-tap checks retained through replay;
  forced crouching does not invent a held sneak button. Blindness participates
  in sprint admission; Swift Sneak scales crouch input from equipped leggings.
  Consumed controller taps do not steal fresh keyboard movement on the next frame.
- Physics uses the selected collision support for landing responses and the
  near-feet material for travel friction. Auto-climb, levitation, restitution
  thresholds and per-axis horizontal epsilon handling follow vanilla tick order.
- When collision data pauses local ticks, the camera holds the completed crouch
  and correction offsets alongside the feet position. Subtick rendering no longer
  repeats an unfinished stance or correction transition. Missing-terrain recovery
  and matched loading/stall behavior remain incomplete.
- Cache responses retain valid requested columns when accompanied by repeated
  verified blobs still in the cache. Late responses after eviction and the
  remaining missing-terrain recovery behavior remain incomplete.
- Prediction corrections replay regardless of distance. Nonzero future ticks
  attach to the current captured frame for a later rewind; zero ticks and ticks
  older than retained history are discarded. MovePlayer teleports keep their
  separate distance rule. Deferred corrections clear old collision flags before
  replay so a relocation cannot invent a ladder climb. If replay cannot query
  terrain, its fallback preserves later retained server positions and motion.
  Unstamped or expired reset corrections keep their incoming destination.
- Validation: 267 simulator tests, 69 semantic-input tests, 345 movement tests
  and 16 focused client tests pass. The touched-crate compile check includes
  tests and the client app. The architecture check passes. Regression tests
  reproduced the stale collision and controller handoff bugs before their fixes.
- Full parity remains incomplete: vehicle prediction, special-block and glide
  coverage, equipment-dependent powder snow, dynamic actor sizes,
  touch layouts, independent orientation, prediction-sync metadata and exact
  loading/stall timing still require matched fixtures. Paired retail-client
  packet captures and live server verification have not been completed, so this
  work does not establish universal vanilla parity or identify a specific ban's
  cause. Existing provisional behavior below stays provisional.

## Held block placement

- A steep downward look past a ledge can acquire nearby support before a placement
  line exists. Use and selection share the downward support and preserve its click
  point; mining keeps the direct ray. The support selection shape is outlined.
- Indirect support admits the first press, while unlined held repeats require a
  direct hit. Locked lines still test the segment to the picked intercept.
- Incomplete: the reported floating outline in an empty destination cell has not
  been verified for the matched version and control settings.

- Ordinary block holds now retain successful destinations, establish an adjacent
  placement line, and use fresh ray segments to continue beyond ledges or upward.
- Repeats use resolved movement and stance; transactions carry their trigger,
  start/stop actions, local outcome, click position and survival inventory delta.
- Presses resolve when they arrive and held repeats only on frames that simulate a tick,
  both before the frame's physics and from the last completed tick's end state, so the next
  tick's movement collides with a placed block. Incomplete: a frame
  that simulates two ticks resolves build actions only before the first.
- Deterministic gameplay and wire tests cover ledges, jump bridging, towering,
  backward sneak bridging, failed attempts and refused transport.
- Provisional: slot or item changes preserve the held placement line and repeat
  schedule. Native slot-change callbacks and their effect on retained history,
  orientation and timing remain unverified; this does not close the hotbar-switch gate.
- Incomplete: complete runtime block-property and custom block-placer admission,
  item-specific replacement/state rules and live vanilla/server acceptance remain open. No
  complete placement parity or performance gate is closed by these tests.
- Vanilla rules: [held block placement](docs/reference/held-block-placement.md).

## Frame attribution and unchanged GPU uploads

- Simulation reads the current admitted frame clock. Deterministic FIFO tests at 60 and
  120 Hz and an uncapped test bound tick flush delay and 20 Hz send jitter to 3 ms;
  manual recording clocks keep their requested steps. Live movement acceptance remains open.

- Actor catch-up uses `world::MAX_TICKS_PER_FRAME`. Prediction and live per-tick actor
  advancement share one clock step, including server-defined interaction boxes after a stall.
  Deterministic regression coverage does not close live animation or movement acceptance.

- Apple pipelined rendering keeps surface creation on the UI thread while render
  submission stays on the render thread. Main and extraction schedules retain
  single-thread execution; runtime startup checks do not close frame-budget gates.
- Idle native metadata publication no longer dispatches through the main thread.
  macOS frame workers use interactive scheduling; bounded-load diagnostics show
  more render-work headroom. Unloaded tail results remain adverse and unexplained:
  the hitch and 120 Hz display gates remain incomplete. See
  [frame-pacing evidence](docs/evidence/zeqa-frame-pacing.md).
- Opt-in Tracy spans cover Bevy and owned streaming/render work; Metal pass
  durations are delayed plots. macOS zones alone cannot separate preemption from waits.
- Per-packet ingress admission preserves queued events when consumer fan-out fills
  headroom; regression tests cover resumption and zero steady-state drain allocations.
- Named schedule traces and bounded frame recordings separate main work, render
  handoff, drawable acquisition, command submission and presentation. Metal
  timestamp queries use owned render passes and leave uncovered stages absent.
- Unchanged hand and cloud uniforms, inactive portals and empty item scenes skip
  redundant staging work. Regression tests assert allocations, writes and retained
  buffers; hardware captures measure elapsed time separately.
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
  see [actor burst evidence](docs/evidence/actor-burst-preparation.md).
- Incomplete actor-join acceptance: readiness-gated first appearance remains provisional.
  Models exceeding the preparation mesh budget use the default rig until their source
  changes or the session resets; exact appearance under that limit remains an open gate.
  First GPU uploads, pipeline compilation, UI publication and exact page comparisons
  remain cold costs. Synthetic crowds and headless screenshots do not
  establish native first-appearance timing, join latency, no-pop-in acceptance or release
  frame budgets; those gates remain open.
- Recurring hand, UI viewport and particle writes share bounded retained GPU
  staging. Allocation regressions cover real Metal owners; lifecycle and byte-order
  tests cover asynchronous reuse and fallback. Attach-only native samples confirm
  staging-allocation mutex contention, drawable waits and scheduler delays.
- Incomplete: the synthetic fixed-camera fixture is not the populated-lobby/flight
  release replay; hidden frames do not establish displayed FPS. Three paired runs
  at each resolution reduce measured maxima but do not consistently improve p99.
  A separate pooled-upload run still exceeds 100 ms with no staging fallback.
  Drawable/present dependencies, remaining lock owners, retirement costs and
  complete long-stall attribution remain open. Shared-pass GPU categories, exact
  per-item costs and native GPU regression validation also remain unqualified.
  See [pooled-upload evidence](docs/evidence/render-stall-pooling.md),
  [frame breakdown evidence](docs/evidence/frame-breakdown.md) and
  [Tracy attribution](docs/evidence/frame-breakdown-tracy.md).

## Camera packets and aim assist

- Packet admission covers spline registries/instructions, aim presets, commands
  and actor-priority updates. Semantic errors are skipped and counted; malformed
  framing remains fatal. See the [Vanilla rules](docs/reference/camera.md).
- Preset inheritance, starting values, offsets, target tracking, camera collision,
  easing, fade, shake, FOV, listener and presentation capabilities have regressions.
  Named and inline splines have independent progress/rotation tracks. Aim selection
  uses retained physics-tick poses, item categories, priorities and visibility,
  with separate interaction direction and action-triggered rotation.
- Incomplete parity: animated attachment anchors, definition-authored actor hitboxes,
  complete native block tags, touch pick-range remapping, equal-score actor order,
  historical remote geometry during catch-up, and fire-resistance lava fog remain
  owner limitations. Legacy education photography output is unsupported. Packaged
  boom/shake/collision defaults and highlight sampler need pinned-version witnesses.
  The gameplay FOV multiplier follows vanilla (speed ratio, slowness, flying, bow,
  spyglass); the swim-speed factor and underwater narrowing remain incomplete.
  Sprint transitions recalculate movement speed from attribute defaults and other
  modifiers, preventing FOV overshoot after attribute resends and sprint restarts.
  Gameplay angles provisionally apply the [5, 130] bound after effects, preserving
  authored camera overrides. Incomplete: the exact-version final-angle rule and
  live post-death distortion acceptance are unverified.
  Finite block sampling is bounded to 4,096 rays. None closes a full parity gate.
- Touched-crate checks and directly affected regressions pass. A headless macOS
  local-server run at 1280×720 captured named/inline splines, local-body visibility,
  hand suppression, aim highlighting and clear/restoration. The 484-frame clip and
  state snapshots are outside git; camera and aim semantic-skip counters stayed zero.
  Matching-version native comparison, physical controller/touch testing and the
  three-run hardware performance qualification remain untested.

## Java 1.7 animations

- Owner-mandated default, selected live from Video › Animations (persisted): Java 1.7 or
  Bedrock. Saved toggle choices migrate without changing the selected mode. Rules and frame mapping:
  [Java 1.7 animations](docs/reference/java-1-7-animations.md).
- First person: Java's item stacks (swing; equip with the old item through the dip and the
  re-equip after a placement; eat/drink; sword block with block-hitting; bow draw and pull
  frames; rods), the empty-hand arm, view bob, sneak eye height, hurt/death roll and arm sway. Third person: Java's
  biped pose, body yaw, limb swing with the hurt flail, held-item grips, the cape's chasing
  swing and the sneak drops; armour flashes red, held items do not.
- Every hurt event immediately resets the limb boost, including consecutive hits; movement
  still contributes to the phase, and death alone does not trigger a flail.
  Fourteen focused hurt tests pass. macOS/Metal captures at 1920×1080, DPI 1, GUI scale 2
  verify single and repeated hits, walking while hurt, and recovery to idle.
- Worn elytra retains authored wing poses and glint in both modes, suppresses the separate cape,
  and uses the cape texture when present. Native controller blend composition stays intact.
- Golden tests assert composed stacks and projected arm, item and cape points against Java's
  calls.
- Review corrections: local head sampling stays in the current render frame; cape and camera
  motion use native velocity, health and riding state. Retained items keep their own use
  clocks through swaps, authored first-person rigs retain ownership, and raster depth stays
  one sixteenth at any texture size. Emote skin layers follow the sampled body pose.
  Unchanged render layers sharing geometry retain their own completed pose allocations.
- Focused regression suites and touched-crate checks pass. Windows/DX12 headless captures
  exercise held-item swaps, use, both third-person views, cape motion and local emotes.
- Incomplete: swimming, crawling, gliding, sleeping and emoting stay vanilla (Java 1.7 has
  none); held attachables Java never had, the third-person bow pull frames and cast-rod item
  stay vanilla. Unclassified mounts retain ordinary player body yaw.
- First-person items and the empty arm use Java's fixed directional lights and gamma-space
  colour multiplication, including normal rescaling during bow stretch. World lightmap
  colours still follow Bedrock; full-frame lighting parity remains open.
- Living mount heading and active creative-flight cape phase now use their own observed
  state, with regression witnesses for wraparound, live local look, dismounting, phase
  freezing and walking resumption. Fixed Java 1.7.10 model/matrix fixtures and Windows
  rendered motion checks validate the covered state product; full game-frame pixel
  equality, lighting and the retained vanilla exceptions above are not asserted.
  Local swing effects are live; remote swings retain the six-tick default.
- Validation after integrating concurrent PR changes: 802 animation-suite tests pass,
  touched-crate checks and the optimized Windows build pass with sccache. Native fixtures
  cover 42 pose/transform states and 20 exact use clocks. Fresh hidden DX12 captures verify
  flight-phase freezing/resumption, living/nonliving mounted views, yaw wraparound,
  dismounting, item use/swaps, both third-person views and local emotes. Full-client pixel
  and performance parity are not asserted.

## Compatibility landing

- Education construction terrain restores allow, deny and all border wall states
  from the pinned current palette, using the textures already in the fetched pack.
  Collision and light read the pinned metadata sources; no guessed shapes or
  sequential palette IDs are added. Linux captures at 1280×720 with GUI scale 2
  show textured allow/deny cubes and connected border shapes. Broader Education
  parity remains incomplete.

- The owner accepted the final live macOS Metal build and authorized landing the
  accumulated server compatibility changes. Formatting and architecture checks
  pass. Known test compilation errors were corrected; further local tests and
  waiting for CI were explicitly waived by the owner. Validation is incomplete.
- Earlier entries record the state when each fix was introduced. Native parity
  and performance limits marked incomplete below remain open.

## Variable-named form buttons

- CubeCraft's Free For All cards use a variable child key containing both an
  instance name and a template reference. The resolver previously kept that
  string as a name, dropping the inherited button border, caption and input.
- Vanilla substitutes the key before splitting its name and template; the
  resolver now does so while preserving inline overrides. A regression covers
  the resulting instance name, button type, inherited children and overrides.
- The canonical client build passes. A minimal reproduction changes from an
  untyped child without descendants to the named button and its two children.
  Captured form definitions with representative menu contents emit all four
  borders, captions and button click regions.
- The owner accepted the actual form's rendered result in the live macOS Metal
  inspection build. The regression covers named inheritance and input ownership;
  this compatibility acceptance does not close the broader native parity gates.

## Cube overlay materials and scoreboard glyphs

- The live CubeCraft comparison shows floating models and the empty hand, but
  the central cube's overlays obscure its yellow base at distance and the VIP
  scoreboard icon falls back to a tiny glyph. After the requested relaunch,
  a fresh client capture shows the colored VIP icon. User acceptance of the
  cube and glyph rendering remains pending.
- Admitted reflection materials request inherited emissive shading and One/One
  additive blending. Those contracts now survive compilation and GPU submission;
  controller illumination multipliers also apply to unlit layers. Pipeline
  warming adds four additive raster contracts and reuses shader-only variants.
- The valid VIP icon U+E250 was dropped when earlier large duplicate glyphs
  exhausted the atlas. Exact raster reuse and height-first packing retain all
  768 captured cells within the existing eight pages, preserving each scalar's
  metrics, colors and lookup priority.
- Incomplete: sampler addressing, the sheen's mask behavior, wider blend factor
  pairs, and exact matching-version built-in material registration remain open.
  Regressions are authored but unrun at the owner's request. The canonical client
  build passes, and the inspection client is running after the requested
  relaunch. Changes are local and uncommitted; nothing is pushed.

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

## Empty boss-text HUD slots

- A captured server HUD uses eight fixed boss-text slots. Missing collection
  values left unused backgrounds visible, stacking into a black patch above XP.
  Vanilla supplies an empty name and false registration for absent entries;
  the HUD controller now supplies those defaults.
- The captured-pack offline draw reproduces all eight overlapping backgrounds
  before the fix. A retained-binding regression covers empty, unrelated, matching
  and cleared names. It is authored but unrun at the owner's request.
- The canonical client build passes. The captured-pack offline draw removes all
  eight unused backgrounds and preserves the XP number and outline.
  Incomplete: live acceptance remains pending. The updated inspection client is
  running after the requested restart. Changes are local and uncommitted;
  nothing is pushed.

## Worn server-pack armour textures

- Worn armour from server packs runs its attachable scripts with the wearer as
  `context.owning_entity` and draws the texture its render controller selects,
  such as owner-driven team variants. Each worn slot keeps its own retained state
  and native model binding name. Owner item-use timing and frame delta are passed
  to the worn scripts. Equipment, properties and local-player identity also
  remain available to owner queries when the body pose is static.
  Attachable timing is independent of owner tick batching, and elytra scripts
  receive the actual render delta.
- Incomplete: worn armour still uses the binding's default geometry on remapped
  body bones. Attachable animations and controller-selected geometry or
  materials are not applied, and vanilla armour keeps its static binding.

## Server-pack actors and conditional forms

- Captured CubeCraft definitions exposed rejected actor queries that discarded
  complete pre-animation scripts, leaving dynamic models at zero scale and player
  animation inactive. Camera-distance ranges, declared properties, armor slots
  and default swing duration now evaluate through the shared actor query context.
- The installed vanilla layer supplies actor rasters omitted by server packs,
  preserving server artwork precedence. Collection factories use their authored
  control-id arrays after view bindings settle, including nested factories.
- Incomplete: custom item swing-duration overrides and the vanilla guard for
  extremely narrow camera-distance ranges remain unimplemented or unverified.
  The integrated Rust client build passes; the user's live screenshots show
  floating models and the first-person hand. Selector and player animation
  acceptance remains pending. Regressions are
  authored but unrun at the owner's request. The inspection build is running
  after the requested relaunch. Changes are local and uncommitted; nothing is pushed.
## Controller state time and pack queries

- `query.state_time`, `query.max_trade_tier` and `query.position` now compile.
  Unsupported queries had dropped whole controller transitions and script
  statements, leaving looping weapon recoil states and misplacing scripted markers.
- Controller weights, clip clocks and bone channels read the elapsed time of
  their own state, including separate outgoing and incoming epochs during blends.
  Position queries require one axis and read the native actor origin.
- Incomplete: `query.state_time` advances in actor ticks, like the existing
  finished-animation queries, rather than per rendered frame.

## OreUI Settings

- Global Resources is an owner-requested design exception using expandable OreUI pack cards,
  imported thumbnails, priority actions and a native variant picker. Vanilla Textures is always
  the immutable base of the displayed stack. Apply tracks staged edits against the acknowledged
  running stack; finished work clears its transient status. Rendered verification is pending.
- Controller uses its native category image; category selection plays the 500ms highlight.
  Account uses the authenticated gamerpic or the signed-out silhouette. Sidebar bevels and
  detail-row top/bottom edges now retain their native state roles and group boundaries.
  The integrated Settings control regressions pass. OreUI now uses shipped Cinnangles carriers.
  The visible macOS client runs with rebuilt block assets; fresh local-world frames confirm textured terrain. These
  follow-up changes remain uncommitted, and matched Settings visual acceptance remains open.
- Settings now uses the native OreUI shell, category sidebar, grouped descriptions, right-side
  switches, filled sliders, inline choices, and independent panel scrollbars.
- Shared launcher options, key remapping, language, accounts, packs and storage retain their
  existing actions. Keyboard focus includes rows below the viewport and reveals them; resize
  clamps scroll positions before painting. Always Sprint and VSync remain user extensions.
- Regression tests reproduce legacy routing and unreachable offscreen focus. Rendered macOS
  acceptance is pending; this does not close the Settings parity gate.
- Incomplete: native options without a host model, controller binding glyphs, account management
  actions/confirmations, Touch/Party/subscription services, and native storage subroutes.
  Runtime vanilla artwork and shipped Cinnangles faces are integrated, with face metrics,
  letter spacing, coverage shading, control states and pointer/navigation timelines. Locale shaping
  and hidden-tab animation resumption still need native witnesses; disabled narration focus awaits a UI narration host. Matched visual
  acceptance remains open. Reference facts are tracked in
  [OreUI](docs/oreui.md).

## Distant grass sides

- Grass sides' tint-mask tiles take vanilla's atlas mips (byte-space box averages of the
  original tile), so the dirt stored under alpha zero no longer turns black at distance.
- The user confirmed distant grass sides in a live macOS build.
- Incomplete: a server pack that replaces an overlay-masked texture still gets the runtime block
  overlay's alpha-weighted mips.

## Block selection outlines and visual bounds

- At the owner's request, untouched and older settings select the native black
  outline instead of the filled highlight. Explicit Outline Selection choices
  retain the native two-mode rendering contract.
- Regular outlines retain the twelve unexpanded bounds edges in a dedicated
  opaque, unlit, depth-writing pass. Unchanged targets retain their geometry and
  upload revision across camera movement.
- The live outline was too faint. A Metal rendering diagnostic confirms FXAA
  dilutes thin black lines more strongly on diagonals. GPU expansion now supplies
  consistent two-physical-pixel coverage. This is provisional compensation for
  Cinnabar's FXAA; matching vanilla sample anti-aliasing and stroke coverage
  remains incomplete.
- Fence and wall selection use visual bounds independently of taller movement
  collision. Wall heights currently follow the pinned visible models.
- Incomplete parity: exact wall selection insets, the matching-version material
  depth offset, scaled deferred outlines and platform-specific factory defaults
  remain unverified. The material witness is from the neighbouring installed client.
- The visibility update's Rust client build passes. The production stroke shader
  renders on Metal and retains dark pixels through FXAA at horizontal, vertical
  and diagonal angles. The user accepts its live visibility; regression tests are
  authored but unrun at the owner's request. Latest dev changes are merged
  locally; the updated test client is running for manual inspection. Visibility edits are
  uncommitted; no push.

## Custom climbing-vine facing

- Vanilla's `query.block_property` and `query.block_state` read the same named
  block state. Both spellings, including their `q.` forms, are now admitted by
  custom-block permutation and bone-visibility evaluation.
- Hive's four climbing-vine states previously all kept their base orientation
  because their facing conditions used the rejected query alias. Their authored
  quarter-turn transformations now reach model compilation.
- The Rust client build passes. Named-state and four-orientation runtime
  regressions are authored but unrun at the owner's request. Live orientation
  acceptance remains incomplete; the inspection build is ready, and the existing
  game remains open. Changes are uncommitted; no push.

## Chat ownership and first-person shadows

- At the owner's request, built-in chat owns its screen, autocomplete and HUD
  history even when server packs replace their layouts. Chat-derived ordinary
  history factories are suppressed; other server HUD widgets retain pack priority.
- Local player shadows now follow the actor frame's drawn bodies, suppressing
  the caster in first person and retaining it in third person.
- Animated loading strips wait for settled artwork instead of drawing a
  downsampled preview. Large custom strips retain the existing size-cap
  approximation; exact large-image animation parity remains incomplete.
- The integrated Rust client and Go core builds pass. Incomplete acceptance:
  regressions are authored but unrun at the owner's request; live inspection
  is still pending.

## NetherNet server trust

- Plain-http NetherNet joins ask vanilla's first-use trust question through the JSON-UI modal
  popup, with the keys stored as vanilla stores them. Provisional deviation: an answer that takes
  longer than 5 s redials the server with the trusted key, because a dedicated server drops an
  idle negotiation within about ten seconds; vanilla's handling of a slow answer is unconfirmed.

## Servers tab experiences

- The Servers tab lists the ServerTab layout's experiences, joined by experience ID. The Go core
  now consumes typed player counts, using the service client's five-minute cache and retaining
  the last successful values on refresh failure. Counts are requested for visible experience
  details on an independent worker, so Home cannot delay them; background layout reads do not
  request them. The existing selected-details binding shows only positive counts as plain
  decimal numbers, without capacity or digit grouping.
- Incomplete parity: the OreUI experience banner still needs its count badge wired by its screen
  owner; this change only supplies data and existing JSON-UI bindings. The menu rereads counts
  through its independent featured worker every 30 seconds; exact refresh dispatch timing remains
  unverified. Listing-only experience details still need their linked detail page. No visual or
  performance gate is closed.

## Friends tab worlds

- Friends' worlds follow the vanilla list rule (`p2p.World.Listed`): members and a host, the player's
  own session only for a Realm, broadcast 3/4 always and 2 for friends. Incomplete parity: friends'
  Realm and experience sessions are left out because joining them from the friends tab is not
  implemented.

## Server-directed local immobility

- Committed local metadata now retains the server's immobile flag independently
  of spatial anchors. Each frozen tick clears velocity and physical jumping,
  suppresses travel/gravity and anchor depenetration, and preserves position,
  grounded state, collision flags, look and input intent. Explicit clears and
  session replacement release the flag; teleports do not.
- Prediction retains the flag per tick, including delayed metadata edits and
  correction replay. F3 and developer state queries expose the current flag.
- Incomplete acceptance: regression tests are authored but remain unrun at the
  user's request. The local Rust build passes; no live freeze/release witness
  has been captured. The reported SkyWars ban does not prove its cause.
  Pose selection with unavailable terrain retains the preceding inferred mode;
  exact native fallback and packet scheduling remain open parity details.

## Entity shadows

- Vanilla blob shadows: a 13-sided volume under each caster darkens the opaque surface inside it
  by the encoded-colour multiplier (0.7 grey with a sky tint), once however many overlap. One
  instanced draw after opaque geometry; casters and parameters upload only when they change.
- Caster rules (radius table, babies, slimes, projectiles, burning, invisible, dead, submerged,
  riders, ghast drops) follow [the vanilla rules](docs/reference/entity-shadows.md).
- Rigged casters follow the actor frame's drawn bodies. Incomplete parity: sign shadows are not
  drawn; the breathing point, item and local volume culling and
  camera-inside behaviour are provisional. Native side-by-side
  comparison is pending.

## Configured inventory hotbar swaps

- User-requested inventory shortcut: the configured hotbar key swaps the hovered cell directly with that hotbar slot.
- Keyboard and mouse remaps share gameplay's saved bindings; replaced number keys no longer perform swaps.
- Local prediction updates both cells immediately without an inventory transition animation or a server round trip.
- Focused text fields retain input ownership. Installed Windows input acceptance is pending.

## Read-back terrain occlusion on direct-draw devices

- Metal (any direct-draw device with compute) never draws indirectly. While the view holds
  still and a verdict could change, solid terrain draws in its own pass ahead of other opaque
  draws; a Hi-Z of that depth tests every resident slot and the occluded bits come back
  through a small readback ring that drops frames rather than wait. Opaque streams of a slot
  are skipped once two consecutive verdicts agree under the same eye, orientation,
  projection, viewport, depth size and geometry; any translation or turn (the near plane swings and can
  clip a near occluder), sub-chunk removal or replacement, cave or tint change voids them.
  `RUST_MCBE_CPU_CULLING=1` turns it off.
- Offscreen tests: kernel bits match a CPU Hi-Z reference; replayed paths (flick, strafe past
  a pillar, wall edit, a turn that clips a wall just beyond the near plane) never skip a
  sub-chunk that shows pixels while a stale-verdict policy does; the walled scene stops
  submitting what it hides.
- Incomplete live visual acceptance: an in-game pass on the M3 Pro (no pop-in, no wrongly
  hidden terrain) and live `gpu_opaque` and render CPU stage captures are pending.

## Bounded lighting CPU work

- Retained-support early exits and section packing preserve light/provenance output;
  [remote CPU evidence](docs/evidence/lighting-cpu-work.md) records workload, allocation
  and queue measurements alongside independent regression oracles.
- Incomplete: these synthetic solver diagnostics do not close native vanilla parity,
  release frame, join or streaming gates; target-hardware captures remain required.

## Headless chunk cost baselines

- Criterion exercises production palette/column decode, light solves, cube/biome
  meshing, bounded ingress-to-CPU-publication bursts, idle polls and metadata scans.
- Fixtures validate decoded cells, lighting, exposed faces and drained stream state.
  Benchmark fixtures run on the pinned toolchain without local carriers.
- The largest fixture stores 871 sections in 218 target columns, with preloaded
  implicit-air neighbours. It is synthetic, not a replay of the reported FPS drop.
- Initial Windows/i9-14900HX baseline: pinned Rust, optimized bench profile, 100 samples
  per case. Burst point estimates for 4/16/64/871 stored sections are
  14.53/19.24/35.82/257.54 ms; the 871 estimate interval is 253.87–261.76 ms.
  Radius-16 metadata scan is 1.33 ms; mixed cube mesh is 0.50 ms; full light solves
  are 0.40–0.48 ms. Local Criterion data is saved as `chunks` under `target/criterion/`.
- Moving-FPS fixes on top of incremental cave visibility and column residency: trailing-row
  eviction finds tracked columns one key per column, retires mesh records by key, and dirties
  neighbours per column, so its cost follows the retired edge rather than the view. The
  per-frame world-stream allocation is half the display interval, between 1 and 3 ms, so a
  streaming backlog cannot halve high refresh rates. Chunk uploads merge abutting arena
  writes into one staged write per run. The F3 overlay keeps its bindings and layout and
  rebinds only changed lines.
- Light/mesh job inputs retain immutable section/column handles without allocation;
  light workers reuse scratch and dispatches publish in batches. Snapshot benchmarks and
  in-flight edit tests cover capture cost, unchanged payloads and stale-result rejection.
- Incomplete: sustained high-refresh release streaming throughput is unverified; whole-frame
  UI rebuilds on any change remain. Cave camera crossings reuse reached exits after a bounded
  exact proof; streamed additions survive journal rollover. Destructive graph changes and inconclusive proofs still rebuild synchronously.
  No parity or performance gate is claimed; commands and boundaries are in the README.

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
  come from `RUST_MCBE_GPU_NODES=1` (below).

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
- `RUST_MCBE_GPU_NODES=1` plots per-node and per-section GPU times under Tracy, and Tracy
  builds expose wgpu's encode and submit zones; see the live-testing guide.
- Evidence: [render frame time](docs/evidence/render-frame-time.md), measured on DX12 before
  Windows defaulted to Vulkan; `WGPU_BACKEND=dx12` selects that backend.

## Compact font carriers and glyph residency

- Font carrier schema 2 stores independently compressed zlib pages, with coverage bytes for
  outline faces and RGBA retained for colored bitmap pages. Decoding checks stored and decoded
  hashes, exact stream lengths, dimensions, and the aggregate decoded-byte bound.
- Font textures use bounded demand atlases. A publication resolves all sampled glyph rectangles
  before uploading or drawing them; warm glyphs retain their placements. Full atlases repack the
  current frame's requests, and frames exceeding the bound are rejected before texture writes.
  New slots share row uploads to bound queue overhead. Source UVs remain unchanged during
  interpolation so relocation preserves SDF derivatives.
- Development-profile startup measurements reach the first completed menu GPU submission.
  Offline mixed-script menu/chat captures and native GPU pixel comparisons cover the carrier
  and atlas changes. Incomplete performance acceptance: release frame/hitch attribution and
  qualification on all reference hardware tiers remain open; these measurements do not close
  those gates.

## Menu frame passes and retained memory

- The HUD composites after FXAA, inside the output pass; FXAA is off with no world drawn.
- The panorama draws opaque in the main pass; a static menu frame is two fullscreen passes.
- Font pages are coverage bytes on CPU and GPU; the transparent ref buffer grows on demand;
  JSON-UI caches evict least recently used entries.
- Incomplete visual acceptance: a rendered menu and HUD frame (text sharpness, panorama,
  crosshair, screen overlays) on the target platform is pending.

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

## Held cube item consistency

- Cube admission now ignores unrelated gameplay flags and terrain occlusion.
- Authored carried faces fall back to ordinary pack faces; transparent cube templates
  and cutout/blended sheets retain block geometry in both hands.
- First-person material modes preserve ice blending and cutout holes.
- Regressions reproduce the previous gameplay-flag, alpha and missing-sheet failures.
- Rebuilt macOS/Metal frames show cube geometry; the user confirmed the live fix.
- Non-cube, animated and high-resolution carried geometry parity remains open.
- Vanilla rules: [held block items](docs/reference/held-block-items.md).

## Flower-pot floor and lily-pad atlas tint

- Flower pots now add the dirt surface four pixels above the block base, below the rim.
- Pot bodies and lantern surfaces use double-sided alpha testing instead of fallback blending and rear-face culling.
- The pot body remains provisional fallback geometry; full current-version visual parity is incomplete.
- Literal lily-pad atlas tint survives raster-only resource-pack replacement. A higher catalog can replace or clear it.
- The current vanilla pack supplies a fixed green atlas multiplier, rather than biome tint.
- Regression tests reproduce the missing floor and ignored pack tint. Installed Windows visual acceptance is pending.

## Java-style Tab player list

- User-requested HUD extension: hold Tab for the authoritative online roster.
- Compact centered columns, bounded to 80 players with an explicit overflow count.
- Roster changes refresh cached JSON-UI; Tab release, focus loss and menus hide it.
- Input, rendered collection, focus/release, roster refresh and cache tests passed.
- Incomplete live Windows acceptance: installed-client Tab capture pending.

## Barrier selection visibility

- User-requested correction: suppress barrier highlights/outlines outside Creative.
- Preserve barrier collision, picking and normal block selection in both ID spaces.
- Incomplete native parity: selection eligibility inspected; remaining comparison
  service unavailable. Windows rendered barrier acceptance pending.

## Always Sprint keyboard/mouse extension

- Optional custom setting, off by default, persisted with the existing settings registry.
- Forward movement requests normal sprint; sneak, hunger and other restrictions still apply.
- Auth-input sprint flags remain derived from the completed physics state.
- Windows official install: Keyboard & Mouse rendered at a 1280×720 client area; label and toggle are legible, aligned and unclipped. Enabled preference persisted during user interaction. Live user movement acceptance remains pending.

## Remote session reconnect

- Reconnect is a requested menu extension on remote disconnect and connection-failure screens.
  It retries the selected destination through the normal join route and retains the originating
  page. OK and Escape dismiss the failure. Account changes and local-world exits clear retry.
- Behavioral regressions cover repeated attempts, teardown, transfers and account ownership.
- macOS rendered input checks passed at 1280×720: keyboard and pointer retry completed
  loopback joins after refusal and server disconnect; OK, Escape and loading Cancel worked.
  This requested extension does not establish native UI parity.

## VSync video toggle

- User-requested deviation: retail vanilla has no VSync menu control (it persists `gfx_vsync`,
  default on; the three-way `vsync_dropdown` exists only in the non-publish Debug section).
- Toggle follows Max Framerate in the advanced video options, labelled `options.vsync`, default
  on, persisted with the settings registry and applied live.
- On keeps FIFO and its DX12 remedy. Off, and `--no-vsync`, prefer Immediate (which can tear),
  then Mailbox, then FIFO. Hidden developer surfaces present unpaced.
  Every choice comes from the primary surface's probed modes.
  `--vsync`, `--no-vsync` and evidence runs pin the session and show the toggle locked.
- Max Framerate offers 1–240 FPS and Unlimited, which is the default. Missing or unsupported
  saved FPS values use Unlimited. `--frame-cap` replaces the saved limit for the session.
  VSync controls display pacing; confirmed variable refresh caps low-latency rendering at
  97% of its maximum, including when Unlimited is selected.
- Variable Refresh Rate offers Automatic / On / Off in both Video menus. Automatic uses the
  current macOS screen's refresh intervals and native fullscreen state; unsupported queries,
  Windows, and Linux remain Unknown. On declares VRR enabled by the player; it does not change
  the display or OS configuration. Off disables the VRR cap. Unknown remains conservative.
  Incomplete: physical VRR cadence, cap overshoot, and tearing need target-display acceptance.

## Unfilled sub-chunk slots light as air

- Probable cause of reported dark corners on distant stepped terrain: a requested sub-chunk
  whose retries ended without data stayed unknown, so the column below lost its sky light.
- Vanilla leaves such a slot empty and lights it as air; the slot is now known air, and a
  column settled this way no longer blocks its neighbours' first light.
- Streaming terrace regressions cover both. Incomplete live visual acceptance: a rendered
  far-terrain frame is pending, as is a check against the open Lifeboat zero-skylight trace.

# Rust Bedrock Client (Bevy + Go Core) — Master Implementation Plan

2026-10-05 falling blocks and beds follow-up — **User-accepted local build**:

- Synchronized source/landing updates retain signed actor identity and notification
  flags, respect cached-column ordering, and fence visibility on mesh publication.
  Source removal and landing placement now work in the actual BDS client.
- Falling light skips matching opaque block types instead of sampling their dark
  interiors. The bed foot uses the authored vertical texture orientation; all
  sixteen item dyes retain distinct atlas sprites despite sharing a placed block.
- Focused regressions failed before each correction and now pass. The canonical
  macOS/Metal client and carriers were rebuilt; the user accepts bed appearance and
  falling source/landing behavior. Creative, OP and cheats are enabled on BDS.
- Falling visibility switches in the same render frame as source removal and
  landing placement; first appearances queue from the current scene. Twenty
  focused handoff regressions pass, and the rebuilt macOS/Metal BDS client received
  the user's in-game acceptance. Full local falling gravity, collision and drag
  parity remains incomplete; existing wire interpolation stays.

2026-10-05 integration — **User approved landing; latest dev synchronized**:
the user accepts the final portal, End, bed and falling-block behavior. All nineteen
affected Rust crates compile with tests; formatting, architecture and targeted
registry checks pass. The full PR CI matrix gates landing. Remaining parity
qualifications below stay open; this acceptance does not establish the release
frame-budget or identical-version pixel gates.

2026-10-04 End test v22 follow-up — **Local test build ready, uncommitted/unpushed**:

- XP colour samples current lifetime and partial ticks, including culled actors and
  animation-budget starvation. Wire interpolation duration and completion flags now
  survive normalization; a bounded queued target preserves completion ordering.
  The client-world suite passed 298 tests with eight existing ignores; all three
  subsequent installed-orb colour regressions passed after the final lifetime fix.
- Sprite rigs sample the current camera between ticks and retain independently
  authored opposing faces. Four Metal sidedness regressions passed; fresh snowball,
  pearl and XP frames at exact upward/downward pitch show complete sprites.
- Beds compile the pinned authored model and render once from the head block with
  both halves' light. Six bed tests passed. Fresh Metal red/white galleries and the
  actual BDS platform show a mattress, pillow, blanket and wooden legs.
- Falling blocks retain variant metadata, full block geometry and packet-centred
  placement, including stepped dragon eggs. Spawn and movement no longer disagree
  by half a block. Egg, sand and gravel Metal galleries use the production mesh.
- Credits include separated file sections and refresh the current local player's
  name. Fifteen credits tests and the committed identity regression passed. Fresh
  JSON-UI Metal frames show logo spacing, name substitution and input-revealed Skip.
  Skip already followed the native input/reveal timeout; it was not made permanent.
- Missing biome samples above the Nether height use the dimension's fallback biome.
  A failing-before regression now passes, and the actual client above the BDS roof
  retains Nether fog without blue sky or clouds.
- Boss UI dispatch waits for its authoritative actor before acknowledging the
  server subscription, and local actor loss retires the bar without a synthetic
  wire Hide reply. Seventeen committed-UI regressions pass. A live timing failure
  exposed the missing admission gate. Fresh BDS acceptance confirms a full-health
  End bar with its matching dragon actor and no bar after returning to the Overworld.
- The canonical dev client and required carriers were rebuilt. Formatting,
  architecture and diff checks passed. The owned BDS test world was backed up and
  reset on its original seed, then restored with portals, Creative, OP and cheats.
  The live End has a full-health dragon; the persistent client is back at the
  Overworld test platform for user testing.

Incomplete parity gates: live moving-XP and full credits return/input acceptance;
the native ForceMove distance/destination-column gate; coupled falling-block
gravity, collision, drag and lighting; independent current-version confirmation of
the numerical credits section gap and all bed directions. The credits gap retains
the identified older reference value with current section construction verified.
No frame-budget or full-workspace verification claim is made by this local build.

2026-10-04 F5 head flick correction — **Incomplete visual/native acceptance**:
first-person and third-person pose histories are separated at the view switch.
A local-only pose refresh handles frames without a tick, preserves simulation
and animation time, and invalidates bone conversion caches. Two synthetic
regressions cover tick and between-tick switches; Windows verification is pending.

2026-10-04 user-requested custom emotes — **Incomplete native/Lunar parity**:
the native four-slot JSON-UI wheel and remappable emote control select an original,
local-only Twerk dance with a faster user-requested loop. The owned clip preserves skin
hierarchy and clothing; render-time body and player-preview sampling leave the
native first-person hand and remote actor animations intact. Slot preferences
persist through Change Emotes, and gameplay input cancels playback. Identified
native references, the public product visual reference, and remaining parity
limits are recorded in [docs/reference/emote-wheel.md](docs/reference/emote-wheel.md).
Exact Lunar keyframes/assets and full native animation parity are not verified.
The revised clip follows the reference video's deeper squat and hip pulse,
retargets both native and independent skin joints, and anchors foot centers.
The pelvis rocks under steady shoulders/head, avoiding a whole-body jumping
pulse; stance compensation keeps foot centers fixed on all three axes.
The user rejected that first steady-head revision as still looking like a crouch.
The next local revision increases the hip thrust, reduces the base torso lean,
adds alternating torso twist and brings the hands closer to the thighs. It
retains rigid legs on unsupported custom models. The latest local revision adds
temporary thigh/shin joints to classic cuboid skins only during playback, crops
their existing UVs, and bends the knees with a planted-foot two-segment solve.
Stopping playback restores the original mesh/pose; native hand, remote actors
and simulation skeletons remain unchanged. Eleven animation tests pass;
exact Lunar/native parity remains incomplete.
The user accepted the installed knee revision; the next local adjustment speeds
up playback and increases vertical pelvis travel while keeping shoulders/head
steady and foot centers planted. The user accepted the installed faster motion.
The accompanying foot correction adds emote-only ankle joints, keeps the entire
sole level, and anchors all four sole corners rather than only the foot center.
The user reported an exposed ankle seam. The next local mesh correction embeds
the ankle inside overlapping, original-textured foot/shin volumes; the timing
and pelvis motion remain unchanged. The user accepted the installed correction on Windows.
Focused checks pass: 480 JSON-UI tests, four saved-binding/slot tests, eleven
animation tests, eleven emote UI tests and twenty-five app emote checks. The
1280x720 software-rendered wheel/equip frames were inspected for readable text
and geometry. The user accepted the installed wheel and knee revision; the
latest ankle-seam correction is user-accepted on Windows. The animation
revision is being integrated with current dev for the requested direct push.
Strict affected-package clippy passes. Required affected verification reaches
the existing ice/water liquid-face test failure after formatting, architecture
and compilation pass; that meshing regression is outside this change.
2026-10-04 F3 diagnostics (user-accepted developer feature): the supplied Java
19w05a screenshot is the requested styling reference. F3 is available by default,
hidden at startup, and the window title contains only the shared product name.
JSON-UI draws player/world/movement/render/queue diagnostics and target-state
rows during gameplay; other screens hide the overlay without resetting F3.
Coordinates use player feet, including third person. A fixed row budget keeps
font size stable as values change; text sits one GUI pixel lower in each strip.
Live data refreshes every frame at the user's request; FPS and frame timing use
a short aggregation window. Data meanings and styling are documented in
`docs/reference/f3-debug-overlay.md`; this does not close a Bedrock parity gate.
The updated client built and joined an isolated offline BDS world on local port
19132 as DebugTest. The user accepts the rendered follow-up on macOS/Metal at
2560x1440 physical resolution, DPI 2, GUI scale 5. Before the final cadence change,
all eight focused JSON-UI overlay tests, formatting and architecture checks passed.
The user requests latest-dev integration, no further tests and direct publication
to dev. The final cadence change and integration have no new test-green claim.
All local test services started for this feature are stopped.

2026-10-04 Lifeboat server forms (compatibility acceptance passed): creation-body
values, ordered button roles, trailing-close predicates, relative references and
evaluated grid capacities now follow the native contracts. Descriptions and
action/header controls render; eight minigame cards fit with the sidebar and
without extra scroll rows. All 487 JSON-UI tests and the installed server-pack
layout test pass. A fresh optimized macOS/Metal frame confirms the selector;
the user tested Lifeboat and accepted the complete result for direct publication.
See [the form evidence](docs/evidence/lifeboat-forms.md). Broader UI typography and
performance parity remain open.

2026-10-04 Lifeboat session palette repair (compatibility acceptance passed): a fresh
join reproduces gray terrain and blocked movement with coherent carriers. The
remote palette must omit vanilla data-driven definitions absent from StartGame;
the full carrier admitted 98 extra types and shifted wire air by 1,181 states.
Session admission now maps wire IDs to stable carrier IDs and converts outgoing
block interactions back to wire IDs. Inventory, falling-block visuals and block
sounds resolve retained wire identities at their consumers. Regression coverage
includes partial/full admission, custom insertion, session replacement and raw
descriptor preservation. See [the session evidence](docs/evidence/lifeboat-offline.md).
The optimized macOS/Metal client passed a 300-second lobby run with walking,
strafing and jumping. A second join opened and answered the Navigator menu,
completed `/transfer sm3`, walked/jumped in Survival Mode and opened its book
menu. Neither run disconnected or reported decode errors.
This does not close broader native timing or rendering parity gates.

2026-10-04 Lifeboat skylight investigation (historical, superseded for this session): the supplied session
trace confirms zero solved sky light but does not identify the blocked input.
Offline chunk-pipeline fixtures already produce sky light 15 when inline upper
slots are omitted, both over empty space and over opaque terrain. Limited and
limitless request fixtures also pass with all-air, empty-success and
out-of-bounds replies. These fixtures do not reproduce the reported failure;
no lighting behavior has been changed and the parity gate remains open.
The trace's `Air` samples describe the camera's non-liquid medium, not the
block's identity or light filter. Its sampled stale jobs were rejected because
their revisions changed. An offline capture of the failing column's packet
mode, palette and contents, or equivalent block/filter and column-light
diagnostics, is still needed to distinguish a missing sky source from filtering
above or at the sampled cells. No public server or account was used.
2026-10-04 fox rendering checkpoint: the pinned adult fox sample now retains
the native inherited body and tail cube bind rotations. Vanilla uses hierarchy
lookup and separate cube-bind transforms; the installed near-version native pack
supplies the retained angles.
The repair is restricted by source path, identifier and digest, preserving baby
geometry, child pivots and custom sources. The corrected entity/actor/equipment
carriers and fresh Metal client show normal adult red/arctic bodies and tails;
the user accepts the live standing gallery, with baby foxes retained. Focused
pinned-source and mesh regressions passed, including a failing-before mesh
witness. Formatting, architecture and affected compilation passed; the final
test run was stopped at the user's explicit request to skip further tests and
push directly to dev. Extended poses/transitions and exact-version native
side-by-side acceptance remain incomplete. See `docs/reference/fox-rendering.md`.

2026-10-04 portal and End follow-up (in progress, uncommitted): latest
origin/dev `800fa170f` is integrated, preserving the accepted Nether portal and
End portal frame/surface fixes. Camera, movement and network behavior follows
the new crate ownership. The dedicated surface draw now uses the updated packed
vertex format and retains shared world-light bindings. Client compilation and 33
post-integration portal regressions pass. Fresh assets include the expanded dragon
rig and complete animation script. The default client build and compilation without
acceptance features pass. No task changes are committed or pushed.

The user accepts Nether rendering, particles, overlay, hand rendering and prompt
travel, plus raised End frame eyes and the layered portal surface. Rules and
verification limits are in `docs/evidence/nether-portal-native-reference.md` and
`docs/evidence/end-portal-native-reference.md`. The scoped pre-integration compiler,
client and block-entity suite passed 269 tests; fresh macOS/Metal frames verified
frame eyes, empty/filled contrast and animated starfield depth. Fast transfer
acknowledges the committed flush immediately and releases after decoded collision,
presented footing and a fresh GPU frame. Exact native loading/cooldown behavior,
identical-version visual comparisons and release performance remain incomplete.

End sky fog modulation and sixteen repeats per face pass three tests, including
two GPU regressions. The rebuilt Metal client joins BDS and the user confirms
the dragon and crystals now appear, while rejecting the wing shape, healing beam
appearance and clouds in the End. These follow-up corrections are in progress;
cloud admission now excludes both the End and Nether and clears stale view records;
six cloud regressions pass. The downloaded dragon's child joint pivots now retain
their relative frames. Compiler, 48-tick animated hinge and emitted-membrane
regressions pass after reproducing detached wings before the fix. Entity and actor
carriers are rebuilt. Isolated Metal inspection covers six views and four flap poses;
the user identified reversed left-wing cutouts in those frames. The bottom box-face
V direction is wrong; its source-verified correction and opposing-face alpha checks
now pass all eighteen geometry regressions. Twenty-four new isolated Metal frames
confirm matching cutouts above and below both wings. The fresh v18 client compiled
and joined the saved BDS world; its architecture check passed. Scoped clippy exposed
an existing horse modulo lint, now replaced with its equivalent integer method;
the final scoped all-target rerun passes. No publication or identical-version live
visual acceptance is claimed.
Healing beams preserve strip-triangle texture interpolation while scrolling;
three renderer regressions pass. Owner-light publication reproduced its scalar
fallback mismatch and is corrected; seven publication tests pass. Exact native
beam AABB light sampling remains incomplete; publication shares body sampling.
The End still selects the ordinary lightmap, with a darker floor than its dedicated
formula; the required tint and ramp inputs remain unverified. Exact End lighting
and healing-beam brightness stay open.
Expanded neck and tail
segments, fixed-tick flap/history, full animation-script admission, healing-beam
selection, death particles, lingering breath clouds and fireball trails pass their
owner regressions: 249 client-world tests, 185 compiler unit tests and two dragon
compiler fixtures. App dimension, respawn and loading filters pass 31, eight and
twelve tests respectively. Normal dragon breath's status-event consumer, native
wall-contact flap slowdown and exact-version comparisons
remain incomplete.

The user expands the follow-up to dragon death animation, sounds and End credits,
and requests the next test launch only after these flows have been checked. The
fixed-tick death counter, cumulative lift and dragon-specific pose now pass their
regressions. Authored frame-alpha sampling retains unclamped dissolve multipliers
without advancing simulation or random state. Material draws preserve the ordered
depth-mask and equal-depth body passes. The v19 live smoke exposed a missing vertex
invariance guarantee for the equal-depth pass; its failing-before compiled shader
contract now passes with all six shader checks. Actual Metal regressions reject solid
backfaces while retaining both authored membrane faces and their separate lighting.
The first real death preview exposed a missing fractional-alpha mask: compiler and
carrier validation now admit it from the shared compiled material contract without
changing its pixels or borrowing color-mask semantics. Both failing-before artwork
fixtures pass; the actor artwork suite passes 19 tests, with three existing optional
fixtures still ignored. Rebuilt actor art and 48 isolated Metal death frames show
the rising, speckled body and expected final disappearance; 24 refreshed ordinary
frames preserve complete, matching wing membranes above and below.

Dragon death audio follows the server cue rather than an early local duplicate.
Fixed-tick synchronized queues retain actor lifetime and dimension fences; a
failing-before app schedule regression now delivers released audio in the actor
completion frame exactly once. The actual pinned death sample decodes through the
production voice, preserving native request-gain clamping before alternative gain.
Explosion previews use the real carrier, simulation, atlas and Metal shader; their
sprite frames advance and both effects expire. Exact attenuation, native mixer/DSP
timing and independent waveform comparison remain incomplete.

ShowCredits decoding, modal input, timed Skip, scroll/fade, session fences and
backpressured completion pass focused checks. Completion uses the current local
actor identity. Credits presentation keys caches and hit regions by session and
sequence. Real Metal frames exposed inherited bundle tooltips and a missing title
request; strict visibility bindings and the normal bounded artwork path correct
both. All twelve credits checks pass. Fresh macOS/Metal frames at 1280x720, DPI 2,
show the runtime Minecraft title, formatted/obfuscated text, wrapping, clipping,
aligned names and headings, and timed Skip. Credits music retains queued ownership
and music-only transitions; four scheduler/fade checks pass. Runtime OGG collection
is corrected and the production voice produces a valid credits excerpt. The pinned
sample pack lacks credits text and music; private runtime supplementation uses the
installed near-version pack and stays outside git. Exact-version content and heading
colors, native audio fade sample timing and live end-return acceptance remain open.

Death-ray geometry and additive composition are implemented and provisional. Some
geometry constants and material state are verified only for a nearby version;
version-matched comparison remains incomplete. Frame publication, retirement,
geometry, normalized fan axes and additive/depth behavior pass focused regressions.
Fresh production Metal frames show the white/magenta fans, growth and final fade.
These isolated body, particle, ray, audio and UI witnesses do not replace a live BDS
death/return sequence or identical-version native acceptance. Final scoped all-target
lint, formatting, architecture and diff checks pass. The UI renderer suite passes
50 tests with two existing benchmark ignores; all twelve credits checks pass again
after integration. The canonical v19 build joined the saved BDS world with Creative,
OP and cheats confirmed, then closed cleanly for the vertex-invariance correction.
The final canonical v20 rebuild passes, as do the shader follow-up lint, architecture
and formatting checks. Its 24 alive and 48 death Metal frames are freshly regenerated
with the invariant shader. The v20 live client joined the preserved BDS world and
released loading after terrain presentation; BDS confirms Creative, OP and cheats.
The equal-depth warning is absent from that live log. The v20 client is now closed
for the next dev integration and End follow-up build. Nothing is committed or pushed
for this task.

The v21 follow-up integrates the latest dev fire renderer and rebuilds its carriers.
The reported persistent explosion route selected a looping effect; scalar huge
explosions now select the source-identified finite emitter. Actual simulation and
fresh production Metal frames confirm sprite progression and retirement. Scalar
snowball impacts now select runtime item fragments, mask the packed particle type,
honor one fragment per received event, and use the native item spread radius. Four
particle controls and the app routing regression pass after reproducing the wrong
explosion, spread and multiplied count. General Molang initialization blocks and
the broader legacy particle table remain incomplete.

End-return ingress previously excluded ShowCredits in raw cached-session traffic;
the corrected ingress, completion wire encoding and credits UI checks pass. Boss
subscription Add/Remove responses are now queued through the session-fenced
transport, with ordered retry under backpressure. The native server can omit dragon
removal broadcasts without the Add registration. Removal ignores unused invalid
presentation fields, and retained HUD removal, reply lifecycle and wire checks pass.
Live dragon-death, boss removal and exit-portal return acceptance remain open until
the saved BDS world is exercised with the new client.

Dragon eggs now compile the native eight stepped boxes, rather than an ordinary
cube, with a failing-before geometry witness and twelve compiler checks passing.
XP investigation found an extra world half-turn that makes the authored single
textured face point away from the live camera. The corrected installed-carrier
regression passes four yaw/pitch headings after reproducing the inverted face.
XP sprite selection now reads only native Int metadata; its failing-before control
and the ordinary value-band check pass. Fresh production Metal frames show all
eleven crisp orb sprites with the actual live camera query. Exact between-tick
billboard sampling, End lighting, beam AABB light sampling and the previously
recorded identical-version gates remain open.

Thrown splash and lingering potion appearance now reads each projectile's own
short auxiliary value and shares the reviewed item sprite routes. Known potion
routes agree with the pinned effect texture arrays; the current native potion
table constructor and auxiliary values without reviewed routes remain incomplete.
This correction does not close the broader actor parity gate.

The canonical v21 debug build passes, along with focused snowball, explosion,
credits, boss UI/wire/app/session, orb, portal overlay GPU and shader checks.
Formatting, architecture and diff checks pass. It is running through a persistent
Orca terminal in the preserved official BDS world. BDS confirms PortalDimTest is
Creative and OP with cheats enabled for this live session. A fresh macOS/Metal
1280x720 gameplay capture shows loaded terrain, the accepted portal/frame art,
legible HUD and normal hand, with no GPU validation errors in the live log. The
user's live End death/return and egg appearance checks remain pending. These task
changes remain uncommitted and unpushed.

The official BDS test world has cheats, OP and creative mode enabled. A private
loopback UDP relay bypasses stalled Docker Desktop port forwarding for this world;
other running test services are untouched. Separate server-authored
WorldCorruption shutdowns remain unresolved, including one without a dimension
transfer; existing saved worlds are preserved. Four earlier debug transfers
completed in 1.142, 0.439, 0.829 and 0.369 seconds. These observations and user
acceptance close the reported slow-travel regression, not whole-client parity.

2026-10-03 current checkpoint (in progress; locally committed, not pushed):
accumulated work and follow-up fixes are committed through `97dccfb3`, including
the dev integration through `58141bc6` and its chunk-pipeline and pack-compiler
ownership split and frame diagnostics. Origin/dev `a1c5880d` is integrated
with the Profile and Marketplace changes; `d2a51dac` is also integrated.
The subsequent `8d3ca7c5` update is integrated in `f024d9ec`. Final verification and a fresh post-integration live frame
remain required. A render binding contract still referenced the old app module
after its extraction; its assertion now targets the actual render-setup owner.
The user accepts the rebuilt night snow-layer colour and actor corrections.
Current snow layers route through ordinary terrain into AO/flat lighting. The real model-fragment Metal regression reproduced
white snow at 101 versus native 33 before the fix; actual ordinary entry points
now pass day/night/fog/animation/AO witnesses, and a fresh canonical live frame
retains dark purple snow and readable terrain. Enhanced is unchanged.
Players and mobs now use native gamma material order, byte-truncated bilinear
light-table sampling and vertex-stage posed-normal shading. The actual actor
fragment GPU witness passes all 36 cases; the canonical client rebuilt and the
user accepts the live actor correction. FancyOff, dimension shade signs and
exact pinned native galleries remain separate incomplete gates. For the new
pale snow-layer edge report, TopSnow uses zero light dampening
and non-solid shading at every height. Registry regeneration changes exactly
the 16 snow states and preserves their emission. Native AO now
drives distinct centre/tangential sample planes; full snow remains geometrically
occluding but not solid for AO. The mesh regressions and 225 meshing tests pass.
Ordinary terrain now interpolates light levels before fragment-stage bilinear
lookup; actual cube/model GPU witnesses fail before and pass after the change.
The `/15` shader lookup contract is corroborated by installed near-version
Metal, not a pinned-version shader dump; exact-version parity remains incomplete.
Actor `/16`, Enhanced and the existing valid-water witness are unchanged.
The corrected carrier and canonical integrated client rebuilt. A fresh Metal
shoreline frame at 1280x752 shows coherent snow top/side shading without the
reported pale bands; the user manually confirms the snow looks perfect.
This closes the reported edge regression, not exact-version/whole-renderer
parity. The full verification gate and direct-dev publication remain pending.
The later isolated bright snow face is reproduced with a covered emitting
brown mushroom: our all-storage emitter query sets snow's directional shading
bit, while native AO/flat lighting reads the rendered block’s emission. Cube and model
lighting now carry the rendered contributor's admission separately from solved
physical emission. The failing-before regression covers all snow heights,
both storage orders, both network modes and direct/cached routes. Covered
mushroom emission and real emitting-surface shading are retained. The canonical
integrated client rebuilt and the user accepts the live mushroom correction.
Final integrated checks remain pending; it is not pushed. The previous final
gate reached clippy and failed on test-only unnecessary Vecs and a duplicate
dead-code attribute after upstream integration; these are repaired and the
complete gate still needs its successful final run.
The new opaque ice/dark shoreline wedge report has a real carrier witness:
ice was CUTOUT despite source alpha 190/255. Vanilla ice selects
layer 3, inherited color is RGBA (1,1,1,1), and ordinary cube AO is retained. Reviewed ice/frosted-ice rules now supersede stale fallback
geometry and alpha metadata; unrelated provisional families are unchanged.
Real registry identities reproduce wrong CUTOUT and zero transparent mesh faces
before the fix; compiler and meshing suites now pass. The rebuilt pinned carrier
has BLEND ice and frosted-ice materials and preserves source alpha. Existing AO,
transparent depth writes, daytime/nighttime and snow behavior are unchanged.
Fresh canonical live shoreline acceptance and final integrated checks remain
open; this does not claim whole-renderer or exact-version gallery parity.
The user rejects the first ice frame: restoring BLEND alone was insufficient.
The shipped LREG also incorrectly gave ordinary ice and all four frosted ages
filter 15. The constructor defaults are filter zero; packed ice
deliberately remains 15. A shipped-registry shoreline solver witness reproduces
zero sky inside and below ice before the fix, and now preserves direct sky 15
and sheltered lateral sky 14. Exactly five light records are regenerated with
coherent manifest pins, leaving snow, packed/blue ice, texture alpha and AO
unchanged. The world carrier is rebuilt; fresh client/frame and final integrated
verification remain pending. This follow-up is not pushed or visually accepted.
Separate incomplete partial-face parity: vanilla bilinearly evaluates
AO colour at actual face bounds while assigning light records by full corner
topology. The current midpoint-based corner selection does not close that gate;
it is not bundled into this emitter-identity fix.
The export-test allocator abort has a narrow initialized-cache representation
fix; its 13 library and nine unchanged export tests, plus ten serial reruns,
pass. The Go registry suite, vet, Rust light-registry tests and focused render
CPU/actor GPU suites pass. These are focused results, not the final integrated
gate. Direct-dev publication is still pending, without a PR or video.

2026-10-03 night/weather integration checkpoint (local, not pushed): ordinary
ambient admission and separate terrain skyDarken are rebuilt and the app suite
passes2296 tests,25 ignored. Fresh Metal frames show readable dark canopy/logs,
neutral gray snow sky and retained clear daytime rendering. A later user frame
still shows snow-layer faces too bright relative to ordinary cubes; that route
remains open, so publication is held. The full workspace gate passed formatting,
architecture and compile, then stopped at jsonui-editor export SIGABRT; it is
not reported green. Remote dev advanced again to52196199 while tests ran and
will be integrated before final verification/publication. Both server cycles
were restored true and the unchanged BDS remains on127.0.0.1:19132. No video.

2026-10-03 final sync and handoff (in progress; local, uncommitted, not pushed):
remote dev is integrated through `e0349351`, including the newly merged plant
first-hit/destroy-effects and HUD owning-rig fixes. Local snow/shrub bounds,
ambient foliage and inherited animal bind metadata remain intact. The user
confirmed daytime grass and leaves look correct after the alpha-mask repair,
but the matched nighttime fixture is still too dark: shaded canopy, logs and
ground lose visible detail. Night lighting remains open and publication is
held pending its native lightmap input audit and live recheck; daytime
acceptance does not close the broader exact-version foliage gates below.
The canonical client and Go core rebuilt successfully after this final sync.
BDS is running on loopback 19132 with eight slots for manual testing. Fresh
integrated verification passed formatting, architecture and workspace check;
the app suite exposed one drag-gesture reset regression (2293 passed, one
failed, 25 ignored). Fresh presses now cancel stale distribution before slot
collection; a subsequent app suite passed (2296 passed, 25 ignored), before
the final night/weather publisher changes and last dev sync. The final gate
must still be rerun. At the user's explicit request,
publication will be a direct commit
to dev, without a PR or recording. Existing local frame/source witnesses and
the manual acceptance are retained; no Mojang assets enter git.

2026-10-03 nighttime and snow-sky follow-up (in progress; uncommitted, not
pushed): both native and Rust brightness are 50%. The vanilla caller
enables the two ambient .96*C+.03 stages in the lightmap; our publisher
incorrectly left that flag false. Its new publisher regression failed before
the repair. Native skyDarken also has a distinct
.2 night floor and +.2 twilight bias, weathered by precipitation fog and
interpolated thunder. Terrain now receives that separate input; the accepted
clear-sky/cloud curve is unchanged. Snowy skies now receive the omitted native
rain admission (current rain>.2) and fog*4 pull toward day-weighted
.5 gray, before thunder and camera glare. These are the vanilla 1.26.50.26 constants. Current rain stays
separate from interpolated rain; the CPU view composes the gray target through
the linear thunder transform before glare, retaining the 128-byte GPU ABI.
Live BDS time query and Rust extrapolation agreed at daytime18149. A live rebuilt
night/day/snow comparison and fresh final verification are required. Minor
native byte-lightmap quantization and fragment lightmap interpolation remain
independent incomplete parity gates; they are not called fixed by this repair.

2026-10-03 dev integration and grass-side regression (in progress; local,
uncommitted, not pushed): preserved all local parity work in a recoverable
snapshot and fast-forwarded to remote dev `7251e86e` (377 commits). Rendering,
seasonal admission and the live split-gesture fixes are integrated with its
new render-api/player-runtime/inventory owners. The user confirmed the leaf
regression fixed but reported olive grass-side dirt. The new gamma cube path
had dropped the side's alpha tint-mask contract. The atlas overlay and installed near-version 1.26.51.01 opaque RenderChunk Metal use gamma RGB mixing by source alpha and opaque output. The actual GPU
regression failed before the repair (alpha-zero dirt channel 73 vs 93).
The shader now masks biome tint without altering leaf lighting/filtering,
carried/model routes or snowy grass variants. Fresh affected verification
and canonical live grass/leaf acceptance are required before pushing; the
older test totals below are pre-sync evidence, not verification of this merge.

### Profile 1.26.50 (incomplete parity acceptance, 2026-10-03)

Profile now has vanilla responsive card/tab geometry, independent scrolling,
Overview friend/follower and Minecraft achievement summaries, completed achievement
ordering, and populated Stats. The Go core maps shared service clients into profile
statistics, achievements, persona avatar and featured gallery data. Authored fixtures
verify the contracts without owner-account requests. Missing values remain unavailable rather
than invented zeros. Exact references are in `docs/profile-parity.md`.

Full 1:1 parity remains incomplete. Dressing Room has no persona destination or hanger
icon; screenshot gallery counts and navigation need local gallery persistence;
followers and achievement detail destinations are missing. Minecraft suggestion order,
persona achievement rewards and progress are not supplied by the Xbox achievement
collection and need the additional native metadata path. Offline, privacy and user-not-found
failures still share a generic error because the feed does not yet preserve their native
classification. Runtime reference artwork and the exact loading animation require the
optional OreUI originals directory; normal mode retains diagnostic fallbacks. English
formatting needs integration with the locale system. Native fixture frames are not matched
vanilla captures, and no overall visual parity gate is closed. No live server, remote
machine or owner's authenticated service request was used for verification.

2026-10-04 Profile loading recovery: direct-address and external-socket startup
now have an account core; Profile opens/refetches independently of Home and
other catalogs. Missing workers, failed control calls and withheld replies end
in the existing unavailable/Retry state. Socket fixtures reproduce the old
stuck state and verify recovery without an owner-account request. The local
60-second RPC bound is transport protection, not vanilla timing parity.
Per-request diagnostics report fixed outcomes with rate limits and no account
material. Native privacy/offline classification and a separate permissions
facet remain incomplete; see `docs/profile-parity.md` for references.

Realms add/join: invitation codes now have a signed-in launcher route, read-only
verification, explicit membership confirmation, cancellable preview requests,
retryable errors and a returned Realm that can be played. Real remote acceptance
has not been verified. Incomplete parity: pending invitation acceptance, subscription
creation, account multiplayer permission admission, service-specific error messages,
and the full options and confirmation presentation. This does not close the Realm
management parity gate.

2026-10-03 overlapping crafting ingredients: native parity remains incomplete.
Consume requests now find a complete assignment for recipes the existing matcher
accepts, including overlapping tags and specific items. Ordinary and craft-all
requests are covered by regressions. The exact native assignment order for custom
overlapping ingredients still needs a vanilla client controller reference;
this fix does not close the crafting parity gate.

2026-10-03 leaf density, atmosphere and daylight-clock audit (in progress;
local, uncommitted, not pushed): current vanilla leaf blocks and
non-seasonal leaves select an opaque deep layer from the six primary
neighbours, while outer fancy leaves remain two-sided cutouts. Shared planes
follow current occluder. Generated world-only selectors retain covered,
exposed, deep and outer leaves without changing inventory art; world carrier
version 12 rejects former selectors/mips and missing leaf face metadata. Native world palette channels
are doubled without clamping (installed near-version RenderChunk Metal), unlike
the particle consumer. The seasonal GPU palette now preserves extended
float values. World-only UNORM views preserve native gamma-space filtering,
without another allocation/upload or changing carried/nonleaf SRGB sampling.
Vanilla atlas mip generation uses
direct byte-space box averages from the original image at every mip, without
premultiplication or coverage correction. The former mip producer weakened
fully covered alpha255 to170; in the audited spruce bilinear footprint it
reduced cutoff coverage from native79% to21%. Separate/deduplicated world leaf
layers now preserve native mips and clone animation timelines, with original
shared/carried bytes unchanged and all page/timeline bounds enforced. Four CPU,
four compiler and two real GPU sampling regressions pass. Installed near-version
1.26.51.01 RenderChunk Metal corroborates cutoff.5 and the gamma palette/output;
active A2C/MSAA and broader cached-solid occluder equivalence remain open.
Server-pack replacement leaves still use the shared old mip producer in
block_overlay.rs; that independent route is incomplete. The MCBEAS12 canonical
rebuild has now been compared live against installed near-version vanilla at two
fixed angles of the reported spruce fixture; exact-version foliage parity stays
open, independently of this regression check.

The user's latest matched spruce views also identified missing shading, face
orientation and sampler contracts. Vanilla shade brightness
assigns primary leaves/emitting blocks .2 independently of cached solid occlusion.
Vanilla ambient occlusion averages outward/side/diagonal shade and raises
average*face-coefficient to the authored exponent before vertex interpolation;
solid side tests alone decide the diagonal fallback. Direct/cached lighting
regressions and all 134 mesh tests pass with leaf shade admission. Current cube tessellation reverses opposing face U axes, Down V, and applies pack-authored
isotropic rotations with wrapping world coordinates. The compiler now carries
per-face isotropic flags and AO exponents (pinned spruce .80); carried choices
remain unchanged. Unsupported custom exponents fail explicitly rather than
silently quantizing; arbitrary custom exponent support remains incomplete.
Leaf quads stay block-local to preserve rotations and clamped texture edges.
The atlas binding uses Dragon 0x155; conversion to the native D3D sampler selects nearest min/mag, linear mip and clamp UVW. The
leaf-only sampler reuses the existing UNORM views with no texture allocation.
Ordinary cube lighting also now follows the native gamma product: exact atlas
creation/upload and installed near-version opaque RenderChunk
Metal corroborate that this is not a leaf-only exception. Ordinary cube input
is converted back from the retained sRGB sampler before lighting; carried,
model, actor, UI and enhanced routes remain untouched. Broader nonleaf atlas
mip/filter parity and fragment lightmap-UV interpolation remain incomplete.
Integrated native sampler/colour/UV GPU and shader-isolation tests pass. The
canonical debug client and assets were rebuilt and restarted; fresh unpaused
Mac/Metal frames at two authoritative matching cameras show the backing log now
dark, with coherent canopy shading/orientation and substantially closer density.
Evidence is local in `.local/dev/leaf-native-v12-acceptance.md`; different window
aspects and installed native 1.26.51.01 are recorded, not called pixel-identical
or version-exact acceptance. Server time/weather cycles have been restored.
The full verification rerun caught an exact-float assertion in the falling-leaf
white-colour test (one ULP below one after sRGB transfer); it now checks the
clamp and one-ULP bound without changing production colour code. The subsequent
app suite passed (2280 tests, 23 ignored); the remaining workspace and doc-tests
passed separately (4502 tests, 67 ignored, exit 0). Full verify-affected returned
exit 1 for the protocol offline transfer harness's transient ConnectionClosed;
both immediate exact-test reruns passed without a protocol implementation change.
Formatting and architecture rechecks passed. Final workspace/all-target clippy
with -D warnings and the last GPU filtering/plugin test rerun passed (exit 0),
as did the canonical client rebuild after lint-only cleanup. Broad foliage
density/colour gates stay open.

The first sky slice follows vanilla's
Y256/radius2000 decagon fan, the orbital builder's fixed UV basis and
28.08/18.924-degree sun/moon diameters, renderSunAndMoon's sampled
alpha/rain scaling, and installed Stars' blend. Real GPU tests cover these
contracts. Star brightness uses the native trig lookup and weather-before-clamp order. Current getSunIntensity and the
camera callers establish narrow camera-alignment glare (.9 threshold), broad
fog/sunrise alignment (.35), full-precision trig and independent weather-fog
attenuation. Sky/cloud colour subtract narrow glare*.2, not rain*.2. Air fog
uses the separate precipitation-lattice accumulator, fullcosf day brightness,
RG.94+.06 / B.91+.09 night floors and broad sunrise blend, without a guessed
thunder tint. The WeatherRenderer accumulator starts at zero, updates previousRain*.5 across27 admitting biome cells, clamps
sum*.2 then smooths .99/.01 at20Hz. Unknown climates are skipped;
mixed rain/snow float addition order and custom-dimension weather admission
remain incomplete. Camera-local ambient exposure/collector remains incomplete.
Classic cloud tessellation now follows vanilla's finite64-cell window,
per-texel RGBA, outward face winding and native back-face culling. Vanilla uses RGB-only writes and strict reverse-Z Greater.
Fancy selects alpha blend/depth-write off.
Current preRenderParameters supplies distance coefficient1; camera adjustment subtracts its piecewise margin (256blocks ->240), minimum40. Classic
DistanceControl reads that scalar unchanged, not quality*3. Conditional platform
caps remain an admission gap. Eight real GPU/cloud contract tests and ten
cloud-config tests pass. Exact gamma render-target blending for clouds/stars and
live matched-view acceptance remain open. LevelRenderer's local cloud clock is separate from named
daylight and starts at zero. Session
recreation mapping and bounded stall catch-up remain provisional.

SyncWorldClocks is decoded and admitted using its generated packet ID, then
published in FIFO row order. Native clock registration and lookup use the pre-registered hashed Overworld identity; names
cannot redirect it. Signed time, independent pause and equal-integer updates
are retained. StartGame elapsed world age is no longer used as daylight:
vanilla stores it separately from registry clock state. Five protocol
and 39 app environment tests pass after the canonical-ID fix, including local
renderer-clock independence. Native client simulation tick/pause/stall cadence,
cycle-disabled bootstrap linkage remain open. The canonical offline BDS run
and user testing confirmed clock updates; diagnostics retain the named clock
anchor, including doDaylightCycle=false, independently of local cloud motion.

2026-10-02 creative inventory disconnect follow-up (test-green; local,
uncommitted): synced remote `dev` at `f3dbc76a` while preserving the local
rendering/BDS changes. The failed session ended on a remote NetherNet channel
close, not a Rust panic. Vanilla's creative
item-creation request and its result action declare the
selected catalog prototype before transferring a full stack. Our creative path
sent an empty result list. Two regressions failed against that implementation;
the request now retains the catalog item's identity, count, aux, block runtime
id and user data. The user confirmed the crash is gone; the rebuilt offline BDS
session logged Accepted creative takes, placements and gathers. This does not
close general creative-inventory parity; final integrated verification remains open.

2026-10-02 incremental inventory dragging, spawn eggs and baby polar bears
(in progress; local, uncommitted, not pushed): primary/secondary drags now apply
and rebalance each newly visited cell before release, preserving destination
baselines and chaining pending request IDs. This matches vanilla’s incremental split behaviour.
The before-fix pointer regression failed; 215 inventory-focused tests then passed.
Spawn eggs resolve actor spawn_egg declarations to item routes; all 86 pinned
retail eggs passed the in-memory compiled-icon lookup. Legacy controller aliases
are lowered to controller__<alias> by upgrade_v1_8_to_v1_10, preventing an adult move clip bypassing the pinned polar-bear
!query.is_baby gate. Three roots tests, 33 entity integration tests and the real
pinned polar regression passed. Canonical carriers/client and held-button/live
visual acceptance remain open. Modern stair corner/raw-direction handling and
snowy-leaf conditions/falling-leaf particles are additional in-progress gates.

2026-10-02 follow-up inventory/rendering regressions (local, uncommitted,
not pushed): BDS accepted the first two drag slots, then rejected a third-slot
request with status 50: its second Place named the first donor's positive ID as
the new destination ID. Vanilla `_transfer` opens/closes an
individual item-stack request scope when no outer scope is active; split
handling has no outer scope. Rebalancing now stages one request per transfer,
binding the next transfer to the prior sparse-cell request ID. Whole-hover
admission is atomic on queue/identity failure. Retained cursor remainder excludes
items added from outside the gesture; failed or interrupted drags are cancelled.
Native container canSet restrictions and contribution correction/clamping remain
incomplete. On 2026-10-03 the user tested the canonical build and confirmed
multi-slot splitting works. The same offline BDS session recorded 199 Accepted
inventory responses, with no rejected response during that test. This closes the
reported multi-slot regression, not the broader container parity gates.

Trader llama adult rasters were rejected for fractional alpha and native three
samplers were incorrectly drawn independently. Content-pinned material policy
and one GPU three-sampler draw now implement the native RGB mixes; exact pinned
controller/texture witnesses and installed 1.26.51 material/shader (near-version)
are distinguished. Pinned adult/baby x four variants, raster admission and shader
validation pass; live geometry/depth/material acceptance remains open. Snow-top
crack/highlight bias now follows model winding, and stair overlays use resolved
variant rotation. Current Bush and DeadBush bounds have focused tests.
Native stair wire outlines intentionally remain full cubes. Fence/gate outline
height, plant random-offset and cutout-highlight parity remain incomplete.

2026-10-03 animal/foliage follow-up (local, uncommitted, not pushed): native
same-identifier geometry history retains the llama body's older cube-only +90 X
bind when the modern downloaded sample omits it. Geometry loading uses missing-field fallback and inherits older definitions;
the exact current bones-parser behaviour remains unconfirmed. The repair is source-digest pinned,
does not rotate the bone or its children, and preserves custom/explicit binds.
Pinned sample and torso bounds regressions pass. The canonical debug build includes rebuilt carriers; the user confirmed llamas and polar bears now render
correctly. The pinned baby-bear runtime test confirms its separate model, body
rest pose and adult-animation gate. This closes those reported regressions, not
general animal parity. The newly reported missing wolf tail is caused by absent
`query.tail_angle`, not missing geometry. Its local implementation follows the
vanilla wolf tail-angle behaviour: angry overrides tame,
otherwise tame health fraction or the wild angle, in radians without frame
interpolation. Four focused query tests and one pinned-carrier full-pose test
pass (wild, tame/full and half health, angry and baby). This fix is not yet in
the running build and its live rendering gate remains open. Missing/invalid
tame HEALTH and custom actor identifiers use a finite conservative fallback;
those incomplete inputs do not establish broader native query parity.

2026-10-02 snowy leaves/ambient foliage (in progress; local, uncommitted,
not pushed): current vanilla seasonal palette generation and native
foliage policy route covered/exposed species cells separately. World leaf
materials now retain that distinction without changing carried leaf art. Live
biome data carries snow_foliage and optional maximum snow accumulation; invalid
optional maxima are counted/skipped, not fatal. Palette snow blending and
half-intensity RGBA8 quantization follow the native consumer. Full-height
shelter snapshots and lower-column invalidation are bounded by resident world
data; TopSnow delegates a non-air extra layer's replaceability before testing
its own layer height, while uncovered eight-layer snow shelters. Shared tests
cover uniform/mixed primary and extra storage, cross-section shelters and carried
leaf isolation. This gate is
INCOMPLETE: the current eligibility uses base biome temperature, not the native
altitude/regional noise and generator/version-dependent threshold; general
replaceable-block coverage beyond the identified native ground plants
and liquids is not complete. The user confirmed native leaves gradually whiten
as weather changes. The vanilla client row update’s weather-driven accumulation/melt and native previous/current rain tick
samples are implemented and test-green, but not yet running. Palette-only GPU
refresh keeps dense biome IDs and mesh identity unchanged. Built-in Overworld
weather admission is constructor-backed; custom dimension admission, climate-row
deduplication/cap handling and the renderer's lifetime across reconnects remain
incomplete. Positional climate is a separate eligibility gate, not an input to
this native row-update formula. Registry/dimension changes preserve the clock;
fresh-session clock reset is provisional until native renderer recreation is proved.
Vanilla leaf ambient particle sampling, independent fixed-tick
cadence and cached biome-tinted emission are in the canonical build above and
passed 18 focused tests; live visibility is still open. Specialized leaf families
and broader block animate-tick callbacks remain incomplete. Falling-leaf parity
is not closed by seasonal palette tests. A read-only emission audit corroborated
manual position plus authored shape,
so the current caller's block-center and pinned shape offsets were not changed.
Opt-in bounded DEBUG admission counters distinguish samples, eligible leaves,
roll hits, below-material admission, returned emitter IDs and a lower bound on
actual particle-count increases. Their instrumented adaptive-sampler workload
is diagnostic evidence, not an uninstrumented performance/parity witness.

2026-10-04 horse rendering: vanilla compilation now selects the greatest compatible
client-entity minimum before rig/artwork binding, instead of filename order. The
pinned adult and foal models preserve their neck, head and hip transforms and
saddle/rein visibility. Horse rearing, grazing, mouth and tail variables now read
their native tick/state inputs. Regression fixtures reproduced the obsolete-model
and missing-rearing failures, then passed with the rebuilt carrier and runtime.
Metal gallery frames show connected adults/foals and correct tack; the user accepted
horses and pigs. Pig hips match the authored model, including its slight rear-leg
overhang. Full native render-time sampling, the complete horse state product,
session-pack version selection and equal-minimum merging remain incomplete; this
closes the reported model defect, not the broad animal parity gate. See
`docs/reference/horse-rendering.md` for the vanilla rules and supported scope.

2026-10-03 Enhanced rendering: hard-disabled after macOS GPU page faults and a
WindowServer watchdog panic. The fixed renderer switch blocks plugin setup,
Enhanced shader specialization and effect passes. The toggle is hidden, and saved
settings, environment and CLI requests resolve to Vanilla. The GPU fault remains
unresolved; Enhanced visual and performance gates remain incomplete.
The disable was inspected on macOS 26.5.1/Metal at 2560x1440 content pixels
(Retina 2x, automatic GUI scale), using the rebuilt client and a saved Enhanced
preference. Home and Video settings remained legible with normal geometry,
clipping, layering and colours; the Enhanced control was absent. Settings clicks,
scrolling, hover focus and Escape navigation worked. This checks the disable,
not gameplay performance or the unresolved GPU fault.

2026-10-02 native comparison follow-up (in progress; local and uncommitted,
not pushed): both real Minecraft and Rust now join the same offline official BDS
world through NetherNet, with the conventional loopback endpoint retained.
Rust's production LAN route uses advertised offline admission and the server
nonce; actual Minecraft Login/spawn passes. Anonymous HTTP still correctly
fails server identity policy. The user confirmed thin-block picking, covered
vegetation and the earlier snow fixes, but reported missing seed sprites,
ordinary grass sides under snow, detached pig bodies and flashing lake water.
The seed crosswalk now records four explicit components from the installed
iOS 1.26.51 item archive; vanilla loads component icons
during item client initialization. This is labeled a near-version asset witness, not an
exact-version executable gate. Seed sprite/carrier tests pass. Missing cube
pivots now use the uninflated box center in both entity and skin parsing, matching
vanilla geometry parsing and the official schema; real pig/cow/sheep model tests
pass. Rebuilt carriers and live Metal/Retina frames show seeds in the inventory,
hotbar and hand, connected cold pig/cow bodies, and snowy grass sides. Vanilla
grass selects the alternate side for TopSnow,
Snow and PowderSnow directly above; per-coordinate greedy merging and upper
sub-chunk invalidation retain that choice. Server-pack alternate remapping and
the complete animal/state product remain incomplete.
Water fixes distinguish transparent full-cube liquid obstacles from air samples and retain native terrain
depth writes/RGB-only writes (with the installed 1.26.51 material as a near-version witness). A real frame-order hazard
was also found: Queue captured old water partitions before PrepareResources
replaced their snapshot. Geometry and both sort publications now precede Queue;
the production schedule regression checks deferred allocations and four changing
partitions through draw. The integrated lake regression now passes actual
sideways movement and several corrected camera angles on official BDS: ordinary
ice and water retain their surfaces, with zero address/reference/segment-budget
fallbacks in the mixed-order diagnostics. Final-build input/captures also verify
seed icons through inventory close/reopen, and a controlled plain-grass -> snow
-> plain-grass fixture verifies both sides of the live material update. Standalone Go tests, vet and the actual Rust
offline-core login harness pass after recording an already-pinned transitive
dependency in core's own module files; no dependency version changed. The latest
water-integrated build passes the complete Rust workspace suite (including
required native Enhanced GPU tests), strict all-target Clippy, formatting and
architecture. This same live build completed ten minutes of advancing Metal frames without a new GPU
report; this is smoke evidence, not permanent stability or performance acceptance.
The broader animal gallery found sheep rejected by the binary-alpha actor-art
filter: their native low-alpha dye-mask texels are not opacity. The content-pinned
native mask route now passes focused tests, the complete integrated workspace,
strict Clippy, architecture and the actual rebuilt-carrier page regression. Live
baby faces are visible, but the user caught a still-missing adult face before
visual acceptance: the legacy wool geometry's derived head replaces rather than
appends the sheared base head's cubes. Vanilla geometry parsing appends cubes unless reset is authored. The shared inherited-cube
merge fix and adult face/snout live acceptance remain in progress. Native sheep
dye palette, complete gamma/lighting/overlay order and arbitrary RGBA zero-sentinel
handling remain incomplete.
The creeper drew blue: one alpha-8 texel outside every face of `creeper.png` failed
the binary-alpha actor-art filter, and the body route then fell through to the
`query.is_powered` armor overlay's `creeper_armor.png`. Fractional alpha is now
rejected only on texels a drawing geometry can point-sample (scrolling `uv_anim`
layers count every texel), unsampled ones are cleared, and the body route takes
only the first unconditional controller's art. The pinned pack now binds the
creeper and admits two NPC skins; blaze, spider, cave spider, enderman and drowned
still fall back because sampled texels carry unverified fractional-alpha material
semantics. Creeper ignition/defuse visual acceptance remains incomplete pending
rendered comparisons. The charged aura material fallback is provisional; exact
target material equivalence remains incomplete.

Ordinary terrain-blend model/water faces now share the current native perspective
metric; ordinary Ice and water use
layer 3. A per-chunk combined draw
planner retains exact uploaded model-order witnesses, including candidate-arena
entity remapping, and indexes the committed water snapshot. It emits contiguous
runs without per-face phase items or a shader fork. Safety policy limits new CPU
planning to the existing per-frame reference count and all mixed draws to 4,096
segments per frame; the CPU limit is separate from the shared GPU upload budget.
The exceptional separate-stream fallback remains incomplete. Full native parity
is not claimed: deferred/enhanced water's separate layer, orthographic ordering,
reverse-winding distance bias and face culling, native equal-distance enumeration,
global inter-chunk ordering, complex noncube motion barriers and the complete
shader/fog/flow-UV contract remain open. The reported moving-camera lake regression
passes; this does not close these wider parity gates.

2026-10-02 live snow/plant/animal follow-up (in progress; local, uncommitted,
not pushed): the integrated client was reopened on the existing offline official
BDS world. The reported fern/short-grass/flower/mushroom diagnostic cubes have
valid compiled crossed models; the contributor resolver rejected native covered
TopSnow plus vegetation as conflicting solids. A bounded two-contributor path
now retains snow occlusion and the plant's own render layer. Vanilla snow layers use terrain tessellation and separate visual bounds.
Separate vanilla selection bounds now target passable plants and thin snow
without changing movement collision. Selection random offsets remain incomplete.
Animal fixes now retain cube-local bind-pose rotations and include native bone
defaults in Molang `this`. The polar-bear sample omission is repaired only for
the exact pinned source digest: the installed iOS 1.26.51 archive is a near-version
asset witness; the transform contract follows current 1.26.50.26 behaviour. The full Rust
workspace suite (including Enhanced GPU validation), strict all-target Clippy,
formatting and architecture pass, and required carriers/canonical client were
rebuilt. An MCBEENT4 bounded preflight regression found during integration is
fixed and its actual rebuilt carrier round-trip passes. Live frames/input remain
pending; no visual or stability gate closes. Local BDS now has an explicit
fixed host-port option for the requested conventional port. HTTP NetherNet
responds but the actual identityless transport probe is rejected with native
error 37 despite offline mode; this is not successful offline acceptance. The
documented LAN signaling path is under investigation, with discovery mapping
kept on loopback and separated from real Minecraft's occupied discovery port.
The Rust client remains closed; all changes are uncommitted and the existing
world is retained.

2026-10-02 Enhanced startup crash: incomplete. Offline native Metal validation
passes a populated graph and 120-frame lobby actor replay, but the reported
post-join crash is not reproduced. See `docs/reference/enhanced-startup-validation.md`.

2026-10-02 dev resync (test-green uncommitted, not pushed): fast-forwarded `dev`
from `a1b0e289` through upstream `502ee525` and restored the uncommitted snow,
GPU-safety and BDS multiplayer fixes. The block-entity shader conflict retains
upstream's selection extraction/opaque-phase reset and the checked constructor.
A recovery stash preserves the pre-sync work. Focused tests, the full workspace
suite (requiring the new native offscreen Enhanced GPU test), formatting,
strict all-target Clippy and architecture pass. Runtime assets and the canonical
client were rebuilt. The game was not relaunched; the previous native smoke
witness does not validate this newly integrated build. Snow's blended fallback
and height-aware face culling are corrected, but the full native angle gallery
and covered-vegetation parity remain incomplete. No visual or stability gate closes.

2026-10-02 macOS stability (in progress; no acceptance gate closed): repeated
Apple M3 Pro/macOS 26.3 GPU events attribute firmware-detected lockups to
`bedrock-client`; subsequent WindowServer watchdog panics and AGX-blocked
threads explain the whole-host freezes. Cinnabar's custom shader constructors
now enable Bevy/wgpu runtime bounds and loop checks instead of the unchecked
default. The shared biome shader bounds its data-driven loop by the existing
CPU lattice limit and guards descriptor/payload/lattice spans and weights.
Metal uses the existing validated direct chunk draw path until indirect
submission clears a native stability gate. These are concrete safety fixes
and an isolation workaround, not proof of which GPU command caused the hangs.
Focused tests, the full workspace suite, formatting, strict all-target Clippy,
architecture, Go tests/vet and the canonical rebuild pass. A bounded native
Metal/Retina BDS smoke run exercises actual walking, server camera corrections
and chunk loads with report/frame/memory monitoring. Prolonged repeated-session
stability and isolated causal attribution remain required.
Snow geometry/material corrections and BDS multiplayer changes remain local
while this stability gate takes priority. See docs/reviews/macos-gpu-lockup.md.

2026-10-01 Enhanced rendering: opt-in non-parity extension; Vanilla remains the
persisted default. This work never closes a vanilla parity gate. T0 adds the
setting, camera marker and pipeline key isolation. Visual acceptance and all
subsequent effect/performance gates remain incomplete. T1 adds HDR, Bevy bloom,
sun-driven grading, an ACES-fit curve and palette-derived emissive surfaces.
T2 adds two default texel-snapped shadow cascades with alpha-tested terrain/model
casters and sky-light-gated PCF receivers. Actors do not cast/receive yet.
T3 enables half-resolution, 16-step shadow-map shafts and matching main/caster
foliage wind plus surface water displacement. T4 adds opaque colour/depth snapshots and water SSR with Schlick Fresnel,
analytic ripples, sun glints and depth absorption. Offscreen reflections use sky
fallback. T5 GTAO/TAA/motion vectors are deferred. Native visual calibration and
60-fps performance acceptance remain incomplete.

Current execution order: [playable multiplayer track](docs/tracking/playable-multiplayer.md).
This preserves the full scope below; historical snapshots are not current runtime acceptance.

2026-10-01 home promo: incomplete. The gathering query now uses the desktop
`Windows10`/`Win32` identity, and downloaded badges bind `RawPath`. Empty button
labels use vanilla's localization fallback. Messaging sends the selected UI
language as `Accept-Language`. The offline fixture is authored; a recorded
response, the exact current public-config request and complete gathering click
behavior remain unverified. No visual parity gate is closed. See
[the investigation](docs/home-promo-investigation.md).
2026-10-01 world lighting (incomplete): the classic RGB table now has the current
vanilla composition, gamma, night-vision normalization and darkness subtraction,
shared by terrain, actors, items and hands. Current dimension-ramp dispatch,
the ambient-adjustment caller flag, effect-duration envelopes, conduit dispatch
and material color-space conversion remain unverified. The existing sky-darken
and effect-envelope inputs remain provisional. Offline GPU evidence is not a
native visual parity gate. AO now uses channel maxima, the conditional diagonal, the opaque-block shade
curve, emitting/ordinary face factors and inset model planes. Full current
shade/solid-render property export, component exponents, dimension shading modes
and box-average interpolation remain incomplete. The full audit work is in
progress on fix/world-lighting.

2026-10-02 inventory model quality (implemented, upstream-integrated and live-checked): vanilla
GUI block/shield and live-player renderers submit geometry at their controls' scale,
not enlarged fixed 16/32/64-pixel item thumbnails or a 96x112 player raster. The new
bounded JSON-UI mesh path uses original carried/material/skin/armor texels, native
projection and face colours, sampled material alpha cutoffs and isolated player
model depth. Flat pixel-art sprites remain point sampled. GUI poses are no longer
rounded to whole pixels or quarter/half-degree steps, and the player's Fancy light
formula is a float separate from byte colour/tint. The follow-up replaces preview
GUI-icon planes with actual six-face cubes, extruded sprites and literal authored
held models, using native hand pivots and both inventory hands. Tooltip layout now
uses the native runtime purpleBorder nine-slice, mouse overflow rules, text offset
and pitch; original floating-point texel UVs preserve extrusion side centers.
The reported flat held-thumbnail and plain-tooltip regressions passed fresh
macOS/Metal Retina-2 rendered-frame checks against offline official BDS. The
first live offhand Shield exposed a missing expression-bound ModelPart origin;
the correction now passes both hand poses and real-carrier tests.
2026-10-07 non-cube block items (slabs, stairs, walls, fences) also draw as GUI geometry at
display resolution instead of a 32x32 thumbnail (icon carrier v3 names each thumbnail's world
state). Incomplete: their GUI tessellation is still the provisional cube projection, blended
materials (stained panes) keep the thumbnail, and the bamboo fence and fence gate items show
misplaced quads, as their thumbnails already did.
Integration retains upstream's gamma-space UI layer, font/animation paths and
independently inherited image/sidecar overrides. Stateful inventory/HUD providers
explicitly clear empty icon bindings so compact icon tables cannot leave duplicate
items behind; moving between hands and repeated reopening passed the live rerun.
After integrating upstream dev through 0979ff22, focused tests, the full workspace
suite, formatting, strict workspace Clippy, the architecture gate, Go tests and
Go vet passed, and the canonical client/core were rebuilt and live-tested.
No complete native parity gate is closed.
Special GUI block shapes, custom/persona/slim
geometry and animation/held-model layers, native glint/material/color formats,
hardware sample coverage and controlled matching vanilla frames remain incomplete.
See docs/reference/inventory-gui-geometry.md, player-preview-rendering.md and
inventory-hover-tooltip.md for the native contracts and exact build/frame evidence.

2026-10-02 dev integration: the first-person item/block/attachable, grass material,
arrow, name-tag, crouch/shield, offhand, inventory reopen, game-mode and inventory
reconciliation corrections below are integrated with the upstream `dev` branch.
The canonical macOS/Metal Retina-2 client was exercised against regenerated,
offline loopback vanilla BDS. Offhand take/place and subsequent moves, repeated
drop/pickup with fresh stack IDs, whole-stack drops, three inventory reopen
cycles, and persistent charged-arrow inventory icons passed live acceptance.
The full workspace suite (four test threads), formatting, strict all-target
Clippy, architecture gate, Go tests and Go vet passed locally. Source records
retain the tested executable hashes and frame identities; private runtime
payloads remain outside git. Historical local/uncommitted statements below
describe their original snapshots, not this integrated revision's Git state.
All explicitly incomplete native visual/material, arbitrary-container and
lifecycle parity gates remain open; this is not full vanilla-parity closure.

2026-10-01 menu scene ownership: gameplay input uses the screen absorption policy;
world queues and both first-person paths obey game visibility. Pack flags retain
vanilla defaults and inheritance. The existing full-screen Settings panorama also
suppresses gameplay. Incomplete: a version-matched native comparison of the menu
background stack and hand appearance; offline snapshots do not close that gate.
References and the raw-input audit: `docs/parity/menu-scene-policy.md`.
2026-10-01 mesh backlog: worker admission keeps a bounded queue beyond one worker wave;
light changes coalesce into one pending successor even while cancelled work retires.
Current-light halo checks, cancellation and output memory reservations remain required.
Retired sessions no longer repeat their last publication backlog in later frames.
See [offline investigation](docs/reviews/mesh-stall.md) for commit attribution and vanilla
references. Incomplete: exact native rebuild timing, release frame spikes and network
latency acceptance. Offline drains do not close those gates.

2026-10-01 JSON-UI layout: sizes, anchors, stack/grid/scroll and clipping follow
the 26.30 layout rules (`docs/tracking/vanilla-parity-gaps.md`). Texture paths
match exactly as the client's asset index does; an unresolved one draws the
default white texture tinted by `color` (vanilla's `textures/ui/White`). Not
live-accepted. Provisional, labeled incomplete: chat autocomplete rows sit at
the top of their grid because the client answers no `#get_grid_size`, so whatever pads them is unidentified; `size`
animations scale draws at paint time instead of relaying out each tick.

2026-10-04 chat editing: physical modifiers survive input suppression, and
select-all/copy/paste use the shared editor. Selected text now reaches the
existing inversion painter through the authored edit target and clipping panel.
Provisional, labeled incomplete: exact Windows Bedrock selection tint/blend
parity remains unverified; visible selection alone does not close that gate.

2026-10-01 JSON-UI control rendering: images follow 1.26.50 `SpriteComponent`
(keep_ratio on by default, fill, uv/uv_size defaults, control nine-slice, tiled
axes and scale, clip direction none by default with pixel-perfect snapping),
labels follow `TextComponent` (0.5/1/2/4 font sizes, line padding, locked
colour/alpha, hyphen chops, per-line alignment, `...` truncation) and UI blends
in sRGB-encoded values through an offscreen layer. Not live-accepted.
Provisional, labeled incomplete: `grayscale` uses Rec. 601 luma (retail
material not inspected); placeholder hiding ignores focus; `enable_profanity_filter` reaches the host but selects nothing.
Nine-slice geometry for controls smaller than their opposing borders remains
incomplete: the current proportional fit needs a native overlap and clipping pass.
2026-10-01 resource packs (in progress, not parity-accepted): Global Resources imports
optional packs above the pinned base and below world/server packs. Applying resource
changes in a live world is an intentional Cinnabar extension; vanilla forbids it.
Worker preparation and revision-stamped publication preserve the connection.
Resident block geometry, biome records and transparent draw references now stage
with the atlas; GPU-wide atomicity still needs a native pass, including non-block
uploads. Actual compiler reads drive subscriber invalidation. Immutable imported
revisions, pack thumbnails, automatic hardware tiers, item metadata sprites and
named bitmap/outline fonts are implemented but not parity-accepted.
Incomplete: native frame/GPU/process-memory acceptance, full remote gates,
TTFMSDF/precomputed MSDF rendering, contextual item icon routes, verified retail
missing-icon fallback, and native font alias/metric comparison. Outline MSDF
fonts currently use provisional alpha rasterization. No visual gate is closed.
See [continuation references](docs/resource-pack-continuation-references.md) and
[original resource-pack references](docs/resource-pack-references.md).

2026-10-01 chunk streaming: missing-column deadlines are local. New data in the
current publisher cohort keeps its quiet deadline active; duplicate and foreign
traffic cannot renew it. Requested neighbours still block and receive priority.
Provisional, labeled incomplete: the existing one-second fallback for unannounced
missing columns remains a Cinnabar approximation, not a vanilla parity gate.
Vanilla chunk rebuilds use a radius-16 X/Z availability check:
all nine eligible horizontal columns are required, with no timeout exception.
No new provisional geometry or lighting path is introduced here.
2026-10-01 modding: Cinnabar extension, disabled by default. The developer-only
WASM component spike exposes a bounded JSON-UI label and local demo keybind,
with fuel/memory limits, trap quarantine and transactional hot reload. This is
not vanilla behavior and closes no parity gate. Incomplete: process isolation,
compiler quotas, package permissions/signatures, server policy negotiation,
multi-mod lifecycle, production API stability, vanilla hide-GUI/alpha propagation,
and native visual/performance
acceptance. Only explicitly selected local developer components are supported;
do not treat this as admission for untrusted downloaded mods. See
`docs/modding-spike.md` for the executable sample and offline evidence harness.

2026-10-04 modding gameplay API: experimental, non-parity extension. Separate,
default-denied grants expose bounded current-frame remote player snapshots and
transactional local actor rotation after physical look and before movement.
Focus, input authority, cursor capture, session presence and camera ownership
gate the app adapter. Incomplete: server policy negotiation, user-facing grant
and revocation UI, production API stability, native multiplayer acceptance and
cross-platform runtime acceptance. Loaded player data is not line-of-sight or
visibility evidence. No aim-assist algorithm is installed and no vanilla parity
gate is closed; see `docs/modding-spike.md` for the contract and opt-in switches.
Generic cosmetic HUD cards and crosshairs add a separate presentation-only grant,
bounded retained JSON-UI data, ordinary cursor visibility gates, transactional
revocation, and 64-control settings pagination. Target-platform rendered evidence
remains required before this presentation addition is cleared to push.

2026-10-08 local player-state extension: a separate default-denied, read-only
grant exposes current-session presented inventory/gear and active status effects.
Unknown cells, item identities and durability remain explicit; finite effect
durations follow the existing estimated server clock. Host callback budgets and
session fences clear stale payloads. Experimental, non-parity API; no vanilla
gate is closed. Incomplete: custom component durability maxima, production API
stability, server policy/grant UI and native cross-platform acceptance. See
`docs/modding-spike.md` for the contract.

2026-10-04 modding screens (`player-mod` world, BEI platform P1–P5): experimental,
non-parity extension. The world joins `cinnabar:extension@0.1.0` beside `extension`.
`CINNABAR_MOD_PACKAGE` or a `CINNABAR_MOD_SET` package entry loads a hashed package
(`mod.toml`); its JSON-UI overlay draws beside every container screen, clipped outside the
container's panels, and its view draws over the still-open container, with the overlay
still taking input beside it. One mod, the earliest drawing in load order, owns the screens.
Session items and recipes (`cinnabar:session`) are read-only. Provisional, labeled
incomplete: the GUI rect is the bounding box of `root_panel` and every laid-out
non-full-screen control, not a per-panel union read from the vanilla pack; the
exclusion list is empty (vanilla status-effect and toast areas are not yet
reported); an overlay node that meets the GUI rect (the view's drawn bounds while it
is open) is dropped whole rather than clipped. Headless macOS captures at
1920×1080 verified the overlay, text input beside an open view, and return to
inventory. Other platforms and scales remain incomplete. Session data
gaps: no smelting at 1.26.x, brewing skipped, recipes with Molang/complex/deferred
ingredients dropped. Incomplete as for the spike: process isolation, signing,
consent, per-mod overlays, rebinding UI. See `docs/modding-spike.md`.

2026-10-01 crouch, shield and crossbow follow-up: the local camera now consumes
the native 0.35-block crouch offset, half-blended once per completed tick and
interpolated per frame. Local actor feet, interaction eye and network anchor are
kept distinct. Crossbow full-charge prediction persists per exact stack/slot
write revision; a fresh loaded click clears it immediately, and every addressed
authoritative write wins, even if byte-identical. Owner hand-item predicates now
expose both real hands to held attachable queries. The owner accepted crouch
camera, shield animation and crossbow charging. The canonical macOS/Metal Retina
build joined regenerated offline vanilla BDS; fresh frames show dual-hand
shields, shield inventory icons and loaded-arrow crossbow icons in the HUD and
reopened inventory. Survival/creative updates consume server confirmations and
change the UI without reconnecting. Provisional, labeled incomplete: crossbow
predicted NBT/inventory observer reconstruction, full load/fire/native-frame
comparisons, sleep/riding camera offsets, shield damage/cooldown/patterned/glint
parity and nonzero-tick game-mode historical replay. Native `query.blocking` reads metadata flag 72,
not the sneak key; the pinned Dragonfly fixture does not implement shield blocking.
No synthetic blocking flag or server-incompatible crossbow transaction is used
to mask fixture limitations. See [camera](docs/reference/crouch-camera.md) and
[crossbow](docs/reference/crossbow-use.md) rules. Local, uncommitted.

2026-10-01 destruction particles: ordinary block-break events now use the native
100-piece default, cube-root intensity and effect-local strict pre-spawn limits.
The native bottom-texture path selects untinted dirt for grass, replacing the
old green top-face heuristic. Focused particle tests pass; the owner manually
accepted block breaking. A controlled native grass/dirt frame comparison remains
pending. Provisional, labeled incomplete: destruction-component
overrides, special block/UV variant branches, seasonal colors, legacy Terrain
events and native crack bounds/cadence. No complete particle parity gate is closed;
see [vanilla rules](docs/reference/block-break-particles.md). Local, uncommitted.

2026-10-01 actor name tags: native-port correction in progress. World-plane projection,
eye-facing cubic-angle billboard, fixed font-pixel world size, ten-pixel line pitch,
independent line centering, full multiline 0.25-black plate, multiline world lift and
0.125-alpha depth-tested sneaking text follow vanilla behaviour. Player tags use synced name metadata, not just their spawn username; scores share
the same path and honor the native ten-block score gate. Provisional, labeled incomplete:
exact matched shader-pack/sampler/alpha-target state, explicit orientation/backface/custom color
branches, filtered names, mounted/riding anchor offsets and missing-data height/range
defaults need native witnesses. Crosshair picking still inherits the existing provisional
selection-shape/reach implementation. Name-tag rendering uses plate no-depth-write,
text depth-write, glyph alpha testing and the runtime environmental-text bias override. The final debug build has a live Zeqa/Metal rendering
smoke pass at Retina scale 2; controlled near/far drift, sneak/occlusion/clipping and
version-matched native frame comparisons remain pending. Workspace tests, formatting,
strict Clippy and the architecture gate pass locally. Changes are uncommitted. No name-tag
visual parity gate is closed; see [vanilla rules](docs/reference/nametag-rendering.md).

2026-10-01 first-person item rendering: ordinary icons and opaque full-cube blocks now use
their separate native camera-space stacks, not the third-person grip on the avatar's
`rightItem` bone. Camera anchor/yaw/scale, cube centering and idle/swing/equip composition
follow vanilla behaviour. The owner reports ordinary
icons render correctly. The block routing/avatar-scale regressions fail before the fix and
pass afterward; the owner has now manually tested and accepted the held-block pose.
Provisional, labeled incomplete: native sine-table rounding, non-cube block geometry,
custom block display transforms, exact held-block face lighting/material state,
item-specific legacy use/mirrored-art branches, custom render offsets and remaining
attachable variants. No complete first-person visual parity gate is closed; see
[vanilla rules](docs/reference/first-person-items.md).

2026-10-01 held bow: vanilla modern attachable routing is implemented locally.
Authored texture meshes, first-person scripts/controllers, owner animation variables,
per-frame pose evaluation and native bow/crossbow frame timing replace the accidental
third-person sprite grip. The same pipeline consumes trident/shield authored transforms;
these variants are not all live-accepted. Workspace tests, formatting, strict Clippy and
architecture checks pass; canonical carriers and debug executable are rebuilt. Live
macOS/Metal Retina-scale-2 frames verify actual server-supplied bow idle, partial/full
draw, release and return to standby in an isolated fixed-lighting loopback world.
This is functional rendering/input acceptance, not a matched native frame comparison.
The vanilla one-time session item-registry initialization also prevents a later
empty/custom table from erasing item identities. Provisional, labeled incomplete: multilayer
materials, custom binding parents, full offhand cached-stack equivalence,
nonuniform scale shear and the separate legacy/custom item paths. Changes are uncommitted;
no complete first-person parity gate is closed. See
[attachable rules](docs/reference/held-attachables.md).

2026-10-04 end crystals and respawn anchors: crystal artwork now bakes native
alpha-test coverage instead of dropping the whole rig over fractional texels;
the pack supplies nested frame rotation, bobbing and base visibility. Native
target-metadata beams use interpolated endpoints, taper, gradient and UV scroll.
Anchor glowstone use selects click-block interaction at every charge, without
adjacent-block prediction; charge, spawn and explosion effects remain authoritative.
Affected-crate compile checks, test suites and formatting pass locally, including
the application suite (2,252 passed, zero failures). No changes were pushed.
Provisional, labeled incomplete: material evidence is from adjacent installed
versions; no fresh target-platform rendered-frame comparison has been performed.
General actor material propagation and exact per-frame nonlinear Molang remain
open. Non-glowstone fallback at an already-selected Nether spawn needs retained
spawn-block authority. No complete visual or interaction parity gate is closed.
See [vanilla rules](docs/reference/end-crystal-respawn-anchor.md).

2026-10-01 arrow entity rendering: native per-face UV defaults now use face
dimensions, including fractional sizes, instead of a one-by-one texel region.
The vanilla arrow's alpha-test/no-cull material samples its authored plane
from both sides. Its animation receives absolute actor yaw, without the mob
body-turn root or head/body clamp. Focused render and query regressions pass;
changes are local and uncommitted, pending fresh flight/embedded-arrow frames.
Provisional, labeled incomplete: the current carrier lacks general authored
render-controller materials, so the no-cull correction is scoped to logically
bound vanilla-arrow geometry. Native signed Shake-event state, positive-only
tick countdown and Molang query are implemented; exact render-time frame-alpha
queries, including nonlinear impact shake, remain missing. No arrow visual
parity gate is closed;
see [vanilla rules](docs/reference/arrow-rendering.md).

2026-10-01 carried grass correction: the inventory thumbnail and held cube share the
pack's carried face pixels, with alpha-mask overlay tint and opaque output following the
matched C++ texture-atlas path. Grass is no longer unresolved. Compiler/carrier/runtime
regressions and the workspace, formatting, strict Clippy and architecture checks pass.
Live macOS/Metal frames at Retina scale 2 verify the hand, hotbar and open-inventory icon:
green top/fringe, opaque brown soil, no missing geometry or clipped slot art. Changes are
local and uncommitted. Provisional inventory shading and the broader material/display
parity gaps above remain incomplete; see [vanilla rules](docs/reference/carried-block-textures.md).

2026-09-30 menus: settings open the legacy JSON screen as retail does (the OreUI
"/settings" route sits behind the off-by-default `mc-new-settings-screen` flight);
unbound `$vars` in `ignored`/`requires` read as null, as in vanilla.
Provisional, labeled incomplete: the OreUI scroll thumb's look and shrinking a
long side-menu label to fit are approximations (no OreUI stylesheet on hand), and
unbacked settings originally showed fixed host values; their vanilla defaults were
not established by the pack binding names. See the settings audit below.

2026-10-01 settings audit — incomplete (the Settings completion gate remains open):

- The legacy desktop host includes migrated JSON tabs. Selector controls use the
  pack's 30px height, without its optional 25px spatial-pattern spacer. The current
  controller derives this flag from realm state and `mc-disable-settings-spatial-pattern-fix`;
  the retail flight assignment is unverified. Compact spacing is provisional.
- Persisted controller-name options now feed camera/input, ten mixer categories,
  GUI scale, fullscreen/FPS pacing, language and live chunk-radius requests. Keyboard
  capture covers the existing semantic gameplay actions; raw inventory/chat/drop
  keys and controller rebinding are incomplete. Duplicate-key rejection and key
  display strings are not established vanilla behavior.
- UI only, system missing: gamma, smooth lighting, leaves, fancy skies,
  particle toggles, most advanced graphics, paperdoll toggles, screen
  animation, auto-jump, spyglass dampening, controller cursor
  options, narration/subtitles, glint settings, Creator diagnostics and
  script options, tutorial/profile preferences and several chat presentation options.
  These values persist but do not close runtime parity gates.
- Global Resources provides empty pack collections as a clean integration hook;
  the pack-list controller belongs to the resource-pack work. Storage actions,
  world-edit/Experiments, Party, several account/help submenus, reset flows and
  hardware/flight-dependent controls remain incomplete.
- Numeric defaults/ranges are provisional unless a source explicitly states them.
  The pack confirms chat notification 10s and toast notification 3s defaults. Mouse sensitivity (default 0.5 over 0..1) and its look curve match the current client, as does field of view (default 60, range 30..110). Other vanilla option defaults remain unconfirmed. Gamma, controller/touch sensitivities, FPS limits and added boolean defaults therefore
  require further current-client evidence; they must not be described as vanilla.
- Offline carrier gallery, geometry and option-family tests provide local evidence,
  not a retail visual acceptance. Focus/hover/pressed, scrolling, all modal flows,
  runtime option effects and flight/platform visibility still need full acceptance.

2026-10-01 inventory reconciliation: the current native sparse-container audit
replaces delta replay over mutable backing truth with absolute request-owned cell
snapshots. Changed and emptied cells can be addressed by prior odd negative
request IDs; responses use requested-slot history and retire only their own
active predictions. This also repairs the offhand-empty push arriving before an
accepted offhand-to-cursor response: the accepted transfer is not applied twice
and cannot incorrectly put the cursor into global recovery. The previously
ignored normal InventoryTransaction receive path now writes full final stacks
(including new IDs and NBT) through ledger, HUD, crafting and identity consumers.
The canonical macOS/Metal Retina-2 offline vanilla-BDS run passed two dirt
drop/pickup/drop cycles, a whole-stack diamond drop/pickup and further move,
the six-step offhand regression, and three inventory close/reopen cycles.
Native references and bounded acceptance records:
[sparse prediction](docs/reference/inventory-sparse-prediction.md) and
[normal transactions](docs/reference/inventory-normal-transactions.md).
Provisional, labeled incomplete: arbitrary open-window normal transactions,
deferred UI output-50 actions, full native lifecycle recovery, and charged
offhand attachable context. Focused tests and the full workspace, formatting,
strict Clippy and architecture checks pass locally; this is part of the dev
integration recorded above.

2026-09-28 inventory and crafting: 2x2 and crafting-table crafting, creative take,
number-key swap and drops are implemented. The older delta/prediction-group
ledger model is superseded by the native sparse-container audit above.
Provisional, labeled incomplete: item tag membership comes
from Dragonfly's table and registry-declared tags are read from
`components.item_tags` without a live capture; the workbench layout, shift-click
destinations, drop bindings and CraftResultsDeprecated contents need
independent confirmation; armor/offhand placement is server-decided;
drag-distribute, double-click collect and workstation windows are missing.

2026-10-04 mob walking clocks: the compiler and runtime now preserve vanilla
`anim_time_update`, so quadruped/chicken leg phase follows modified distance moved
instead of elapsed seconds. Scripted clocks retain previous time, pause at zero
weight, reset with their controller state, and share one evaluation across model
layers. Zero-length procedural clips keep their unbounded clocks; finite timelines
use the verified native endpoint/loop/hold rules. The entity carrier version changes
to invalidate old compiled catalogs. Source and remaining limitations are recorded in
[actor animation clocks](docs/reference/actor-animation-clocks.md). The client and
carriers were rebuilt on published `dev` base `a2a8b6cf`; the user manually tested
the macOS Metal client against the existing offline BDS and accepted the mob feet
movement. The original worktree was based on stale `main`; it was updated before
this accepted test. Final automated verification remains incomplete: the user
requested skipping tests and pushing after manual acceptance. This does not close
the broader actor animation parity gate. Incomplete:
ordinary actor Molang still evaluates on fixed ticks and interpolates completed bone
poses, whereas vanilla samples interpolated motion queries during render evaluation;
start/loop delays and shared-clip instance identity remain unported.

2026-10-04 sneaking head rotation: the compiler now retains the vanilla bone
`relative_to.rotation` setting, and pose composition keeps the pivot parented while
using the entity frame for the bone's rotation and scale. The player head no longer
inherits the root's crouch tilt. Entity, artwork and equipment carriers were rebuilt;
the user manually tested the macOS Metal client in the existing offline BDS world
and accepted the fix. Compiler/carrier, transform and player-animation regressions
passed before integration; the affected core crates passed strict Clippy and the
architecture gate passed. App-inclusive Clippy encountered two pre-existing renderer
warnings. Latest `dev` was integrated, and further tests and the normal PR/CI gate
were skipped at the user's explicit request to push directly to `dev`.
[Rotation-frame behavior](docs/reference/actor-rotation-frames.md) records the fix.
The broader actor-animation parity gate remains incomplete.

2026-09-28 actor animation: remote players and mobs animate through the vanilla
controllers with full Molang evaluation; not visually accepted (facing, box-UV
side faces and limb swing need a native capture). Provisional, labeled
incomplete: motion-model constants, the 6-tick swing, the look clamp, gliding
divisor, seeded variables and Molang math tolerances need
independent measurement; `loop` is capped at 1024 (vanilla has no cap); undefined
variables read 0; non-uniform parent scale over rotated children is approximated
without shear; `->`/`for_each` take their empty path; blend
transitions and per-axis rotation objects are missing; queries without retained data read idle values; held items and most mob
artwork are deferred. A first-person held item with no drawable layer shows the
bare swinging arm instead (vanilla always draws the item).

2026-10-01 placement prediction correction: stateless full cubes no longer lose local
prediction when held and clicked block types match. Retained signed block-ID fields preserve
unsigned wire hash bits for both prediction and collision checks; empty/air/uninitialized IDs
remain rejected. The matching native placement and correction paths ground this change; see
[vanilla rules](docs/reference/block-placement-prediction.md). Regressions reproduce both old
blockers, and the local lighting/meshing test publishes an urgent placement mesh without a
server acceptance event, then removes it on authoritative correction. Focused tests,
workspace all-target tests, formatting, strict Clippy and architecture checks pass locally.
The canonical debug build is rebuilt; changes are uncommitted. An 800-ms delayed loopback
session now has a controlled grass-on-grass witness: the predicted block is visible in a
PNG written 226 ms after right-click and persists in the post-reply frame. The selected
grass item's hand, hotbar and open-inventory visuals also render. This is live functional
acceptance of the reported bugs, not complete native placement parity.

2026-10-03 instant block destroys: the pick registry now separates movement
collision from vanilla visual bounds for grass, flowers, saplings, bushes,
mushrooms, reeds, crops, nether sprouts and torches. A first-hit zero-hardness completion
retains its block id for local break particles and sound; incoming effect echoes
share the existing bounded echo ledger. The negotiated block-action/item-use
routing and authoritative server correction remain in place. Focused offline
regressions cover both runtime-id spaces, both breaking authorities, prediction,
and completion effects. General pick-shape coverage (including vines and corals), cutout highlight masks and live visual parity remain incomplete; no
visual or live-server parity gate is closed here.

2026-10-02 block selection: gameplay picks now publish the native black wire box
when Outline Selection is enabled and a brightened model overlay when it is off.
This omission also exists at 199e0856; it is not a regression in the first-parent
history since that commit. Offline GPU regressions cover both depth-tested passes
and clearing a lost target. Native visual parity remains incomplete: the existing
pick-shape coverage is incomplete for non-colliding blocks, and cutout texture masks are not
yet carried into the highlight overlay. No live visual gate is closed here.

2026-09-30 block interaction: breaks (every game mode, both block-breaking
authorities; Creative repeats while held), stateless full-cube placements and
trapdoor/lever/button uses are predicted locally and replaced by the server's
block updates; Build, Mine and DoorsAndSwitches/OpenContainers gate separately.
Not live-accepted. Provisional, labeled incomplete: oriented, sized and merging
placements and door/fence-gate uses are not predicted; the pick ray uses
collision boxes outside the vanilla plant and torch selection bindings;
other non-colliding pick shapes remain incomplete; Adventure CanDestroy/CanPlaceOn are not modelled; MineBlock requests
cover recognized tools only; the standalone CreativeDestroyBlock and
DenyDestroyBlock PlayerActionPackets the reference appears to send are not sent
until a capture confirms their routing.

2026-09-28 survival interaction: hold-to-mine (both block-breaking authority
modes), MineBlock wear with reconciled responses, standalone ClickBlock
placement, and melee with swings and missed-swing reporting are implemented but
not live-accepted. Provisional, labeled incomplete: tool/harvest classes are
Java-derived (PrismarineJS) and may predict early on Bedrock-specific tool rules;
unresolved rows use the slowest rate; hardness is 1.26.30 data; the completion
threshold, pick ranges, server pick slack, entity pick radius, swing
adjustments, placement repeat timings, attack-to-use block and bridging rule
need independent measurement; replaceable, interactive and unpickable-entity
lists are local choices. A vanilla packet capture must still confirm the attack
swing count.

2026-10-05 dimension transfer: one coordinator waits for the server acknowledgement
and destination's loaded area before sending the local acknowledgement. Stationary
input ticks continue while prediction is held. Loading-screen IDs survive retries;
LoadingEnd waits for the JSON-UI loading presentation and a fresh destination frame.
Session height definitions also govern destination probes and synced-block admission.
Provisional, labeled incomplete: the local readiness delay uses a later app frame;
the vanilla readiness updater's exact scheduling clock remains unverified.
The user reports the Hive test account is unbanned; renewed live acceptance after
integrating current dev is in progress. Loading UI and Hive gameplay parity gates
remain open until their corresponding acceptance checks complete.
The renewed join exposed omitted legacy block texture bindings and three actor
catalog capacity failures. Their captured-stack and lower-owner regressions pass:
all artwork bindings publish, and exact shared geometry fits the existing vertex
budget. A live Metal pass restored the lobby floors and custom NPCs; title panels,
holograms, plant lighting and lamps still failed visual inspection. Complete model
chains, authored light filters, material states, selector capacity and active-registry
hay admission pass their focused regressions, with a fresh client pass pending.
General custom AO exponents, exact blend defaults, custom shader defines and
cross-family transparent ordering remain incomplete. The latest live session ended
with an opaque server disconnect after roughly 23 minutes; disconnect acceptance
remains open. The inspection build received another opaque server kick after
roughly nine minutes with zero decode errors. Targeted-entity F3 diagnostics
pass their regressions. The user
identified the sheep as zero-scale floating-text hosts. Scale zero now suppresses
their bodies while retaining names. Ordinary SDR title panels blend encoded
destination colors; a fresh Metal frame shows dark panels and no floating sheep.
Enhanced/HDR/MSAA actor blending and exact cross-family order remain incomplete.
Standing and hanging lantern body, cap and handle measurements use a near-version
native witness. Their geometry, UVs and lighting remain fallback support pending
exact current-version verification. NPC controller particle routing and alpha-first
hex tint decoding pass owner and app route regressions; user frames show NPC
particles. Authored emitter lifetimes now survive ten minutes and remain bounded
by actor/state teardown and admission limits. Unsupported locators and initialization
scripts remain incomplete. Actor raster pipelines prewarm during loading; the
loading gate holds until their compilation completes. Focused regressions pass;
this inspection build does not establish frame-budget or session acceptance.
See `docs/evidence/hive-connection.md` for the functional checks.

2026-10-05 custom actor interaction: omitted collision dimensions independently
retain generic actor defaults, and server scale multiplies the physical box once.
Regressions reproduce a custom NPC miss and verify its attack target and world-space
hit point. F3 shows effective hitbox dimensions; attack logs distinguish the picked
target, packet kinds and transport admission. The user's live Hive check reached
game selection and a destination lobby; logs picked selectors with server scale
applied. Post-fix actor tests remain deferred at the user's request.
Provisional, incomplete: zero scale currently produces a point box before pick-radius
inflation; the native positive minimum dimension remains unverified. Definition-specific
collision defaults, authored picking-box collections and invalid-dimension behavior
remain incomplete.

2026-10-09 server hitboxes: the `HITBOX` actor data compound (id 118) appends
custom interaction boxes. Each `Hitboxes` entry uses normalized Min/Max extents
centred on its pivot relative to the native actor position. Render yaw and scale
do not affect these boxes. Empty updates preserve previous boxes; unreadable and
non-finite data is skipped and counted. Missing or differently typed float fields
use zero. Crosshair, use and melee picks share the predicted geometry and use the
first intersection in server order within each actor. Aim assist considers each
box separately, retaining all boxes of an actor admitted before its candidate
threshold. Collision, placement obstruction and F3 retain collision geometry.
Definition-authored boxes and multipart actor picking remain incomplete.

2026-10-10 actor use: a use press on an actor always sends the Interact
transaction, then falls through to item use (a throw, for example) unless the
target offers an interaction. Players offer one only while the server's interact
text (actor data 100) on the local player is non-empty. Provisional, incomplete:
other actors' component interactions (trading, taming, leashing, riding and so on)
are not modelled, so a use on any non-player actor still consumes the press; the
local can-ride-target flag and the target's prevent-default flag, which suppress
the fall-through, are not modelled. Not live-accepted.

2026-10-05 server HUD composition: partial server edits overlay the built-in HUD
without withdrawing its whole namespace. Regressions reproduce top-left chat and
item-name overlap while preserving explicit server anchor overrides. Live Hive
inspection confirmed bottom chat, but found suggestions near the screen top;
the autocomplete grid now derives its height from its collection so short lists
stay next to the editor. Transfers have a separate loading presentation that
suppresses the join animation while retaining destination text and backdrop.
Provisional, incomplete: the ordinary no-bar policy is supported by current
handler initialization and a near-version native dispatch witness; exact current
dispatch and a fresh live check remain outstanding. New chat/loading regressions
are authored but unrun at the user's request. Native
`#item_name_text_offset` controller binding remains
incomplete; this correction preserves the approved built-in pack geometry.

2026-10-05 custom actor lens materials: the Murder Mystery lens overrides its body's
wildcard material with `slime_outer`. The compiler previously ignored bone-specific
rules, drawing both coincident opposing lens faces with the body material. Per-bone
last-match routing now retains authored visibility and alternate/inherited geometry;
the glass uses blending, culling and depth writes. The patch is uncommitted and its
three regressions remain unrun at the user's request. No extra depth bias is warranted.
Incomplete: broader native
pattern semantics, dynamic material arrays, custom shader defines, and the strict
default actor depth comparison remain unverified or unsupported. A rebuilt Metal
inspection still flickered in the lens and game titles. Frame camera-position sampling
now refreshes the body and controller-selected geometry poses; two regressions are
authored but unrun at the user's request. Those changes alone did not resolve flicker.
Actor queueing also used the preceding frame's span indices before preparation
replaced the spans and GPU buffers. Preparation now precedes queueing, while binding
creation remains after view uniforms. Two behavioral regressions are authored but
unrun at the user's request. The user confirmed that movement flicker is resolved
in the rebuilt Metal client on Hive. Latest dev through
`41c72fb3d` is merged locally; inspection fixes are uncommitted and nothing was pushed.

2026-10-05 custom actor panel UVs: finite normalized cube UV corners now clamp to
the image bounds before interpolation, matching vanilla. The Bridge title panel's
overflowing face UVs previously stretched transparent corner pixels across its
right side. Three focused regressions cover full-width coverage, signed flips and
box UVs; they remain unrun at the user's request. The Rust client rebuilt successfully.
The user confirmed the correction works after relaunch. It remains uncommitted.

2026-10-05 actor name-tag bold and backgrounds: server bold markers were retained
but atlas rasterization ignored them, while shared layout omitted bold advances.
The open-font route now shares a one-design-pixel bold offset across layout,
UI drawing and atlas rasterization, including spaces and shifted ink bounds.
Ordinary SDR name tags join encoded-color transparency without changing the
0.25-black plate opacity or depth order. Provisional, labeled incomplete: exact
current final text attachment dispatch, native Unicode/forced-Unicode adaptation,
HDR and MSAA. Near-version shaders corroborate encoded text output. Focused
regressions are authored but unrun at the user's request. Local and uncommitted;
the fresh rendered comparison remains pending, with no parity gate closed.
The Rust build succeeded and the fresh Metal client is running on Hive. A visible
frame confirms world/actor/name-tag rendering without logged shader validation
errors; the upgrade NPC's native comparison remains pending with the user.

The subsequent user check exposed split bold strokes. Name-tag CPU sampling
treated exclusive glyph rectangle ends as inclusive, pulling transparent padding
into the glyph. Sampling now matches the compiled and runtime-sheet UV contract;
empty source rectangles draw no ink. A private diagnostic from the loaded font
reproduces the split title and confirms continuous stems after correction.
Regression cases are authored but unrun at the user's request. Fresh rendered
acceptance remains pending; the compiled-font adaptation of native Unicode and
TrueType half-offset drawing is still explicitly incomplete. Local, uncommitted.
The corrected Rust build passed and is running on Hive. A fresh Metal frame
renders the current game lobby; the user's close title comparison remains pending.

2026-10-05 server sidebar placement: a full-screen server scoreboard replacement
retained the built-in HUD's asymmetric outer anchors and moved Hive's level row
down by half the viewport. Symmetric vanilla outer anchors now preserve the
server's top-right placement; adjusted inner positioning retains the compact
built-in sidebar. Chat styling and server precedence remain unchanged. Regression
cases are authored but unrun at the user's request. Local, uncommitted; the fresh
game-lobby comparison remains pending.

2026-09-27 chunk decode parity: chunk payload contents now follow the 26.30 client's
lenient stream decode (palette clamp and index zeroing, zero-fill past the end,
null biome slots, per-entity tail skips, unknown ids to air/default biome, inline
slots `i & 0xff`, unsent inline slots known air). Provisional, labeled incomplete:
legacy sub-chunk versions 0/2–7 decode as air (no legacy id table), persistent
palettes now resolve exact current name/state identities; legacy state upgrades,
default-state reconciliation and unknown-property handling remain incomplete. Block-entity id and
block-actor-type checks are not emulated. StartGame custom blocks are known only
when every custom name sorts after vanilla in sequential mode (Lifeboat's case);
they collide as full cubes with stone's surface facts and render as diagnostic
cubes until runtime pack application lands. Hashed-id sessions now register custom
states by network hash (pack visuals via the runtime overlay; stone-surface collision
provisional); sequential customs sorting among vanilla remap wire ids, custom collision
uses `minecraft:collision_box`, and vanilla blocks retexture from a pack's terrain keys
via the `.matkeys.json` sidecar (rebuild assets to emit it). Vanilla item icons override by
short-name key (provisional). Custom-block selection boxes drive the pick ray. Server-pack entities compile in memory per session (`compile_actor_pack`) into their own
index space (pack rig ids from `PACK_RIG_ID_BASE`) and layer over the vanilla catalog: pack
entities win by identifier, render scene geometry/artwork are rebuilt per session. Provisional,
labeled incomplete: neutral material profile only (custom materials fall back; a controller's
several textures draw as stacked layers of its own geometry; `uv_anim` is evaluated per tick,
not per frame, and any animated layer samples with repeat wrap); actors are lit by the solved
light at their feet through the shared light curve with daylight fixed at 1 (the vanilla light
texture, tint and sample point are unmatched), pack attachables (held/worn on player bodies) layer over the equipment runtime per session
(pack bindings win by item identifier; pack property defaults seed only from `entities/` in
resource packs), rigs depending on vanilla clips are attributed as fallbacks; pack precedence follows the Bedrock stack (last entry wins). No vanilla acceptance gate is closed by this change.

2026-09-30 protocol-2193 wire target (owner decision): Cinnabar moves from
Bedrock 1.26.44 / protocol 2168 to 1.26.50 / protocol 2193, because Gophertunnel
`lunar` now supports only 1.26.50 (Mojang's current release is 1.26.52 on the
same protocol). The Go core, fixture generator and local server pin Gophertunnel
`b725d82563e93308fd1f92d27da5e97301ad5040` on `resource-pack-changes` and accept
only `minecraft.DefaultProtocol`; the client reports game version 1.26.50. The
vendored Valentine crate is regenerated from protocolgen `0b8f17e3`'s reconciled
1.26.51 manifest (`valentine_bedrock_1_26_51`, see `crates/protocol/vendor/UPSTREAM.md`)
and the fixtures by `tools/fixturegen`. Local worlds provision BDS 1.26.52.x.
Codec, fixture and unit coverage only: no 1.26.50 server join, native visual, or
vanilla acceptance gate has been run on this target yet. New 2193 fields are
decoded but not yet consumed (MoveActorDelta interpolation ticks, PlaySound
range bypass and playback offset, camera preset starting rotation, dimension
default biome, SetPlayerFurnaceOptions, RecordStarted); that parity work is
incomplete. The account/auth cache is keyed on the game version, so the first
join re-authenticates.

2026-09-09 loading publication: the owner authorized publishing the completed
loading/auth work; the broader track and unused solver experiments remain paused.
The reviewed ordered-batch dependency is published on `resource-pack-changes` at
`3d9f4b7a4ac0f19f9565cca98b7f17fe918acd38`, and its Go CI passed. Both current Go
consumers and the acceptance scripts now pin its public pseudo-version
`v1.25.3-0.20260908230935-3d9f4b7a4ac0`; the former local override is no longer
needed. This is the same dependency source used by the recorded native A/B runs,
not a wire-version upgrade. Exact-version provenance checks remain enforced.
Fresh full core, fixture-generator and registry-generator tests/vet and the core
build passed against the public pin. The fork's full Windows test run reproduces
only the two previously established zero-duration timer failures; the remaining
tests, vet and staticcheck pass. Race instrumentation remains unavailable locally.
The complete outgoing loading range and pin changes cleared publication review.
Fresh Rust workspace tests passed 3,591 tests with 16 ignored; strict all-target
workspace Clippy, formatting and architecture checks passed. PowerShell 5.1 and
Git Bash acceptance harnesses and the asset contract suite passed; the latter's
deep Bash extraction legs remain skipped locally without unzip/cc. Cinnabar CI
remains pending until the authorized push; the earlier native A/B evidence is
unchanged, and no faster-than-vanilla acceptance gate is closed.

2026-09-08 dependency refresh: the Go core and fixture generator pin Gophertunnel
`649c0edad68caf669e89215106403369deed5e03` on `resource-pack-changes`, containing
latest lunar `80a44ec6a6b974d63cbfdd6b1fb4e273d534e997` and the restored resource-pack
snapshot APIs. The core pins go-raknet lunar `216ccc2404e808b0b76e622b7eb13e990a0a8054`.
The local listener and fixture generator explicitly select the 1.26.44 adapter;
this does not upgrade Cinnabar's wire target to the dependency's 1.26.45 default.
The inventory-response decoder and fixture now include the filtered-name presence
byte. Structure-editor strings retain their existing encoding. Fork restoration
review approved; its tests, vet, and published CI passed. Cinnabar's Go tests,
vet and build, Rust workspace tests, clippy, formatting, release build, architecture
check, and shell acceptance tests passed. Independent review found that the
listener also accepted the dependency's newer default; a pre-preparation check
now rejects unsupported local protocol IDs and advertised game versions,
including 1.26.40 sharing ID 2168. Both hermetic regressions failed before their
fixes and passed afterward, including repeated race-enabled runs. Independent
review approved the complete `8e2b19ba..0ab3ff1a` range with no remaining findings.
The final Go tests, vet, build and architecture recheck passed after both fixes.
The acceptance shell suite passed with the canonical `Downloads` path casing;
PowerShell and Windows-native checks were not run locally. Cinnabar CI will run
on the authorized push; its result is not part of this local verification record.
No live vanilla or native acceptance gate is closed.

2026-09-08 loading-only local checkpoint: upstream was pulled through `3438c89d`;
the broader completion track remains paused. Independently approved lighting
tranches `ef5d0ea6`, `f99eb3ab`/`d7216891`, and `b0635a7a` bound waiter cleanup to
the six possible face sources, cache retained light reads within one bounded solve,
and canonicalize packed output once per channel instead of after every voxel write.
The fixed retained-light witness covers exact channels, provenance, storage and
queue statistics against the pre-change result. Fresh world/client-world/meshing
tests, strict all-target Clippy, formatting, architecture checks, and the release
client build passed. These local changes do not close lighting parity or performance.

The complete persistent-auth range `3438c89d..e96589d1` received independent
APPROVE with no findings and is integrated history-preservingly in `9179f145`.
The optional owner-restricted, bounded disk sidecar preserves expiry-checked
device/proof-key, Xbox and service credentials across processes. OAuth material,
client configuration and freshly discovered service environment bind reuse;
each connection still receives a newly minted credential. Invalid/unsafe caches
are optional misses, and derived writes require a verified exclusive lease.
Fresh full Go tests, vet and the production core build passed. Darwin authcache
tests compile but were not executed on macOS; race instrumentation remains
unavailable locally without a C compiler. Discovery/JWKS caches, NetherNet's
separate service path and cross-process OAuth refresh coordination are unchanged.

Windows/DX12 live Zeqa cold and fresh-process warm launches reached the lobby.
The warm process reported a bound disk-cache hit and service reuse without
rewriting the sidecar. Connection time was 5.625 s cold versus 3.635 s warm in
that pair, including the server's pre-login transfer. Expiry and rejection handling
have synthetic regression coverage, not a forced real-credential expiry test.
With the same `b0635a7a` release renderer, full terrain work drained in 33.933 s
on the warm run but took 144.006 s on an earlier run; both ended at 224 columns
and 5,376 subchunks. These are one-second observer measurements after connection,
not loading-screen durations: the lobby can render before all work drains.
An earlier waiter-only Lifeboat run took approximately 81 s to drain, also with
the lobby visible sooner. Normal uncapped runs are not the capped resource-budget
gate, and no matched vanilla speed comparison is established. The complete
`b0635a7a..e4043212` waiter-coalescing range received independent APPROVE with no
findings and is integrated in `86ac5fc2`. Pending wakeups retain their revision
and queue age, restore consumed scheduling candidates, and preserve urgency;
in-flight-only targets still receive invalidating revisions. Both earlier review
findings have dispatch-driven regressions. Fresh world/client-world/meshing tests,
strict all-target Clippy, formatting and architecture checks passed. The analytical
3-by-3 emitter witness verifies every light channel and final current/empty state,
but controlled accepted work remains 21 jobs: this is not the full convergence fix.
The reviewed release build passed and two fresh Zeqa runs drained terrain work in
32.20 s and 33.01 s after connection, with the same 224-column cohort and actual
lobby rendering verified. The larger 257-column Lifeboat run still had pending
lighting after 180.30 s; it eventually drained after 194,505 accepted light jobs.
That is a failed loading-performance result, not an improvement claim. All four
fresh native launches reused the saved auth bundle and valid service credentials
without refresh. No compiler or test ran during these native measurements.
A bounded regional-lighting experiment remains isolated on
`fix/bounded-region-lighting-20260908` from `e4043212`, not integrated. Controlled
initial-load comparisons did not justify its complexity, so batching is excluded
from the shipping candidate. The smaller current-air boundary check received fresh
independent APPROVE for the complete `c51ba65a..04350544` range, with no findings,
and is integrated history-preservingly in `3a77c1fd`. It suppresses a neighbor's
light requeue only when every new contribution is already covered by current light
and the source face has no level or direct-sky provenance loss. Unknown, stale,
dirty, in-flight and non-air targets retain existing behavior; mesh invalidation
is independent. Fresh post-integration world/client-world/meshing/render suites
passed 999 tests with two ignored; all 950 app-library tests, strict affected
all-target Clippy, formatting, architecture and the release build passed. The
controlled mixed-terrain fixture preserved exact light/provenance hashes through
initial load, emitter removal and addition; initial median was 225 ms versus an
earlier 512 ms baseline. This synthetic result is not a native loading improvement.

Independently approved `7b089c7e` is integrated in `c51ba65a`. It adds a bounded
once-per-session logical terrain-ready timestamp without changing readiness
thresholds or presentation behavior. Fresh post-integration verification passed
all 950 app-library tests, strict all-target Clippy, formatting, architecture and
the release build. The first Windows/DX12 Zeqa run with this diagnostic reported
23.862 s from the first connected-session observation to logical terrain readiness.
The full terrain pipeline drained approximately 25.13 s after upstream connection,
with 32,988 accepted light jobs and the same 224-column, 5,376-subchunk cohort.
The actual lobby rendered, saved authentication was reused without refresh or
sidecar rewrite, and no compiler or test ran during measurement. This is a
pre-boundary-check baseline, not an optimization result or exact first-pixel time.
Two normal Windows/DX12 Zeqa runs of the integrated boundary check reached logical
terrain readiness in 31.371 s and 28.701 s; full work drained approximately
35.95 s and 29.74 s after connection, with 43,993 and 38,745 accepted light jobs.
Both retained 5,376 subchunks and rendered 1,108 at the same spawn/settings. These
results do not establish improvement over the diagnostic baseline. The large
Lifeboat baseline retained 3,035 and rendered 1,863 subchunks; logical readiness
took 32.484 s, with work still pending after 195.62 s. Both candidate Lifeboat
scenes retained only 298 / rendered 226 subchunks, so their 5-6 s logical
readiness cannot be compared with that large workload. The bridge received 258
ordinary chunk packets in both sizes. Follow-up diagnostic runs showed publisher
radii of eight chunks for the large scene and two for the small scene, despite a
confirmed chunk radius of eight in both. Admission/order attribution remains open;
the smaller scene is not evidence that the server sent less terrain. Native performance remains
open; no compiler or test ran during these measurements.

Saved auth was reused throughout. During the repeated Zeqa run, the service token
naturally expired: the bound disk bundle was accepted, the service credential was
refreshed and persisted, and connection succeeded in 3.832 s. The replacement
sidecar retained its protected owner-restricted access. Credential contents were
not read for verification. A new isolated investigation on
`fix/light-trust-convergence-20260908`, based on `3a77c1fd`, reproduced
dirty-neighbor darken/restore amplification. A source-first scheduling experiment
helped a flat emitter fixture but increased accepted work on the 9-by-9-by-24
mixed-height fixture; all final light/provenance hashes matched. That scheduling
change is rejected, and the uncommitted diagnostic tests remain isolated.
No new production behavior from that investigation is integrated.
A separate isolated dependency lane, `fix/loading-batch-order-20260908` from
`649c0eda`, reproduced a concurrent FIFO violation between the deferred and ready
batch queues. The complete `649c0eda..f2586456` fix received fresh independent
APPROVE with no findings and is integrated locally on `resource-pack-changes` in
`3d9f4b7a`. The mutex-protected ring preserves batch order, login deferral and
close/deadline draining; normal full-backlog operations neither relocate entries
nor allocate, consumed references clear, and drained burst storage returns to eight
slots. The earlier review's compaction and retained-reference findings have
deterministic RED/GREEN regressions. Fresh integrated Go tests and vet passed,
excluding only two unchanged packet-timer assertions that also fail on the untouched
base on Windows. Race instrumentation remains unavailable without a C compiler.
Full Cinnabar core tests, vet and build passed with an ignored local module override;
the public dependency pin is unchanged, and the fork is not published.

With the same release renderer and this core, a normal Lifeboat run loaded all
257 columns / 3,035 subchunks and rendered 1,863. Logical terrain readiness was
29.063 s; work drained approximately 85.42 s after connection with 90,717 accepted
light jobs. This is one combined-change run, not isolated causal attribution or
vanilla-speed acceptance. A fresh repeat still loaded only 25 columns / 298
subchunks despite 258 ordinary chunk packets at the bridge, so the fixed FIFO race
does not resolve the publisher-area discrepancy. Zeqa retained its usual 224
columns / 5,376 subchunks; logical readiness was 39.879 s and work drained in
approximately 40.71 s with 37,323 light jobs. No compiler or test ran during these
measurements, but substantial unrelated/background process CPU was observed after
the Zeqa run; its timing is not a clean causal comparison. All three launches
reused the saved auth bundle and service token without rewriting the sidecar.
The complete publisher-diagnostic range `10f14313..fddfda1e` received fresh
independent APPROVE with no findings and is integrated in `b07482e7`. It records
the first 16 numeric publisher updates independently at the upstream callback and
relay, including preceding chunk counts, without changing publisher behavior.
Full writer Go tests, independent focused/proxy tests and vet, fresh integrated
proxy/command consumer tests with the local fork, vet, architecture and core build
passed. Race instrumentation remains unavailable on this host.

Two short Windows/DX12 attribution runs with unchanged release renderer reproduced
both sizes. The full run received one 128-block publisher update and retained
3,035 subchunks. The small run received a 128-block update followed by a 32-block
update before the remaining terrain; the callback and relay recorded identical
publisher order, centers and radii. Both runs received 258 ordinary chunk packets;
all publisher saved-list counts were zero. The small run retained only 298
subchunks / 25 columns and visibly lacked most of the lobby. Thus this pair
attributes the small publisher update to the upstream stream, not proxy reordering
or a nonempty saved-list omission. Whether the client incorrectly rejects new
chunks outside that publisher area remains under investigation; no admission
change follows from this observation alone. These instrumented, early-closed runs
are not performance comparisons. Both reused saved authentication without refresh.
Lighting work remains paused until this admission contract is resolved.
All changes in this loading-only checkpoint are local, not pushed.

The subsequent admission investigation identified 232 valid inline chunks and 18
block updates discarded solely by the small publisher envelope in that run.
The complete `2eb94414..52bf5425` stream range and `2eb94414..db609e6e` render/app
companion received fresh independent APPROVE, with no Critical or Important
findings, and are integrated history-preservingly in `7ad83fd5` / `0e0471c4`.
Data admission now includes the independently confirmed player grid while raw
publisher center, radius, epoch, and teleport/control decisions remain unchanged.
Required membership and render expectations follow admitted announcements, and
normal retention changes prune departed requirements without dropping incomplete
nearby requests. The initial review's stale/unbounded membership finding has
long-travel and radius-shrink regressions. Explicit render membership preserves
foreign/source/stale blockers and invalidates candidates when membership changes.
Fresh integrated world/client-world/meshing/render suites passed 1,012 tests with
two ignored; all 951 app-library tests, strict affected all-target Clippy,
formatting, and architecture checks passed. The sole final review comment was
corrected without changing behavior. The release rebuild passed; normal live
full-lobby checks remain pending, so this does not yet establish a loading-time improvement.
The existing loading-screen threshold is unchanged. New lighting optimizations
remain isolated until the admission candidate has a comparable native baseline.
These changes are local and have not been pushed.

The admission candidate retained all 257 Lifeboat columns but initially still
hid the lobby in fog. The independently approved `e8329093..f05bd346` correction,
integrated in `196f3077`, derives fog distance from the confirmed chunk radius;
raw publisher controls remain unchanged. Fresh app/client-world tests, strict
Clippy, formatting, architecture and release build passed. Normal Windows/DX12
native runs then displayed the complete Lifeboat and Zeqa lobbies. Lifeboat sent
the same 128-to-32-block publisher update and still retained 3,035 subchunks /
rendered 1,863; Zeqa retained 5,376 / rendered 1,108. Logical terrain readiness
took 26.549 s and 35.778 s respectively; all terrain work drained approximately
73.63 s and 52.45 s after connection. These are comparable-scene baselines,
not a loading-performance or complete fog-shape parity claim. Zeqa naturally
refreshed its expiring service credential and persisted the replacement with
protected owner-restricted access; subsequent reuse succeeded.

The complete `2eb94414..a7006f96` lighting boundary-scan range received fresh
independent APPROVE with no findings and is integrated in `5831f6e4`. It avoids
sampling strict interior voxels during external boundary seeding while preserving
boundary traversal, queue order, light values, direct-sky provenance and error
behavior. Nonvacuous full-volume oracle tests and exact full-solve fingerprints
passed. A quiet release seed-phase microbenchmark measured 2.21x and 2.83x on
the two fixed geometries; this does not establish an end-to-end speedup. Fresh
post-integration world/client-world/meshing/render tests passed 1,019 tests with
four ignored, and all 951 app-library tests passed. Strict affected all-target
Clippy, formatting, architecture and the release build passed. Normal native
boundary-only runs retained and displayed both complete lobbies. Lifeboat logical
readiness / full drain took 31.286 / 85.72 s, worse than its fog-only baseline;
Zeqa took 24.702 / 26.36 s, better than its baseline. Accepted light work varied
substantially, so these mixed results establish no reliable universal speedup.
Both fresh processes reused disk authentication without refreshing or rewriting
the valid bundle. No compiler or test ran during native measurement.

The complete current-resident light-dominance range `e8329093..6ef82c72`
received fresh independent APPROVE with no findings and is integrated in
`f4892583`. It extends the existing conservative face proof to current known
resident blocks, with no palette lookups or weaker stale/dirty/in-flight guards.
Exact mixed-terrain light/provenance hashes remain unchanged through initial load,
emitter removal and addition; timing under build contention is excluded. Fresh
integrated app and client-world library tests passed 951 and 347 tests respectively,
with two ignored; all 34 client-world integration tests, strict affected all-target
Clippy, formatting and architecture checks passed. Release/native/performance
gates were initially open for this extension; the combined candidate below now
has repeat native evidence.

The complete lazy destination-filter refinement `f4892583..acdcbefc` received
fresh independent APPROVE with no findings and is integrated in `735a422a`.
Only cells that fail the cheaper bound consult the exact destination layer/filter
semantics, with at most one destination lookup per proof. All current-state,
monotonic-source and direct-sky provenance guards remain; mesh invalidation is
independent. The old shared lighting fixture is unchanged. Exact 3-by-3-by-24
light/provenance hashes remain `8406a83a02b26fbd` initially,
`fdb331a60caa17e5` after emitter removal and `8406a83a02b26fbd` after addition.
Fresh integrated app/client-world units passed 951/352 tests (two ignored);
world/client-world/meshing/render consumers passed 1,026 tests (four ignored).
Strict affected all-target Clippy, formatting, architecture, diff checks and
the release build passed.

Normal Windows/DX12/Immediate release A/B runs, with no concurrent compiler or
test, retained and visibly rendered the complete lobbies at the same spawn and
settings. Full terrain-work drain was measured by a one-second observer after
upstream connection, not by the loading-screen timestamp:

| Server | Boundary-only baseline runs | Resident + filter runs | Retained / rendered subchunks |
| --- | --- | --- | --- |
| Lifeboat | 85.72 s, 61.51 s | 10.98 s, 11.45 s | 3,035 / 1,863 |
| Zeqa | 26.36 s, 30.07 s | 5.80 s, 5.97 s | 5,376 / 1,108 |

Lifeboat accepted 86,706-113,456 light jobs in those baselines versus
12,950-14,134 in the candidate; Zeqa accepted 35,107-40,482 versus 8,120-8,293.
The fresh-baseline comparison is roughly 81% less terrain-drain time for both
servers. Arrival order, live server activity and publisher updates still vary;
this supports the combined optimization, not isolated attribution to either
refinement. The Lifeboat candidate's first logical milestone used an early
transient drained state and is not a full-lobby time; its repeat used the ordinary
dense-opaque milestone at 6.075 s. Zeqa logical milestones were 5.020/5.511 s.
Readiness thresholds are unchanged. Existing pack-dependent rendering and glyph
defects remain, and there is no matched vanilla-speed or capped resource-budget
acceptance claim.

Every fresh process in these A/B runs reused the bound disk authentication and
valid service token; the sidecar remained unchanged after the earlier natural
expiry refresh. Candidate connection times were 1.693/1.187 s for Lifeboat and
3.678/3.477 s for Zeqa, including its pre-login transfer. A separate resource-pack
cache warning was traced to a parent directory whose inherited permissions fail
the existing owner-only check. Authentication caching is unaffected; permissions
and pack acquisition/application policy were not changed. This is not evidence
of persistent pack reuse. The long-lived Zeqa baseline later exited on the
pre-existing unconsumed block-crack queue limit, after its loading measurement;
that separate gameplay defect is recorded, not fixed by this loading tranche.

The more complex initial-floor runtime experiment is deferred: the conservative
candidate already produces a repeatable large improvement. Its opt-in pure solver
remains isolated on `fix/loading-increase-only-solver-20260908`, based on
`049f2889`, with no runtime caller or integration. Review of `95c5a1e4` requested
one defensive filtered-halo correction, now committed separately in `c7dae5ab`.
The regression failed before the fix and passed afterward; all nine focused
debug/release tests, 136 world tests (two ignored), strict Clippy, formatting,
architecture and the ordinary exact release fingerprints pass. A fresh
independent re-review of the complete `049f2889..c7dae5ab` range returned APPROVE
with no findings and independently repeated those checks. It is parked, clean
and locally committed, without runtime integration or native acceptance.
Do not merge or enable it from this checkpoint. The broader track remains paused.
Upstream was re-fetched and remains `3438c89d`; all loading changes are local,
not pushed. The tested core still uses the reviewed local Gophertunnel override;
publication and an exact reachable dependency repin remain separate gates.

2026-09-06 local integration: `9bbeeca762fe3310d87dc6d297babb4e7440dfae`
adds bounded block actions and embedded creative-break encoding to PlayerAuthInput,
plus the Windows physics-install shell-test correction. Independent reviews approved
the complete tranche without blocking findings. App-side mining production, live
server acceptance, break timing and crack rendering remain open; no interaction
or parity checkbox is closed. See the execution track for verification status.
Follow-up repairs normalize failed upstream connection cleanup (including a
fresh native rejection test) and cover both Windows attribute line endings in
the carrier-upgrade test. The new stationary lighting/startup stall remains open.
Later independently reviewed repairs through `e175556d` fix mixed vertical light
propagation and upstream disconnect delivery. A native BDS 1.26.40.8 run drained
the lighting/mesh queues for 314 columns, but does not close the intermittent
Lifeboat stall or performance gates. It exposed a separate Disconnect wire-format
bug, repaired in `24db95be` with full protocol tests green,
independent approval, and a fresh native kick preserving the exact message and
reason with zero decode errors. Those repairs and the cleanup-test synchronization
fix are pushed through `706d2b10`, whose complete CI run passed. The later local
menu and mining state is recorded below.

Later local integration through `c7b8184e` includes independently approved menu
input/session teardown and bounded toast placement. A fresh Windows/DX12 rendered
pass at 1280x720, platform scale 1 and GUI scale 2 verified editor Tab/Shift-Tab,
select-all replacement and clipboard round-trip, local-server join, eight top-aligned
toast rows from a synthetic packet fixture, Pause/Settings return, stable Home with
zero chunks after disconnect, and keyboard-confirmed clean exit. No full feature,
arbitrary-DPI, transfer-failure, or performance gate is closed by this pass.
Creative mining is locally integrated through `47f8668d` after independent review,
including ordered successful-write traces. Native testing then exposed selected-slot
state remaining unknown despite full server inventory updates. Independently reviewed
default-descriptor routing is integrated in `38c4bd73`; full protocol tests passed.
A fresh production-client/core run against BDS 1.26.40.8 sent one combined Creative
break at tick 825, the server confirmed air at the target, and the client rendered
the hole and fell into it. Server-assigned stack counts now appear in the hotbar.
This closes only that narrow live break witness, not survival timing, repeated/held
attacks, placement, full interaction parity, or inventory management. Item artwork
and inventory-panel stack presentation remain visibly incomplete; an attempted
inventory transfer did not establish a successful management workflow.
Post-integration workspace verification passed 3,444 tests with 13 ignored, including
all 867 client-library tests. The scheduling witness now checks the explicit
evidence-marker → mining-production → movement-send chain.
An additional native launcher kick retained the complete server reason and returned
to Play with zero chunks, but exposed a separate fixed-height launcher message panel:
three wrapped rows extend below its background. The independently approved bounded
layout repair is integrated in `4923ac5e`. Its native retest exposed a separate
disconnect race: a late movement send could close the app before launcher recovery.
The independently approved repair is integrated in `4f4c335c`. A fresh Windows run
survived two server kicks with a successful rejoin between them, displayed both
complete three-line reasons within their panels, reset to zero chunks, and exited
cleanly by keyboard confirmation. Go core tests and vet, strict workspace all-target
Clippy, formatting, and architecture checks also passed. Independent final batch
and publication review approved the complete range without findings.
The checkpoint above is pushed as `ad3cb10c`; its complete hosted CI run passed,
including main verification, desktop compile jobs, macOS bootstrap, and Windows acceptance.
The independently approved inventory checkpoint through `b925fa82` restores
pointer ownership, artwork-independent counts,
personal open notification, empty-destination stack ID zero, retained server window
identity (including zero), and bounded ordered close controls. The final two fixes
(`d5ba4791`, `84bc9a84`) accept the observed matching, admitted local `None` close
acknowledgement and preserve confirmed cursor state across it. Uncertain requests,
server-forced closes, and existing recovery flags remain conservative. Independent
review found no blockers; one non-blocking direct reset-after-retention test remains
coverage debt, with production reset paths verified by inspection.
The normal Windows/DX12 build at `b925fa82` passed physical Take → occupied-cursor
close → reopen → Place against BDS 1.26.40.8. Independent server queries confirmed
32 stone in the destination and none in the source. Repeated reopening and Swap
also passed: seven apples ended in main inventory and 32 stone in the second hotbar
slot, independently server-confirmed. GUI scale 2 and sprite/count visibility were
checked; this is a bounded usability gate, not full inventory parity. Writer checks
passed 892 client-library, 16 integration, and 18 lifecycle tests, plus strict
Clippy, formatting, and architecture. Fresh post-integration workspace checks passed
3,471 tests with zero failures and 13 ignored; strict workspace all-target Clippy,
formatting, architecture, and diff checks also passed. No diagnostic source changes
remain in the production build. The checkpoint is pushed through `2759e46d`;
hosted run `34088016127` passed every job, including Windows acceptance,
macOS workspace tests, and all desktop compile targets.
The personal window ID is server-assigned and is retained from its open response.
A timed-out, uncorrelated personal Open/Close currently leaves that lifecycle
unavailable until the next session; broader timeout recovery remains incomplete.
Offline reconnect currently generates a new player UUID, so reconnecting with the
same display name is not a valid persistence witness. Block-item artwork, broader
inventory gestures, and full inventory visual parity remain open.

2026-09-07 follow-up: independently approved `abea4179` adds positive bounded
explicit-count Take/Place operations for player and generic storage slots, with
empty destinations only. Partial predictions retain both counts; accepted
responses must establish usable distinct identities before the halves can be
reused. Ambiguous identity responses enter existing bounded recovery. Full-stack
Take/Place share the implementation and occupied Swap remains unchanged. Writer
and fresh reviewer each passed 902 client-library, 16 ledger integration, and 21
storage integration tests, plus strict lint, formatting, and architecture checks.
This backend tranche is locally integrated through `a9f60ac4`, not a physical
right-click or complete inventory acceptance gate. Occupied merging, drag, and
native explicit-count validation remain open.
The same local checkpoint includes independently approved `83d42234`, bounding
each read/write phase of the local status endpoint to two seconds so a stalled
tool does not permanently block later clients. Focused repeated control tests,
full Go core tests, and vet passed; root post-integration Go checks passed too.
Race instrumentation was unavailable locally because the required C toolchain
was absent. Fresh post-integration Rust workspace tests passed 3,481 tests with
zero failures and 13 ignored; formatting, strict workspace all-target Clippy,
architecture enforcement, and diff checks passed.
The dependent right-click input tranche is independently approved and locally
integrated through `26b737e3`: ceiling-half pickup and single-item placement into
empty player or admitted storage slots. Existing primary clicks are unchanged;
occupied secondary targets do not merge or swap. Writer checks passed eight
focused production-schedule tests, all 910 client-library tests, strict all-target
Clippy, formatting, and architecture checks; a fresh reviewer independently
passed the focused tests with no findings. Its physical reference parity remains
provisional; full inventory parity remains open.
A fresh normal Windows/DX12 build at `3eff46cf`, 1280x720, platform scale 1 and
GUI scale 2, passed physical right-click tests against BDS 1.26.40.8. Splitting
33 apples, placing one in each of two empty slots, closing/reopening with the
remainder, and placing that remainder produced server-confirmed counts of
16/1/1/15. An even split, single-item transfer, and occupied-target no-op also
passed independent server queries. Settled counts and sprites were legible,
within their cells, and correctly layered; no gameplay-use action was observed
during inventory input. The client exited cleanly with no runtime errors.
This closes the bounded personal-inventory input witness, not storage-native,
occupied merging, drag, block-item artwork, arbitrary-scale, or vanilla parity
acceptance. Fresh post-integration workspace verification passed 3,489 tests,
zero failures, and 13 ignored, plus strict workspace all-target Clippy,
formatting, architecture checks, and the normal client build.
The follow-up is pushed through `defac786`. Completed hosted run `34092936659`
passed Windows acceptance, main verification, macOS bootstrap, and Ubuntu/Windows
desktop compilation, but the macOS Go
suite timed out in the existing required-pack ignore-policy StartGame witness.
Independently approved test-only synchronization is locally integrated through
`3eff46cf`: receive the connected client, explicitly flush its queued final
acknowledgement, then join server StartGame. The old failure did not reproduce
locally, so periodic final flushing is a hypothesis, not a proven root cause.
The admission and no-pack-data assertions are unchanged; repeated focused tests,
fresh full Go core tests, and vet passed. The replacement checkpoint was pushed
through `529d8450`; hosted run `34097979449` completed successfully across Windows
acceptance, main verification, macOS bootstrap, and all three desktop compile
jobs. Pack-server joining policy is unchanged.
The same local batch includes independently approved artwork redirect validation
in `bc5aa862`: redirects retain the initial HTTPS requirement, allow HTTPS CDN
hosts, and remain bounded. Focused and full Go tests passed.

> **For agentic workers:** This is a program-level master plan. Phases 1–8 are sub-projects;
> each gets its own detailed task-by-task plan (per superpowers:writing-plans) when its turn
> comes, executed via superpowers:subagent-driven-development or superpowers:executing-plans.
> Phase 0 is fully detailed here and is executable directly.

**Goal:** A performant, vanilla-parity Minecraft Bedrock client for macOS/Linux/Windows that
joins current third-party servers (RakNet), Realms and friend worlds (NetherNet), and creates
local worlds — built as a Rust/Bevy renderer in front of a Go core derived from Lunar.

**Architecture:** The Rust client owns everything visual and interactive (rendering, world
model, input, UI, audio, one pinned protocol codec). The Go core owns everything network and
identity (Xbox/PlayFab auth, RakNet, NetherNet, Realms, friends, protocol conversion,
resource-pack negotiation), reusing gophertunnel/go-xsapi/go-nethernet unchanged. They talk
over a local byte-stream socket (UDS on macOS/Linux; named pipe or 127.0.0.1 TCP on Windows)
carrying plain Bedrock packets pinned to ONE protocol version, plus a small control channel.
Local worlds run dragonfly behind the same core, over the same client path.

**Tech Stack:**
- Client: Rust, Bevy (wgpu), rayon (meshing), axolotl-stack `valentine` packet defs (protocol 2193)
- Core: Go, `lunar` gophertunnel + go-raknet fork; upstream `df-mc/go-nethernet` and `df-mc/go-xsapi/v2`; dragonfly
- Boundary: socket-file transport already implemented in `bedrock-mc/plugin` (reference impl)
- Assets: Mojang/bedrock-samples (full vanilla resource pack); `refs/pocketmine/bds-data`
  for server data (`definitions/`, `blocks.json`) and a live BDS test server

## Global Constraints

- Pinned loopback wire target: **Bedrock 1.26.50 / protocol 2193** (bumps are deliberate, lockstep with a core release; the core's gophertunnel protocol conversion absorbs upstream server version variance).
- The Rust side NEVER implements auth, encryption-to-upstream, RakNet-to-upstream, or NetherNet. If a task seems to need one of those in Rust, the task is wrong.
- The loopback game channel is Bedrock packets with length-prefixed framing; no RakNet on this leg. Encryption on this leg: whatever gophertunnel's Listener does by default — do not fork to remove it; AES on loopback is negligible.
- Single source of truth for protocol/data lives in the Go estate: packet truth = gophertunnel (validated against Mojang bedrock-protocol-docs via `cmd/protocoldrift`); block/item/biome registries = generated exports from dragonfly; client packet defs = valentine (docs-generated), conformance-tested against gophertunnel bytes.
- New Go types get doc comments at creation (contract on the type, one-liner per method).
- `lunar` gains at most ONE new consumer (the core). Respect the frozen-facade/ABI rules in `platform/Lunar/AGENTS.md`.
- **Go relay source rule:** copy Lunar's `lunar/internal/relay/relay.go` package logic and
  its relay tests into this repository's Go core (target: `core/internal/relay`), recording
  the exact Lunar source commit. Preserve its forwarding, transfer, resource-pack, and
  lifecycle behavior. Replace only its tiny `lunar/utils` panic-helper dependency with a
  local equivalent. **Do not import `github.com/lunar-bedrock/lunar` or add Lunar as a Go
  module dependency; the copied relay package is the only Lunar code the core consumes.**
- Never edit anything under `refs/` (read-only).
- Model routing (per workspace CLAUDE.md): bulk/mechanical implementation → gpt-5.5 via codex skills; anything user-facing (UI, menus, copy) and plan/impl reviews → fable-5/opus-4.8 taste bar.

## Repos and Layout

- **`bedrock-mc/client`** (new greenfield repo; do not reuse `bedrock-mc/Rust-LCE` code, assets, renderer, or world model because it targets the Legacy Console Edition rather than current Bedrock/BDS data):
  - `crates/protocol/` — vendored/generated Valentine protocol-2193 defs + login-sequence state machine
  - `crates/world/` — chunk store, sub-chunk decode, block registry, light engine
  - `crates/render/` — meshing, atlas, chunk/entity/sky rendering (Bevy plugins)
  - `crates/sim/` — movement physics (bedsim-parity port)
  - `crates/assets/` — vanilla + server resource-pack loading (models, textures, sounds, lang, fonts)
  - `crates/ui/` — menus, HUD, inventory, forms, chat
  - `crates/bridge/` — socket transport (from `bedrock-mc/plugin`) + control-channel client
  - `app/` — the Bevy application binary
  - `core/` — **Go module**: the core service; copy/adapt Lunar's relay package and logic as donor code, but never import Lunar as a module dependency
  - `tools/` — Go: registry/asset exporters, conformance fixture generator
- **`platform/Lunar`** — small additions only: anything the core needs exposed through the facade; conformance fixture corpus generator may live in `cmd/`.
- **Decision log** (settled in design discussion, 2026-07-09/10): hybrid over pure-Rust (single protocol treadmill, reuse of auth/NetherNet/Realms/physics estate) and over pure-Go (renderer ecosystem); docs-generated codec over gophertunnel-AST codegen (docs are machine-emitted wire descriptions; gophertunnel Marshal funcs are not generatable); socket file over TCP loopback (permissions, no ports); valentine defs adopted as-is for v1 (Hashim PR'd 1.26.30 to axolotl) — vendor/fork decision deferred until Phase 1; conformance harness deferred to Phase 1 (Phase 0 spike acts as the manual conformance test).

## Risk Register (tracked, each owned by a phase)

| Risk | Phase | Mitigation |
|---|---|---|
| Bevy meshing/frame-pacing insufficient | 0 | Spike acceptance gates the whole program |
| valentine defs drift from gophertunnel bytes | 0→1 | Spike surfaces; Phase 1 builds automated conformance harness |
| Dragonfly registry sequential IDs differ from valentine protocol-1001 palette IDs | 1→2 | Resolved: validated runtime assets now derive the unique canonical air identity for both sequential (`13094`) and hashed (`0xdbf44120`) sessions; the checked-in BREG→compiler→blob regression prevents stale protocol constants from turning air into diagnostic geometry on third-party servers |
| Client-side lighting (Bedrock sends no light data) is a full subsystem | 2 | Scoped task; flood-fill block/sky light, correctness vs vanilla screenshots |
| Molang/entity animation scope explosion | 4 | v1 = molang subset for vanilla mobs; static fallback pose; explicit cut-line |
| Sound binaries not fully in bedrock-samples | 8 | Audited against pinned `v1.26.30.32-preview-full`: 1,815 definitions reference 1,497 unique paths, with 185 binaries absent (94 Nether ambience, 87 gameplay music, 4 menu music); effects are substantially source-unblocked, while the missing ambience/music still requires a lawful local-client import or fallback |
| Windows transport (no tokio UDS) | 1 | Transport behind trait/enum; named pipe or TCP flavor chosen at startup |
| axolotl-stack bus factor | 1 | Vendor valentine output (generated code) into `crates/protocol`; upstream fixes when friendly |
| dragonfly vanilla worldgen parity | 7 | v1 local worlds = dragonfly's gen as-is; parity gaps documented, not chased |
| Bevy 0.x quarterly breaking releases | all | Pin per phase; upgrade as a deliberate task, never mid-phase |

## Current integration snapshot (2026-08-16)

**Protocol-2193 content cutover (2026-09-30).** `assets/bedrock-target.json`
now names Minecraft 1.26.50 / protocol 2193 (codec `bedrock_1_26_51`, measured
server BDS 1.26.52.3) and every production consumer selects the v2193
block/light/biome/physics/fallback/route carriers. Block states come from
Dragonfly v0.11.5 (`4c7b5074`, 22,091 states; every network hash cross-checked
against the 1,020 block items of a BDS 1.26.52.3 CreativeContent capture). Facts
project from the reviewed protocol-1001 records in three classes: 15,963 exact
keys, 3,440 states that differ only by the new `minecraft:connection_*` /
`minecraft:corner` keys (fences, panes, bars, stairs, trip wire), and 2,026
states of 119 new retail blocks borrowed from reviewed schema-identical twins
(poplar family, wool/concrete slabs and stairs, red shrub). 662 states are
reserved. Retail item, biome (adds `dappled_forest`) and item-capacity tables are
re-measured on BDS 1.26.52.3. **Provisional, incomplete against vanilla:**
- The client still derives fence/pane/stair/trip-wire shapes from neighbours and
  ignores the server-sent 1.26.50 connection and corner states.
- Twinned blocks use their twin's model, collision, light and friction; their
  own collision/light are unverified against the reference client.
- `shelf_mushroom` and `straw_bed` (retail in 1.26.50) have no reviewed fact
  source and stay reserved (invisible, passable).
- The pinned pack is bedrock-samples `v1.26.50.4` (release). New blocks draw
  their own pack textures through their twin's model family, and
  `dappled_forest` compiles its own biome rule (absent atmosphere/lighting
  components fall back to the default settings). Still diagnostic:
  `poplar_shelf` (other shelves use the provisional vanilla fallback, which has
  no entry for it) and `red_shrub` (data-driven block whose texture lives only
  in its behaviour-pack `material_instances`). The legacy icon crosswalk is
  still the 26.30 client's, so poplar boats, cushions, and the poplar door and
  hanging-sign icons lack their sprite routes.

**Protocol-2168 target cutover (2026-08-26).** One canonical
`assets/bedrock-target.json` now owns the active game/protocol/codec identity,
carrier paths, and artifact hashes. Cinnabar's runtime world provenance,
collision/physics loading, diagnostics, Make defaults, install paths, dist
layout, and generated block-item routes consume the 2168 set; no production
consumer selects the v1001 block/light/biome/physics/world/item carriers.
Registrygen verifies all manifest hashes and exhaustively guards each production
consumer against legacy carrier drift. The ignored local world/entity carriers
must be rebuilt before live testing. Deterministic closure does not itself close
the LBSG live confirmation gate. (Superseded by the protocol-2193 cutover.)

**`dev/ox-alpha` branch audit closeout (2026-08-26).** The complete
`59f1f8e2..dcf780f1` repair range closes the repository review findings across
session boundaries, retained-tick correction and motion replay, player-list and
inventory projection, process/session cleanup, saved-server bounds, auth-cache
file identity and ACL handling, acceptance duration/disconnect classification,
foreground-input identity, atomic asset publication, and registry-tool CI
coverage. Every behavior fix carries an observed RED-to-GREEN regression. Fresh
post-integration verification passed the full locked Rust workspace, strict
all-target Clippy, formatting, architecture policy, Go core tests/vet, full
registrygen tests/vet, the Bash acceptance harness, and the macOS/Linux
no-replace publisher contracts; the Windows auth-cache suite cross-compiles,
while native PowerShell execution remains the CI matrix's platform gate. A fresh
Sol-high review of the entire range returned APPROVE with no Critical,
Important, or Minor findings. No live/native gameplay or visual gate is closed
by this code-quality tranche.

**LBSG anti-cheat diagnosis and live movement tranche (2026-08-22):** a ranked read-only
investigation attributed the Lifeboat "movement cheats" rejections to semantic state vanilla
never claims plus server-authoritative signals the client consumed and ignored, led by
sprint-without-forward claims, discarded `SetActorMotion` knockback, never-set teleport
acknowledgement, unanswered latency probes, and the catch-up overflow that permanently silenced
the outbound input stream mid-session. Five bounded tranches landed on `dev/ox-alpha`:
`e304b9af` gates processed sprint on forward movement input for both simulator and encoder;
`39076d8e` ingests local-player knockback as tick-keyed prediction overlays that survive
correction replay; `e219f0e3` answers server `NetworkStackLatency` probes (its exact-timestamp
echo was superseded on 2026-08-25 by a provisional `×1,000,000` scaling after four authenticated
sessions on one target proved its anti-cheat normalizes echoed ids before matching and tears down
unresponsive clients; see the vanilla-parity-audit NSL row — prior exact-echo live evidence no
longer describes current bytes until BDS/LBSG re-runs land);
`60977327` adds the authenticated Venity live target across launcher, validators, app args,
and all 93 Pester contracts; `aaccc94c` stops revoking movement authority over catch-up overflow,
keeping retained samples contiguous so the 20 Hz stream stays reconcilable. Each tranche has
regression coverage plus green focused suites, strict Clippy/formatting, and architecture checks.
A first authenticated Venity candidate run reproduced the exact historical death: join-time
streaming stalls dropped seven of fifteen due ticks and permanently revoked authority. After
`aaccc94c`, a second authenticated Venity run held `outbound_authorized=true` through 2,762
transmitted physics packets and 148 seconds of debug-build streaming stalls with zero authority
faults and zero decode errors; the session ended only when Venity's upstream RakNet read timed
out against a stationary, unregistered client — an idle-kick profile, not a movement-cheat
rejection. Remaining live-gate work: organic-movement drivers, the HandledTeleport window and
jump/sneak flag witnesses, sustained-session survival on both live targets, and native stall
measurement for the provisional overflow policy.

**Venity abandonment root cause and first full-duration session (2026-08-25).** Four further
authenticated sessions reproduced the teardown deterministically (56–107 s, always after exactly
the server's spawn-region stream of 421 inline columns) and proved it independent of idleness: an
OS-driven organic-input session transmitting 1,445 movement packets died on the same signature.
Read-only diagnosis attributed the deaths to the target's anti-cheat latency-ACK watchdog — its
matcher normalizes echoed `NetworkStackLatency` ids before matching stored batches, so exact-timestamp
echoes never resolve, and its responsiveness counter only advances while the client streams
movement, expiring into a silent disconnect. Independent source verification confirmed every cited
mechanism at the anti-cheat's pinned version (60 s default budget matching all observed lifetimes).
The provisional ×1,000,000 echo scaling (`1ef3e2a8`) plus the committed organic-movement driver then
produced the first complete Venity session ever recorded: 603.2 s wall (the full 480 s acceptance
window), 1,356 transmitted physics packets from source=Physics with a `Drained` terminal at depth
zero, zero decode errors, zero drops, zero authority faults, and normal shutdown telemetry. The
strict aggregate verdict remains open on: the inline-cohort world_ready defect (`317a3de1` pending
review closes it deterministically; live confirmation is that tranche's own gate), one
`non_monotonic_frame` witness marker in the debug run, the spawn-geometry embedment that keeps
re-engaging the provisional settle gate, GamePad-frame validation (no physical gamepad), BDS/LBSG
tolerance re-verification of scaled echoes, and authoritative retail-client measurement of the NSL
echo contract.

**Session record (2026-08-25, continuation).** Five reviewed tranches landed on `dev/ox-alpha`: `d0bb7ccc` derives every hotbar cell from one ledger-snapshot authority revision (VPA-123 deterministic slice); `b6d689eb` bumps PHASE3_VIOLATION to v2 with a ten-key identity payload for `non_monotonic_frame` so future one-off live occurrences are self-diagnosing; `57b59b06` lands the canonical container-address projection (VPA-122) after a recovered interrupted lane was completed and a review-caught silent narrowing of prior window-0 admissions was restored; `b3b14bc5` adds the bounded spawn-anchor depenetration probe with `SpawnSettleGate::EmbeddedHold` and exact surface-spawn feet (VPA-109 slice); `f9cbe242` adds opt-in (`RUST_MCBE_TELEPORT_ACK=1`) HandledTeleport acknowledgement wired through production reconciliation for correction snaps, teleported MovePlayers, and Respawns after its first review found the Respawn site unwired. Each has fresh independent review evidence in the tracking table, and the branch-adjudication startup obligation is discharged: 49 unintegrated local heads dispositioned as 30 superseded + 12 stale-historical + 7 backup + 1 genuine candidate (`agent/combat-phase5`, kept as reference only — it predates the protocolgen rewrite so it seeds a fresh reviewed tranche, never a direct merge) + 4 low-confidence entries requiring object inspection before any deletion. Live: the first authenticated current-binary LBSG session joined, streamed its full inline cohort, transmitted 44 physics packets, then reproduced the exact `"We've detected movement cheats"` rejection at 49.8 s after two 200-tick embedded-anchor fail-open lifts with a constant +1.00 X-block/tick inputless slide — the embedment mechanism is now confirmed live, the probe correctly refused to invent motion, and the next cure tranche is collision-truth adjudication instrumentation. The same session exposed and fixed a launcher stale-binary defect (machine-wide `CARGO_TARGET_DIR` redirecting the harness build while it executed a days-old project-local exe) and an auth-cache ACL quarantine that blocked all authenticated runs until the operator cache was restored under its required protected DACL. BDS/LBSG tolerance of scaled NSL echoes remains unreverified because this session ended in a movement rejection before echo behavior could be decisive.

**Repository-wide parity audit follow-up (2026-08-20):**

 the durable mismatch inventory is
tracked in `docs/tracking/vanilla-parity-audit.md`. It covers protocol/cache/session, world
retention, movement/input, HUD/inventory/entities, assets/meshing/render/resource packs, and
product/platform/audio/tooling. Its P0/P1/P2 entries are open gates, not phase completion. The
audit must be updated as reviewed tranches land; captures and non-redistributable payloads remain
outside git.

**Repository-wide audit wrap (2026-08-21):** all research and writing workers are finished. The
selected-stack authority task head `2a17459881ea5f586b8e1cb937c4ad7337882924` was independently
approved and integrated as `9d6e291b` plus `d90d8764`. The malformed-inner-wire task head
`14e2ceedf3fdc58a60141410731605cef03e4f81` completed repeated fix-first review cycles and received
a final fresh Sol-high `ship` verdict; it is integrated history-preservingly as `2e6feae1`,
`83198075`, `e0277132`, `1322fb49`, `8e647d01`, and `eb017fac`. Coordinator world, protocol,
client-world, app, strict Clippy, formatting, architecture, and diff gates passed at the reviewed
heads. The durable remaining mismatch inventory and exact native/live gates are in
`docs/tracking/vanilla-parity-audit.md`; no open row is closed by this audit alone.

Eight bounded audit fixes are integrated and independently approved on `dev`.
`e21e99b6` makes verified blob entries process-owned across network-worker replacement and isolates
unrelated semantic skips from pending cache transactions. `a63431b0` through `6c7e2672` send the
checked current ledger prediction in `MobEquipment`, retain one latest-wins selection through
backpressure, and cancel stale pending sends on a valid server-forced selection. `76f8c87c` keeps
failed request-mode columns outside loaded/cohort readiness and admits required LevelChunks only
after successful decode and world admission. `9dfecb80` skips non-finite remote player poses and
unknown equipment containers at the semantic boundary without ending the session or overwriting
usable actor state. `c4ccc81c` and `5cbfbcea` remove the persistent gameplay player preview and keep
it confined to personal inventory across Open-before-Content plus 27/54-slot storage lifecycles.
`25d87058` adds a separately pinned BedSim v0.1.5 liquid oracle (the four-record output is
byte-identical to the historical v0.1.4 slice), corrects non-swimming water gravity, and applies
the observed water-to-ledge boost only after a bounded clear, dry raised probe. BedSim remains a
Go reference/oracle used by trace generators; production movement is the Rust `crates/sim` port.
Fresh coordinator crate suites, formatting, strict Clippy, architecture enforcement, and fresh
Sol-high review passed for each tranche. These commits close only their bounded implementation
defects; cache ordering/timing, selected-store unification, item rendering, inventory-preview
geometry and animation, broader actor behavior, exact chunk retention, and native/live acceptance
remain open.

The 2026-08-21 parallel audit additionally confirmed the existing version-coherence, resource-pack,
post-login transfer, movement/input/pose, actor/viewmodel, audio, menu/settings/touch, auth recovery,
control, local-world, shutdown, Windows transport, packaging, and visual-calibration gaps. New
bounded rows VPA-122 through VPA-139 and VPA-209 through VPA-212 record container identity,
response fan-out, deferred close, item-container data, gameplay item-use, item identity/components,
server camera instructions, forms, player-list binding, toast/scoreboard/boss behavior, non-HUD
inventory layout, upstream blob-cache capability, persistent blob storage, credential permissions,
saved-server durability, archive/runtime cleanup, Windows backend negotiation, and Java-HUD
calibration. These are code-backed findings, not completed implementations.

Cross-repository disposition is narrow: no required `bedsim` or `bedrock-docs` correction was
established. Future resource-pack/cache handoff changes may require
`HashimTheArab/gophertunnel:resource-pack-changes`; never push them to `lunar`. Local-world work
requires a new Dragonfly lifecycle integration in Cinnabar. Exact movement modes and flags,
correction semantics, chunk/cache ordering and retention, UI timings/layouts, rendering transforms,
lighting/atmosphere/liquids, audio mixing, Windows ACL/DPI/backend behavior, packaging, and every
native visual/performance gate remain unconfirmed until the matching live tests named in the audit
ledger are run.

The item trace found no safe approximation to land. Sprite icons currently mix atlas-alias identity
with canonical live registry identity, while block items need a separate 3-D renderer; the
repository has no complete authoritative crosswalk and filename inference is forbidden. The liquid
slice does not close broader swimming: v0.1.5 adds swimming/flow/pose, knockback, item-use, and
provider contracts that still need a deliberate Rust port, plus immersion, pitch steering, currents,
input flags, camera state, and current-version production physics content under VPA-010 through
VPA-014.

This is the authoritative current snapshot. It supersedes the dated ledgers and handoffs
below without deleting their historical evidence. The code audit covers the Bedrock
1.26.40 migration at `e7901ae`, the fork-repin closure at `a5c327d`, and the integrated
runtime state represented by this tree.

The mandatory public fork and repin are closed deterministically. The core and fixture
generator resolve `HashimTheArab/gophertunnel:resource-pack-changes` commit
`434923f163a15144cdaa44356536cdc76722c50d` through module pseudo-version
`v1.25.3-0.20260816120458-434923f163a1`. The two Go consumers and their checked-in
provenance are synchronized to that public revision; all 32 checked-in fixture `.bin` files
remain byte-for-byte unchanged. The Go and protocol test suites passed. This establishes the
pinned wire/tooling baseline and a compile-time witness for clone-safe, complete offer and stack
snapshots. Advertisements and selections are forwarded to the local client with exact metadata,
ordered built-in/ignored entries, sub-pack selections, base-game version, experiments and editor
state. Downloaded entries retain ordered archives and memory-only content keys; the one-shot
handoff remains bounded and validated.
Optional stack entries that are unavailable, malformed, duplicated, or select an unsupported
sub-pack are retained for exact Go replay and ignored by the Rust application handoff instead of
terminating the session. Required selections remain strict.
The private core-to-client hop forwards the upstream offer and stack, projected onto the admitted
packs, with the server's own required bits; a required offer the core could not fully acquire is
refused with `disconnectionScreen.resourcePack`, and the client refuses a required pack it cannot
apply. Per-download byte/count/time bounds, HTTP opt-in policy, and digest-bound
cache identities from the retired `cinnabar` fork are deliberately not carried onto Lunar's
resource-pack branch yet. Archives are not extracted or applied, application remains unavailable,
and this is not live gameplay, native visual, or performance evidence.

This tree now contains the protocolgen-backed Valentine 1.26.44 projection, derived from the
reconciled 1.26.40 base because Mojang retained protocol 2168 while adding the outer optional
marker around `RemoveScore.ObjectiveName`. It replaces
the retired Prismarine-derived packet generation path while preserving the public protocol
crate facade. Protocolgen reconciles pinned Mojang and Endstone sources, fingerprints every
adjudication, and compares the result against pinned Gophertunnel: 177 packets agree
automatically, three reviewed divergences retain the Mojang-plus-Endstone shape, 48 remain
explicit static-analysis coverage gaps, and one packet has no Gophertunnel counterpart. The
protocol crate exposes only the current generated version and preserves unavailable wire
values as neutral reserved or opaque data.

A release client at `1095c693` completed authenticated Lifeboat negotiation, handed off the
ten-pack optional resource-pack offer, entered gameplay, and processed 680 ordinary level
chunks with no packet decode error. Lifeboat later disconnected the client for movement-cheat
detection, which leaves movement parity and server-authoritative reconciliation open rather
than invalidating the protocol/resource-pack acceptance. A later rerun was externally blocked
by an Xbox title-endpoint timeout before upstream login.

The transitional block/light/physics/visual production carriers remain bound to the older
16,913-state content corpus. Their retail projection is internally deterministic and fail-closed,
but it does not make them current 1.26.40 content. Standalone versioned protocol-2168 carriers now
bind all 17,499 current block identities, a BREG-bound fail-closed block-light projection, and the
exact 88-entry public-retail numeric biome projection. The BREG and LREG hashes are respectively
`e3768f6d70195b22ac3843f6ef49261a80cd83284bc9741c7eb4a446def6bec8` and
`f188240ec053128f771f0267d0197c19c071d57e67bd3c2cf69ae6ba5601cbab`; BIOREG remains
`5209a8ec6d9b2690d062c124e206dc0f565d1937601c181798dbffbd9904272c`.
The protocol-2168 foundation manifest is structurally ready, but every production default remains
on v1001 until current physics and dependent visual evidence close as one coherent switch.
Cross-version carrier fallback is forbidden.

Current implementation state:

- Protocol, world, movement, gameplay-HUD, and launcher foundations have landed. Production
  physics is enabled by default. Generated collection decoders are allocation-bounded, the
  current generated borrowed packet views retain byte fields without an eager copy, and the
  StartGame world clock is separated from the provisional local prediction-tick anchor.
- Replay-stable local Jump Boost, Levitation, and Slow Falling snapshots have landed with
  correction-safe history. A bounded collision-shape-aware block interaction ray query has
  also landed. The pinned simulator's consumable-use slowdown and replay snapshots are now
  modeled behind an explicitly false production input; activation remains blocked on authoritative
  consumable classification and item-use lifecycle. None of these tranches closes the remaining
  movement strata, outbound block transactions, gameplay reach, or live server-authoritative
  acceptance.
- Actor CPU/GPU rig foundations have landed. Spawn and incremental actor links now feed a
  bounded, lifetime-safe riding-authority ledger; remote animation evaluation receives
  authoritative `query.is_riding`, and local mount changes reach the existing UI authority
  path. Rider attachment and pose completion, non-player families, held items, dropped
  items, and live actor acceptance remain open.
- The Go core now performs one upstream pack negotiation before downstream login.
  Nonempty selections are forwarded with their exact composite `UUID_version` identities and
  selected sub-pack; upstream metadata arrival order is not treated as semantically ordered.
  Required upstream selections are offered as optional only on the private local hop
  so pack application incompleteness does not block login. Rust bounds and validates each archive
  transfer and retains the selected archives and memory-only
  content keys as a one-shot login handoff. The owner-only persistent cache is now wired into
  upstream admission behind explicit directory and quota configuration, with bounded load, hit,
  miss, store, and error telemetry and fail-closed cache setup. Lunar keys entries by UUID,
  version and size and Cinnabar revalidates those fields plus archive readability; the current
  API does not expose the server digest, so same-identity, same-size replacement is not
  cryptographically distinguished. Every app-managed core launch supplies one stable
  layout-owned cache path while leaving secure creation, leasing, and the default quota under Go
   ownership. The core now also exposes an explicit opt-in `-upstream-client-cache`
   capability (default off, byte-identical wire when unset) that flips the outbound upstream
   `ClientCacheStatus` enabled byte through an observe-then-flip dialer hook, and both production
   app spawn paths enable it exactly when the Rust session owns its process-lifetime verified
   blob cache; live cached-chunk streaming evidence on authorized targets remains open. A separate opt-in
   read-only Status v1 control endpoint and strict Rust bridge reader expose secret-safe lifecycle
   and latest pack-admission state. Live Lifeboat negotiation and handoff are now evidenced, but
  pack-content semantic validation and parsing, archive extraction, application,
  resource-pack-driven UI, and app presentation of status remain absent; no server pack is yet
  usable by the renderer.
- A bounded player-inventory authority ledger and one-at-a-time Take/Place/Swap requests for
  the 36 player slots and cursor have landed, including rollback, full-authority recovery,
  transport admission, and pointer routing. The same single-flight authority now supports a
  neutral type-0 generic storage surface with exact 27/54-slot bounds, close correlation, and
  server-authoritative recovery. Crafting, furnace roles, advanced gestures, and live
  container acceptance remain open. Receive-side crack presentation remains read-only.
- Exact protocol-2168 click-block, client-close, actor-attack, and actor-interact fixtures plus
  strict block-use and actor-use packet builders have landed. The application sends none of
  those transactions yet: target selection and hit testing, gameplay reach, packet-position
  provenance, ability authority, selected-stack correlation, and live evidence must close
  before wiring app senders.
- Bounded `UpdateAbilities` evidence now follows the sequenced world commit into
  the accepted local player's session binding. Unknown, received-empty, and
  unavailable evidence remain distinct; layer order, raw masks, and float bits
  are retained without assigning effective permission semantics. Terminal and
  replacement-session paths retire the binding, and stale setup cannot replace
  current evidence. Fresh protocol/client-world tests and the application suite
  cover raw framing, FIFO ordering, local identity, and lifecycle guards. This is
  passive retention, not effective permissions, Survival admission, or a mining
  sender; end-to-end handshake and native permission acceptance remain open.
- Supervised first-run device-code authentication and cached-account validation have landed;
  token bytes remain Go-owned. Accounts without an Xbox profile can open Microsoft's hosted
  signup and resume sign-in with the same Microsoft login. Live Xbox account creation acceptance
  remains open. A cached-account authenticated Lifeboat join is evidenced; native
  first-run/device-code UX acceptance remains open. Bounded named PlaySound, StopSound, and LevelSoundEvent ingress now reaches
  an app same-frame delivery seam; a bounded session-owned outcome queue resolves named plays through the
  optional compiled sound-definition catalog into finite-checked playback records (stops catalog-free,
  level events transport-only) but there is still no audible runtime, mixer, listener math, category
  settings, pack routing, or session-reset owner.
  Local worlds remain absent.
- Hosted Windows, macOS, and Linux compile-readiness jobs and cross-platform local-endpoint
  path derivation have landed. A fallible installed/developer layout owner and unsigned local-only
  bundle staging tool now produce deterministic Windows, macOS, and Linux layouts without changing
  public distribution policy. Native installed launch, signing/notarization, installer generation,
  and redistribution approval remain open. Platform CI remains a continuing gate rather than
  evidence of native gameplay parity.
- The open issue/PR sweep is current: obsolete phase trackers and the superseded broad pack
  ingestion branch are closed with recorded reasons. The remaining open items cover pack
  application, measured join latency, combat, and actor animation; none of their old branch
  heads is approved for direct merge into `dev`.
- The protocol/resource-pack join path has live Lifeboat evidence. Native vanilla-comparison,
  movement/session-lifecycle, visual, and performance gates remain open; do not infer their
  completion from this connectivity result.
- Content assets intentionally remain on the older pinned inputs until a coherent
  protocol-2168 content migration is verified; packet migration alone does not authorize a
  piecemeal asset bump.

Immediate execution order:

1. **Protocol/content debt closure:** the generated allocation guards, borrowed packet views,
   bounded `LevelChunk` retained-byte hot path, and standalone v2168 BREG/LREG/BIOREG evidence are
   closed; do not redo them. Produce the dependent current physics and visual evidence, then switch
   block/light/biome/physics carriers only as one coherent, verified set. Keep conformance and
   adversarial coverage green.
2. **Connectivity and authentication:** preserve the closed cached-account Lifeboat join path,
   close movement-safe session lifecycle and transfer behavior, then validate the first-run
   device-code UX with explicit native evidence.
3. **Resource packs:** the bounded persistent cache admission and versioned/correlated Status
   v1 control surface are closed; do not redo them. Validate and apply server packs, hand off
   content keys safely, consume status in the app where needed, and implement
   resource-pack-driven UI contracts. Required packs must remain truthfully rejected until
   application exists.
4. **Interactions:** use the landed protocol-2168 block-use and actor-use builders only after
   the frozen target, ray, position, reach, ability, and selected-stack authorities are proven.
   Add exact break and placement fixtures before their app senders. Extend the landed player
   plus bounded generic-storage ledger to later container and crafting roles without weakening
   single-flight reconciliation.
5. **Actors and UI:** complete non-player/held/dropped rendering and actor live gates, then
   interactive forms and the remaining HUD/menu/UI acceptance work.
6. **Product phases:** proceed through online product surfaces, local worlds, audio, polish,
   packaging, and their native/performance acceptance gates.

## Historical integration snapshot (2026-07-16)

This preserved ledger was the authoritative hand-off for its 2026-07-16 branch audit. Its
audit base is clean `render-integration` commit `1ac547b`, published exactly as
`github/phase2-textures`; the last functional implementation commit before the workflow
documentation is `efa5400`. The agent runtime does not expose exact model/version or effort
selectors, so reviews use the configured runtime default rather than claiming a particular
model setting.

At audit time the repository had 65 linked worktrees. Twenty-five worktree heads were
ancestors of the audit base, 26 more were patch-equivalent or historical, and 14 had
patch-unique commits. The three genuine candidates and their current disposition are:

| Tranche | Base -> head | Review/integration state | Next action |
|---|---|---|---|
| Phase 3 movement foundation | `efa5400` -> `71d38a3` | Integrated history-preservingly as merge `e370880`; focused and full locked workspace tests, strict Clippy, formatting, and architecture enforcement are green | Keep production outbound movement disabled while FreeCamera is active; finish the remaining movement strata and live server-authoritative acceptance before enabling `Physics` transmission |
| Phase 2.6 leaf litter | `efa5400` -> `698be1c` (`phase26-leaf-litter`) | Preserved on its feature branch, review **needs changes**, and deliberately unmerged | Deferred until further authoritative state-to-visual data is available; retain the branch and finish this exact family with the other residual visual-authority work last |
| Phase 4 entity geometry carrier | `105107d` -> `4a6696b` | Integrated history-preservingly as merges `1e4ba3c` and `73b8de7` after three Important parser/inheritance findings were fixed, the complete behavior range received fresh APPROVE, and the policy-compliant module split received a separate APPROVE | Consume the bounded geometry/bone/cube payloads in runtime rigs; animation clips, Molang/controller evaluation, GPU posing, and native animated-actor evidence remain open |

Phase status at this audit:

| Gate | Accurate state |
|---|---|
| Phase 2.5 biome blending | Open: vanilla 3D lattice cache port is local; graphics dispatch, native boundary comparison and live acceptance remain incomplete |
| Phase 2.6 visual coverage | Open: the production carrier has zero diagnostic states, but 2,397 non-air states across 487 names use an explicitly provisional vanilla fallback. This removes pink vanilla blocks without claiming exact geometry/UV parity; each fallback remains an open acceptance item |
| Phase 2.7 lighting/sky/fog/clouds | Open: the cloud evidence sub-gate is complete, but calibrated atmosphere parity, native cloud/celestial comparison, and the <=2 s teleport-remesh gate remain open |
| Phase 3 movement | Packet/simulation foundations plus the reviewed PR #6 input-parity and correction/acceptance lanes are integrated through merge `a9593e7`. Implementation and deterministic verification are complete, but native/live, performance, and touch-parity acceptance remain open. By owner decision touch is deprioritized and does not gate Phase 3 acceptance; the scenario records it as deferred rather than satisfied. Production outbound `Physics` transmission remains intentionally disabled pending a separate reviewed change |
| Phase 4 actors | Actor tracking, standard-skin biped rendering, Oomph-style three-tick player convergence, distinct per-frame render interpolation, and the bounded MCBEENT3 geometry/bone/cube carrier are complete. Runtime rig consumption, animations/Molang, persona/custom rendering, legacy/outer skin layers, and remaining entity families are still open |

Nine other patch-unique branch heads were audited as superseded/reimplemented and were
deleted locally after their authoritative replacements were verified: local `phase2-textures`
copper work, `phase27-atmosphere-parity-fix`,
`phase2-block-entity-manifest`, `repair-mushroom-typed`, `phase27-light-core`,
`phase27-light-scheduler`, `phase3-physics-fix-resume`, `phase26-static-signs`, and
`transparent-model-stream`. Two more, `fix/frustum-culling` and
`phase26-wood-shelves`, were obsolete/withdrawn and were also deleted locally. The remote
`github/phase2-textures` integration branch was not deleted.

Five worktrees contain preserved, uncommitted work and are intentionally not cleaned by this
audit: the root `cinnabar-work`, `external-mode-diagnostic`, `blockentity-evidence`,
`atmosphere-black-diagnosis`, and `static-fence-gates`. Their changes are not part of the
three integration candidates above.

## Historical execution order (2026-07-16)

The exact leaf-litter route and any other residual family that lacks sufficient
state-to-visual authority are deferred until further authoritative data is available. Keep
their feature work isolated and unmerged; already integrated and verified Phase 2.6 families
remain in place. Continue with work that does not depend on that missing authority, in this
order:

1. **Phase 2.7 runtime and visual closure:** first run the fresh release/BDS trace for the
   already integrated adaptive chunk application/upload scheduler, then tune the stage that
   trace proves is still preventing the <=2 s teleport-remesh gate. Follow with the
   identical-scene FIFO/Immediate visibility witness and any proven void-band source, then
   native celestial and finite-cloud parity acceptance.
2. **Phase 2.5 biome blending evidence:** the bounded provisional 3x3 implementation is already
   integrated. Measure a version-matched native abrupt biome boundary to determine its radius,
   offsets, weights, and colour space; change the CPU/WGSL kernel only if that evidence disproves
   it, then close matching-scene GPU/live and performance acceptance.
3. **Phase 4.3 entity rigs and animation:** consume the integrated bounded geometry carrier,
   extend the already integrated pinned animation/controller catalog with bounded animation-clip
   payloads, implement the reviewed Molang/controller subset and shared runtime posing, and prove
   animated players/mobs without per-actor resource churn.
4. **Phase 4.4 actor acceptance:** close live ground contact plus the distinct three-tick
   network convergence and adjacent-frame render interpolation witness.
5. **Phase 3 movement completion:** add the remaining simulation strata and live
   server-authoritative acceptance before enabling outbound movement from `Physics`; keep
   `FreeCamera` network-silent. Incomplete after the rewind-timeline tranche: server flags
   only end swim/glide/crawl (never start them) and a held sneak button reasserts itself;
   item-use slowdown assumes no `minecraft:use_modifiers` on the used item; swim stop
   predicates, conditional vertical steering and Dolphin's Grace are unported; no
   client-predicted vehicles (vehicle corrections are ignored as for an unpredicted mount).
6. **Phase 5 UI:** implement the UI foundation, receive-only text/HUD, chat, scoreboard and
   boss bars, inventory/interaction, and forms in the numbered 5.1-5.7 order.
7. **Deferred final Phase 2.6 authority pass:** return to leaf litter and other exact residual
   visual families only when further authoritative state selectors, geometry/UV, rotation,
   layer, tint, animation, and occlusion data are available. Re-review, verify, integrate, and
   measure the production diagnostic ratchet family-by-family; do not infer ambiguous mappings.

### Historical PR 6 cache-enabled live-validation handoff (2026-08-01)

The local `agent/track-phase3-movement` implementation through `7d89ff5` is ahead
of its pushed remote (`d8637a2`), independently approved, and ready to push for
CI/merge; it is not yet pushed or integrated. Cache-enabled BDS load exposed and
fixed a self-sustaining blob-recovery loop, unbounded publication-authority scans,
repeated per-frame cohort hashing, scheduler starvation, intra-transaction relight
cascades, and the requested-cohort boundary stall. Optimized-dev BDS acceptance run
`phase3-018ae154de9b40cb8f12aea378c48752` reached zero
pending/in-flight light and mesh work across a stable 939/939-column cohort:
8,130 resident meshes, 5,416 submitted/GPU-completed meshes, 33,205 accepted
light jobs, and zero stale light jobs. The inspected Windows scene was complete.
After the final visibility-cache optimizations, acceptance run
`phase3-33bd062a9cc04078b8ffeb68573d7746` measured idle
frames at 10.272 ms median (the 100 FPS VSync ceiling), zero-byte publication at
23.061 ms median, and payload publication at 29.836 ms median. Active chunk
publication still causes visible frame drops and remains explicit follow-up work.
This closes the blank-window and visible-radius convergence regressions; it does
**not** close release-performance or vanilla-lighting-parity gates.

The current column transaction solves contiguous vertical work against retained
3x3 subchunk snapshots, retains immutable generations, and admits requested
cohort-edge columns without waiting forever for unrequested neighbours. It does
**not** yet reproduce vanilla's atomic 3x3-column ownership, cross-column writes,
or 500 ms lock-contention retry, so it remains an explicitly incomplete
convergence architecture and cannot close the native lighting-parity gate.
Audit the full architecture before further queue-cap tuning. Remaining measured
follow-ups are active-publication frame pacing, release-mode performance,
version-matched native comparison, and the native atomic lighting architecture.

---

## Phase 0 — Spike: prove the stack end-to-end (DETAILED, executable now)

**Goal:** Bevy app connects through a core process to a real server, decodes chunks with
valentine defs, meshes and renders a 16-chunk radius with acceptable frame pacing.
**This phase gates the program.** Failure modes and their outs: defs drift → fix
gophertunnel/defs (expected, fine); frame pacing fails → revisit meshing strategy before
any other phase proceeds.

**Files:**
- Create: `bedrock-mc/client` repo — `app/` (Bevy spike), `crates/protocol/`, `crates/bridge/`, `core/` (minimal Go main)
- Reference (do not modify): `bedrock-mc/plugin` (socket transport), `platform/pc-client` (lunar embedding), `libs/gophertunnel/minecraft/dial.go`, `libs/dragonfly` `chunk` package (sub-chunk decode reference)
- Test server: `refs/pocketmine/bds-data/bedrock_server-1.26.32.2` (run a local BDS) or a dev Lunar upstream

**Interfaces (produced for later phases):**
- `core/`: Go binary `bedrock-core` — flags `-socket-dir <dir> -upstream <host:port>`; listens with `minecraft.Listener` on the socket transport, dials upstream via `minecraft.Dialer`, forwards packets both ways at pinned protocol (this is a ~200-line pc-client-shaped proxy main for the spike; productized in Phase 1)
- `crates/bridge`: `fn connect(socket_dir: &Path) -> anyhow::Result<FramedStream>` where `FramedStream: Stream<Item=Bytes> + Sink<Bytes>` (length-prefixed batches)
- `crates/protocol`: `fn decode_batch(bytes) -> Vec<Packet>`, `fn encode(packet) -> Bytes`, `enum Packet` (valentine-generated), plus `LoginSequence` state machine: `RequestNetworkSettings → Login (self-signed chain, no XBL) → handshake → ResourcePackClientResponse (decline/none for spike) → await StartGame → RequestChunkRadius(16) → await spawn → SetLocalPlayerAsInitialized`
- `crates/world` (spike-minimal): `SubChunk::decode(&[u8]) -> SubChunk` (paletted storages), `Chunk { sub_chunks: Vec<SubChunk> }`

**Tasks (each = write failing test → run → implement → pass → commit):**

- [x] **0.1 Repo scaffold.** Cargo workspace + `core/` Go module; CI stub (`cargo test`, `go test ./...`). Complete at `41112b2` (review clean).
- [x] **0.2 Spike core proxy.** Complete at `823bf49` (live BDS join passed; lifecycle hardening review clean). Go: socket-transport `net.Listener` (port from `bedrock-mc/plugin`) + `minecraft.ListenConfig{AuthenticationDisabled: true}` + upstream `minecraft.Dialer` forwarding loop. Test: Go integration test dials the socket with gophertunnel's own client, joins local BDS through it. Run: `go test ./core/... -run TestProxyJoin -count=1`. This test is load-bearing: it proves the core path with a known-good client before Rust enters.
- [x] **0.3 Bridge crate.** Complete at `7ce7309` (17 Rust unit tests + Go echo integration; review approved). Rust: connect + framing. Test: echo fixture against a Go test binary serving the same transport. `cargo test -p bridge`.
- [x] **0.4 Protocol crate: vendored defs + decode smoke.** Complete at `a7bbfac` (five exact gophertunnel fixtures; review approved). Vendor valentine 1.26.30 generated output. Test: decode a fixture corpus of gophertunnel-encoded packets (generate fixtures with a small Go tool in `tools/fixturegen` — encode one of each: NetworkSettings, StartGame, LevelChunk, MovePlayer, AddActor). Any decode failure here = defs/gophertunnel drift: adjudicate against bedrock-protocol-docs, fix gophertunnel upstream or patch defs, record in `crates/protocol/DEVIATIONS.md`.
- [x] **0.5 Login sequence.** Complete at `1fa35ee` (encrypted Rust bridge login, strict protocol-1001 conformance fixtures, bounded malformed-input handling, independent review approved). `LoginSequence` reaches StartGame through the spike core. With `BEDROCK_BDS_DIR` set, `cargo test -p protocol --test login --locked -- --nocapture` builds the Go external-client harness, starts/stops core+BDS itself, and verifies clean shutdown.
- [x] **0.6 Sub-chunk decode.** Complete at `7d9248a` (12 reproducible goldens from pinned Dragonfly, packed/paletted v1/v8/v9 decode, atomic sparse chunk ingestion, 28 Rust world tests, three independent reviews approved). Runtime storage remains palette + packed words and preserves high-bit network block hashes without a flat per-block array.
- [x] **0.7 Spike renderer.** Complete at `f2a6a1c` (400 Rust tests, strict all-target Clippy, independent review approved, and live fly/input pass recorded). First extend `crates/world` with packed-palette `UpdateBlock`/`UpdateSubChunkBlocks` mutation and full-column eviction APIs; expand each changed key through `mesh_dependents` before remeshing. Bevy app: consume LevelChunk and SubChunk responses → decode → cull-meshing on rayon → vertex buffers → draw untextured (per-runtime-ID debug colors); fly camera. Pure meshing remains unit-tested. Use Computer Use for a live interaction pass covering window focus/capture, keyboard inputs, fly movement on every axis, mouse-look yaw/pitch, and clean input release (no stuck movement or rotation); the acceptance run below remains the end-to-end renderer gate.
- [ ] **0.8 Acceptance run.** Connect to BDS world, render 16-chunk radius, fly at speed, break/place blocks from a second client to force live remeshing. Repeat the Task 0.7 Computer Use interaction checklist in the live streamed world and record the result. Before the run, resolve the recorded `AvailableCommands` live drift and remaining protocol conformance coverage from `crates/protocol/DEVIATIONS.md`. **Gate: p99 frame time ≤ 8ms on the dev MacBook at 16 chunks; remesh of a modified sub-chunk visible ≤ 100ms; zero decode errors over a 15-minute session (or all errors adjudicated as 0.4-style findings and fixed).** Record numbers in the phase report.
  - Historical Windows evidence at `3898530` passed: 900.0015 s, radius 16/16/16, p99 5.1 ms, 432/432 visible mutations, max mutation-to-visible 45.4522 ms, zero decode errors, clean shutdown. At that revision Phase 0 was **CONDITIONAL GO**, pending only the authoritative dev MacBook p99 run.
  - **Current-candidate performance audit (2026-08-02, local and uncommitted):**
    a release baseline at `.local/acceptance/20260802T174927Z-10764` recorded
    p50/p95/p99 frame times of 13.4/17.3/60.8 ms and 263.79 ms maximum
    mutation-to-visible latency. The audited candidate enables thin LTO with one
    codegen unit, avoids unchanged extraction/GPU scans, reuses neighbour palette
    resolution, skips model voxel scans when the palette has no model geometry,
    reuses render-queue sorting storage, uses bounded direct deflate reads, and
    removes avoidable acceptance-diagnostics work. It also replaces runtime
    SipHash maps/sets only where keys are already bounded internal identities;
    untrusted/network-key maps retain their existing hashers.
  - The environment-gated stage profiler is retained as durable attribution
    telemetry. Instrumented run
    `.local/acceptance/20260802T191930Z-2188` passed the complete Windows gate
    with p50/p95/p99 10.0/14.0/14.9 ms, 76.9395 ms maximum
    mutation-to-visible latency, zero decode errors, and drained light/mesh
    queues. A later final-source profile attributed wall time primarily to
    transparent preparation (15.17%), the transparent worker (9.09%), cave
    visibility (8.57%), and opaque queue preparation (6.38%); chunk extraction,
    GPU preparation, and render-queue application were each below 1%.
  - Two consecutive uninstrumented final-source runs
    (`20260802T213056Z-4332`, `20260802T213439Z-8848`) passed the complete
    Windows gate with p50 10.2 ms, p95 17.0-18.2 ms, p99 22.8-31.7 ms,
    maximum mutation-to-visible latency 76.9016-86.3247 ms, and zero decode
    errors. Live block mutations now retain urgent priority through lighting,
    meshing, publication, and GPU upload; changes whose old and new palettes
    have identical emission/filter semantics skip redundant relighting. Maximum
    remesh latency was 78.9989-119.2461 ms. Phase 0 remains **CONDITIONAL GO**
    pending the authoritative dev MacBook p99 run; the Windows mutation and
    decode gates are cleared by these repeated final-source runs.

**Exit criteria:** acceptance gate met; deviations documented; go/no-go written up. Everything after this phase is "build the game", with the architecture de-risked.

---

## Phase 1 — Core service (Go): productize the boundary

**Goal:** `bedrock-core` becomes a real service the client can ship: session lifecycle, auth,
control channel, conformance harness. Deliverable proof: a headless Go CLI (`corectl`) can
device-code-auth, list Realms/friends, and join any of the three transport targets through
the core — before any more Rust exists.

Scope (detailed plan to be written at phase start):
- Control channel on `control.sock`: protobuf or JSON-RPC; methods — `Status`, `StartAuth` (device-code events streamed), `SignOut`, `ListServers`, `ListRealms`, `ListFriends` (gophertunnel realms package + go-xsapi sessions; the join side of what go-mcxboxbroadcast does), `Connect{target}`, `Disconnect`; events — auth state, connection state, transfer notices, disconnect reasons.
- Session lifecycle: begin by copying Lunar's `lunar/internal/relay/relay.go` and relay tests
  into `core/internal/relay`, adapting only imports/helpers needed to make the package
  standalone. The core uses that relay logic to dial upstream (RakNet / NetherNet via Xbox
  signaling / Realms address), serve the game socket, and handle transfers. Lunar remains a
  source donor, never a module dependency.
- Resource-pack negotiation upstream; pack payloads handed to client over the control channel as files in a cache dir (client applies them — Phase 6 renders them).
- Windows transport flavor (named pipe or TCP) behind the same listener interface.
- **Conformance harness (promoted from deferral):** `tools/fixturegen` grows to full packet coverage; CI job round-trips gophertunnel↔valentine bytes both directions on every core and defs bump. This is the automated version of spike task 0.4.
- Consumer-surface work in `platform/Lunar` to expose what the core needs through the facade (measured, minimal, per AGENTS.md ABI rules).

**Early authenticated RakNet smoke (Zeqa):**

- [x] Add an explicit, ignored Microsoft token cache and optional authenticated upstream
  dial mode while preserving the offline BDS path when `-auth-cache` is omitted.
- [x] Document the exact `bedrock-core` and release `bedrock-client` commands, device-code
  stdout flow, cache privacy requirements, and Rust → local socket → Go → RakNet boundary.
- [x] Report the core's startup lifecycle synchronously: Go build start, process/auth state,
  published local endpoint, local Rust-client acceptance, and upstream connect/success/failure.
  Commit `46a4e9f` covers ordered, secret-safe logging and fatal-startup tests.
- [x] Live smoke: authenticate `bedrock-core` to `zeqa.net:19132` with
  `.local/auth/microsoft-token.json` and confirm the current release client reaches Zeqa.
- [x] Record non-secret live evidence below; never record the device code, access token,
  refresh token, or token-cache contents.

Live evidence:

- Date/time: 2026-07-11 19:17 PDT
- Authenticated upstream connection observed: yes; the authenticated protocol-1001 entry
  connection returned Zeqa's pre-login transfer to `pvp.inpvp.net:19132`, and the bounded
  core transfer follower completed the regional connection.
- Client reached Zeqa lobby/session: yes; the release client reached position
  `(-117.50, 87.62, 195.50)`, streamed `1105/5376` chunks while the count continued rising,
  and held approximately 100 FPS. A native Windows screenshot was inspected from the user
  temp directory and was not added to the repository.
- Credential hygiene (`git ls-files .local` empty; no credential material in retained logs):
  passed. The token cache remained inside the ignored `.local/` tree, its contents were never
  inspected, and the temporary device-code stdout log was removed after authentication.

This early direct-RakNet smoke does not close the phase-wide control-channel, `corectl`,
Realms, friends, NetherNet, general/post-login transfer, or sign-out work above.

Exit: `corectl join --friend <gamertag>` works from a clean machine; conformance CI green.

## Phase 2 — World rendering (textured, lit, real)

**Goal:** the spike renderer becomes the real world pipeline. Deliverable: fly through any
live server world and it *looks like Minecraft*.

Scope: block registry + block-state → model/texture mapping (generated export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome cross-check, Axolotl Valentine's versioned typed-state approach as a state-selector/catalog reference, and Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template/occlusion inputs—not render/UV authority); vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version (terrain textures, `blocks.json`, flipbooks, and biome colors) — NOTE: the pinned samples contain no block-render model JSON, so deterministic reviewed family generators combine these sources and vanilla-reference evidence; BDS `resource_packs/vanilla` is server-minimal (blocks.json + texts only), a data reference rather than the texture source; 2D texture array pages + per-layer mipmaps; greedy/culled meshing with transparency layers (opaque/cutout/blend) and per-face culling; **client-side light engine** (block + sky flood-fill, per-vertex light, day/night); biome tinting (grass/foliage/water); sky, fog, clouds; chunk streaming/eviction tied to `ChunkRadiusUpdated` + `SubChunk` request flow; block entities with custom renderers deferred (chests/signs get static models in this phase). Zuri is not a rendering or asset-system input.

**Phase 2 progress (kept current as work lands):**

- [x] **Sequential/hash third-party block identity compatibility.** Runtime
  classification derives canonical air from the validated compiled registry
  instead of the stale protocol bootstrap constant. The checked-in production
  path proves sequential air `13094` and hashed air `0xdbf44120`, rejects
  ambiguous/decoy AIR records, and covers both `WorldStream` modes. This closes
  the all-pink-air failure seen on sequential-ID third-party servers while
  preserving hashed servers.
- [x] **2.1 Local-only vanilla source and deterministic asset pipeline.** Pinned
  `bedrock-samples` provenance, Dragonfly registry export, pack parsing, bounded
  compiler, per-layer mips, versioned runtime blob, and diagnostic fallback are
  implemented; Mojang payloads remain ignored.
- [x] **2.2 Opaque full-cube render path.** Material-aware binary greedy meshing,
  exact eight-byte quads, one shared material buffer/texture array/bind group,
  vertex-pulled repeating UVs, oriented faces, and live asset selection are
  implemented. Two current-HEAD 60-second Windows radius-16 runs passed with
  p99 4.1 ms and zero errors; see `docs/phase-2-texture-slice-report.md`.
- [ ] **2.3 Close the opaque texture slice.** The deterministic named-block BDS
  gallery now passes with all faces/log axes, greedy repetition, mips, supported
  and diagnostic cases recorded, and the clean no-assets full gate passes. The
  fail-closed material path, local relay 1,600-packet ceiling, and deterministic
  inbound/command network arbitration are implemented and independently reviewed.
  A 2026-07-11 interactive radius-16 run at `00b7a32` reached world-ready with
  zero missing mappings, but is diagnostic rather than acceptance evidence:
  849,117 rendered quads used material zero across 9,040 resident/7,093 visible
  subchunks, and exact inspection of blob SHA-256
  `1fbd361c489d3cf90edb49c0056b83ffd9a2a114a36ac1eaf28cfd1103ecf508`
  found only 661 of 16,913 registry visuals
  mapped to real materials. Evidence is in
  `.local/acceptance/20260711T192110Z-16912/app.stdout.log`. Most of that visible gap
  belongs to Tasks 2.4–2.7 (leaves, tint/grass, water/blend, and models). The exact
  two-second teleport/full-view remesh gate and fresh combined RSS/steady-CPU
  evidence remain open; close those findings before completing Task 8.
- [ ] **2.4 Cutout cube materials and leaves.** Preserve independent geometry,
  occlusion, and cave-connectivity semantics; keep the packed subchunk/quad and
  shared GPU architecture. Tasks 1–4/5 are complete at `f768cfa`, `4d23356`,
  `f33b71c`, and `8391a58`: the versioned
  registry now exports independent air, cube-geometry, full-face-occlusion,
  and leaf-model facts with exact pinned counts, and leaf-only cutout materials
  now use coverage-preserving per-layer mips. Palette-native `u64` meshing now
  applies ordered leaf/opaque culling and non-occluder cave connectivity without
  widening the eight-byte quad. The existing single opaque shader now applies
  bit-8 alpha cutout with depth writes and no blending. No Mojang payload is
  tracked. The deterministic live-evidence task remains open.
- [ ] **2.5 Biome palettes and tinting.** `P2.5-NATIVE-BIOME` Decode/store biome data and apply
  grass/foliage/water tint without widening the eight-byte quad record.
  - [x] Palette-native v1001 biome storage/column decoding, including padded
    Bedrock words, `0xff` previous-storage reuse, strict malformed-input
    rejection, atomic inline block+biome commits, and biome-only column
    lifetime independent of all-air block subchunks.
  - [x] Carry request-mode and inline `LevelChunk` biome payloads through the
    Rayon/FIFO streaming path, decode the full dimension column independently
    of requested block count, and commit it before subchunk requests.
  - [x] Retain the live biome definition mapping needed to resolve palette IDs
    to climate and vanilla tint rules, including bounded custom-biome fallback.
  - [x] Remove the grass-block diagnostic fallback: compile bottom/top/side
    independently, preserve grass-side alpha as an opaque tint mask through
    mip generation, and apply the pinned pack's deterministic default grass
    tint until live per-biome color lookup replaces it.
  - [x] Compile grass/foliage/water tint classifications and biome color rules,
    upload palette-native biome/tint tables, and apply them in the chunk shader
    without widening the eight-byte quad record. Grass plus generic/birch/
    evergreen/dry foliage are now resolved from `MCBEAS03`, revision-gated,
    and applied palette-natively. **Complete (2026-07-13):** Task 13's real
    animated water route applies the live palette-native water tint in the
    liquid shader without widening the eight-byte cube quad. Native run
    `20260712T203607Z-7596` proved five runtime water tints, consecutive exact
    GPU witnesses, generation 518 presented, p99 14.0 ms, and zero decode
    errors; fresh assets/render/client focused suites remain green.
  - [ ] Add vanilla-reference biome tint blending for grass, foliage, and
    water. Determine the matching Bedrock radius/kernel from native reference
    evidence, sample the bounded palette-native biome neighbourhood across
    chunk boundaries without flattening columns, preserve special foliage
    rules and custom-biome fallback, and cover abrupt-boundary, missing-neighbour,
    teleport/eviction, GPU, and performance cases.
    **Bounded implementation landed; parity evidence remains open (2026-07-14):**
    grass, generic foliage, and water now share a radius-one horizontal 3x3
    linear-colour box blend across self-contained palette-native neighbour
    snapshots. The descriptor deduplicates equal packed payloads, clamps a
    missing neighbour to the center's nearest edge, records a uniform fast path,
    retains direct birch/evergreen/dry-foliage selection, and validates all nine
    immutable source identities before publication so neighbour replacement,
    eviction, and teleport churn fail stale. Request-mode biome-only commits now
    dirty resident cross-column consumers. The eight-byte cube quad and packed
    Bedrock storages remain unchanged; a uniform record is 52 bytes, or
    1,359,072 bytes for 33x33x24 subchunks at radius 16, while the exact
    descriptor ceiling and the existing GPU arena cap bound adversarial palettes.
    The 3x3 kernel is explicitly provisional because no reviewed native Bedrock
    radius/weight evidence was available; keep this checkbox open until a native
    abrupt-boundary reference fixes or confirms the kernel and the live
    performance/visual gate passes.
    **Committed diagnostic contract complete (2026-07-16):** the provisional
    radius, nine-sample count, and denominator are now one explicit CPU/WGSL
    contract over fixed-size packed records, with cross-chunk diagonal samples
    and exact missing-neighbour edge clamping. Acceptance-only
    `BIOME_BLEND_COMMITTED stage=app_committed` telemetry is sourced from the
    immutable camera-subchunk `ChunkRenderInstance` record after world apply and
    binds its key, chunk generation, tint identity/revision, full-record hash,
    local coordinate, and exact sample set. Unchanged identities are deduplicated
    so stationary runs cannot grow logs. This is deliberately not labeled
    GPU-presented evidence. Independent review and full affected tests/strict
    checks are green through merge `a0f08d9`; the native abrupt-boundary and live
    performance/visual adjudication remain the only completion gate.
- [ ] **2.6 Static/non-cube models, blend/water, and flipbooks.** Complete the
  remaining block visual classes and animation path per
  `docs/superpowers/specs/2026-07-11-phase-2-6-noncube-water-design.md`.
  - [x] Pin and securely acquire the exact local-only PMMP, PrismarineJS,
    Axolotl, and Dragonfly evidence bundle. Whole-bundle atomic publication,
    byte/hash/time bounds, junction rejection, concurrent-winner handling,
    exact license notices, and the no-tracked-payload contract are complete at
    `c44de03`.
  - [x] Preserve complete bounded flipbook metadata and compile the real pinned
    pack's physical frames into deterministic page-aware staging data without
    changing the v3 runtime schema. Commits `143c68d` and `e6e49e1` cover 83
    animations, 1,209 physical frames, 1,323 timeline references, and 1,901
    deduplicated layers on one 2,048-layer page.
  - [x] Export and strictly decode `BREG1003` typed model/contributor/selectors,
    face coverage, collision seeds, and per-state provenance at `e58d083`.
    PMMP/Dragonfly/Prismarine form a full 16,913-state/1,356-name bijection;
    Valentine is an exact ordered 15,845-state subset with 1,068 attributable
    missing states across 35 wholly absent names and zero extra/mismatched
    states. The deterministic registry SHA-256 is
    `3669be82850824af8592276afe864d903495e743b8af81dfcf1d3aa1586231a4`.
  - [x] Version the bounded runtime asset schema to `MCBEAS04`; compile the
    typed registry selectors, template tables, page-aware flipbook data, and
    attributable per-family diagnostics without committing Mojang payloads.
  - [x] Upload the bounded one/two-page `MCBEAS04` texture resource, immutable
    material/animation/frame tables, and a stable 16-byte animation clock in
    one shared chunk bind group. Commit `a30a0ef` adds page-aware current/next
    frame selection, cross-page interpolation and wraparound, a real diagnostic
    second-page fallback, atomic asset-revision replacement, derivative-safe
    WGSL sampling, and no per-frame texture upload; 82 render tests, strict
    Clippy, and independent spec/quality review are green.
  - [x] Generalize bounded chunk rendering to named cube, model,
    model-lighting, liquid, and liquid-lighting streams while preserving the
    eight-byte greedy cube record. Commit `5734872` adds exact combined byte
    accounting, one consolidated word-addressed geometry arena, transactional
    all-stream allocation/rollback/retry, generation/tint gates, expected/drawn
    presentation masks, and identical direct/MDI addressing. The projected
    vertex storage-binding count including future templates is seven of the
    common minimum eight; 89 render tests, strict Clippy, and independent
    re-review are green.
  - [x] Produce stable eight-byte face-specific model/liquid lighting sidecars
    from a palette-native center-plus-26-neighbour snapshot. Commit `8b5c5a6`
    bakes exact Phase 2.6 block-light 0 / sky-light 15 values plus per-vertex AO,
    registers generation-scoped diagonal AO/liquid dependency masks, and covers
    inline columns, known-air replacement, stale rejection, and conservative
    unknown targets. Render 93/93, world 51/51, client 187/187, strict combined
    Clippy, and independent re-review are green; Phase 2.7 will replace only the
    light inputs, not this format or addressing.
  - [x] Add palette-native multi-layer contributor resolution, retaining the
    eight-byte greedy cube record and adding compact model/liquid streams with
    atomic queue/GPU generation accounting and direct/MDI parity. Task 10 now
    resolves up to 16 packed storage layers without a flat 4,096-block array,
    fails closed on contributor conflicts, and retains simultaneous primary and
    liquid contributors. All three seagrass states and all 26 kelp ages compile
    with exact animated material identities; kelp head/body selection is driven
    only by the primary block above, including across subchunk boundaries. The
    deterministic 29-state BDS water-tank gallery passed from both directions
    at current HEAD with zero target diagnostics/decode errors, p99 15.5 ms,
    377,843,712-byte peak combined RSS, 5.78% mean combined CPU, and native temp
    screenshots confirming real green cutout models. Water geometry remains
    deliberately invisible until Tasks 11–13.
  - [x] Track the exact bounded liquid mesh neighbourhood and invalidation set.
    Task 11 keeps the shared palette-native `world::MeshNeighbourhood` as the
    render API boundary, exposes the deduplicated 23-subchunk liquid sample set
    (current/upper horizontal 3x3 plus lower center and four cardinals),
    applies its checked inverse for liquid-only diagonal dirtying, preserves
    ordinary six-face cube invalidation, coalesces duplicate rapid updates, and
    rejects stale dependency masks. World, client, formatting, strict Clippy,
    and independent review are green; no flat block or whole-column snapshot was
    introduced.
  - [x] Compile crossed cutout plants/crops with exact variants and biome tint;
    compile all physical flipbook frames into texture-array layers and animate
    them from immutable descriptors without per-frame texture uploads. Commit
    `a24370b` covers all 443 terrestrial Cross/Crop states (279 Cross, 164 Crop)
    with zero diagnostics, reusable two-quad templates, compact model refs,
    face-specific lighting, a shared bounded/no-cull direct+MDI GPU path, and an
    exhaustive hash-bound gallery; assets 112, render 102, world 51, client 187,
    acceptance, strict Clippy, and final independent re-review are green.
    Wheat's later farmland reproduction exposed the generic cross as incorrect:
    its eight stages now use four native quarter-position rows with a -1/16 Y
    offset and their original pinned sprites. Compiler and subchunk-boundary
    regressions pass; see `docs/reference/farmland-rendering.md`. This does not
    establish native geometry parity for the other generic Crop families.
  - [x] Mesh animated, biome-tinted water from the shared bounded palette
    snapshot. Task 12 preserves all 16 water depth/falling states, vanilla-like
    weighted four-corner surfaces, diagonal and cross-subchunk influence,
    same-water/solid culling, clipped sides and bottoms, signed flow gradients,
    waterlogging, still/flow per-face materials, stable stream order, and one
    face-specific eight-byte lighting sidecar per 16-byte liquid quad. The
    liquid dependency set was corrected to the exact 23 samples needed by
    lower-cardinal waterfall flow. Only all-face alpha-blended, water-tinted
    families enter this stream; lava remains attributable diagnostic until its
    Task 19 depth-writing route. Seventeen liquid integration tests, full locked
    affected-crate suites, exact strict Clippy, real 16,913-visual asset compile,
    and final independent re-review are green. Water remains deliberately
    invisible until Task 13 installs the transparent GPU path.
  - [x] Mesh animated, biome-tinted water with same-liquid culling, vanilla-like
    corner heights, diagonal invalidation, and a correctly ordered transparent
    phase with depth testing and no depth writes. Its deterministic BDS gallery
    requires one real water tint referenced by the committed/presented liquid
    snapshot (BDS commands cannot assign fixture biomes); a separate end-to-end
    app integration test proves two raw biome IDs with distinct map-water
    colours preserve dense lookup and distinct renderer tint records. Water
    visibility churn and generation-only remeshes retain the last ordered
    snapshot only while every absolute address stays physically resident: same
    key/metadata, active tint, valid lighting, and a same-start liquid range
    containing the old range. Moved/shrunk ranges, eviction, metadata reuse, or
    asset/tint replacement use bounded copy-on-write quarantine: retired spans
    remain drawable and unreusable until an empty-or-nonempty replacement frame
    is submitted and its independent GPU retirement epoch completes. Cap
    exhaustion backpressures the update/removal rather than risking stale reads
    or a blank transparent frame.
    Gallery freezes its 60-second duration/frame metrics at the original
    deadline, then permits at most two unmeasured seconds for a non-empty
    committed=encoded=GPU-presented transparent generation; timeout is a
    logged nonzero failure. Its manifested p99 frame-time gate is exactly
    1000/60 ms.
    **Task 13 complete (2026-07-12):** after fixing bounded GPU-upload
    starvation and four-word liquid-stream arena alignment, exact four-key GPU
    witnesses passed repeatedly across `WaterGalleryFront` and
    `WaterGalleryBack`. Commit `38c1f5d` moved the block-constant packed biome
    tint lookup from the fill-heavy liquid fragment stage to a flat vertex
    varying without changing tint, alpha, ordering, uploads, or lifecycle
    bounds. Native run `20260712T203607Z-7596` froze exactly 60 seconds and
    passed at p99 14.0 ms (limit 16.667 ms), with 38,214 transparent refs,
    five runtime water tints, consecutive exact four-key GPU witnesses,
    request=result=committed=encoded=presented generation 518, zero ceiling
    rejects, zero decode errors, radius 16, 414,187,520-byte peak combined RSS,
    and 6.438% mean combined CPU. Full render/client tests, strict Clippy,
    acceptance tests, and independent re-review through test-hardening commit
    `ba3ea3f` are green with no findings.
    **Camera-motion regression closed (2026-07-13):** exact floating-point view
    keys no longer discard a safe same-address transparent snapshot midway
    through its bounded inactive-slot upload. The staged generation now commits
    atomically before the latest pose is requested, while allocation, asset,
    tint, or stream-address changes still cancel immediately. This prevents
    newly streamed water from starving and appearing or disappearing with
    camera movement (`a4f7da5`; regression-first test, full 278-test render
    suite, strict Clippy/formatting, and diff check green). A fresh native BDS
    camera-motion capture is required below before closing live evidence.
    **Native rerun integration follow-up (2026-07-13):** the first DX12 water
    rerun exposed two independent GPU setup defects introduced by the new
    transparent-model path. The opaque packed-model pipeline now explicitly
    selects `fragment` after `model.wgsl` gained the separate
    `fragment_blend` entry point. Debug DX12 also uses the equivalent direct
    cube/model/depth-liquid draw path because wgpu 27's indirect validator
    expands a 20-byte indexed command to 32 bytes for D3D12 special constants
    while its debug batching assertion still advances by 20; release DX12 and
    unaffected backends retain multi-draw indirect. Both failures have
    regression-first coverage, the full 279-test render suite and strict
    Clippy/formatting are green, and independent review found no blocking
    issues. The corrected native run stayed alive and continuously committed
    transparent snapshots through generation 187 / 31,915 refs instead of
    panicking or starving. It still timed out at the 180-second fixture-ready
    marker while loading 7,934/9,132 debug-path subchunks, and the required
    fresh GDI capture remained pure black under the already-isolated RX 570
    Bevy/wgpu presentation failure, so this is correctness evidence rather
    than visual closure.
  - [ ] Add compact static templates in impact order: slabs/stairs,
    wall-attached vines/lichen/sculk-vein and related thin faces,
    doors/trapdoors, connection-aware panes/fences/gates, then static
    chest/sign models; retain conservative culling/connectivity for partial
    models until exact face-coverage optimization is separately verified.
    - [x] Slab asset templates: all 272 BREG1003 bottom/top/double states now
      compile through opaque six-quad packed model templates with exact
      face-specific materials, UV crops, boundary cull flags, deterministic
      deduplication, and zero pinned-pack slab diagnostics (`c64330b`; assets
      tests, strict Clippy/formatting, and independent review green).
    - [x] Slab packed rendering and occlusion: lower/upper/double slabs remain
      compact model references with six lighting sidecars and no cube stream;
      double slabs provide full-face cave/cull occlusion while partial slabs
      remain conservatively cave-open. Internal and all six cross-subchunk
      model/cube boundaries are covered without lighting reindexing
      (`6df380d`, `09279a1`; 122 asset tests, 44 render-mesh tests, strict
      Clippy/formatting, and independent review green). Gallery acceptance
      remains part of Task 14 before the parent item can close.
    - [x] Stair templates and neighbor-derived straight/inner/outer selection:
      all 512 BREG1003 states across 64 names compile through compact five-shape
      groups per material/upside signature, with exact S/W/N/E transforms,
      both Dragonfly side-isolation guards, same-half matching, all four
      horizontal cross-subchunk boundaries, selected-template lighting, and
      conservative cave connectivity (`859fb13`, `0475516`, `e1732eb`,
      `1766a56`, `469695b`). The exact pinned pack has zero stair diagnostics;
      real-pack assets/render tests, the 43-witness/five-pose deterministic
      gallery, strict MCBEAS04 integrity/tamper gates, full PowerShell dry-run
      acceptance, strict Clippy/formatting, and independent re-review are green.
    - [x] Door and trapdoor templates: all 672 door states and 336 trapdoor
      states compile through compact six-quad alpha-cutout cuboids with exact
      typed open/orientation/hinge/half selection, 3/16-block thickness,
      lower/upper door materials, conservative partial-model culling, and
      deterministic template reuse. Legacy oak-through-iron door texture arrays
      and modern bamboo/cherry/mangrove/pale-oak/nether/copper/waxed aliases are
      covered. Dragonfly's rotated door-state encoding is inverted before its
      logical-facing/open-hinge transform; an independent review caught and
      corrected the initial direct-orientation interpretation before push.
      Missing or out-of-range selectors fail closed, collision-only seeds are
      not used as render authority, and the real-pack exhaustive gate removes
      exactly 1,008 diagnostics with no additions (`1a69fca`; 145 assets tests,
      strict Clippy/formatting, full 16,913-state ratchet, and independent review
      green). Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [x] Connected wall templates: all 5,184 states across 32 wall materials
      decode the exact 9-bit north/east/south/west none/short/tall selector and
      center-post bit into deterministic zero-to-30-quad packed models. Visible
      bounds come from the local vanilla `template_wall_post`,
      `template_wall_side`, and `template_wall_side_tall` render models rather
      than Dragonfly/Prismarine collision boxes: post-off states omit the post,
      short arms reach 14/16, tall arms reach full height, and UV projection
      follows the vanilla UV-locked blockstate contract. Invalid selectors fail
      closed, partial-model culling remains conservative, collision-seed removal
      leaves output byte-identical, and the real-pack ratchet removes exactly
      5,184 diagnostics with no additions (`09ba163`; 148 assets tests, strict
      Clippy/formatting, full 16,913-state ratchet, and independent correction
      review green). Deterministic gallery/native GPU evidence remains in the
      shared residual-family live gate.
    - [x] Pressure-plate templates and typed pressed selector: BREG1003 now
      preserves `redstone_signal` only for the 256 pressure-plate states as an
      explicit unpressed/pressed flag, without affecting redstone wire or any
      other record. All 16 material families compile through two deterministic
      opaque templates using the vanilla `pressure_plate_up/down` bounds and
      exact UV crops, including the pressed model's half-texel side strip;
      missing/invalid selectors fail closed and collision data is not render
      authority. The selector-only registry regeneration is byte-reproducible,
      changes exactly those 256 records, and the real-pack ratchet removes all
      256 pressure-plate diagnostics with no additions (`4c83afd`; registry SHA
      `fda4b40335c24b0019049ce572668b03f8ddb9a705de88abb4d724aa7ff81106`,
      152 assets tests, 23 strict-coverage tests with one real-blob gate ignored
      by default, registrygen tests/vet, strict Clippy/formatting, and independent
      correction review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Fence-gate templates and bounded compound model references: all 192
      states across 12 materials require the exact typed `Orientation`, `Open`,
      and in-wall flag mask, fail closed on missing, invalid, or additional
      selectors, and compile from the vanilla render-model oracle rather than
      collision boxes. Closed/open and normal/in-wall forms preserve exact
      UV-locked geometry; bamboo uses its distinct custom 38/40-quad topology
      and reversed/rotated UV rectangles. Because exact gates exceed the
      existing 32-quad mask, one visual now selects a validated pair of
      consecutive 24+16 (bamboo closed 22+16) templates, emitted as two
      independent packed model references with contiguous lighting and draw
      records while preserving the 16-byte reference, `u32` visible mask,
      MCBEAS04 field widths, and GPU shader contract. Encoder/runtime trust
      boundaries reject empty, truncated, nested, incompatible, or directly
      referenced continuations. The production 16,913-state ratchet removes
      exactly 192 gate diagnostics with no additions and now holds 8,301
      diagnostics including air (`f4bcfe0`, `1aaf952`; full assets/render and
      visualcoverage suites, strict Clippy/formatting, real pinned-pack run, and
      independent review/re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Connection-aware pane and fence templates plus transparent model
      streaming: all 43 pane/bar states select one of 16 exact post-and-arm
      masks, and all 13 fence states select compact post plus connection-arm
      templates while preserving wood/nether connection classes. Internal and
      all four horizontal cross-subchunk seams suppress only true pane joins;
      fences connect to full occluders, matching fences, and only the sides of
      axis-aligned gates. Mixed connected-template flags fail closed. Alpha
      admission is descriptor-scoped so stained panes retain blend materials
      without accidentally admitting full stained-glass cubes that share the
      same texture path; reviewed beacon and liquid routes remain intact.
      Alpha-blended model quads now reuse the same packed model references and
      lighting sidecars but enter a dedicated no-depth-write phase, sorted
      back-to-front by retained view and face. Sorting runs through a
      latest-wins Rayon worker cache keyed by exact CPU/GPU generation and
      stream identity; water and model uploads share one whole-subchunk,
      per-frame transparent-reference budget. The production 16,913-state
      ratchet removes exactly 56 diagnostics with zero additions and leaves
      8,066 diagnostics including air (`a2c3a5a`, `5024f21`; full assets/render
      suites, strict Clippy/formatting, pinned-pack ratchet, and independent
      review/re-review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Carpet and stateful pale-moss-carpet templates: all 17 ordinary
      stateless carpets compile as exact opaque 1/16-block cuboids with the
      pinned wool/moss aliases, while all 162 pale-moss states enforce the
      exact four `none`/`short`/`tall` side properties and upper-bit contract.
      Pale bases stay opaque; side planes use the pinned two-entry cutout pair
      in its verified tall/short order, render two-sided with conservative
      connectivity, preserve the isolated-upper base-plus-four-tall special
      case, and quantize vanilla's unrepresentable 1.6/256 inset symmetrically
      to 2/254. Missing, invalid, extra, or mismatched typed selectors fail
      closed; collision seeds do not affect rendering. The production ratchet
      removes exactly 179 carpet diagnostics with no additions and now holds
      8,122 diagnostics including air (`8087b6a`, `9323093`, `9e99a5e`; exact
      opposing and direction-specific Java UV corner orders, two byte-identical
      pinned builds, full assets/visualcoverage suites, renderer regressions,
      strict workspace Clippy/formatting, and independent final re-review green).
      Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [x] Button templates and exact wall UV locking: all 168 states across 14
      materials enforce the exact `Orientation` plus pressed-flag mask and
      canonical schema, fail closed on missing/extra/invalid selectors, and map
      Bedrock's six outward-facing values to deterministic floor, ceiling, and
      four wall transforms. Unpressed and pressed forms use the exact vanilla
      bounds and face rectangles, with the unrepresentable 1.02-pixel pressed
      depth deliberately quantized to one pixel. Wall faces derive UV-locked
      rectangles from rotated target bounds; independent literal six-face
      goldens cover all four directions and both pressed states after review
      caught and corrected the initial source-space projection. Materials stay
      opaque, partial models advertise no boundary culling/coverage, and
      collision seeds are not render authority. The production ratchet removes
      exactly 168 button diagnostics with no additions and now holds 7,898
      diagnostics including air after integrating the already-landed 56-state
      pane/fence tranche (`8b427eb`, `fe55779`; deterministic pinned
      builds, full assets/render/visualcoverage suites, strict Clippy/formatting,
      and independent final re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Canonical huge-mushroom cube states: all 48 states across brown
      mushroom blocks, red mushroom blocks, and mushroom stems now select the
      pinned pack's exact six-face material aliases from the canonical tagged
      `huge_mushroom_bits` integer. Missing, extra, untagged, mistyped,
      noncanonical, or out-of-range selectors fail closed. The focused
      production-pack gate preserves diagnostics for all 43 legacy flags-zero
      cube records, all 25 stained-glass/copper-grate/slime transparency-family
      cubes, and `minecraft:invisible_bedrock`; record reordering remains
      byte-deterministic. The production ratchet removes exactly 48 intended
      diagnostics with zero additions. After integrating the already-landed
      128-state glow-lichen/sculk-vein tranche, that historical checkpoint held
      7,722 diagnostics including air (full assets/visualcoverage suites, pinned
      compiler tests, strict Clippy/formatting, and zero-delta refreshed ratchet
      green).
    - [x] Ordinary stained-glass cubes: the exact stateless
      `minecraft:black_stained_glass`, `minecraft:blue_stained_glass`,
      `minecraft:brown_stained_glass`, `minecraft:cyan_stained_glass`,
      `minecraft:gray_stained_glass`, `minecraft:green_stained_glass`,
      `minecraft:light_blue_stained_glass`,
      `minecraft:light_gray_stained_glass`, `minecraft:lime_stained_glass`,
      `minecraft:magenta_stained_glass`, `minecraft:orange_stained_glass`,
      `minecraft:pink_stained_glass`, `minecraft:purple_stained_glass`,
      `minecraft:red_stained_glass`, `minecraft:white_stained_glass`, and
      `minecraft:yellow_stained_glass` records now render as alpha-blended
      six-quad models. Palette-native meshing suppresses a shared face only for
      an equal six-face material identity under the checked transparent-cube
      semantic, preserves both cross-colour boundary faces, culls glass behind
      full opaque neighbours without hiding the opaque face, stays cave-open,
      and applies across all six subchunk boundaries. The production ratchet
      removes exactly these 16 IDs with zero additions, leaving 7,706
      diagnostics and 7,235 cumulative removals; the ignored integrated blob is
      SHA-256
      `61025bb3e8e1b9ca0d5e2ec1cd7847433333a20f99948c6193fbb370a0d4900f`.
    - [x] Copper grates: the exact stateless `minecraft:copper_grate`,
      `minecraft:exposed_copper_grate`, `minecraft:weathered_copper_grate`,
      `minecraft:oxidized_copper_grate`, `minecraft:waxed_copper_grate`,
      `minecraft:waxed_exposed_copper_grate`,
      `minecraft:waxed_weathered_copper_grate`, and
      `minecraft:waxed_oxidized_copper_grate` records now use homogeneous
      alpha-cutout six-quad transparent-cube templates. Waxed variants retain
      the exact unwaxed face-material aliases, while shared-face culling uses
      exact network identity so wax and oxidation boundaries remain visible in
      sequential and hashed modes. Grates stay cave-open, route only through
      ordinary depth-writing model draws, cull against identical states across
      all six subchunk boundaries, and preserve opaque-neighbour asymmetry.
      Slime, stained glass, panes, copper bars/bulbs/doors/trapdoors,
      unrelated grate names, legacy flags-zero records, and
      `minecraft:invisible_bedrock` remain outside this admission. The
      production ratchet removes exactly eight IDs with zero additions, leaving
      7,698 diagnostics and 7,243 cumulative removals; the ignored integrated
      blob SHA-256 is
      `20cd1b4301f40736468a3249acf21fdea0544d74fa238d8faae04aaee1af9940`.
    - [x] Chiseled bookshelves: all 256 canonical
      `minecraft:chiseled_bookshelf` states (sequential IDs 1,605–1,860) now
      compile from the exact `books_stored:int 0..63 × direction:int 0..3`
      product into 64 immutable north-facing templates and four opaque source
      materials. Each template has five ordinary full faces plus six coplanar
      front-slot quads; native 1.26.33.1 evidence fixes bit order to top-left,
      top-middle, top-right, bottom-left, bottom-middle, bottom-right and fixes
      directions 0/1/2/3 to south/west/north/east. Exact pair/static terrain
      access, unit collision, flags, face coverage, typed state, ID formula,
      and complete-family cardinality all fail closed. Ordinary and six-quad
      front faces cull across every subchunk boundary, the full model closes
      cave connectivity, and the dense-subchunk fixture emits exactly 1,352
      model refs and 2,816 visible quad refs with stable 11-record lighting
      spans. The exact production ratchet removes only IDs 1,605–1,860 with
      zero additions, leaving 2,570 diagnostics including air. Registry SHA-256
      is `3e0a67718b6368d8b5f7755e9e49a1241233f21bcea8724a9163febb4f1b1d92`;
      the ignored compiled pack SHA-256 is
      `df82f3408ee5805bcd536a484b6d0e8831eb972d76225c17eda005695e4d982c`.
      - [ ] Live presentation acceptance: capture Cinnabar from the matching
        native-gallery viewpoints and require two consecutive exact
        GPU-completed model-stream witnesses. Keep both native and Cinnabar
        screenshots local-only; do not commit Mojang-derived imagery.
    - [x] Resin clumps: all 64 canonical `minecraft:resin_clump` states
      (sequential IDs 2,930–2,993) now require the exact typed
      `multi_face_direction_bits:int 0..63` product, formula IDs, empty flags and
      face coverage, empty collision, and the exact scalar/static
      `resin_clump` texture route. Native 1.26.33.1 support-removal/readback
      fixes bits 1/2/4/8/16/32 to down/up/south/west/north/east, matches the
      glow-lichen UV projection, and proves that a written zero mask reloads as
      63. The compiler preserves every protocol record while aliasing mask 0 to
      mask 63, emitting one static alpha-cutout material, 63 templates, and 192
      quads. Sequential and hashed mesh gates cover every mask, all six
      boundaries, cave openness, opaque-support visibility, layered water, and
      the dense 4,096-reference/24,576-draw-light bound. The exact production
      ratchet removes only IDs 2,930–2,993 with zero additions, leaving 2,506
      diagnostics including air. Registry SHA-256 is
      `33a31ec89a04fe638a4f59ab315561c1c0d897e04f2041d5643262d3de56d30c`;
      the ignored compiled pack SHA-256 is
      `91998c61a9f8c40a72e73e45167d7448e9ad18271b561bc61f8d839584603e19`.
      - [ ] Live presentation acceptance: reproduce the native resin viewpoints
        in Cinnabar and require two consecutive exact GPU-completed model-stream
        witnesses. Native and Cinnabar screenshots remain local-only.
    - [x] Reviewed selector-alias opaque cubes: validate all 38 records in the
      complete hay, bone, quartz-block, smooth-quartz, chiseled-quartz, purpur,
      and TNT products, then promote exactly 27 compatibility states. Exact
      typed wrappers, raw keys, values, formula IDs, Primary/Cube facts, shape 1
      CollisionOnly unit bounds, complete-product cardinality, and exact
      static/non-tinted/opaque vanilla pack descriptors fail closed. Native
      1.26.33.1 evidence preserves Y caps down/up, X caps west/east, Z caps
      north/south, and a quarter-turn on all four non-cap faces for X/Z;
      `deprecated=0..3` and `explode_bit=0..1` are static visual aliases.
      Sequential/hash rendering covers every state, all six cross-subchunk
      culls, dense six-quad greedy output, cave closure, and zero model,
      transparent, and liquid streams. The exact production ratchet removes
      only IDs 2,908-2,910, 2,912-2,914, 2,916-2,918, 5,443-5,444,
      6,466-6,468, 6,470-6,472, 6,474-6,476, 7,082-7,083, 13,113,
      14,686-14,687, and 15,345-15,346 with zero additions, leaving 2,479
      diagnostics including air. Registry SHA-256 is
      `9f67a14d73cf958b53557cc31c601168aa0eb95c5d46dfac1299f8412a0cb74f`;
      ignored compiled-pack SHA-256 is
      `18a4718d6fd03a66c0eb30e0a28444dcf80159c658cf4f7712e5ff342f7740ca`.
      - [ ] Live presentation acceptance: reproduce the matching native axis,
        TNT, and deprecated-state viewpoints and require two consecutive exact
        GPU-completed cube-stream witnesses. Screenshots remain local-only.
    - [x] Exact cactus family implementation: all 16 canonical
      `minecraft:cactus` states (sequential IDs 13,606-13,621) now require the
      complete exact `age:int 0..15` product, formula IDs, Primary/Cuboid
      ownership, empty flags and face coverage, exact shape-84 collision, and
      exact static side/down/up pack routes. Native 1.26.33.1 evidence fixes the
      visible X/Z inset to 1/16, full Y height, and side UV crop to source
      columns 1..14. Every age reuses one six-quad template and three static
      alpha-cutout materials. Sequential/hash, all-boundary, opaque-adjacency,
      cave-open, additional-water, dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,479 -> 2,463
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce the matching native cactus
        overview, stack, grazing, and top-inset viewpoints in Cinnabar and
        require two consecutive exact GPU-completed model-stream witnesses with
        stable generation/ref counts and zero contamination counters.
    - [x] Exact cake family implementation: all seven canonical
      `minecraft:cake` states (sequential IDs 14,055-14,061) now require the
      complete exact `bite_counter:int 0..6` product, formula IDs,
      Primary/Cuboid ownership, empty flags/coverage, exact collision shapes
      89-95, exact six-face block routing, and literal untinted terrain pairs.
      Native 1.26.33.1 evidence fixes west as the advancing cut plane and binds
      bite zero to `cake_side` versus bites one through six to `cake_inner`.
      Seven immutable six-quad templates use
      `[16+32*b,0,16]..[240,128,240]`; sequential/hash, all-boundary,
      opaque-adjacency, cave-open, additional-water, dense
      4,096-reference/24,576-draw-light, deterministic-registry,
      deterministic-pack, and exact 2,463 -> 2,456 visual-coverage gates are
      green. No Mojang payload or screenshot is tracked.
      - [ ] Live presentation acceptance: reproduce representative bite 0, 1,
        and 6 native viewpoints in Cinnabar and require two consecutive exact
        GPU-completed model-stream witnesses with stable generation/ref counts
        and zero contamination counters.
    - [x] Exact farmland family implementation: all eight canonical
      `minecraft:farmland` states now require the
      complete exact `moisturized_amount:int 0..7` product and unique identities
      from the target registry (see `docs/reference/farmland-rendering.md`),
      Primary/Cuboid ownership, empty flags/coverage, exact shape-43 collision,
      and literal untinted side/top routes. Native 1.26.33.1 evidence binds
      amount zero to dry terrain-array index 1 and amounts one through seven to
      wet index 0. Two immutable six-quad templates use full X/Z and 15/16 Y;
      sequential/hash, all-boundary, opaque-adjacency, cave-open,
      additional-water, uniform/mixed dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,456 -> 2,448
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce representative moisture 0,
        1, and 7 native viewpoints in Cinnabar and require two consecutive
        exact GPU-completed model-stream witnesses with stable generation/ref
        counts and zero contamination counters.
        The October 4 macOS Metal manual check confirms that farmland and the
        corrected wheat model render correctly; the user approved this fix.
    - [ ] Slab/stair native and packed-GPU live acceptance: capture all five
      fixed Cinnabar poses through native `%TEMP%` screenshots and require two
      consecutive exact GPU-completed model-stream witnesses. Automated gallery
      construction is complete. The Top pose now reaches two consecutive exact
      7-key GPU witnesses with 276 model references and zero missing, stale,
      wrong-stream, zero-reference, or draw-mismatch counters after moving the
      camera teleport ahead of the synthetic fixture-update flood. The five
      inspectable native captures and a clean performance-gate run remain open;
      the first repaired live run was rejected at 138.3439 ms mutation-to-visible
      against the 100 ms gate. Audit found that the already-complete exact model
      witness remained armed throughout the later timed session, rebuilding a
      full frame probe over thousands of instances every frame. The probe now
      disarms immediately after its exact two-frame pair, and gallery publication
      now waits for an exact Rust-side committed-camera marker before sending the
      fixture-update flood. Unit, full-workspace, acceptance dry-run, and runtime
      safety regressions are green; a fresh native five-pose rerun remains open.
      North run `20260714T011758Z-840` then passed its exact camera fence,
      77-command result fence, and consecutive GPU model witnesses (sequences
      907/908, seven keys, 277 refs, all contamination counters zero), but the
      timed gate still failed at p50 41.7 ms / p99 47.6 ms and 140.1161 ms
      mutation-to-visible. Its 55-second camera delay was a bounded Rust ingress
      bottleneck (four queued packets and eight admissions per rendered frame),
      not relay reordering; the channel and per-frame admission window are now
      coherently 32, matching the existing heavy-event cap while preserving FIFO
      order and decode/mesh worker budgets. GPU cost and fresh native visual
      evidence remain open. The first GPU-side correction now rejects all padded
      and neighbour-masked slots in the model vertex stage before template,
      lighting, texture, tint, and fragment work; the fixed 32-quad/reference
      storage contract remains bounded while a live A/B measures the reduction.
      Optimized North run `20260714T013915Z-6480` reduced teleport
      acknowledgement-to-ingress/commit to 8.18 seconds (sequence 2,128, with
      ingress and commit in the same update) and again passed the exact model
      witness. Vertex culling improved p50 from 41.7 to 39.6 ms despite 8,699
      versus 5,679 resident subchunks, but p99 remained 47.7 ms and the 100 ms
      mutation gate still failed at 139.9718 ms. Structural exact-count model
      drawing is now complete in `fcb1989` and ownership-hardening `b07e194`:
      one exact eight-byte visible-quad indirection record replaces the fixed
      32-quad vertex launch while preserving 16-byte model refs, ordered
      lighting, one direct/MDI command per subchunk, arena/COW bounds, and model
      witness semantics. Full render/client tests, strict Clippy/format/shader
      validation, release build, and independent review/re-review are green.
      Live VineGallery run `20260714T030538Z-22388` passed exact GPU witnesses
      at sequences 191/192 (four keys, 95 stable refs, all contamination
      counters zero) but measured p50 40.6 ms, p99 47.7 ms, and 142.6 ms
      mutation-to-visible with 8,345 resident subchunks—neutral versus the prior
      p50 40.3 / p99 47.8 / 138.9454 ms run. Exact drawing therefore closes the
      required packed per-quad architecture but not the performance gate; GPU
      stage timestamps/workload counters must identify the remaining cost before
      considering a one-sided/two-sided pipeline split. Resident and
      frustum-visible model workload counters are now implemented: acceptance
      JSON distinguishes 16-byte model refs from exact eight-byte quad draw refs
      and reports the former fixed 32-quad slot invocations avoided. Full
      render/client suites are green. Acceptance/profiling runs now also enable
      Bevy's asynchronous DX12 timestamp recorder and report paired, deduplicated
      p50/p95/p99/max timings for the chunk-containing opaque and transparent 3D
      passes without blocking the GPU. Live VineGallery North run
      `20260714T032404Z-12360` recorded 1,296 GPU samples: combined opaque plus
      transparent was 4.9 ms p50 / 10.2 ms p99 (10.54048 ms max), while full
      frame time remained 40.2 ms p50 / 47.6 ms p99. Its 29,083 visible model
      refs issued 80,233 exact quad draws and avoided 850,423 of the former
      930,656 fixed-slot invocations (91.38%); resident totals were 63,327 refs,
      161,125 draws, and 1,865,339 avoided invocations. The exact-draw path is
      therefore effective and model shader work is not the remaining frame-time
      bottleneck; do not add a speculative one/two-sided model pipeline split.
      The run again passed adjacent exact GPU witnesses (sequences 801/802,
      four keys, 92 refs, zero contamination), stayed within the RSS/CPU budget
      at 638,537,728 bytes and 2.82% mean CPU, and failed only the shared 100 ms
      mutation gate at 142.5286 ms. The next performance investigation must
      target frame scheduling/presentation and mutation-to-frame latency.
      Two process-scoped pacing A/Bs on the same approved DX12 runtime ruled
      out unsafe workarounds: AutoNoVsync run `20260714T033621Z-14584` never
      reached the world-ready/mutation fence, while FIFO with
      `WGPU_DX12_USE_FRAME_LATENCY_WAITABLE_OBJECT=dontwait` in run
      `20260714T034035Z-22400` reached the clean gallery/camera fence but never
      produced a GPU-completed model witness and resisted graceful shutdown.
      Keep wgpu's default waitable-object behavior for correctness; surface
      acquisition/presentation remains the evidence-backed external blocker.
      The backend/presentation investigation is now
      conclusive: five direct swapchain captures from Cinnabar, minimal Bevy
      Camera3d and Camera2d clear-only probes, a camera-local red clear, and
      DX12/FXC were byte-identical pure black. Vulkan exposes no surface present
      modes and GL exposes no adapter on this machine. This isolates the native
      black-window symptom below Cinnabar to Bevy 0.18.1/wgpu DX12 on the RX 570
      driver `31.0.21924.61`; chunk, camera, shader, and custom render-phase code
      must not be changed to mask it. A driver or isolated Bevy/wgpu A/B plus a
      tiny startup clear-color smoke gate remains required before native visual
      evidence can close, while deterministic GPU witnesses can continue.
      **Visibility investigation checkpoint (2026-07-15; telemetry only):**
      acceptance/metrics runs now publish one coherent, fixed-size diagnostic
      snapshot for each rendered frame. It fingerprints the extracted camera
      identity, pose, and frustum with monotonic pose/view generations; records
      resident-mesh, cave-visible, Bevy-frustum-visible opaque, and actually
      submitted opaque Direct/MDI key counts plus deterministic hashes; and
      reports the three adjacent stage-loss count/hash deltas. Collection is
      disabled outside the existing acceptance/metrics diagnostics path, emits
      at most one aggregate marker per second, retains no per-key history, and
      caps the transient submitted-key set at 65,536 entries; overflow marks
      the submitted digest and its adjacent loss unavailable instead of
      publishing a truncated prefix as exact. Deterministic/empty/mutation,
      generation-coherence, Direct/MDI-parity, and bound tests are present. This
      checkpoint changes no culling, meshing, shader, draw-order, or presentation
      behavior and does not establish a visibility fix. A fresh affected live
      run must still capture consecutive `RUST_MCBE_VISIBILITY_SNAPSHOT`
      markers spanning the symptom and identify the first nonzero adjacent loss
      before any repair is proposed.
      **Live diagnostic-geometry attribution (2026-07-16):** every actually
      emitted diagnostic cube quad now carries its exact raw network identity
      and resolved protocol-1001 sequential identity through bounded meshing,
      resident replacement/eviction, JSON, `WORLD_READY`, and revision-gated
      live telemetry. Per-mesh/global top sets are deterministic and bounded;
      omitted contributions remain exactly reversible under capacity churn;
      identical remeshes do not resort or relog unchanged data. Independent
      review and post-merge meshing/app/assets tests are green through
      `eda5ba9`. A native BDS run then proved the dominant visible magenta
      geometry is `minecraft:sulfur` (14658/`0x2d658dd8`) and
      `minecraft:cinnabar` (12638/`0xbda02665`), with `minecraft:leaf_litter`
      far behind, replacing screenshot-based family guesses with exact live
      evidence.
      **Sulfur/cinnabar exact route complete (2026-07-16):** both single-state
      minerals now compile as exact opaque full cubes for sequential and hashed
      protocol identities. Admission proves the reviewed collision, selector,
      sound, static texture, tint, flipbook, alias, and extension-metadata
      boundaries and rejects the pair atomically on any mismatch. Independent
      review, the ignored pinned-pack witness, full affected suites, and strict
      checks are green through the `6aeb8c8` merge. Production visual diagnostics
      fell exactly from 2,400 to 2,398 with zero additions. **Native gate passed
      on `3bdb317`:** fresh ignored carriers joined BDS and both minerals were
      absent from the day and forced-night diagnostic top sets, replacing their
      former ~328k/~322k quad dominance with leaf-litter states at roughly 2.5k
      quads each. A residual magenta rectangle and remaining 2,398 compile-time
      diagnostics keep the broader zero-magenta Phase 2.6 gate open.
      **Fixed-pose diagnostic attempt (2026-07-15):** run
      `20260715T164233Z-14908` disabled auto-fly and retained one stable render
      view generation, but the client never emitted its mutation/world-ready
      marker, so the harness timed out before publishing and facing the requested
      `Front` fixture. The resulting 62 stable-pose snapshots therefore do not
      constitute the required aimed-camera reproduction. They still narrow the
      pipeline: 61/62 had exact frustum-visible/submitted counts and hashes; the
      sole mismatch was the final snapshot taken as the failed harness entered
      cleanup. No culling repair is authorized by this evidence. The next probe
      must establish a fixture-facing camera independently of the world-ready
      marker and reproduce the user's visible disappearance before changing
      cave, frustum, or submission behavior.
    - [ ] Wall-attached vine family: replace the diagnostic pink-cube fallback
      for every `minecraft:vine` direction-bit state with compact cutout face
      templates selected from its exact attachment mask, including conservative
      cave connectivity, cross-subchunk neighbours, texture/UV parity, and a
      deterministic gallery plus native screenshot/GPU evidence. Extend the
      same reviewed thin-face route to glow lichen and sculk vein separately;
      do not collapse their distinct state/property contracts into vine logic.
      **Implementation complete; live gate open (2026-07-13):** all 16 masks
      compile to foliage-tinted two-sided attachment planes with exact UV axes,
      zero diagnostics, zero-mask no-draw behavior, and all-mask/all-boundary
      CPU mesh coverage (`ff7066b`; focused Go/assets/render tests and two
      independent reviews green). Deterministic acceptance is now complete in
      `489af26` and `748438c`: five canonical poses bind the exact 0..15 mask
      bijection and compiled-asset hashes, build isolated direction-exact stone
      supports, preserve mask 0 as zero-draw, fence the committed camera ahead
      of publication, and require two adjacent GPU-completed markers over the
      exact requested subchunks with stable generation, manifest, nonzero model
      reference total, and zero missing/stale/wrong-stream/zero-ref/draw-mismatch
      counters. Live evidence proved the total is subchunk-wide rather than one
      reference per central fixture (94 refs in run `20260714T023010Z-7488`), so
      the invalid interim 15/43 equality was removed in `eec96e2`; review then
      withdrew that recommendation and passed the corrected contract. The run
      produced adjacent exact witnesses at sequences 461/462, four keys, 94
      stable refs, and all contamination counters zero. Its only failure was the
      shared model-performance gate: 138.9454 ms mutation-to-visible against
      100 ms, p50 40.3 ms, p99 47.8 ms, with 8,595 resident subchunks. Combined
      RSS peaked at 532,107,264 bytes and mean CPU was 2.52%, within resource
      budgets. Native captures and the structural exact-count model-draw
      optimization remain required before this item closes; native capture is
      separately blocked by the confirmed RX 570 Bevy/wgpu presentation failure
      above. Exact visible-quad drawing is complete but was performance-neutral;
      GPU-stage measurement and the shared 100 ms gate remain open.
      **Glow-lichen/sculk-vein implementation complete (2026-07-13):** the
      remaining vine-like pink blocks were not `minecraft:vine`; they were all
      64 `minecraft:glow_lichen` and 64 `minecraft:sculk_vein` states still
      classified as unknown. Distinct registry families now preserve their
      different six-bit face orders, render mask zero with the vanilla all-six
      fallback, and compile exact 1/256-inset two-sided cutout planes with no
      occlusion coverage. Sculk vein additionally binds its pinned four-frame,
      20-tick flipbook. Exhaustive 128-state selector/geometry/UV/material
      tests, the real pinned-pack compiler, registrygen, runtime rendering,
      strict visual-coverage ratchet, Clippy, formatting, and independent
      review are green; the combined 16,913-state report has zero
      glow-lichen/sculk-vein diagnostics and 7,770 diagnostics remaining
      overall (`a70d3c6`). Native screenshot closure remains part of the shared
      RX 570 presentation gate above.
    - [x] Exhaustive vanilla visual-coverage ratchet: inventory every one of
      the 16,913 protocol-1001 canonical states through the production registry
      and runtime decoders, bind the exact registry/asset hashes, and reject any
      newly diagnostic, newly provisional-fallback, or unjustifiably invisible
      state. Diagnostic/fallback shrinkage is allowed while residual families are
      implemented; the final gate requires zero diagnostic and zero provisional
      fallback non-air states. The accepted design is recorded in
      `docs/superpowers/specs/2026-07-13-exhaustive-vanilla-coverage-design.md`.
      **Complete (2026-07-13):** `visualcoverage` uses the production decoders,
      enforces the exact 1,356-name/16,913-state/one-air protocol corpus and
      exact hash-to-sequential bijection, bounds all inputs, rejects diagnostic
      regression/invisible laundering, and writes deterministic hash-bound
      reports (`b131247`; 11 tests, strict Clippy, real-pack run, and independent
      review green). The reviewed baseline was refreshed cumulatively for the
      already-landed door, trapdoor, wall, pressure-plate, fence-gate, pane,
      fence, carpet, button, huge-mushroom, glow-lichen, sculk-vein, exact
      ordinary stained-glass, exact copper-grate, static-sign,
      chiseled-bookshelf, resin-clump, selector-alias opaque-cube, and exact
      cactus tranches.
      After lava, vine, and those connected/static/multiface/glass/grate
      families plus the exact chiseled-bookshelf, resin-clump, selector-alias
      opaque-cube, cactus, cake, farmland, and exact bee-housing tranches, the
      reviewed baseline held 2,398 diagnostics including one air state.
      **Provisional zero-pink cutover (2026-08-02; incomplete):** the compiler
      now maps the exact 2,397 non-air baseline identities to bounded neutral
      geometry and pinned-pack textures where safely resolvable, otherwise
      canonical stone. The identity table is bound to each network hash plus a
      canonical-state fingerprint, so unknown/custom server blocks remain
      diagnostic. `VisualSupport` preserves this distinction through MCBEAS06
      and runtime decode; visualcoverage v2 reports zero diagnostics and 2,397
      provisional fallbacks across 487 names instead of laundering them as exact.
      The source inventory and hashes are pinned in
      `assets/vanilla-fallback-source-v1001.json`. This removes pink vanilla
      blocks but closes no vanilla parity acceptance gate.
  - [ ] Complete the exhaustive residual-family report, continuing from the
    completed lava/flowing-lava depth-writing non-water-liquid pipeline, so
    every non-air one of the 16,913 canonical states has an exact or otherwise
    authoritative accepted visual; provisional fallbacks do not satisfy this
    requirement. Close deterministic galleries and live acceptance with
    globally zero diagnostic and zero provisional-fallback counters,
    vanilla-reference screenshots, upload/memory/CPU
    metrics, and teleport-remesh evidence.
    - [x] Torch, ladder, rail, tulip, golden-dandelion, and coral-plant exact
      routes (97 states) supersede their envelope inventory entries; leaves
      render Fancy. Wall-torch pivot, ladder/rail offsets, and rail curve
      sprite orientation need native measurement; the coverage baseline must
      be regenerated. Residual names: `docs/phase-2-family-inventory.md`.
    - [x] Round two: night light floors lowered to the night sky transfer,
      biome tints blend on a 4-block lattice (four taps per axis; vertical axis
      unblended, needs measurement), chains route through crossed link planes.
      Remaining families and notes: `docs/phase-2-family-inventory.md`.
    - [x] Lava implementation: all 32 `minecraft:lava` and
      `minecraft:flowing_lava` depth states compile through the animated liquid
      mesher without water tint or alpha blending, use an immutable packed route
      bit, retain the O(n) transparent-water/depth-lava partition, and draw in a
      separate opaque depth-writing direct/MDI pipeline. Mixed interfaces and all
      six cross-subchunk boundaries are covered. Full assets/render suites,
      strict Clippy/formatting, the real 16,913-state ratchet, and independent
      review are green; deterministic native gallery/GPU/performance evidence
      remains part of the residual-family live gate.
    - [x] Exact bee nest/beehive cubes: all 48 canonical states across
      `minecraft:bee_nest` and `minecraft:beehive` now preserve the typed
      direction 0..3 by honey-level 0..5 product, full-cube collision and
      occlusion, and the compact packed cube stream. Protocol direction maps
      the front south/west/north/east; only honey level 5 selects the honeyed
      front. The compiler requires the pinned six-face block maps, singleton
      static terrain arrays, exact two-entry front arrays, literal paths, and
      no tint/extra/flipbook metadata. Both network-ID modes, all states, dense
      greedy closure, and all six cross-subchunk boundaries are covered. The
      full real-pack ratchet removes exactly IDs 10,395..10,418 and
      12,495..12,518 with zero additions, shrinking 2,448 -> 2,400 diagnostics.
      Bee occupants remain a separately reviewed block-entity concern rather
      than block-state geometry.
    - [ ] Shelf visual authority: the exact twelve-name/384-state registry
      contract is classified and now fails closed if one complete family is
      missing or an unexpected `_shelf` family appears. The versioned retail
      package exposes direction-specific `minecraft:voxel_shape` files and the pinned vanilla packs
      expose shelf texture routes, texture sets, and pixels, but none defines
      visible render geometry or per-face UV mapping. Collision/voxel bounds
      must not be promoted into an exact render model. All 384 shelf states use
      the explicitly provisional non-magenta fallback and remain open visual
      parity work. Resume shelf work only
      from legitimate version-matched render/UV authority or a reviewed native
      procedure; precise paths and hashes are recorded in
      `docs/evidence/phase-2-shelf-source-reference.md`.
    - [ ] Close static sign visual parity and its deterministic native gallery.
      - [x] Eliminate all 4,872 standing, wall, and hanging-sign diagnostics
        with typed, order-independent selectors; exact 16-way rotation, six-way
        facing, attachment, and hanging matrices; pinned terrain aliases; and
        bounded static model templates. All-six-facing tests caught and fixed
        reversed wall/wall-hanging support placement. The classic raw 24x12
        board plus 2/3 render transform establishes the 16x8 world silhouette,
        and sign text remains a block-entity deferral (`5987ed6`, `fba9e2e`).
      - [ ] Source-adjudicate exact board thickness, standing-post dimensions,
        and hanging board/support cuboids, then close UV/native-reference and
        GPU evidence. The pinned Bedrock sample pack exposes sign terrain aliases
        but no authoritative sign geometry, so the current shapes must not be
        described as final 1:1 geometry until this evidence is recorded.
    - [ ] Run all 67 exact-state GPU gallery pages (256 targets per logical page,
      with one final 17-state page), require exact palette readback plus two
      consecutive GPU-completed frames for every canonical target, and inspect
      fresh native `%TEMP%` screenshots. Family-specific support/neighbour
      fixtures do not count toward the 16,913 target inventory. The reviewed
      implementation order starts with a deterministic BREG/MCBEAS/hash-bound
      logical page inventory, then adds exact app-side palette witnesses,
      per-target GPU evidence, family-aware placement, and native captures; the
      logical inventory is independent of the RX 570 presentation blocker.
      - [x] Compile the exact logical target inventory: all 16,913 sequential
        IDs are assigned once and in order to 66 full 256-target pages plus a
        final 17-target page. The deterministic artifact binds BREG, MCBEAS, and
        coverage-baseline hashes, retains per-target diagnostic/drawable/
        invisible status, and is deliberately non-accepting until both the
        diagnostic count reaches zero and the strict semantic render-route gate
        passes. The CLI writes atomically and preserves an existing output on
        failure (`500c4af`, `59f692c`; full tests, pinned real-pack check,
        strict Clippy/formatting, and independent re-review green).
    - [ ] Generate a separate version-pinned block-entity inventory and reviewed
      renderer manifest. Prove chunk-NBT and live-update handling, required NBT
      variants, and GPU/no-draw evidence for every source ID; block entities are
      not folded into the canonical block-state count. The ingestion audit found
      21 explicit Dragonfly source IDs plus an id-less Note producer that must be
      explicitly adjudicated. Packet-56 and chunk/subchunk-tail NBT now reaches
      the bounded sparse world store; per-ID renderer evidence remains open. The
      bounded implementation order is
      NetworkLittleEndian NBT prefix decoding plus atomic sparse storage first,
      then a separate deterministic source/renderer-manifest generator and
      strict join, followed by per-ID GPU/no-draw witnesses.
      - [x] Implement bounded NetworkLittleEndian NBT prefix decoding and
        atomic sparse ingestion for inline and request-mode LevelChunk tails,
        successful SubChunk tails, and packet-56 live updates. Exact NBT bytes,
        optional source IDs, and absolute positions remain sparse and
        palette-native; strict byte/depth/collection/entity limits, scope and
        duplicate checks, vanilla dimension Y validation, per-chunk cumulative
        record/raw-byte caps across tail and live updates, FIFO worker decoding,
        malformed-update retention, all-air cleanup, and chunk eviction are
        covered without flat block expansion. The separate deterministic
        inventory/renderer manifest is now generated below; per-ID renderer
        and GPU/no-draw witnesses remain open.
      - [x] Generate and commit the independent protocol-1001 block-entity
        inventory plus reviewed renderer manifest from the exact Dragonfly
        `b85c56ffea6b306798a935f14cc941c76618be52` registration pin and the
        hash-bound BDS 1.26.32.2 runtime evidence. The isolated generator
        rejects workspace dependency drift, generic `unknownBlock` NBT
        passthrough, source/hash drift, missing/extra/duplicate source keys,
        ambiguous aliases, and missing chunk/live-update/variant declarations.
        Its deterministic artifact covers all 21 explicit Dragonfly NBT IDs
        plus the id-less `Note` producer, 63 backing block names, 446 canonical
        backing states, and 42 required NBT variants without changing the
        16,913 block-state count. The strict-final report truthfully remains
        red at 0 proven/22 deferred: renderer implementations, gallery builders,
        per-variant witnesses, and per-ID GPU or no-draw evidence remain open.
      - [x] Add the hash-bound runtime adjudication and lifecycle foundation for
        the reviewed block-entity catalog. Barrel, BlastFurnace, Furnace, and
        Smoker reuse their exact existing cube state; Jukebox and the exact
        id-less Note candidate route to logical no-additional-draw outcomes;
        every reviewed deferred ID requires its exact canonical backing; all
        mismatched/unknown sequential or hashed states fail closed with
        non-conflating route digests. Inline LevelChunk, request-mode
        LevelChunk, and packet-56 NBT-only replacements preserve block, light,
        mesh, connectivity, and render generations when backing/biomes are
        unchanged, while changed backing/biomes, eviction, dimension reset,
        and session reset retain destructive lifecycle behavior. Full affected
        Rust/Go suites, strict Clippy/formatting, and independent re-review are
        green through `558de41`. This is logical routing evidence only: the
        strict-final artifact remains 0 proven/22 deferred until the required
        per-variant GPU/no-draw witnesses below are generated and joined.
      - [ ] Implement the reviewed per-ID renderer routes and gallery builders,
        then close every required NBT-variant witness and GPU/no-draw witness so
        the block-entity strict-final gate reaches 22 proven with no deferrals.
        - [ ] Provisional, uncompiled and unmeasured (never closes a gate): the
          `.mcbeben` carrier (`make block-entity-assets`) and a dedicated
          block-entity pass draw chests (single/double, lid cue), ender and copper
          chests, beds, shulker boxes, skulls (dragon and piglin from entity
          geometry), banners, bell with frame and swing, lectern, enchant/lectern
          book, conduit, decorated pots, item and glow frames with their items,
          campfire items, tinted scrolling beacon beams, end portal, sign text and
          model-shaped break cracks. Entity-drawn block states compile to
          `Invisible` terrain (`entity_drawn.rs`). Filled maps, spawner mob,
          flower-pot plants, conduit wind cube, campfire/hopper/brewing-stand
          terrain and native lighting/measurement remain open.
    - [ ] Merge both the Axolotl protocol-fix branch and Cinnabar feature branch
      into their respective `main` branches through reviewed PRs using normal
      history-preserving merge commits (never squash or rebase the feature
      history), only after the applicable deterministic tests, native/GPU
      acceptance, zero-diagnostic state gate, and block-entity manifest gate
      are green.
  - [ ] Render-invisible blocks. Barrier, structure void, light blocks 0–15, invisible bedrock
    and moving block compile to non-occluding `Invisible` terrain ahead of the fallback
    inventory (`literal::is_default_invisible`). Incomplete: vanilla also tessellates barriers
    and light blocks as camera-facing sprites (`materials/barrier.material`) and structure voids
    as cube faces into their own terrain layers, drawn only while a creative local player's
    selected item is that block; those layers are not built.
- [ ] **2.7 Client lighting and atmosphere.** `P2.7-ATMOSPHERE` Block/sky flood fill, baked vertex
  light and day/night, then sky, fog, and clouds; finish the Phase 2 parity and
  teleport-remesh acceptance gates.
  - [x] Normalize SetTime and rain/thunder level events into bounded,
    vendor-independent protocol events; retain StartGame's initial current
    tick, day-cycle lock time, case-insensitive boolean `doDaylightCycle`
    state (default enabled when absent), and clamped initial rain/lightning.
    Runtime `GameRulesChanged` packets normalize only a case-insensitive
    boolean `doDaylightCycle` entry and ignore wrong types. Two deferred
    pre-spawn SetTime
    packets retain FIFO order in Play, post-spawn normalization is identical,
    and non-finite initial weather values fail closed.
  - [x] Consume the normalized environment stream into app-owned clock and
    weather resources without interpreting visual curves. A replacement
    StartGame begins a new environment session, anchors its exact initial tick,
    preserves its cycle lock and bounded rain/lightning targets, advances only
    while `doDaylightCycle` is enabled, and uses the lock tick only when the
    rule is explicitly disabled. A runtime false transition freezes the exact
    current visual tick; true re-anchors and resumes it. Exact signed SetTime
    values immediately re-anchor either a running or frozen clock. Dimension
    changes
    preserve that world-session snapshot. FIFO-committed SetTime/day-cycle/weather
    updates do not dirty meshes, enqueue mesh changes, or change cave
    connectivity. Mesh-baked light response and vanilla atmosphere parity
    remain open.
  - [x] Implement the renderer-independent sparse light core: independent
    uniform-or-packed block/sky nibble volumes, copy-on-write snapshots,
    generation-checked storage/eviction, and a bounded darken-then-increase
    solver over explicit Unknown/KnownAir/Resident cells. Exact one-cell halo
    samples carry scheduler-owned trust and direct-sky provenance; unknown,
    dirty, or untrusted boundaries never seed light, Nether/End reject sky,
    and all propagation shares one enforced queue budget. WorldStream now owns
    the authoritative state metadata and bounded scheduling below; mesh baking,
    GPU/shader light consumption, and vanilla atmosphere parity remain open.
  - [x] Generate and ship bounded per-runtime-state light metadata without
    changing the reviewed `BREG1003`: `LREG1001` proves the exact protocol-state
    count, identity, property order, and committed-BREG SHA-256 before emitting
    one packed emission/filter byte per state. Dragonfly revision
    `dbbd8b787946e53b1def8d532050751dfcdc80e7` is authoritative for 16,911
    concrete states; exact, identifier-uniform pinned PMMP fallback supplies only
    `minecraft:redstone_lamp` and `minecraft:lit_redstone_lamp`, with deterministic
    provenance IDs/reporting and fail-closed disagreement/range checks. `MCBEAS05`
    atomically carries the byte beside each visual, exposes it through both
    sequential and network-hash resolution, rejects stale `MCBEAS04`, and names
    the `--light-registry` rebuild command. The solver and WorldStream scheduling
    integration are covered below; mesh baking, GPU/shader light consumption,
    vanilla sky/fog/cloud parity, and visual acceptance remain open.
  - [x] Derive a deterministic per-frame atmosphere snapshot from the app-owned
    clock and weather state: real elapsed time advances enabled sessions at 20
    ticks per second, explicitly disabled cycles freeze at their lock tick,
    signed times use
    Euclidean day and moon-phase wrapping, and rain/thunder remain bounded.
    Extract one stable 96-byte uniform, render the first procedural sky/sun/moon
    pass at reversed-Z far depth with per-view MSAA specialization, and apply
    camera-distance fog to chunk, model, and liquid paths without adding storage
    bindings or per-frame texture/bind-group churn. These checked curves and
    procedural disks establish the integration slice, not vanilla parity;
    asset-backed GPU hookup, precipitation visuals, underwater/lava medium fog,
    and live reference acceptance remain open.
  - [x] Pin and carry the exact vanilla sun, moon-phase atlas, and cloud texture
    in an independent bounded `MCBEATM1` runtime blob. The compiler requires the
    exact tracked Mojang manifest fields, canonical LF bytes (accepting only a
    uniform LF or Windows CRLF checkout), and per-source PNG hashes, records
    encoded and decoded hashes, rejects malformed/noncanonical layouts, and
    publishes only to ignored local paths. `make assets` and `make client`
    refresh the serialized blob/report pair through one portable producer;
    normalized, case-variant, symlink/junction, and hardlink output aliases fail
    before either write. Focused pinned tests, full assets/client-asset suites,
    strict Clippy, and independent re-review are green through `aed8d7f`; no
    Mojang payload is tracked.
  - [x] Load the required sibling `MCBEATM1` carrier once at startup and render
    its exact sun, 4x2 moon-phase atlas, and repeating cloud texture through the
    existing reversed-Z atmosphere phase. Three persistent GPU textures, one
    sampler, and one identity-cached bind group replace the procedural celestial
    disks without per-frame or per-subchunk resource churn; missing or malformed
    carriers fail hard with the exact rebuild command. Clouds use absolute world
    time, altitude 128, weather/fog fades, and a tested +X eastward speed of 0.03
    blocks per tick. Full app/render suites, strict Clippy, WGSL validation, and
    independent review are green through `4bf2c8c`; native multi-platform visual
    tuning and parity acceptance remain open.
    The original fixed 3x3 opaque finite-cloud visual behavior is superseded by
    `docs/superpowers/plans/2026-07-16-native-cloud-parity.md`: exact local-only
    1.26.33.1 occupancy, transparent depth-aware legacy composition, directional
    lighting, exact weather contributions, and calibrated native quality controls.
  - [x] Accept the exact local-only Bedrock 1.26.33.1 cloud PNG as an optional
    atmosphere compiler override without changing the pinned sun/moon inputs or
    `MCBEATM1` schema. The compiler fails closed on the exact 7,880-byte encoded
    SHA-256, 256x256 dimensions, decoded RGBA8 SHA-256, and 13,356 occupied
    texels; the runtime carrier and deterministic report retain only the canonical
    `textures/environment/clouds.png` logical path and independent hashes. The
    wrapper API and no-override fixture remain deterministic, while
    `assetc atmosphere --clouds-override` and optional `CINNABAR_CLOUDS_PNG`
    expose the local input portably. Both Make modes conservatively force the
    atmosphere producer, preventing a set-to-empty override transition from
    retaining a stale carrier. Startup evidence now identifies
    `cloud.wgsl` separately from `atmosphere.wgsl`. Synthetic rejection tests,
    synthetic accepted options tests through a private injected identity seam,
    a parsed-CLI-to-encoded-report test through the production command handler,
    environment-gated installed-input acceptance, the full assets suite, app asset
    suite, strict relevant Clippy, formatting, and diff checks are green;
    calibration and live parity acceptance remain open in the native-cloud plan.
  - [x] Replace opaque/depth-writing finite clouds with one transparent sorted
    render item using alpha blending, reversed-Z depth testing without depth
    writes, exact sequential rain/thunder colour contributions, real sun-vector
    directional lighting, bounded distance-fog alpha, and deterministic negative
    coordinate wrapping. Empty/sub-threshold alpha emits no geometry, GPU records
    and bind groups remain immutable, and collapsed/reversed/non-finite fog ranges
    fail to finite deterministic alpha. Focused meshing/render tests, WGSL/Naga
    validation, strict Clippy, formatting, independent review, and post-merge
    verification are green through `87e856f`. A live night run additionally
    proved that zero daylight could still make otherwise-transparent clouds
    pure black; clouds now share terrain's provisional `0.2` night sky-transfer
    floor through `e7c85ea`. **Forced-night native WGC evidence on `3bdb317`
    passed this colour gate:** cloud faces remained visibly medium gray against
    the purple night sky rather than becoming black. The same run was only
    16-18/11,760 chunks at about 8 FPS, so it is not a performance or full-view
    acceptance witness. Native geometry scale/density and
    above/below/within/grazing visual acceptance remain open.
    **Cloud geometry evidence contract complete (2026-07-16):** one bounded,
    identity-deduplicated `CLOUD_GEOMETRY_EVIDENCE calibrated=false` marker now
    records exact occupied texels, uploaded quad count/bytes, instance count,
    provisional 256-block period and Y=128..132 bounds, pinned High-quality
    native `grid_size=3`/`mesh_size=64`/`distance_scale=3` controls, and the
    atmosphere asset identity. It is produced only when a new atmosphere asset
    identity is prepared; no draw, shader, buffer topology, or per-frame work
    changed. Independent review and the complete render suite/strict checks are
    green through the merge following `c86d184`. The marker deliberately keeps
    `calibrated=false`: matching native views must derive the missing world-space
    mapping before the provisional layout changes.
  - [x] Resolve the camera-eye medium directly from palette-native liquid
    contributors, including secondary waterlogged layers, and use the exact
    two-triangle surface drawn by the shared quad index buffer for the air/water
    or air/lava transition, including noncoplanar corner heights. The sparse
    camera query reads packed indices and palette entries directly without
    constructing the mesher's allocating palette-fact cache; missing and
    non-finite samples fail open to air. Water and lava replace weather distance
    fog in the existing 96-byte atmosphere uniform, so chunk, model, liquid, and
    infinite-sky/celestial rendering share one medium response without a new
    binding or per-subchunk resource. The bounded water/lava colours and 32/3-
    block visibility ranges are the Phase 2.7 baseline; native reference
    calibration and precipitation visuals remain open.
  - [x] Compile and route exact pinned client biome/fog profiles by camera
    biome with Overworld/Nether/End fallback. `MCBATM2` carries the bounded,
    hashed environment-profile and fog-distance tables; biome-specific fog
    wins per medium and missing media layer from `minecraft:fog_default`.
    Fixed and render-relative air, weather, water, and lava endpoints plus
    exact sky colour now reach the shared atmosphere frame while clock,
    weather, and celestial state remain intact. Lighting and atmospherics
    identifiers are retained as explicit provisional routes rather than
    claimed native lighting calibration. Exact pinned-pack provenance,
    envelope, routing, render, and integration tests are green through
    `53fd591`.
  - [x] Wire the sparse solver and MCBEAS05 per-state emission/filter metadata
    into generation-qualified WorldStream light storage and bounded,
    nearest-first one-subchunk solves. Exact face block/light halos, dirty
    boundary trust, separately retained direct-sky provenance, decoded/update/
    eviction invalidation, convergent neighbour iteration, and current-light
    mesh scheduling are covered without flat block arrays. Mesh light baking,
    GPU/shader consumption, sky/fog/cloud rendering, and visual acceptance
    remain open.
    - Release scheduler workload gate on 2026-07-14: the exact radius-16 square
      (33×33 columns) across all 24 Overworld subchunks completed 26,136
      known-air light solves, with 26,136 uniform fast-path completions, zero
      stale completions, and all keys current in 1,006 ms. This measures the
      lighting scheduler only; live teleport/full-view remesh acceptance,
      mixed-block workloads and rendering acceptance remain open.
  - [x] Capture palette-native mesh lighting in a fixed 27-slot identity halo,
    gate mesh dispatch until every known slot is current, reject and losslessly
    requeue completions after exact light or direct-sky identity changes, and
    invalidate the center plus all 26 dependants on light completion, load, or
    eviction. Center/face/edge/corner routing and absent dark fallback read
    nibble channels directly without flat staging arrays. App tests passed
    233/233 (with one release-only test ignored), all app integration and world
    suites passed, and the exact release workload completed 26,136/26,136
    current subchunks with zero stale completions in 987 ms. The halo now
    implements the allocation-free render sampler and the worker calls the
    light-aware mesher. Cube, model, cross, and liquid CPU sidecars retain
    independent block/sky/AO channels; cube greedy merges split on exact packed
    lighting, and the cube sidecar survives bounded render-queue extraction
    with exact byte accounting. The combined app/render suites and strict
    Clippy are green. GPU arena/shader consumption, mixed-block visual
    acceptance, and live teleport full-view remesh acceptance remain open.
  - [x] Consume cube, model, and liquid light sidecars in the GPU world shaders
    without adding a buffer, bind group, or per-subchunk render resource. Commit
    `fe1a2ea` appends cube sidecars to the existing binding-13 arena, expands the
    per-draw origin ABI to carry exact cube/light bases, validates aligned and
    disjoint direct/MDI addressing, and converts discrete block/sky/AO samples
    at the vertex before smooth interpolation. Daylight affects only sky light;
    full solved skylight retains a named provisional `0.2` transfer floor at
    true night while block light remains independent. This floor is a
    conservative calibration to the existing horizon baseline, not a vanilla
    parity claim; native Bedrock reference tuning remains open. Alpha and fog
    ordering remain intact. Full render and app suites, strict
    combined Clippy, WGSL semantic/Metal-stage checks, formatting, and diff
    checks are green. Live mixed-block GPU parity and teleport/performance
    acceptance remain open.
  - [x] Preserve the authoritative all-air suffix omitted by limited-request
    `LevelChunk` columns and feed it into the sparse top-down skylight graph.
    `highest=0` columns require no outbound slot or packet; replacement,
    eviction, direct-sky propagation, and the final mesh-light sidecar are
    covered. This fixes the live zero-skylight world while leaving block light
    independent. Full client, render-atmosphere, asset, camera, strict Clippy,
    WGSL, and independent review gates are green through `7805402`. That commit's
    RGB black-key celestial workaround was later disproved by the pinned
    near-black border texels and is superseded by the additive tranche below.
  - [x] Keep solved light level zero visibly non-black through a named
    provisional `0.04` linear ambient floor applied after independent
    block/sky/daylight combination. The floor-to-one remap preserves every
    higher light step and exact full brightness instead of flattening low
    levels; AO remains independent. Native Bedrock capture tuning of the exact
    floor remains part of final visual acceptance.
  - [x] Composite the pinned opaque sun and all eight moon phases as emissive
    additions rather than RGB-keyed opaque replacements. A decoded `MCBEATM1`
    regression traverses every 32x32 tile border, covers the exact problematic
    sun `(1,1,0)` and moon `(0,0,1)` samples against bright and dark skies,
    preserves dark lunar detail and HDR energy, and Naga-validates the shared
    WGSL path. Startup now emits ordered full envelope/shader SHA-256 evidence
    without paths or payloads. Full workspace tests, strict Clippy/formatting,
    and independent re-review are green through `384e08a`; the live all-phase
    native/GDI comparison remains open in the visual blocker below.

Perf budget carried from Phase 0 gate; add: full remesh of view distance after teleport ≤ 2s.

**Edge anti-aliasing (2026-07-15):** [x] the client uses single-sample world
targets plus an FXAA-only post-process through `cd31f7a`. This replaces the
nominally portable 4x MSAA default after a real Radeon RX 570 DX12 adapter
presented black frames for every multisampled view (including a minimal Bevy
clear-color reproduction), while a macOS adapter had already rejected 8x
`Depth32Float`. Bevy's umbrella anti-alias plugin is disabled so unavailable
TAA/SMAA/CAS graph nodes are not installed; only `FxaaPlugin` runs. The full
280-test client suite, strict Clippy/formatting, release build, independent
review, and a fresh BDS/GDI above/within/below gallery are green. Higher
multisample modes remain conditional on a future capability- and live-tested
resolve path rather than a hardcoded camera sample count.

**Live visual acceptance (Computer Use):** run the Bevy app in representative vanilla
world scenes and compare visible results against the matching Mojang vanilla assets/reference
client at multiple distances and view angles. Verify exact texture/model selection, UV orientation
and wrapping, per-layer mip quality, opaque/cutout/blend behavior, flipbooks, biome tints,
block/sky lighting across day/night, fog, sky, and clouds. Exercise focus, keyboard input,
movement, and mouse-look/rotation during the pass. No placeholder/debug texture or visibly
non-vanilla rendering ships past this phase; record screenshots and any adjudicated parity gaps
in the phase report. If Computer Use window capture fails, take a native Windows screenshot,
store it only under the user's temporary directory, inspect that file, and never commit it.

**Open live defects (reported 2026-07-15; Phase 2 acceptance blockers):**

- [ ] Reproduce and eliminate the moving horizontal/vertical "TV static" or
  void-band artifact under camera motion. Separate presentation tearing from
  resident/frustum/submission loss with one identical-scene FIFO-versus-
  no-vsync capture and coherent resident/frustum/submitted/GPU-completed frame
  identities; do not change culling unless those identities prove a missing
  draw.
  - [x] Install bounded exact missing/extra key evidence through resident,
    cave, frustum, submitted, and GPU-completed stages for one coherent
    view/frame in both Direct and MDI paths. Acceptance defaults to FIFO;
    Immediate is an explicit A/B whose effective mode is proven from the
    primary surface capabilities with the same Bevy render instance/adapter,
    while absent/failed/fallback evidence rejects the comparison. Release
    profile, backend, adapter, and driver provenance plus PowerShell/Bash
    contracts passed independent review through `14db67f`.
  - [x] Remove the proven multisample resolve/presentation failure through
    `cd31f7a`: the affected Radeon DX12 adapter rendered black for every 4x
    MSAA view even in a minimal Bevy clear-colour reproduction, while the same
    release BDS scene renders through single-sample targets plus FXAA. The
    post-fix GDI gallery contains no black presentation or thin screen-space
    band, but this does not close the user-reported artifact without the
    binding identical-scene FIFO/Immediate motion comparison below.
  - [x] Make direct Windows launches and native visual inspection reliable
    through `1cad274`: default ignored-local assets fall back to the executable
    project root, Windows selects DX12 unless `WGPU_BACKEND` explicitly
    overrides it, and the local BDS scene was enumerated and captured through
    native Windows Graphics Capture at 1282x752. This closes the launch/capture
    tooling failure only; visual-parity and performance gates remain open.
  - [ ] Run the identical-scene release FIFO/Immediate capture, classify the
    artifact from coherent evidence, and eliminate the proven source.
- [ ] Make initial chunk publication and steady streaming meet the frame and
  teleport budgets without debug-only DX12 direct-draw collapse. Attribute
  decode, light-halo readiness, mesh queue wait, worker time, GPU-upload budget,
  render submission, and present latency independently; validate the final
  release path at radius 16 with no visible stalls and full-view remesh in at
  most two seconds.
  `P2-CHUNK-PUBLICATION`
  - [x] Eliminate unchanged-light publication churn through `39c44e8`: exact
    no-op and direct-sky-provenance-only completions preserve sampled light
    identity/generation and perform zero mesh invalidations, while genuine
    nibble changes retain the full 27-dependent invalidation contract. Exact
    saturating outcome counters and independent review are green; publication
    latency attribution, the full release benchmark, and live gates remain open.
  - [x] Attribute decode/light/mesh bounded-queue wait independently from
    worker duration and emit one coherent periodic publication snapshot that
    joins outcome counters, pending/in-flight gauges, upload bytes, draw mode,
    build profile, present proof, and adapter provenance. PowerShell and Bash
    reject missing, extra, duplicate, malformed, wrong-typed, or mismatched
    rows. Full client/script suites, strict Clippy/formatting, adversarial
    contract tests, and independent re-review are green through `0811c0a`;
    the full release radius-16 benchmark and live resource gates remain open.
  - [x] Install one shared adaptive item-and-byte publication budget across
    world handoff, main-world application, extraction, and GPU preparation.
    Genuine frame-pressure stalls reduce the budget multiplicatively, normal
    FIFO pacing is tolerated, and sustained healthy frames recover
    conservatively; nearest-first ordering and exact cohort identities remain
    unchanged. Zero-byte removals retain a separate 256-operation hard cap,
    and whole-arena growth copies are reserved inside the same GPU byte budget.
    Deterministic pressure, continuity, accounting, app, client-world, and
    render tests plus independent review are green through `9ea25e1`; the live
    radius-16 <=2-second acceptance gate remains open.
  - [x] Prove the deterministic full radius-16 publication path through
    `7098b65`: 26,136 current subchunks pass accepted lighting, meshing,
    main-world queue application, real render extraction, production GPU
    preparation, and exact acknowledgement in 1,369 ms on the integration
    rerun. The 64 populated witnesses produced 190,384 positive upload bytes;
    31,786 known-air removals completed without spending the production
    128-item non-empty upload budget, while all applications remained capped at
    256 per frame. Zero stale, pending, in-flight, duplicate, or
    unacknowledged work remained. Full render/client suites, strict
    Clippy/formatting, and three independent reviews are green. This closes only
    deterministic Task 3 Steps 1–3; the release BDS join/teleport,
    presentation, RSS, settled-CPU, and visible-stall gate remains open.
  - [x] Ship the bounded publication, same-connection fast-transfer recovery,
    and player-near request-priority implementation on canonical
    `phase2-textures` through `2fc7a33`. The combined history includes bounded
    publication at `dedbef1`, local reset and request-ordering work from
    `a7d8a90` through `c4ed5a9`, integration at `f0f27eb`, vanilla slash
    `CommandRequest` encoding at `be32657`, and final cross-platform CI repairs.
    Canonical CI run `29671070071` is green. This is an implementation-only
    milestone: the binding LBSG same-connection reset witness, ordered
    Lunar/Zeqa native evidence, and the master <=2-second live gate remain open.
  - [x] Remove the frame-coupled four-decode throttle through `2b16368`.
    The 2026-07-15 opt-in live trace proved that all 4,912 admitted jobs
    completed (3,988 SubChunk jobs at about 0.02 ms each), while 1,218 frames
    over 180 seconds limited throughput to 27.29 jobs/s solely because only
    four jobs could dispatch per frame. Decode dispatch now drains the already
    bounded 32-heavy-event admission window in one poll; retained payloads
    remain capped at 32 and the result channel at 128. The deterministic
    regression, all 284 client unit tests (two ignored), strict Clippy,
    formatting, and CI are green.
  - [ ] Mesh neighbourhood gate, provisional and labeled incomplete: a resident
    mesh waits while any of its 26 neighbours is owed (requested, or unsent in
    the announced Euclidean disk while its cohort made new progress within 1 s).
    Current vanilla behaviour instead requires eligible horizontal columns
    before rebuilding; the historical missing-column claim is superseded. The
    1 s quiet fallback remains provisional, with no native reference.
    `streaming_harness` checks slow delivery for transient geometry and dark seams.
  - [ ] Replace the provisional universal Euclidean publisher-disk rule with
    per-publisher-epoch membership from unique FIFO-committed request-mode
    `LevelChunk` announcements. The raw block radius remains a separate
    retention/classification witness: it does not define one protocol-wide
    enumerable cohort. The earlier BDS-specific run through `1fdd874` observed
    797 complete columns, while authoritative Lunar raw-128 evidence observes
    exactly 177; Dragonfly's attributable classifier gives 177 at raw 128 and
    749 at raw 256, and raw 120 => 177 remains labeled compatibility policy.
    A later unique announcement must expand the same epoch and invalidate the
    frozen count/hash/presentation proof; publisher identity, session, and
    dimension changes reset membership. Exact source eviction and GPU-manifest
    evidence remain mandatory. Earlier independent verification passed all 289
    client unit tests (two ignored), the 43/14/14 integration groups, strict
    Clippy, formatting, and diff checks. Live DX12/FIFO BDS run
    `20260716T001125Z-16608` reached `exact=true` with its 797 announced
    columns, 22,488 resident subchunks, 14,018 known-air
    subchunks, zero source/foreign evidence, and every network, request,
    decode, light, mesh, render-upload, and acknowledgement queue drained to
    zero. The run still timed out after this boundary because no binding pair
    of exact GPU-completed presented-frame acknowledgements was published;
    presentation-gate diagnosis and the <=2-second remesh gate remain open.
  - [x] Correct exact presented-frame acknowledgement for legitimately culled
    resident allocations through `9a195c1`. The gate still proves the complete
    target GPU allocation manifest, exact generations, and zero missing,
    unexpected, source, foreign, stale, or orphan evidence, but now requires
    every stable visible allocation to be drawn instead of incorrectly requiring
    every hidden allocation to be drawn in one frame. Two adjacent frames must
    retain the same visible manifest. The visible-undrawn, hidden-resident,
    visibility-churn, and skipped-frame regressions are green along with all 157
    render tests, 289 client unit tests (two ignored), strict Clippy, formatting,
    and diff checks. Live DX12/FIFO run `20260716T003124Z-7592` advanced through
    `RUST_MCBE_TELEPORT_SETTLED` with a complete 6,951-allocation manifest and a
    stable 1,676-allocation visible/drawn subset, with every contamination
    counter zero. The binding far teleport still took 48,128 ms and the forced
    full-view remesh did not finish inside the 60-second timed session; peak
    pending meshes reached 43,024, maximum mesh queue wait reached 30,833 ms,
    and the final snapshot still had 148 uploads queued. The <=2-second live
    remesh/performance gate therefore remains open and is the next publication
    bottleneck.
  - [x] Remove semantically empty work from the forced acceptance remesh. Trace
    attribution of `20260716T003124Z-7592` proved that the old gate scheduled all
    22,488 resident identities even though only 6,951 had published GPU
    allocations: 14,018 known-air plus 1,519 packed-empty identities created
    15,537 guaranteed no-mesh jobs, 69.1% of the forced cohort. The gate now
    remeshes the teleport's complete frozen allocation manifest, never its
    visible subset, and fails closed on empty, duplicate, stale-generation, or
    nonresident entries. Exact cohort hashes, forced generations, full
    allocation identity, zero contamination, and the adjacent GPU-presented
    frame proof remain unchanged. This reduces the hard application floor from
    88 to 55 frames; it does not yet prove the binding two-second gate. All 292
    active client unit tests plus the 43/14/14 integration groups, strict Clippy,
    formatting, and diff checks pass. A fresh live run must measure the new
    floor and determine whether baseline frame rate or upload scheduling is the
    next limiter.
    **Live result (2026-07-15):** release DX12/FIFO run
    `20260716T052758Z-9172` retained the exact 6,951-allocation cohort and zero
    contamination, then completed the forced remesh in 8,596 ms over 67 frames
    instead of timing out. The binding teleport remained 48,547 ms. At roughly
    eight presented frames per second, the current 128-nonempty/256-total
    frame-coupled budgets impose a 55-frame theoretical minimum, so adaptive
    upload/application scheduling is now the measured next limiter. The script
    failed only after both binding markers when its 60-second client exited
    during the later steady-resource sample; no final resource pass is claimed.
- [ ] Remove every dark rectangle/background pixel around the pinned sun and
  moon textures. The acceptance test must exercise decoded pinned pixels and
  mip/filter edges, not merely string-inspect WGSL, and must prove both bodies
  against bright and dark skies across all moon phases.
  - [x] Land the decoded-border additive composition and path-independent
    envelope/shader identity tranche through `384e08a`.
  - [ ] Run fresh release/GDI views against the matching native client for sun
    and all moon phases, including horizon and filter-edge cases, before
    closing the visible defect.
- [ ] Precipitation parity with the 26.30/1.26.50 `WeatherRenderer` and `Weather`
  material: ten wrapped 30-block particle layers per kind over a 2,500-quad mesh
  (925-particle pool), velocity-stretched sheet streaks, exact rain/snow params,
  UV cells, lattice offsets, intensity smoothing and density, and a 64x64 column
  occlusion grid replace the per-column Java-style sheet. Provisional, not
  closing the gate: wind uses our own seeded simplex (not the native permutation),
  the 0.01-scale per-layer turbulence and block-light tint are omitted, the
  density-halving view flag is assumed unset, and no native side-by-side capture
  has been taken.
- [ ] Replace the current infinitely thin cloud plane with a vanilla-parity
  cloud volume/layer that has visible thickness and side faces while retaining
  bounded GPU cost, world anchoring, weather/fog fades, and the existing shared
  atmosphere resource architecture. Verify from above, below, within, and at
  grazing angles against matching native reference captures.
  - [x] Record the installed native occupancy/material/config evidence and the
    reviewed greedy packed finite-mesh design/implementation plan in
    `docs/superpowers/specs/2026-07-15-finite-cloud-mesh-design.md` and
    `docs/superpowers/plans/2026-07-15-finite-cloud-mesh.md`.
  - [x] Implement and independently review the deterministic periodic CPU
    mesher: exact 256x256 alpha occupancy, toroidal seam culling, greedy exposed
    faces, an eight-byte packed ABI, checked worst-case ceilings, and canonical
    snapped 3x3 origins are green through `03a8c3a`.
  - [x] Render that finite mesh through one identity-cached custom GPU pipeline
    with immutable eight-byte quad records, vertex pulling, one nine-instance
    draw, physical reversed-Z depth, per-view MSAA/HDR specialization, bounded
    weather/fog shading, and explicit Metal-safe binding visibility. The old
    fullscreen sampled plane is removed. Full render/client suites, strict
    workspace Clippy/formatting, and independent review are green through
    `e2d0ea8`; native visual and performance acceptance remains open.
  - [ ] Resolve the 2026-07-15 live user rejection that the finite clouds look
    substantially non-vanilla. Reproduce the exact visible scale, thickness,
    silhouette, face shading, motion, fog, and seam behavior against the
    matching native Bedrock client, correct the proven differences, and obtain
    a fresh accepted above/below/within/grazing gallery before closing this
    blocker. Deterministic mesher/pipeline tests alone do not satisfy it. The
    first live/evidence audit proves three root mismatches that must be removed:
    the compiled `v1.26.30.32-preview` mask differs from the installed 1.26.33.1
    mask at 24,175/65,536 occupancy coordinates; the fixed 3x3 256-period draw
    ignores native `cloud_mesh_size: 64`, quality grid, and distance controls;
    and the opaque, depth-writing, fixed-face shader contradicts native
    transparent, directional-light, and exact weather-colour inputs. Exact
    native cloud bytes remain local build inputs and must never be committed.
    The opaque/depth-writing/material mismatch is resolved through `87e856f`;
    native mesh size, quality/distance controls, density, scale, thickness,
    silhouette, and live gallery acceptance remain open.
  - [ ] Apply the 26.30/1.26.50 `Clouds` material and cloud renderer values: one
    `clouds.png` texel per 16x16 blocks (4,096-block period), a 4-block slab at
    192.33, baked face shade (top 1, bottom 0.75, x sides 0.925), the
    vanilla day/weather/sunrise cloud colour with alpha 0.7, drift 0.02
    blocks/tick toward -X, and the 0.9D-1.9D distance fade with no fog. Landed
    provisionally: the pre-Caves-and-Cliffs 128 height, thunder mixing, the
    sunrise darkening term, above/below face flags and the quality/weather lerp
    of the fade distance are unverified, and no native gallery was taken.
  - [ ] Implement, independently review, and live-verify the finite cloud mesh.

## Phase 3 — Movement and the local player `P3-MOVEMENT`

**Goal:** playable movement that servers accept. Deliverable: walk/sprint/jump/sneak/swim/
climb on a vanilla parkour course and on Lunar-fronted servers with server-auth movement,
no rubber-banding.

Scope: input → `PlayerAuthInput` at 20Hz with correct flags; client prediction in `crates/sim`
as a **behavioral port of bedsim** — test strategy: golden traces (bedsim runs input scripts →
JSONL of per-tick positions; Rust sim must match within epsilon; reuse the pathfind-bot log
tooling patterns); collision against `crates/world`; camera = per-frame interpolation of
tick states; correction/rewind handling (`CorrectPlayerMovePrediction`).

- [x] **3.1 Server-bound movement foundation.** A vendor-neutral Rust movement snapshot now
  maps byte-for-byte to gophertunnel's protocol-2168 `PlayerAuthInput` fixture, including the
  current position/predicted velocity, processed/analogue/raw move vectors, rotation, input flags, input
  mode, camera orientation, and server tick. A deterministic 20 Hz scheduler, bounded 32-tick
  retry FIFO, StartGame/session reset, and `CorrectPlayerMovePrediction` reanchoring are in
  place behind an explicit movement-source authority gate. The gate defaults to `FreeCamera`,
  and outbound free-camera `PlayerAuthInput`/position updates are suppressed completely;
  StartGame and corrections cannot authorize them. Only the future `Physics` source may use
  the scheduler and transmit. Real movement remains incomplete until the bedsim-parity
  fixed-tick simulation, collision, prediction/history/rewind replay, and render interpolation
  are implemented and wired to that source.

- [x] **3.2 Physics-safe simulation foundation.** `crates/sim` now provides transactional
  one-call/one-20-Hz fixed ticks, feet-origin player AABBs, bedsim-order swept collision and
  stepping, basic walk/sprint/jump/sneak forces, packed-palette `crates/world` collision queries,
  and bounded tick-keyed correction replay. Unknown runtime IDs, unloaded chunks, invalid
  collision shapes, and failed replay queries stop prediction instead of guessing. A generator
  pinned to bedsim v0.1.3 records a checksum-bound JSONL trace, and the Rust conformance test
  matches it at `1e-12` epsilon. App integration is tracked separately in 3.3. Remaining bedsim
  movement strata, expanded terrain/correction traces, and live vanilla/Lunar verification
  remain required before Phase 3 is complete. Freecam remains a non-authoritative mode and
  must be network-silent.

- [x] **3.2a Current-BDS movement reconciliation correction.** Older
  Bedrock 1.16.201 `ServerPlayer` behaviour initially suggested that takeoff should
  switch to air acceleration and drag before horizontal travel. A strict
  server-authoritative BDS 1.26.32.2 run disproved that as the target-version
  contract: over the six ticks ending at the jump apex, the server advanced
  `0.85394` blocks, matching the pinned bedsim ground-takeoff/ground-drag
  ordering rather than the provisional air-first result (`0.75231`, corrected
  by `0.10162`). The simulator therefore retains its checksum-bound bedsim
  equations; the older implementation is not used as a current
  vanilla authority.
  The same strict run exposed the actual live defect: every retained
  `CorrectPlayerMovePrediction` rebuilt its tick from position-only state,
  erasing velocity and restarting acceleration even for a `0.000017`-block
  correction. Correction replay now replaces the server-owned position and
  grounded flag while retaining velocity, movement, and jump state before
  replaying later inputs. Unconfirmed collision flags and their dependent
  upward ladder velocity are still cleared.
  Strict current-BDS confirmation at commit `5312070` used
  `server-authoritative-movement-strict=true` and a `0.001` position threshold.
  An 18-tick uninterrupted ground walk received only `0.000017`–`0.000124`
  corrections while acceleration continued through each six-tick correction;
  an open jump received only `0.000015`–`0.000126` corrections through takeoff,
  apex, landing, and continued travel. This closes the correction-reset defect,
  not the Phase 3 native-client visual/feel acceptance gate.

- [x] **3.3 App-side local physics integration foundation.** App input now drives the fixed
  20 Hz simulator against checked-in sequential and hashed collision registries; unavailable
  collision data fails closed, render frames interpolate the simulated eye position, and
  session/correction events reanchor or reset prediction without leaking into teleport
  tracking. Catch-up, history, and input-edge queues remain bounded. This landed through
  `71d38a3` and merge `e370880`; 25 focused movement tests, the complete client-world and sim
  suites, the full locked Rust workspace, strict workspace Clippy, formatting, and architecture
  enforcement are green. The controller deliberately cannot authorize network transmission:
  production remains `FreeCamera` and sends no local position updates. Enabling `Physics`
  authority requires the remaining movement strata plus live server-authoritative verification.

- **PR #6 Phase 3 integration record (2026-07-26).** Both independently reviewed lanes landed
  with history preserved. The input-parity lane `codex/pr6-input-parity` was approved at
  `c1bb584` after five independent Sol-high review rounds and four fix rounds, with final
  decision `APPROVE` and no findings, then landed as merge `cfdf897`. The correction and
  acceptance lane `agent/pr6-phase3-completion` was approved at `e099529` after six independent
  Sol-high review rounds and five fix rounds, with final decision `APPROVE` and no findings,
  then landed as merge `a9593e7`.
  - Post-integration deterministic verification on the merged tree passed:
    `cargo test -p semantic-input --locked` (53 passed);
    `cargo test -p bedrock-client --locked` (lib 420, assets 41, hud_assets 9,
    inventory_router 3, physics_assets 2, doctest 1; all passed);
    `cargo test -p protocol --locked` (21 test binaries, all passed);
    `cargo test -p client-world --locked` (lib 236 passed / 1 ignored,
    entity_runtime 11, item_actions 14; all passed);
    `cargo fmt --all -- --check` passed;
    `git diff --check` passed;
    `cargo clippy --workspace --all-targets --locked -- -D warnings` passed with zero
    warnings and is CI's exact Clippy command; and
    `Invoke-Pester -Script 'scripts/tests/acceptance/Phase3.Tests.ps1' -PassThru` passed
    89/89.
  - `cargo test --workspace --locked` was not run locally as a single invocation. The
    per-crate suites above were run instead; CI runs the workspace form.
  - Implementation and deterministic verification are complete. Native/live and performance
    acceptance remain open and are **not** closed; Phase 3 is not complete. No native/live
    acceptance checkbox is closed by this integration.
  - Production outbound `Physics` transmission remains intentionally disabled. Enabling it is
    a separate reviewed change.
  - The network lifecycle emits one bootstrap per `NetworkHandle`; same-handle session
    replacement remains a known uncovered lifecycle.
  - Follow-up harness correction: by owner decision touch parity is deprioritized and does
    not gate Phase 3 acceptance. `CandidatePhysics` requires keyboard/mouse and gamepad
    witnesses, while its manifest and final evidence name Touch as `Deferred`, attribute the
    deferral to the owner decision, and leave touch parity open rather than treating it as
    observed or satisfied.
  - `Phase3Launcher.ps1` now supports authenticated `Zeno` runs at
    `zenomc.org:19197`, following the same candidate/free-camera scenario and five-minute
    minimum used by the other external targets. Zeno is the low-population official-BDS
    server-authority target for movement rejection and correction checks.
  - Pre-existing, out-of-scope observation: adding `--all-features` to Clippy fails in the
    vendored `crates/protocol/vendor/jolyne` crate because optional dependencies are not
    vendored. This work did not cause that failure, and CI does not use `--all-features`.

- **Phase 3 local BDS smoke findings (2026-07-26, four runs at BDS 1.26.32.2).**
  These runs found defects; they did not validate native parity, remote-server behavior, or
  Phase 3 acceptance and do not advance Phase 3 closure. Phase 3 remains incomplete; native/live
  and performance acceptance remain open.
  - Fixed and confirmed only against local BDS 1.26.32.2: blob-cache admission exhaustion at
    the former 256-transaction cap was fatal and is now recoverable, improving session survival
    from 9.1 seconds to 114–187 seconds. Zero-blob cache-miss responses were incorrectly treated
    as invalid, causing arbitrary FIFO retirement and a feedback loop; they are now successful
    no-ops, with live runs reporting `skipped_miss_responses = 0`,
    `retired_cached_transactions = 0`, and 283 / 4,666 empty responses handled cleanly.
    Skip telemetry is now separated by reason and resync lifecycle counters exist; this
    observability isolated the remaining defect in one run. Before the loop fix, measured join
    high-water marks were 1,194 / 845 pending transactions, independently showing that the old
    256 cap was under-calibrated.
  - Open, root-caused, and not fixed: blob-cache FIFO head-of-line blocking in
    `crates/protocol/src/blob_cache/resolver.rs`. Cached chunks, ordinary packets, and world
    events share one `pending + ready` budget (`resolver.rs:153`, `:249-254`, `:287-292`,
    `:454-459`), while draining requires the front transaction to have every hash cached
    (`:811-817`). One unresolved cached transaction therefore blocks hash-free world events and
    all later work. At capacity cached packets are skipped (`:387-392`), and world events are
    silently and permanently discarded without stored retry or resync (`:271-275`).
  - Live evidence at `b29966d`: pending transactions pinned at 2,047/2,048 in both scenarios;
    `skipped_world_events` was 9,139 / 15,002 and cached transaction-pressure skips were
    2,312 / 3,953. Byte-pressure, semantic-shape, unsolicited, and integrity skips were all
    zero, as were all resync lifecycle counters.
  - Pressure-skipped `SubChunk` packets receive no resync because
    `queue_level_chunk_resync` returns `None` for non-`LevelChunk` packets
    (`resolver/helpers.rs:35-40`). They fall back to two client-world retries
    (`stream.rs:101-102`), then finish with `collision_authoritative = false`
    (`stream/retries.rs:247-255`) while the column is still added to `loaded_columns`
    (`retries.rs:39-47`), so cohort completeness can be misleading. Consequently
    `mutation_coordinate` remains `null` (`app-metrics.json:7`);
    `RUST_MCBE_WORLD_READY` never fires (`app/src/acceptance/mutation.rs:270-272`), the
    60-second acceptance clock never arms, and runs hang until the launcher's 180-second bound.
    **No `phase3-final.json` has ever been produced; there is no Phase 3 acceptance verdict of
    any kind.**
  - Open separate defect: the local Go bridge reported
    `invalid checksum of packet 6217`, then the local listener was forcibly closed
    (OS error 10054). Saturation is neither its cause nor consequence: FreeCamera saturated
    well before it, while CandidatePhysics saturated more severely without a checksum error.
    Likely investigation targets are cipher-counter divergence, send-buffer reuse, or frame
    corruption in the Rust-to-Go bridge; this needs a separate instrumented investigation.
  - Open and unchanged: every CandidatePhysics run still reports `physics_tick_overflow` with
    `detail.dropped = 5` and an authority-fault violation. The prior diagnosis is that
    "collision data unavailable, wait" and "cannot keep up, dropped time" are conflated in
    `dropped_ticks`, and any nonzero value revokes authority. This is not fixed.

- **PR #6 resumed closure record (2026-08-02, local commit `17d3627`).** The previously open
  resolver, transport, collision-readiness, and movement-admission defects above are fixed in the
  PR worktree, but the commit is not pushed and Phase 3 remains incomplete.
  - Blob-cache cached, ordinary, and recovery lanes are independently bounded; pressure no longer
    pins ordinary intake. Cached SubChunk admission is explicitly rolled back with exact-Y
    recovery only when client-world recorded that admission. Coalesced and stale admissions clear
    all markers before one bounded retry. Pending and reconstructed-but-unpublished transactions
    preserve rollback across semantic and transfer resets.
  - Cached status delivery is cancellation-safe. Socket transport retains the exact encrypted
    frame, the session drains that frame before exposing admission, and transfer rollback is
    emitted before any replacement-candidate status or admission.
  - This interim commit defaulted production to server-authoritative `Physics`; the final
    integration head recorded below restores the binding candidate-only gate. Its bounded
    fixed-tick catch-up, collision readiness, reanchor, send cancellation, and terminal-drain
    paths retain or fail closed instead of silently dropping movement authority.
  - Deterministic verification at this commit passed 2,472 workspace tests (13 ignored),
    strict workspace Clippy, formatting, 106 Phase 3/FastTransfer Pester contracts, focused
    rollback/reset/cancellation regressions, and the debug client build. Independent latest-diff
    review returned correct with no blocking findings.
  - The committed local BDS `FreeCameraSilence` run
    `phase3-020595ebff304770b201765b51070585` did **not** close live acceptance. It timed out:
    terminal telemetry reported 256 retained cache transactions, 5,626 pressure skips,
    31 recovery requests, 12,770 pending mesh jobs, a one-item publication cap, and no clean
    shutdown. The run rendered 477 resident meshes but remained under sustained load.
    Release-budget evidence, a version-matched native lighting comparison, clean live shutdown,
    integration, and final PR acceptance therefore remain open. The Linux acceptance shell suite
    was not exercised by that Windows-only run.

- **PR #6 blob-pressure convergence follow-up (2026-08-02, local commit `4726be0`).**
  The cache-pressure deadlock exposed by the committed run above is fixed and independently
  reviewed, but Phase 3 acceptance remains open.
  - The terminal state at `676719f` retained 255 unresolved transactions plus one same-column
    reconstructed transaction, then later all 256 slots as unresolved work. New cached packets
    were skipped indefinitely, so incomplete columns could not enter `loaded_columns`; the
    unmodified 3x3 lighting-context gate correctly retained 1,968 light jobs with no worker able
    to dispatch them.
  - At the transaction or aggregate recovery-slot bound, the resolver now rotates the
    deterministic oldest unresolved transaction through its existing exact recovery and
    hash-owner promotion path. LevelChunk replacement can use the released slot immediately.
    SubChunk replacement waits until the prior exact recovery is observable, preventing a new
    admission from being erased by an older rollback. Distinct and coalesced secondary recoveries
    retain exact request/accounting counts, including staged- and pending-byte rejection.
  - Deterministic verification passed all 282 protocol tests. Focused regressions cover both
    transitions to the transaction bound, eventual multi-column SubChunk admission, same-position
    recovery-before-admission ordering, and distinct/coalesced recovery accounting. Independent
    latest-diff review found no remaining blocker.
  - Clean committed BDS run `phase3-3e439a915ab648619f496236364297c8` proved the deadlock removed:
    the required cohort reached 314/314; pending light work fell to zero; pending mesh work fell to
    zero; transparent sorting committed and presented generation 1,020; and shutdown completed
    with app exit code 0. Cache occupancy continued bounded 255/256 rotation instead of pinning
    publication.
  - The launcher still timed out and `world_ready` remained false, so this is not Phase 3
    acceptance. The remaining world-ready predicate, release-budget evidence, version-matched
    native lighting comparison, integration, and final PR acceptance remain open.

- **PR #6 world-ready acceptance follow-up (2026-08-02, local commit `a8d0866`).**
  The cache-enabled BDS `FreeCameraSilence` path now produces a clean, attributable
  verdict, but this debug run does not close Phase 3 performance or native-parity gates.
  - `phase3-180d6ca5fd7e4dd2b28999c10cabe8c1` is a clean 60-second run built from
    `a8d0866`: `phase3-final.json` is `valid`, `WORLD_READY` was emitted, the app and
    core both exited 0, and the launcher did not time out.
  - Vanilla BDS load peaked at 1,695 retained/pending blob transactions after raising
    the client admission bound to the measured server burst. Intake never skipped a
    packet, no cached transaction was abandoned, and no recovery request was needed;
    the prior 256-entry pressure loop is absent. Redundant missing requests and empty
    miss responses remain measured efficiency work, not a demonstrated correctness
    failure in this run.
  - World-ready presentation now accepts a stable drawn superset of the visible
    allocation manifest while requiring identical drawn manifests across the two
    frames. Relevant admitted network ingress, forced-remesh evidence, and world-ready
    presentation each carry an independent monotonic fence, and camera input is frozen
    only while the presentation gate is armed.
  - Deterministic verification at this commit passed 2,487 workspace tests (13 ignored),
    strict workspace Clippy, formatting, 93 Phase 3 Pester contracts, focused render and
    world-ready suites, and the debug client build. Independent latest-diff review
    approved each behavioral tranche.
  - This was a debug `FreeCameraSilence` smoke, averaging 7.62 FPS while the world
    streamed. It is not release-performance evidence. CandidatePhysics, external-server
    matrices, version-matched native comparison, release budgets, integration, and final
    PR acceptance remain open.

- **PR #6 final integration follow-up (2026-08-02, local commit `f381a19`).**
  The final local head is not yet pushed or integrated.
  - Normal production sessions again install `PhysicsAuthorityGate::ProductionDisabled`.
    `CandidatePhysics` and authenticated `FastTransferWitness` are the only attributable
    harness lanes that add `--phase3-candidate-physics`; `FreeCameraSilence` remains
    candidate-free and adds only `--auto-fly`. Dry-run and final evidence both report
    `production_physics_default_enabled=false`.
  - The first pushed-head CI run exposed ten architecture-size violations. Coherent private
    module/test extractions now restore the unchanged policy without deleting coverage or
    relaxing a limit; the exact local architecture check passes.
  - A final live run exposed a nondeterministic process hang after `app.run()` returned.
    The bounded shutdown watchdog had been completed before synchronous network/App cleanup.
    The watchdog now remains armed through `NetworkHandle::shutdown()` and explicit App drop;
    `SHUTDOWN_COMPLETED` is emitted only after both return.
  - Clean BDS run `phase3-1326f452cbe34833b1d5da44c81b6ca0` at `f381a19` produced a
    `valid` 60-second `FreeCameraSilence` verdict, `WORLD_READY`, app/core exit code 0, no
    launcher timeout, no watchdog firing, and an ordered `SHUTDOWN_WATCHDOG_ARMED` then
    `SHUTDOWN_COMPLETED` tail.
  - The final safety integration passes formatting, the architecture checker, all 496
    bedrock-client tests, strict bedrock-client Clippy, and all 106 Phase 3/FastTransfer
    Pester contracts. The preceding integrated head passed all 2,487 workspace tests and
    strict workspace Clippy. Linux acceptance remains delegated to CI.
  - This is one BDS FreeCamera smoke, not CandidatePhysics, external-server, native-parity,
    release-performance, or Phase 3 closure evidence. Those gates remain open after PR merge.

- **Protocol-2168 PlayerAuthInput semantics follow-up (2026-08-15).** A live normal-gameplay
  attempt was rejected by Lifeboat with `We've detected movement cheats`; a separate Lunar
  attempt timed out, which is recorded as a symptom rather than attributed to the same cause.
  Comparison against `oomph-ac/bedrock-docs` at `bd37783e` proved that `PosDelta` is the
  simulator's predicted end-of-tick velocity, not the difference between consecutive network
  positions. Completed and correction-replayed physics samples now retain that velocity, emit
  horizontal/vertical collision hints, and emit all four processed diagonal flags. Non-finite
  velocity fails the existing local-authority boundary closed. The protocol-2168 wire fixture
  remains byte-exact and deterministic tests cover velocity, replay/reanchor preservation,
  collision hints, all four normalized digital diagonals, non-diagonal analogue/cardinal
  exclusions, and invalid velocity.
  This correction does **not** close Phase 3: fresh native/live Lifeboat and Lunar verification
  remains required. Independent raw/processed/analogue input carriers and an authoritative
  protocol-2168 physics-registry generation path also remain open; the older registry was not
  relabeled or regenerated without evidence.

- **Repository-wide Bedrock client-reference audit (2026-08-16).** Comparison against
  `oomph-ac/bedrock-docs` at `bd37783e` found and corrected three additional retained-state
  mismatches: normal `ItemStackRequest` IDs now begin at `-3` and descend through the negative
  odd namespace without wrapping positive; an accepted slot correction that omits a positive
  stack-network ID retains the predicted/authoritative ID; and remote main-hand and offhand
  equipment are stored independently so a later update cannot erase the other hand. Focused
  protocol, inventory-ledger, and client-world tests cover these contracts.
  This audit does **not** close the surrounding parity gates. Independent raw, analogue, and
  processed `PlayerAuthInput` vectors; independently measured mouse sensitivity/window
  behavior; post-login `Transfer`; bounded entity-link endpoint/pending/cycle handling;
  `SetHud` and the broader JSON-UI controller surface; resource-pack activation; crafting;
  combat; and world ticking remain open where already scoped by their phases. Required server
  packs are forwarded as required, as vanilla receives them: a join whose required packs cannot
  all be acquired or applied is refused with vanilla's resource-pack message rather than
  downgraded to optional.

- [ ] **3.4 Semantic controls and camera perspectives.** `P3.4-INPUT-CAMERA`
  Touch parity remains an explicit open closure item. Its owner-deprioritized witness does
  not gate the Phase 3 scenario verdict, and a passing candidate run does not close touch.
  **Provisional (incomplete, closes no acceptance gate):** sprint latch/double-tap/toggle
  options, forced sneak and crawl under low ceilings, mode entry/exit timing, elytra
  gliding, the double-tap window and scaffolding descent still need native measurement.
  **Flight and liquid correction (2026-10-04, full parity gate remains incomplete):**
  vanilla applies independent flight vertical drag,
  creative hover before movement, sprint water drag, independent water vertical drag,
  held liquid ascent/descent and swimming pitch steering. The simulator now implements
  those ordinary-player paths with behavior regressions. Keyboard input now
  carries the missing processed `WantUp`/`WantDown` lanes read by the current server
  movement handler; in-session position snaps preserve the locomotion tracker. Swimming
  upward steering samples the primary liquid material at the tick-captured pose eye anchor.
  Desktop swim entry now checks head water and view direction; continuation follows the
  native input, hunger, surface and standing-space conditions rather than requiring sprint
  or forward input. Body sensing uses the current position and previous pose. Liquid ledge
  escape uses the resolved pose box for swimming as well as ordinary water/lava travel,
  and retained dry swimming uses ordinary travel. Prediction and correction replay retain
  the native swim blend and previous pose flag, with blend updates before swim triggers and
  transition jump suppression. Standing-space probes use the native box inset.
  Liquid prediction now derives current vectors from palette liquid depths and native
  material/face facts, including lower-neighbor gradients and falling-water pull. It
  uses native cell order, float normalization and water/lava impulses before jump and
  travel, with the preceding flying flag suppressing flow. Historical world snapshots
  retain the same current query during correction replay. The integrated client builds;
  tests and the affected verification suite were skipped at the user's request. The
  user accepted flight, swimming transitions and flowing-water movement in the rebuilt
  macOS/Metal client on the local BDS listening on 19132. This manual acceptance covers
  the reported movement defects; the broader parity boundaries below remain incomplete.
  See `docs/reference/flight-control-corrections.md` and
  `docs/reference/liquid-movement.md`, `docs/reference/liquid-currents.md` and
  `docs/reference/swimming-trigger.md` for identified
  bodies and boundaries. Touch and stalled-entry swim predicates,
  unregistered flow materials and specialized directional/waterlogged flow faces,
  specialized jump paths, bubble columns, custom movement components and
  complete waterlogged/surface behavior remain open. Controlled live results are recorded
  separately; source-derived regressions alone close no acceptance gate. Wire edges for
  swim/glide/crawl/fly and `PersistSneak` still need complete native input comparisons.
  Glide travel, firework glide boosts (replayed from their stamped tick), glide start/stop and
  the seven-tick flight double-tap follow the identified vanilla systems; the held-jump glide
  lift gated by an unidentified movement ability and geyser boosts remain incomplete.
  Water and lava travel read the underwater and lava movement attributes, and a swimmer's
  dolphin boost scales speed and drag as vanilla does; riptide launches remain unimplemented.
  Incomplete safety bound (a known divergence; vanilla is unbounded): Depth Strider's water-speed
  blend target is clamped so the steady water velocity stays inside the simulator's block-sampling
  budget. Land movement speed is not clamped.
  Honey jump/slide, soul speed and depth
  strider coefficients are provisional (honey and soul speed have no public value). Riding
  suspends player physics and streams steering input with boat paddle flags; rider seat
  following, client-predicted vehicles (`IsInClientPredictedVehicle`), horse jump wire
  signalling are not implemented. Sweet berry bush slowdown is written in `tools/registrygen`
  (unverified state count and coefficients) and the sim, but the physics carrier is not
  regenerated: at reconcile run `go -C tools/registrygen run . -physics-v2168-out
  crates/assets/data/block-physics-v2168.bin -physics-v2168-sha-out
  crates/assets/data/block-physics-v2168.sha256 -physics-v2168-breg <v2168 BREG>
  -physics-v2168-manifest <manifest> -pmmp <pmmp root> -prismarine <prismarine root>` and
  fix any count or provenance mismatch it reports for `minecraft:sweet_berry_bush`.
  Client-predicted vehicles, precisely: vanilla registers
  boat and horse "client predicted" systems plus boat paddle/move/friction systems, and a boat's
  friction comes from the block under it. The predicate that sets the predicted state, the boat paddle/turn/acceleration
  coefficients, and the horse travel coefficients remain unknown, so no vehicle
  simulator was built and no value guessed. Missing before it can be built: (1) the predicate and the input-to-vehicle
  mapping, (2) boat and horse constants measured from a native client, (3) vehicle position,
  delta and rotation carried by `PlayerAuthInputSnapshot` (not yet in the snapshot type).
  `ClientMovementPredictionSync` is sent (fields match vanilla and the pinned gophertunnel) after a server correction is applied, at most once per second, and is
  skipped (counted, debug-logged) while any of the six attribute-map values is unset;
  live-test gate item: confirm anti-cheat servers accept the sync; the
  vanilla timer interval, the attribute names for friction/bounciness/air drag (sent as
  1.0/0.0/1.0 provisionally) and extended actor-flag word 2 (sent as zero) need measurement.
  `IsInClientPredictedVehicle` is deliberately never set: the public notes state riding does
  not imply prediction and no vehicle simulator exists, so all rides send ordinary player
  input. The horse jump has no dedicated packet in the pinned gophertunnel; it rides the raw
  jump flags and the mount's jump strength.

## Phase 4 — Entities and other players

Scope: actor lifecycle packets, metadata/attributes, movement interpolation, biped rendering
with standard + persona skins (skin data arrives via PlayerList/AddPlayer), name tags, vanilla
mob geometry + textures from bedrock-samples, **molang subset** for vanilla animation
controllers (walk cycles, look-at; documented cut-line, static pose fallback), item entities
and dropped-item rendering, paper-doll first-person arm/held item.

**Phase 4 progress (kept current as work lands):**

- [x] **4.1 Bounded actor lifecycle ingestion.** The protocol layer now materializes and
  normalizes `AddPlayer`, `AddActor`/`AddEntity`, `RemoveActor`/`RemoveEntity`, absolute and
  delta actor movement, `SetActorData`/`SetEntityData`, `UpdateAttributes`, and `PlayerList`
  into vendor-neutral events. All retained identifiers, names, metadata/NBT, attributes,
  modifiers, properties, and roster collections have explicit limits and finite-number
  validation. The app owns a sparse runtime-ID actor store with a unique-ID removal index,
  atomic duplicate replacement, bounded player roster, FIFO/session/dimension rejection, and
  dimension/session reset semantics. Foreign `MovePlayer` packets route to this actor stream
  without entering the local-camera path, and actor updates do not dirty chunk/light/mesh
  state. This substep deliberately does **not** implement entity rendering, interpolation,
  skins/persona data, Molang, name tags, or item/first-person visuals; those remain open Phase 4
  work.

- [x] **4.2 Standard remote-player render slice.** `PlayerList` now retains explicitly bounded
  classic 64x64, 128x128, or 256x256 RGBA skin images, with deterministic unavailable states for
  persona, malformed, or retained-budget-exhausted data; `AddPlayer` UUIDs join those roster
  profiles to the sparse actor store, whose cumulative retained skin bytes remain capped across
  incremental roster packets. Foreign `MovePlayer` metadata preserves head yaw, ground state,
  mode, and signed source tick while retaining explicit packet-coordinate origin. `AddPlayer`
  and `AddActor` remain feet-space, while `MovePlayer` and absolute actor movement are normalized
  exactly once after retained actor kind and metadata are known. Standing players use Bedrock's
  `1.62001` offset; sleeping players use `0.2` only when retained sleeping flags prove the pose;
  retained item actors, falling blocks, and the reviewed vanilla minecart identifiers use `0.5`;
  `minecraft:boat` uses `0.375`; and primed TNT uses half its retained bounding-box height with
  the vanilla `0.49` default. The
  separate `AddItemEntity` spawn packet still awaits ingestion. Partial/delta movement and actors
  without a reviewed offset remain unchanged. The client-world store performs
  Oomph-style three-20-Hz-tick player convergence;
  teleports and actor replacement snap immediately, and the renderer performs the distinct
  adjacent-tick frame interpolation with shortest-path angles. Camera-frustum and conservative
  192-block distance culling happen before the runtime-ID-ordered 128-instance upload cap, so
  invisible players cannot displace visible ones. This path consumes no free-camera or local
  movement state and sends no free-camera position updates. One
  custom `Opaque3d` instanced draw expands a shared six-cuboid standard
  Bedrock biped vertex buffer and samples a bounded 64x64 texture array, with no `StandardMaterial`
  or per-actor Bevy mesh. Missing/unsupported/invalid skins use the documented, locally generated
  `Cinnabar Default` skin (no Mojang or diagnostic bytes). Focused protocol/app/render tests,
  shader parsing, no-op-backend binding-layout validation, pipeline specialization, format, and
  warnings-denied workspace Clippy are green. Persona/custom geometry,
  legacy 64x32 skins, outer skin layers, limb animation/Molang, name tags, equipment, mobs/items,
  first-person visuals, live render-pipeline creation on a hardware backend, and multi-client
  visual evidence remain open Phase 4 work.
  The invisible-player capture fixes retain PlayerSkin updates, polygon bodies, inflated planes,
  named classic models and native skin texels, and apply vanilla classic alpha validation.
  Persona face/body atlases now retain their own geometry and texture and follow the pack
  animation rate and blink controller. Player appearance parity remains incomplete: local
  piece/tint assembly, repository trust/fallback decisions, geometry version upgrades and persona
  atlases above the admitted size still need validation. Authored skin visibility bounds now reach
  render, cave and animation culling; complete transformed-bounds parity still needs evidence.
  Offline capture renders establish coverage; they do not close native visual parity.
  The complete absolute-movement origin correction, regression suite, independent review, and
  post-merge protocol/client-world/app verification are green through `e7c85ea`; the LBSG live
  ground-contact witness remains open under 4.4.

- [ ] **4.3 Data-driven Bedrock entity rigs and animation.** `P4.3-RIGS` Ingest the pinned
  vanilla resource pack's `entity`, `models/entity`, `animations`,
  `animation_controllers`, `render_controllers`, and `textures/entity` trees as
  bounded compiled assets. Evaluate only the reviewed Molang subset needed by
  those controllers, drive poses from protocol metadata/attributes and the
  20-Hz actor state, then perform the distinct adjacent-tick frame
  interpolation in the renderer. Preserve shared geometry/material/texture
  storage and bounded per-frame actor work.
  **Bounded asset-catalog tranche complete (2026-07-16):** the pinned vanilla
  `entity`, geometry, animation, animation-controller, render-controller, and
  entity-texture trees now compile into the deterministic `MCBEENT3` carrier
  with exact source-manifest provenance. The real reviewed pack produces 3,247
  source records, 2,993 symbols, and 3,071 dependency edges (2,929 internal and
  142 explicitly external); duplicate identifiers remain selectable candidates
  rather than silently choosing a generation. Texture identifiers and
  conditional render-controller keys resolve canonically, startup fails closed
  on stale provenance, and generated carriers/reports and Mojang payloads remain
  ignored. Independent review and post-merge assets/compiler/client tests and
  strict checks are green. **Geometry payload tranche complete (2026-07-16):**
  the carrier now preserves bounded, deterministic geometry, bone, cube,
  pivot/rotation, mirror, inflate, and UV payloads. Legacy inheritance resolves
  sparse overlays through the selected parent chain, exact reviewed legacy/modern
  schema versions fail closed, and geometry JSON rejects duplicate semantic keys
  recursively. The complete `105107d..d84667d` behavior range received fresh APPROVE
  after all three Important review findings were fixed and landed as merge `1e4ba3c`.
  The subsequent behavior-preserving compiler split through `4a6696b` independently
  passed review and architecture enforcement and landed as merge `73b8de7`.
  Animation clip payloads, the reviewed Molang/controller evaluator, runtime rig
  consumption and skeletal GPU skinning/posing, and native animated-actor evidence
  remain open.
- [ ] **4.4 Live actor ground-contact and interpolation witness.** `P4.4-LIVE-ACTOR` Join
  `play.lbsg.net:19132` with the normal authenticated core, observe at least one
  remote player's spawn, ordinary movement, rotation, and teleport, and prove
  that AddPlayer/MovePlayer origins, three-tick convergence, frame
  interpolation, and the shared biped model keep both feet on the same ground
  plane without a 1.6-block jump. Keep the visual standing eye height (`1.62`)
  distinct from Bedrock's standing-player movement network offset (`1.62001`).
  Capture bounded native visual and packet/pose evidence.

- [ ] **4.5 Held items, actions, dropped items, and viewmodel.** `P4.5-ITEM-ACTIONS`
  - [ ] Pickup sound and the three-tick copied-item flight survive immediate server
    removal. The squared trajectory and collector offset follow the current vanilla
    rules; a version-matched rendered comparison remains incomplete.
  - [x] Render supported ordinary opaque full-cube held blocks using the current
    selected stack, world materials, and session-bound local presentation authority.
    A live controlled check covered Stone, distinct crafting-table faces, empty-slot
    and unsupported-item fallback, resize, and reconnect. Current GPU completion
    suppresses only the matching CPU fallback; rejected or stale submissions retain it.
    This is a static geometry slice, not complete item, pose, animation, lighting,
    or matched retail visual parity. Tinted, animated, partial, and unsupported
    items remain on the existing fallback path.
  - [ ] Server custom opaque cubes now use the shared block-item face sheets for
    inventory and held rendering. Preserving higher-resolution custom face textures
    is incomplete: the sheet builder currently selects the mip matching
    `BLOCK_ITEM_FACE_SIDE`. Custom shapes, tinting, animation, and matched retail
    visual parity remain incomplete as well.

## Phase 5 — Interaction, inventory, UI

Scope: block breaking (server-auth crack progress overlay), placement, item use via
`InventoryTransaction`/`ItemStackRequest`; hotbar + survival/creative inventory + containers
(chest/furnace/crafting UIs); forms (modal/menu/custom JSON forms — Lunar's ClickUI depends on
these); chat with Bedrock formatting codes; HUD (health/hunger/armor/air, bossbar, scoreboard,
title/actionbar); Bedrock bitmap font rendering from pack `font/` assets. Taste bar applies:
UI phases get fable-5/opus-4.8 review before merge.

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

**Approved Java-style gameplay HUD contract (2026-07-19):** the clean-room
visual reference is Minecraft Java Edition 26.2 with default resources on
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
map), bow/crossbow draw frames, animated compass/clock frames, trimmed armor and
broken elytra overrides, and native icon acceptance.
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
the retail GUI tessellator.
Server item components (StartGame registry) drive display names, rarity and
hover colours, durability maxima, stack-merge capacity, held grip, wearable
slots and use durations. Item glint (provisional): stacks vanilla
marks as glinting (an `ench` list, the glint component, always-glinting vanilla items)
draw a procedural scrolling purple overlay in HUD and JSON-UI item cells, not
the retail glint texture; held items and the item viewmodel do not glint.
`minecraft:render_offsets` is not applied.
Initial negotiated item-registry binding for world item visuals is also separate
from the inventory-ledger bootstrap fix. The visual resolver still starts from
built-in mappings and only replaces them on a later registry event. Preserve
session/FIFO ordering when adding that initial binding; do not claim custom-item
visual authority from the merge fix alone.

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

- [ ] **5.6 Server forms.** `P5.6-FORMS` Implement modal/menu/custom JSON forms, validation, cancellation,
  keyboard/controller/touch navigation, and response routing. This is the prerequisite for
  Lunar ClickUI compatibility.
  Status: server resource-pack UI now reaches the JSON-UI form engine (per-layer `ui/*.json`
  merge, `$screen_content` routing, expression, view-binding, and vanilla factory rules), with a resolve/layout cache and raw-edge pointer input. Checked only in
  tests against local pack fixtures, not against a live vanilla capture; the virtual-root scale
  constants, per-visual-line label alignment, and image aspect defaults remain unconfirmed.
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
  see [desktop video settings evidence](docs/evidence/desktop-video-settings.md).
  **Incomplete parity:** the transferred behavior has not been compared with a
  version-matched native client. The settings context selects the pinned pack's
  compact spacing branch; Bedrock's service-controlled spacing treatment is
  not mirrored. Language-specific minimum-scale dialogs,
  safe-zone adjustments, and touch/console behavior remain unimplemented in
  this adapter. Linux rendered-frame and live-input verification validates the
  local wiring only; it does not close this or any broader UI parity gate.

## Phase 6 — Online product surface

Scope: main menu + settings (video/controls/audio/account); server browser (saved servers);
Realms and friends lists in-UI (control-channel data from Phase 1) with one-click join;
transfer/reconnect UX; **server resource packs applied at runtime** (cache from core →
`crates/assets` hot-swaps textures/models/sounds/lang over the vanilla base — the asset
system from Phase 2 must have been built pack-stack-aware); disconnect screens with real
reasons; auth/device-code UX polish. Optional stretch: Lunar module toggles surfaced in-client
via control channel (v1.x, not v1).

- **Marketplace rows — provisional, incomplete.** The home is the `storeRoot` known page. Curated rows carry
  their offers inline; `StoreRow` and `HeroRow` draw vanilla's pre-content-card cards, as the live session
  config sends no `contentCardStyles` (the client's content-card flight is not read). Query rows are filled
  from their first query via `marketplace.Query.SearchFilter` onto PlayFab `Catalog/Search`, whose vanilla
  request body is unconfirmed. Not drawn yet: `PromoBanner`, `NavButtonRow`, `CoinBundleRow`, the `Layout`
  top-bar row, offer type badges (icon overlays), the rating count beside the average, and the row's "See
  All" tile and page (the item list's `linksTo`). Does not close the store parity gate
  (`docs/marketplace-services.md`).

## Phase 7 — Local worlds on dragonfly

Scope: core embeds/spawns dragonfly (`platform/pc-server` and dragonfly-server skill patterns
as reference); world create/select/delete UI; settings (name, gamemode, seed, flat/normal);
LevelDB world persistence via dragonfly; pause/resume semantics on window focus; same client
path as online (core points the game socket at the local dragonfly). Documented v1 limits:
dragonfly's generation and mob AI parity gaps are accepted, not chased.

Status: provisional (see `docs/local-worlds.md`): BDS 1.26.52.3 (native, or the manifest-pinned container on macOS) for default worlds, dragonfly for Flat worlds; menu create/edit/delete/templates and staged loading are built. Live-verified on macOS through the core's control channel (create, staged open, spawn, server-side teleport and client fall, pause, close with no orphan); the menu click-through and on-screen input were not exercised (locked screen), and the create screen lacks vanilla's Multiplayer, Cheats and pack tabs, so no acceptance gate is closed.

## Phase 8 — Audio, polish, packaging

**World-drop audio:** successful world-input single and whole-stack drops now
emit one local `drop.slot` cue through the active pack, without waiting for or
repeating server replies. Failed and inventory-screen drops stay silent on this
route. See [the rules and regressions](docs/reference/item-drop-audio.md).
Matched-version live audio acceptance remains incomplete.

**Audio callback scheduling:** Hardware playback requests Windows MMCSS priority
once per pulling thread, with a current-thread priority fallback. Submission-time
resampling leaves the frame thread alone; capture playback keeps its existing
scheduling. Windows scheduling and audible acceptance remain incomplete.

**Sound-bank loading:** First-use lookups queue file descriptors; the existing
workers read and decode the sounds. Loading queues up to four finite sounds each
for UI, footsteps and interactions, with a 1 MiB input limit. Generation checks,
admission limits and PCM cache accounting stay in place. Live first-play latency
and cold-cache frame acceptance remain incomplete.

Scope: audio via bevy_audio/kira — sound events mapped through `sound_definitions.json`,
positional sounds, music/ambient (asset-availability audit from Phase 2 decides
bedrock-samples vs. client-assets-import); performance hardening pass against budgets;
macOS .app + codesign/notarize, Windows installer, Linux AppImage; core binary bundled and
lifecycle-managed by the app; local crash records; auto-update channel;
first-run experience.

**Packaging status (provisional):** `packaging/` holds macOS `.app`/DMG, Windows MSI, and Linux
AppImage recipes plus `.github/workflows/package.yml`; first-run asset preparation, local crash
records (never uploaded), signed-manifest update checks, and the core log/backoff helpers are in
`app/src/{first_run,lifecycle}` and `crates/update-manifest`. Unverified until compiled and run on a clean machine: every
recipe, the WiX authoring, and notarization. Incomplete: a graphical progress/consent surface (native
dialogs only), locating a user's own Bedrock install instead of the pinned pack, in-app update
install, mid-session core restart wiring, and any crash upload (removed until a reporting project exists).

**Core joins phase 2 review corrections:** the core uses the fork pinned in `core/go.mod`,
including the listener shutdown fix. It preserves batch boundaries, retains friend-world Xbox
services until the joined session leaves, and checks required acquisition separately from the
selected pack stack. Offline fixtures cover all required-bit combinations and selected subsets.
The comparison benchmark now accounts for every packet and measures the first nonempty flush;
manual ticks and barriers cover idle delivery, coalescing, and failure attribution.

Rust forwards startup Transfer as a typed reconnect event. The startup contract is
radius → loading-start → local presentation readiness → loading-end → initialized, with
an actual Rust-through-core order fixture. See [the vanilla startup rules](docs/core-join-startup.md).
**Incomplete parity:** the existing terrain presentation thresholds, consent UI, dimension
transitions, and matched retail/live visual and timing evidence remain open. This packet-order
correction does not close those broader gates. No live server was used for these corrections.
**Dragonfly join (provisional, incomplete):** Dragonfly streams no terrain until initialized, which
deadlocked the gate. A session whose server sent no terrain before spawn now releases once received
work drains. The vanilla zero-terrain completion path is unconfirmed: see the open questions
in [the join evidence](docs/core-join-startup.md). StartGame's vanilla data-driven
definitions are retained separately from server custom visuals: the complete carrier holds their
states, but remote sessions admit them only when StartGame supplies their definitions.
The namespace distinguishes vanilla definitions from server custom blocks; the presence of
`vanilla_block_data` alone does not. A server
block's own item (no components, no item version) stacks to `Item`'s default 64.

---

## Sequencing and program rules

- Order is 0 → 1 → 2 → 3 → (4 ∥ 5) → 6 → 7 → 8. Phases 4 and 5 can run in parallel worktrees once 3 lands (disjoint crates; both consume `crates/world` + `crates/protocol` which are stable by then).
- Each phase starts by converting its scope block into a full task-by-task plan (superpowers:writing-plans), gets brainstorm-level review if its scope shifted, and ends with the requesting-code-review flow. PR-bot adjudication rules apply throughout.
- Every phase must leave `main` in a runnable state (`app` launches, joins BDS, does everything prior phases delivered) — CI runs the Phase 0 acceptance connect as a smoke test forever.
- Protocol bumps during the program: deliberate, one task, lockstep — regenerate valentine defs, run conformance, bump core, bump `registrygen` exports, fix findings. Never mid-phase.

## v1 Definition of Done

From a clean machine: install → sign in with Xbox (device code) → join a third-party RakNet
server, a Realm, and a friend's world (NetherNet) → play survival basics (move, build, mine,
chest, craft, chat, forms) with vanilla look and feel at 60fps on the dev MacBook → create a
local dragonfly world, play it offline, reload it. Server resource packs render. No Rust-side
auth/transport code exists.

---

## Appendix: Rendering Performance Playbook (binding for Phases 0 and 2)

FPS and memory in this client are dominated by chunk meshes; these techniques stack
multiplicatively and are the required approach, not suggestions:

1. **Paletted chunk data stays paletted at runtime.** Mesh directly from palette + packed
   indices; never expand to flat per-block arrays (the naeast2 lesson, client-side). Uniform
   subchunks (all air/all one block) store one palette entry and skip meshing entirely.
2. **Binary greedy meshing.** Per-axis-column `u64` bitmasks; face culling and coplanar
   merging via bitwise ops (target: tens of µs per subchunk, making remesh-on-update ~free).
   Merges split where baked AO/light values differ. References: `block-mesh` crate and
   TanTanDev binary-greedy-meshing demos.
3. **Packed vertices / per-quad vertex pulling.** Local position 5+5+5 bits, face ID 3 bits
   (normal from LUT), texture-array layer index, AO 2 bits, light 8 bits → 1–2 `u32` per
   vertex, subchunk origin as a per-draw push constant. Preferred form: one ~8-byte record
   per quad in a storage buffer, corners reconstructed in the vertex shader, and one shared
   static index buffer for all chunks. This targets roughly 20–40× less mesh memory than
   naive 32-byte vertices.
4. **Custom Bevy render phase for chunks.** No per-subchunk `Mesh`/`StandardMaterial`; use
   one chunk pipeline family with at most two immutable state variants
   (opaque/cutout with depth writes, blend without depth writes) and one shared bind group,
   with `multi_draw_indirect` where available.
5. **Visibility culling.** Per-subchunk frustum culling + cave/connectivity culling
   (Checchi-style: face-to-face connectivity flood-filled at mesh time, then BFS from the
   camera through the chunk graph—the approach used by vanilla).
6. **Budget spiky work.** Decode/mesh/light only on Rayon workers; GPU uploads capped per
   frame and nearest-first; light updates deduplicated and queued; block + sky light baked
   per vertex at mesh time so lighting cost rides the remesh budget.
7. **2D texture arrays, not a stitched atlas.** This avoids mip bleeding, permits greedy-quad
   UV wrapping, and implements flipbooks as layer swaps; mipmaps are generated per layer.
   Use one measured physical array when the reachable deduplicated layer inventory fits the
   minimum target adapter, otherwise at most two equal-format array pages in the same shared
   bind group. More pages, frame dropping, or silent animation degradation are forbidden.

Explicitly deferred past v1: distant-chunk LODs (not needed at a 16-chunk radius), GPU
occlusion queries (cave culling suffices), and mesh shaders.

Resource budget (tracked from Phase 2 onward; reference machine class = Ryzen 5 3600 / mid
Apple Silicon, 16-chunk radius, capped 60fps): combined RSS (client + core) ≤ 650MB
steady-state; steady-state CPU ≤ 15% total; join/teleport bursts may saturate cores but must
settle within ~2 seconds. Baseline for comparison: vanilla Bedrock client on the same
machine runs at 800MB–2GB and 30%+ CPU.

Binding Phase 2 scope: block registry + block-state → model/texture mapping (generated
export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, with
pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome
cross-check, Axolotl Valentine's typed state catalog as a versioned selector reference, and
Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template and
occlusion inputs rather than render/UV authority); vanilla
asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version
(terrain textures, `blocks.json`, flipbooks, and biome colors). The pinned samples have no
block-render model JSON, so deterministic reviewed family generators combine collision
bounds, Dragonfly behavior rules, Mojang texture mappings, and vanilla-reference evidence.
Zuri is not a rendering or asset-system input. BDS
`resource_packs/vanilla` is server-minimal (`blocks.json` + texts only): it is a data
reference, not the texture source. Use the bounded one-or-two-page 2D texture-array scheme
above with per-layer mipmaps; meshing per
this playbook with opaque/cutout/blend layers; a client-side block + sky flood-fill light
engine with per-vertex light baked at mesh time and day/night; biome tinting for
grass/foliage/water; sky, fog, and clouds; chunk streaming/eviction tied to
`ChunkRadiusUpdated` + `SubChunk` request flow. Custom block-entity renderers remain
deferred; chests/signs receive static models in this phase. The Phase 0 performance budget
carries forward, with full remesh of view distance after teleport ≤ 2 seconds.


### Settings chat popup follow-up (incomplete)

The native chat gear now opens `chat_settings.chat_settings_popup` from the carrier.
Persisted mute, color, typeface, font size, spacing, duration and opacity reach the chat
presentation; emote mute, TTS and mentions color remain UI only because those systems
are missing. The compiled open font remains the repository's accepted font deviation.
Font size 5–20/default 10 and spacing 0–100/default 0 are provisional host
ranges, not confirmed vanilla defaults. The scale mapping is size / 10 and
spacing is truncated to one decimal plus 0.001.
Smooth font controls hide for zh_TW, zh_CN, ko_KR and ja_JP. Color defaults (white/yellow),
typeface default still need current-controller confirmation.
Do not close the exact chat parity gate from these provisional values.
Pack controls: `ui/chat_settings_menu_screen.json:68,139,272,299`. The chat settings
controller binds these controls and retrieves their options. Seven colors use the
vanilla indexed palette order.
### Projectile rendering fixes (incomplete parity)

The `fix/projectile-render` investigation fixes item-icon carrier admission and
resolution, sprite UV eligibility, arrow face UV defaults and neutral-profile plane
backs, projectile world yaw, and remote motion retention/initial arrow orientation.
See `docs/projectile-rendering.md` for vanilla rules and failing-first regressions.
Offline frame coverage does not close the native projectile gate. Exact projectile
lerp steps, stuck-state/shake runtime, tipped-arrow behavior, target materials and
lighting, and AddActor velocity-only launch remain open. No live connection was used.

## Biome boundary cache port (2026-10-01)

Incomplete parity work on `fix/biome-blend`: the vanilla
lattice cache replaces the provisional CPU box/shader separable kernels,
including vertical neighbours and inverse-distance weights. Evidence and
remaining questions are in `docs/biome-blending.md`. This does not close
P2.5-NATIVE-BIOME: tint-specific dispatch, graphics-setting selection, native
neighbour-arrival remeshing and the owner's live screenshot attribution remain
unverified. CPU palette previews are not native or GPU acceptance.

Windows DX12 debug builds remain incomplete: FXC's unoptimized compilation of the
generated biome lookup table exceeds its temporary-register limit. The GPU bounds
regression uses optimized shaders with backend validation enabled, matching release
shader compilation. Supporting unoptimized FXC shaders still needs a table-layout
change; the bounds regression does not close that follow-up.


### World-lighting follow-up (incomplete parity)

The shared RGB lightmap implements the vanilla composition and effect formulas.
Current dimension ramps/dispatch, ambient flags, sky-darken input and effect envelopes
remain unverified. AO uses channel maxima and the default shade curve, but registry
shade/solid-render properties, component exponents and special dimension/unshaded
routes remain incomplete. Inset sampling does not implement the separate box-average
route. These corrections do not close RM-01–04 or AO-02–04 in full.

State emission now uses the current trial-spawner, vault, anchor and sensor accessors.
The light registry and target bindings are rebuilt; complete dynamic-emitter parity
still needs copper-bulb constructor constants, cauldron identity and sensor emission dispatch.
Default shaded grass uses the reference packed-byte transform. Water surface opacity
is retained as a vertex byte through the biome carrier and GPU blending. Its final
texture-alpha multiplication and special neighboring-material side factor remain
provisional until the material route is resolved. Swamp grass retains row 255 and
uses the current seed-2345 float simplex sampler at absolute world positions; native
color/blending comparison remains open. None of these changes closes a native gate.

The star field now draws seed-10842 candidate quads with the reference radius,
size, alpha and draw consumption. It remains incomplete: float trigonometry is
used in place of the runtime sine table, and current sky rotation/material blend
state still need verification. This does not close RM-05's numeric/native gate.

Top-boundary sky seeds now reach known occupied cells and use the solver's destination
filter. LP-05 remains incomplete: normal/render packet heightmaps, custom dimension
bounds and initialization before the upper-neighbor readiness gate are still missing.

RM-06 and GEO-01 remain partial as recorded in the continuations below. GEO-02–04
remain open: complete repeater/comparator geometry and per-species offsets are not
implemented. Ordinary cubes and dirt-path models consume authored per-face rotation
masks with position-hashed quarter turns. Compiler and GPU regressions cover path
top/bottom variation, upright side controls and unchanged carried icons; this does
not close the full geometry parity gate.
Static face-to-UV orientation for named cuboids remains incomplete, including the
dirt-path underside. Existing template axes are retained by the rotation fix.
RM-07, RM-09 and RM-10 retain their older-reference-only status. Offline tests and GPU
captures are local evidence; they do not close native visual or shader-performance gates.

### Settings desktop continuation (incomplete parity)

The desktop host now consumes Hide HUD, Hide Hand (animated and fallback paths),
player-name visibility, panorama speed, cloud visibility, darkness strength, HUD
opacity and HUD text-background opacity. Focus-loss pause reads its saved setting;
explicit pause-menu state remains authoritative. These are runtime adapters, not a
closed visual or numeric-default parity gate.

References: P:ui/settings_sections/general_section.json:3302,3332,3374,3407,3544,3636;
P:ui/hud_screen.json:3556–3569.
Vanilla option numeric defaults remain provisional pending current-client confirmation. Cloud and hand preferences do not modify the JSON-UI engine.

Desktop continuation also wires section reset confirmations (Video, Accessibility and
Audio), each using the existing option registry; spyglass turn scaling, secondary
Enter for Chat until that binding is remapped, notification duration and the Creator
chat coordinate copy/paste header. These changes remain incomplete parity until the
full gates and rendered evidence pass. Registered provisional defaults remain
provisional after a reset; a working consumer does not establish a vanilla default.

Sources: P:ui/settings_sections/general_section.json:3000–4058,4738–5506;
P:ui/chat_screen.json:740–904. Spyglass item damping is 0.05.
The coordinate-copy toast behaviour remains unconfirmed. Full Keyboard's alternate layout and smooth rotation rate,
Safe Zone, glint defaults, world Experiments and several unsupported subsystem
controls remain open. JSON-UI engine changes remain on the separate branch.

Glint accessibility factors now reach the existing UI item renderer: strength scales
its additive RGB, and speed scales elapsed time before the procedural phases.
The procedural glint appearance
and phase periods remain provisional; this adapter does not establish texture,
world-item, or entity-glint parity. Current vanilla option defaults/ranges remain open.

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
[item-particle-lighting.md](docs/reference/item-particle-lighting.md).

The user accepted dropped items, particles and survival mining on macOS/Metal,
Retina 2× with the rebuilt client. During the earlier test, survival was
incorrectly taking the creative mining route because Instabuild overrode game mode.
Vanilla selects creative destruction from the creative game mode, not that
ability; the narrow capability correction and transition regression are recorded in
[game-mode-updates.md](docs/reference/game-mode-updates.md). This does not close
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
No live server connection was made. Use RUST_MCBE_MOVEMENT_TRACE=1 for outbound PAI
and the new unthrottled inbound movement and latency-fence records. Normal MovePlayer acknowledgement
no longer needs RUST_MCBE_TELEPORT_ACK; that opt-in still enables unverified extra routes.

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
  - `cargo test -p bedrock-client --locked` (unit, integration and documentation tests)
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
  - `cargo test -p server-experience -p bedrock-client --locked` (35 server-experience
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

### Between-frames world servicing (incomplete native acceptance)

The production client lends `WorldStream` to a `stream-service` thread after each frame's
last schedule and reclaims it before the next frame's first, so decode commits, light and
mesh scheduling and retention evictions overlap extraction and pacing. The frame keeps a
floor poll and still commits the chunk data that a ready block change, retention change
or barrier waits behind. Resident membership, keyed stream state and terrain lookups use
hashed indexes, and mesh halo and neighbour checks read each index once. Offline synthetic
replays (radius 12, 120 Hz) cut frame-thread stream time several-fold and converge to the
same presented meshes ([offline evidence](docs/evidence/async-chunk-servicing.md)); joins
mesh the faced view sooner and rear sub-chunks of nearby columns later. Release captures on
target hardware, live join and streaming budgets, and real-pack meshing and lighting load
remain open.

### Inventory/HUD correction continuation (2026-10-02, incomplete general parity)

Selected-item text now positions its spawned Java-look factory root above the
hotbar, retaining the inherited Bedrock label and animation. Bare block stacks
can split/restack without rejecting nonzero block identity; ingredient plainness
no longer requires zero aux/block identity. Supported recipe shapes are retained
independently of discovery metadata, including high-bit result block identities.
Personal/workbench closes explicitly return crafting inputs and cursor items to
player inventory, dropping only overflow, with sparse dependency preservation
and rejected-return recovery. References and scoped acceptance are in
`docs/reference/selected-item-hud-label.md`, `inventory-block-restacking.md`,
`inventory-recipe-admission.md` and `inventory-crafting-close.md`.

Scoped offline BDS acceptance exercises split/restack, manual 2×2 and workbench
crafts, input/cursor returns and repeated reopen with server-verified counts.
Rendered survival/creative selected-name geometry is inspected at the owner's
Retina scale. See `docs/reviews/inventory-hud-crafting-fixes.md` for exact builds,
local evidence paths and verification state.

Full structural-NBT merge parity, descriptor-dependent capacity/variant rules,
limited-crafting/unlocked-recipe client gating, recipe-book discovery state,
arbitrary container return flags and exact native close/flush timing remain open.
These corrections do not close the overall Phase 5 inventory parity gate.

### Zeqa regression follow-up (incomplete visual/performance acceptance)

Nametag phase traversal, omitted catalog plane backs, active player appearance
lifetime, and matrices cached across rig replacement have focused corrections.
The supplied offline witnesses do not close the live form layout/FPS, missing
hotbar icons, all nametag size/garbling symptoms, or RustMCBE stretched-limb gates.
The player-body report omits equipment and GPU execution. See
`docs/reference/zeqa-regression-investigation.md` for source boundaries, vanilla
references, PNG evidence and the limitations of the capture.

### JSON-UI review follow-up (incomplete live form acceptance)

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

## Go core simplification (2026-10-02)

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
menu doll framing uses the native model origin. Marketplace ribbon fields now
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
remaining limits are in [fish-rendering.md](docs/reference/fish-rendering.md).

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

## Servers catalogue and classic OreUI view

The classic Servers feed now retains ranked featured and creator groups, localized descriptions,
tagged logos/banners/activity artwork and real pong MOTDs. The detail pane renders its populated
sections with independent scrolling and quick optional selection/glimmer motion. Regression
checks cover grouping, rank zero, image roles, selection across catalogue reorder, detail content
and bounded artwork packing. Integrated visible macOS review is in progress. Version-matched
native acceptance, narrow metadata layout, full-capacity badges and server notification panels
remain incomplete; this work does not close those parity gates.

## Freelook extension

Freelook is a requested Cinnabar extension, not vanilla behavior. Hold its configurable
Keyboard & Mouse binding (default F) to orbit a collision-resolved third-person camera
while retaining gameplay facing, movement and interaction direction. Release, UI focus
or window focus loss returns to the prior perspective. Windows/DX12 1280x720 hidden
capture verifies the Freelook/F settings row. Routed tests cover independent rotation,
release/focus restoration, persistence and existing-F migration. A manual in-world
orbit acceptance pass remains incomplete.

## Desktop chat web links

- Requested desktop extension: recognize HTTP(S) links locally in displayed chat,
  including bare web domains, and require an in-game Open/Cancel prompt before
  handing a selected URL to the default browser. Chat messages and server packets
  are unchanged. This Java-style interaction is not a closed Bedrock parity gate.

## Worn elytra glint acceptance

Actor glint uses the vanilla raster, two centered UV rotations and independently
wrapping scrolls. Exact 1.26.50 glint pixel comparison and enhanced graphics glint
remain incomplete and do not close the rendering parity gate.

## Server primitive shapes

The retained renderer and packet pipeline implement the six Script API debug shapes. See
[the Vanilla rules](docs/reference/primitive-shapes.md) for packet patches, geometry, text,
attachment and distance rules. Lifetime deliberately follows server removal packets: the
version-matched client does not autonomously expire a shape from its time-left metadata.

The parity gate remains incomplete. Shared-kind draws do not reproduce native equal-priority
sorting for overlapping coplanar shapes, and batched text does not preserve every per-shape
alpha overlap. Parsed debug text also lacks invalidation when only input mode or interaction
model changes; the shared text resolver does not expose those signals. The finite text atlas
remains an implementation resource bound. The debug material witness is a nearby patch version; a matched native visual comparison remains open.
The 1920×1080 headless macOS/Metal local gallery verifies all six kinds, text background,
color updates, actor following without instance rebuilds, and complete removal.
Synthetic CPU/upload benchmarks and this gallery do not qualify the release hardware frame,
streaming or hitch budgets, or establish native 100k-shape performance.

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
See [the Vanilla rules](docs/reference/rendering.md) for established behavior and evidence
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

## Crosshair preferences

Video settings expose Third Person Crosshair (off by default) and Invert
Crosshair Colors (on by default). Both persist and reset with Video settings.
The third-person option covers both camera directions; spectator and Hide HUD
still suppress the crosshair. Color inversion uses the existing scene blend,
and turning it off preserves the selected pack texture with ordinary blending.
Nine focused crosshair tests pass, covering persistence, Video reset, live
visibility/blend changes, hidden HUD, spectator mode, scaling, and pack textures.
A macOS/Metal client pass at 1920×1080, DPI 1, GUI scale 2 verified centered
geometry, scene-dependent inverted colors versus plain white, both third-person
views, F1 visibility, and legible unclipped settings with working pointer focus
and immediate toggle updates. This verifies the preferences, not broader HUD parity.


## Absorption HUD limits

Absorption uses the local attribute's current points and the vanilla JSON-UI native
heart renderer. Incomplete: the retained HUD stat supports at most 65,535 current
points and health containers remain capped at six rows. Larger valid values are
skipped or bounded; this change does not close an unrestricted custom-health or
visual-comparison parity gate. See `docs/reference/absorption-hearts.md` for the rules.

Local swing publication: duration and progress tests cover both animation modes,
and Java torso turning uses matching committed local ticks. Incomplete: native
player body-turn timing remains on the provisional actor motion model. This work
does not close the broader native body-motion or live visual parity gate. See
`docs/reference/swing-duration.md` and `docs/reference/actor-animation-clocks.md`.

## Server pack compatibility

Galaxite's full-block geometry, composite Battle Pass models, translucent podium
glows, state-filtered path borders, correctly lit benches, custom hotbar/held items
and source-pixel form borders render in a 1920×1080, DPI 1 macOS/Metal hidden-client
pass. Custom geometry preserves explicit absorption and its legacy type-light flag.
Item registries retain numeric aliases and populated definitions accompanying empty declarations.
Zeqa equipment sources and dynamic UI textures retain bounded native dimensions;
rejected UI publications preserve the previous catalog and retry pending artwork.

Incomplete: merging multiple different populated component definitions, unrestricted
pack-size parity, ordinary actor endpoint lighting, exact native frame comparisons
and release hardware budgets remain open. Two terrain texture keys absent from the
served Galaxite stack still report diagnostic textures; this compatibility work does not close those parity gates.

Follow-up compatibility covers client-authoritative inventory opening and transfers,
authored entity visibility bounds, filtered HUD control messages, and multipart
custom-block collision admission. Collision lists retain at most 256
primitives. Per-state collision and selection overrides now resolve once at session
admission and register in both network-ID spaces. Captured bottom/top slab fixtures
match their authored geometry heights and retain stationary support for 100 ticks.
Arbitrary collision transformations remain incomplete. The definition tree
coalesces Float and Double tags, so exact rejection of Double-only collision fields
remains incomplete. Unbound looping effects now stop emission after their first
active/sleep cycle and let existing particles drain; bound effects can restart
until their owner disappears. The actual captured gem effect passes this lifecycle
regression. A 1920×1080 macOS/Metal replay of one unbound gem packet shows three
gems at 0.267 seconds and none at 3 seconds. The live lobby sequence and apparent
slab/stair hovering still require emitter and support-position evidence. None of these reports is cleared by the earlier lobby
render pass.

Client-authoritative inventory follow-up: ordinary moves, splits, swaps, quick
moves and drops use normal old/new-descriptor transactions; successful transport
commits local cells and later authoritative Slot/Content updates correct them.
A 1280×720, DPI 1 macOS/Metal loopback pass opens personal inventory four times
and chests twice. Its independent server validates old descriptors and item
conservation before accepting six normal take/swap/place transactions, with no
sparse requests or rejections. Reopened screens retain the final server cells
and an empty cursor. Public-server game acceptance remains separate.
Incomplete: legacy crafting/creative actions, semantic merging of nonplain item
user data, arbitrary open-window normal-transaction corrections, and a separate
native client-mode Open/Close acknowledgement contract remain unimplemented or
unverified. These changes do not close general inventory parity or the live
server acceptance gate.

Server audio follow-up registers definitions without a total decoded-pack cutoff
and decodes selected files on demand with a bounded cache and persistent worker
pool. Waveform-only replacements remain available. Incomplete: true incremental
streaming, unsupported audio codecs, unrestricted pack-size parity and matched
live Galaxite playback timing remain open; mixer tests alone do not close those
parity gates.

HUD trigger follow-up preserves raw messages for server factories while applying
authored visibility to ordinary chat, title and action-bar controls. Aseprite
sidecars retain their frame coordinates and durations independently of image
residency. Offline 1280×720, DPI 1 published frames cover Galaxite static,
jumpscare and lighting effects, including changing noise frames, authored loop
durations and mapping source coordinates into resized atlas placements.
Incomplete: exhaustive authored effect variants, matched live game-mode sequencing
and hardware frame budgets remain open.

Live follow-up confirms custom sound playback. The latest user test reports the
earlier actor, HUD, collision and inventory bugs fixed, including the actual
Galaxite death animation. Matched native pixel comparisons remain separate.

HUD compatibility now retains the native nested title/subtitle override paths,
answers empty title strings during blank frames, and schedules a pending authored
visibility transition even when controller input is unchanged. A valid cached
MineVille pack fixture publishes one unclipped top-center tutorial panel, preserves
it across unrelated titles, and draws no raw markers or secondary note/minimap
labels. Settled unchanged HUD frames remain cached. The exact cached MegaSMP UI
revision now retains its first document's partial output after a syntax error,
matching vanilla's resource merge behavior. Unread controls remain absent, later
malformed overlays are skipped, and trailing commas still report errors. The
unmodified pack fixture changes from a missing tutorial to one retained panel;
six document regressions cover first, later, null and independent-path reads.
A 1920×1080 macOS/Metal client replay renders the colored tutorial at top center
and retains it through a later title. Matched live gameplay comparisons remain
incomplete.

The exact admitted Hive overlay now gives each title creation fresh binding
state, while unrelated and nested retained controls keep their saved values.
Completed-group visibility expressions preserve their following conditions.
The original pack's offline replay switches between one authored modal and one
ordinary title, then repeats and clears the modal without stale labels. The
MegaSMP tutorial remains retained through those lifecycle changes. A 1920×1080,
DPI 1 macOS/Metal loopback replay of the unchanged admitted Hive UI renders one
legible purple death header with its shadow and one body line, with no oversized
fallback or stale labels. Repeating the title preserves that result; clearing it
removes the panel, and ordinary titles still render once. Geometry, clipping,
layering and colors were inspected in fresh frames. Required touched-crate checks
and the canonical developer-control build pass. Public-match title sequencing and
matched-version pixel comparisons remain open.

Server-selected animation follow-up retains named clips that an entity does not
alias, binds shared channels by model bone name, and preserves packet transition,
stop expression/version and outgoing blend fields. Domain regressions cover
starting the pose, finished-query timing, timed stop blending and local actor
selection. Existing actor animation clocks and poses remain green. The offline
Battle Pass camera-edge witness changes from no draw with default bounds to a
visible draw with authored bounds at the same camera; Entity close-up bounds also
retain the draw with its origin behind the camera.
Rendered offline Entity fixtures now cover the server-selected death motion,
close camera orbits, and moving spawn/despawn poses with the actual pack geometry
and materials. The first-person jumpscare exposed a separate late-activation
clock error; direct players now pause and resume applied time independently of
owner age, with render-delta regressions. Held root placement also preserves the
owner actor frame for unbound roots and the matching owner matrix for named roots.
A fresh 1920×1080 macOS/Metal replay shows the actual jumpscare rising at screen
center, changing pose, approaching the camera and lowering at its authored endpoint.
Arbitrary expression parents, independent bindings on parented bones,
independent instances of a shared clip, default controller-player pause,
version-specific Molang grammar differences and matched live sequencing remain
incomplete. The user reports the earlier live rendering bugs fixed. A fresh
1920×1080, DPI 1 Metal replay also retains the Entity across two enclosed rooms
and close oblique views through a 145-frame camera crossing. The latest live
test reopened angle-dependent Entity visibility: the small-room witness did not
cover a crowded map's actor admission. The crowded replay below covers that
separate failure; broader animation contracts remain incomplete.

The standard world camera now uses vanilla's 0.025-block near plane, shared with
first-person rendering and boom clearance. The former renderer default of 0.1
clipped nearby geometry and let sprint-FOV near-plane corners cross a wall even
when the player collision box stayed outside it. Two presentation spawn
regressions fail before the correction: a wall at collision clearance intersects
the near rectangle, and geometry 0.05 blocks ahead is clipped. Setting the
perspective distance also updates its explicit clip plane; all 188 camera tests
pass. The static hand fallback uses the same near distance, with its reverse-Z
regression failing before the change and passing afterward. Fresh 1920×1080,
DPI 1 Metal before/after frames reproduce the wall hole with the former near
plane, then retain continuous wall geometry throughout a 93-frame contact run.
The same replay reproduces post-death radial distortion through the gameplay FOV
modifier before the angle correction; afterward its perspective stays bounded.
The user confirms the actual post-death distortion is fixed. The exact-version
final-angle reference gap remains open separately from these functional results.

Server packs now choose a device-compatible authored subpack when the server
leaves its selection blank or selects an unsupported option. Legacy manifest
memory requirements convert to performance tiers before selection; supported
explicit choices and explicit global root selections stay intact. The actual
Fonts archive now admits its detailed U+E141 Orebits glyph page on this device
instead of the Lite dot. The former conversion and server admission both have
failing-before regressions; all 52 resource-pack unit tests and its integration
test pass. Fresh 1920×1080, DPI 1 Metal sidebar frames replace the Lite dot with
the detailed purple coin, with readable text and unchanged sidebar placement.
The tutorial stays legible at top center through a later title. Matched-version
pixel comparison remains open. The user reports the remaining live bugs fixed
after testing the rebuilt client.

The live angle-dependent Entity disappearance has a separate actor admission
cause. A 151-actor Metal scene retains the focal actor in authority and inside
both camera frusta, but turning admits more map actors and drops its draw at the
former player-body limit. Resource-pack artwork now uses the shared render arenas
independently of player texture residency; shared skins stay usable when distinct
skin residency fills, and invisible routes do not reserve a skin. Six admission
and overflow regressions fail before the change and pass afterward. The existing
fixed instance, pose and distinct-skin resource policies remain incomplete
relative to dynamic actor collection. A fresh 1920×1080, DPI 1 Metal replay keeps
all 151 qualifying actors and the focal Entity across the crowded camera turn,
where the before frame admitted only 128 and lost the focal draw. Authored
geometry, material, pose and bounds remain unchanged. Both endpoint frames and
the intervening camera-crossing recording were inspected; the user also reports
the remaining live rendering bugs fixed.

The rejected Hive menu is a well-formed button collection above the former menu
limit. Packet-bounded menus now retain every label, image and response index;
custom-form and NPC bounds retain their separate contracts. Engine factory and
node budgets reject a whole form instead of publishing a shorter button list.
Rejected binding attempts retain their input identity until model, components or
catalog changes. Protocol, full-hierarchy and final-button response regressions
pass. A fresh 1920×1080, DPI 1, GUI scale 2 Metal replay renders the complete
300-button menu, scrolls to button 299 and submits index 299 to an independent
loopback server. The response closes the form; labels, images and hover feedback
remain legible. The user reports the remaining live UI bugs fixed. Larger
hierarchy admission remains an incomplete implementation resource policy.

Zeqa's ordinary title patches inherited the built-in HUD's extra text scaling.
The first authored ordinary-title override now inherits the vanilla definition;
custom title factories and later server edits retain their priority. Layout,
layer-order and indexed-document regressions pass, including the reproduced
title/subtitle overlap. Exact admitted Hive and retained tutorial baselines are
checked separately. Fresh 1920×1080, DPI 1, GUI scale 2 Metal frames of Zeqa's
unchanged admitted pack show its title at twice the subtitle height with a clear
vertical gap. The original Hive ordinary/death/repeat/clear sequence still renders
one authored panel, and the MegaSMP tutorial remains retained with its detailed
Orebits glyph. The user confirms the rebuilt client looks correct. Required
touched-crate checks and the canonical developer-control build pass; release
hardware budgets and matched-version pixel comparison remain incomplete.

## Spear actions

Spear bindings and pose inputs now consume authored swing and kinetic timings.
Attacks at actors, air or blocks send the item-directed transaction with aim and cooldown state,
allowing the server to apply damage and Lunge movement. Component, admission,
catalog/reset and real-carrier animation regressions pass. Matched live captures
show the jab, charged hold and default-Java third-person arm; server-confirmed
Lunge moves the fixed client without movement input. The complete matched-version
native comparison remains incomplete. See
[spear actions](docs/reference/spear-actions.md).

## Third-person held attachables

Held models evaluate authored scripts and texture meshes against the owner's
posed bones, including aiming skeleton bows. Third-person bow draws supply the
owner's use duration, draw frame, and render delta through release. Unchanged
controller poses proven independent of timing reuse their evaluation; dynamic
channels and persistent scripts continue running. Matched native motion captures
and frame budgets remain incomplete; this does not close the complete held-item
parity gate.

## Local placement prediction

- Pillars, slab halves and matching doubles, trapdoors, hoppers, supported attachments,
  colored carpets, fence/pane connections and stacking existing snow resolve locally.
- Stairs and changed stair corners, loom/glazed rotations, dispenser/piston facing, door pairs,
  levers, ordinary signs, unlit torches, first dry vines/multiface attachments, conservative
  first snow placement, and dry candle/pickle stacks now resolve locally.
- Placement parity remains incomplete for directional types outside the verified table, beds,
  walls, rails, redstone dust, plant substrates, wet/existing attachment masks and special supports.
  Beds also need local block-entity visual publication. Unknown door-hinge materials defer.
  See [local placement rules](docs/reference/block-placement-prediction.md).
- Non-air replacement is server-confirmed; complete replacement-component classification and
  its effective placement face are incomplete. Clicked-cell selection retains the existing rule.
- Local commit starts bounded worker work; the late render handoff services available completions
  without another server poll. Frame receipts distinguish commit, staging and observed upload.
  Headless stairs first render +3 frames after commit with the server paused; both door halves
  also render before confirmation.
  Asynchronous lighting/meshing keeps click-frame visibility incomplete. Full family visual parity
  and hardware frame-budget gates remain open.

### Furnace recipe panel continuation (incomplete general parity)

Furnace, blast furnace and smoker screens retain their server recipe catalogs and
show result items through the pinned JSON-UI recipe panel. The toolbar, category
tabs, search and supplied-ingredient filter reach the inventory controller; a
selection chooses the alternative with the most matching fuel and inventory
items and places them in the ingredient role. Ordinary window updates address
the whole station by window ID. Unsupplied recipes preview their ingredient and
result; repeat selection returns the ingredient before clearing the preview.
Replacements return the previous
ingredient to its source cells before other available inventory cells. Expanded
ingredient groups, exact ghost rendering, saturated-inventory replacements,
recipe discovery and server-persisted
furnace UI options remain incomplete; the full furnace parity gate stays open.

## Entity interaction and emote starts

Entity-use presses now send the selected stack and fresh actor hit in the input
frame, with block occlusion and bounded queue retries. A live cow interaction
produces a server-confirmed milk bucket. Selecting an equipped emote sends its
catalog identifier and duration once in that frame; idle and cancellation send
no start packet. Marketplace clip ownership/playback, remote custom-emote
synchronization, and the complete villager trade UI remain incomplete. These
producer fixes do not close those parity gates.

The pinned bamboo visual gate is covered for all twelve admitted states. Stalks,
radial leaves, column UVs and offsets, selection and breaking overlays, picking,
movement and camera collision, placement obstruction and overhang culling share
the resolved column transform. Admitted random-offset components distinguish
absence from explicit zero and reach both network ID spaces and compound models.
Terrain sampling uses nearest minification/magnification, linear mip filtering,
clamped source rectangles and byte-space RGBA mips; mixed texture sizes retain
their own source-pixel gradients. Missing required face textures retain fallback
support. Focused behavioral and Metal GPU regressions cover those contracts;
fresh whole-client captures cover Vanilla and bounded Enhanced gallery views,
selection and advancing cracks. Enhanced remains disabled for ordinary launches.
Arbitrary runtime face-key remapping and non-power-of-two pack raster equivalence
remain broader pack-stack work. Hidden debug captures do not close displayed-frame
performance or the overall UI and performance gate.

## Death-screen reasons

The dedicated server death-information packet supplies the localized reason to
the death screen independently of chat. Reasons survive either arrival order
around zero health and clear on authoritative health recovery or session replacement.
Recovery reads positive actor health independently of the rounded HUD values.
The default death screen uses the owned OreUI renderer with a radial world overlay,
centered title and literal wrapped reason, Respawn and Game menu actions, and the
HUD beneath it. Message, button, backdrop and loading animations have separate
clocks. The prompt still waits before accepting actions when animations are disabled.
Game menu returns to the same death presentation. Respawn retains progress until
authoritative recovery; pending requests survive outbound backpressure. Forced
death cancels hidden key capture, and formatted reason parameters remain literal.
The legacy JSON-UI renderer retains literal reason handling for fallback controls.
Ordinary first-person world damage rotation samples the actor's completed hurt
and death counters independently of hand animation. Scripted captures retain
their requested pose after those effects run.
Portal and fire overlays sample the final rendered camera pose and projection.

Incomplete parity: exact target-version animation constants,
respawn retries, death camera/FOV, hurt and HUD flash timing, hardcore and secondary
client variants, and matched native frames remain open. The modern implementation
and its focused regressions do not close the full death-screen parity gate.

### Local custom models (incomplete parity acceptance, 2026-10-09)

The Dressing Room imports paired PNG/geometry JSON and free static skin packs.
It retains authored geometry and the minimum engine version across restarts and
sends them in login and skin updates. Custom models keep their authored shape;
rename and delete remain available. Preview preparation runs on a worker, while
local world presentation uses the shared actor preparation cache in both perspectives.

Imported raster dimensions follow the native 64×32, 64×64 and 128×128 contract.
Archive, geometry and library ceilings are Cinnabar resource limits. Pack minimum
engine versions remain separate from geometry schema versions. Full native skin-pack
import parity, animated imports, persona and Marketplace trust remain incomplete.
No visual parity or hardware performance gate closes with this extension.


### Shipped font grid and coverage (incomplete text parity)

Shipped carriers include every mapped source scalar. Sans uses its 18-pixel raster em;
Seven, Ten, Five and Five Bold use 20, with one atlas texel per source-grid unit.
Semantic line and word-space metrics come from the carrier table. OreUI rounds final
unrotated glyph edges onto the device grid while retaining semantic sizes and tracking.
Five and Five Bold are available as roles; current screens still use Seven and Ten.
Missing regular, bold, italic and monospace mathematical Latin letters and digits use
original grid-based variants built from the shipped Latin artwork. This fixes the
sampled styled MOTD boxes; unrestricted Unicode and native fallback shaping remain open.

The display faces now use original ASCII pixel drawings and declared per-glyph
advances. Ten has heavier stems and narrower I/i; Five and Five Bold use wider,
five-row letterforms. Seven has revised lowercase letters, digits and descenders.
Greek and Cyrillic display capitals follow the adjusted Latin proportions. The
generator preserves the 64-unit outline grid, complete source coverage and fixed
metadata, and reproduces the shipped fonts and manifests byte for byte.

Full parity is incomplete. Advances round to the nearest font unit, while ink
widths and stroke weights remain quantized to the source grid. The carrier compiler
still rounds glyph advances to whole atlas texels, limiting runtime spacing precision. Ten's 0.052-em
bearing is represented by 0.05 em on its 20-texel carrier. Five's fine weight and
bearing differences cannot all fit that grid. Seven's accented fallback drawings
have not all been revised to match the new ASCII letters. A few narrow accented
Ten capitals retain their prior forms when a mark cannot fit the adjusted base.
At GUI scale 7, Seven's
14-texel capital height maps to 39.2 screen pixels; preserving this size cannot also
give every texel a uniform integer width. Rounded edges and nearest sampling do
not close that scale/weight gate. Source line metrics retain sub-unit rounding.
Version-matched live text-raster evidence across all GUI scales, locale shaping,
kerning, and the remaining per-glyph differences are open gates.

## Device render-distance defaults

Ordinary Video settings now choose their default from installed RAM and the
selected GPU's dedicated memory. Explicit saved values win, and Video reset
restores the device recommendation. Slider stops start at 5 and extend in
one-chunk steps to the owner-chosen 255 maximum. See
[the rules table](docs/reference/render-distance-default.md).

Incomplete parity: experimental low-memory overrides, advanced graphics presets,
and native VRAM probes on unsupported backends are not verified. These remain
open and do not close a parity gate.
