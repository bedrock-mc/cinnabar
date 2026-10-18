# World and gameplay status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Fence gate targeting

Gate picking uses the same thin directional bounds as its outline for closed, open
and in-wall states. Rays can pass through empty cell space to the lower gate in a
stack. Movement boxes and carried-block visuals keep their separate geometry.
Regression coverage includes every registered gate state in both runtime ID modes.
Broader block-family parity remains incomplete.


## Prepared session block registries

Session preparation constructs collision registrations, custom state identities and merged
block artwork before publishing StartGame. Bootstrap checks the session generation, ID mode,
exact carrier inputs and admitted definitions before installing the prepared bundle.
Registry snapshots share immutable tables; replaced collision tables retire off the frame.
Existing shared custom-state enumeration remains the source for registry consumers.
Regression tests cover cancellation, replacement, palette modes, immutable bases and shared
identity storage. Incomplete: compilation, executed tests, join captures and performance
measurements remain pending. No parity or hardware performance gate closes here.


## Raw intake during blob disk reads

Blob prefetch shares the transport's existing bounded raw-frame storage while its one disk
future is pending. Disk completion is checked between frames; reaching the byte bound or a
terminal marker stops reads and keeps waiting on the same future. Classification, status
writes, admissions and ready-event ordering retain their existing paths.
Incomplete: controlled-future and framing regressions are written but unrun. Compilation,
version-matched protocol evidence and measured receive gaps remain pending. Status-writer
restructuring is separate; no performance or parity gate closes here.


## Shared solid-actor collision snapshots

Actor state retains ordered solid bounds as poses, metadata and membership change. Movement
and correction adapters share their converted bounds until the source changes. Mover and
mount filters refresh before queries, and historical prediction readers keep their old bounds.
Effect-driven pose changes, including the dragon death lift, refresh bounds at the mutation.
Incomplete: behavior, query and allocation-work regressions are written but unrun.
Compilation, live comparisons and performance measurements remain pending; no behavior or
performance acceptance gate closes here.


## Shared menu list snapshots

Menu views share saved servers, featured cards, realms, friends and local worlds with their
semantic owners. Vector edits detach retained views, and equal feed deliveries preserve the
current list storage. Interaction state and service details still publish independently.
Incomplete: behavioral and copy-count regressions are written but unrun. Compilation,
rendered comparisons and measured menu publication cost remain pending; no performance or
output-parity gate closes here.

Published form draw nodes share immutable snapshots. A bounded engine cache retains logical
base texture paths and renderer names for the exact snapshot and omission values; overlays,
renderer callbacks, source resolution and atlas residency still update every prepare.
Oversized entries use uncached discovery. Regression coverage is written but unrun; compilation,
rendered comparisons and measured dependency-discovery cost remain pending.

Oversized artwork sources retain sorted, immutable metadata with shared encoded bytes. Painted
frames reuse that metadata and request artwork only after source ownership changes. Provider
replacement retires stale fallback artwork; image polling, page publication and completed
gallery work continue independently. Focused artwork and atlas regressions pass (34 and 9
cases), and touched-crate clippy passes. Rendered comparisons and measured artwork
publication cost remain pending. Edit-text/report copies and other form work remain separate;
no F4 or performance gate closes.


## Sleeper-only bed sampling

Actor ingress retains sleeping actors and their streamed bed positions. Presentation samples
only those beds, clears unavailable headings, and reads direction facts compiled with each
runtime-ID registry. Wake-up, bed changes, replacement and reset discard stale samples.
Incomplete: regression tests are written but unrun; rendered comparisons and performance
measurements remain pending. No parity or performance acceptance gate closes here.


## Single-cell palette edits

Paletted block storage retains live counts and bounded reusable physical slots. Single-cell
edits patch packed words while the logical palette preserves exact survivor/insertion order.
Last-use removal and width shrink defer canonical word packing until a consumer requests it;
thread-safe snapshot-local caches preserve the exact public palette, width, words and equality.
Physical width growth, noncanonical first edits and larger batches retain canonical rebuilds.
Mutation results prove store changes without immediately materializing words through equality.
Untouched decoded representations and no-ops remain unchanged; snapshots stay independent.
Memory accounting charges physical words, historical slots and canonical caches without packing.
Slice admission reserves possible cache growth within the existing process-wide byte limits.
Regression tests and preparation benchmarks are written but unrun. Incomplete: executed
verification, consumer-cadence measurements and retained-memory/performance measurements.
No output-parity or hardware performance gate closes here.


## World-service handback

Retention and full-detail transitions share the poll deadline and resume between columns.
Moving views skip unchanged row interiors. Server teleports use the same retention scan;
later commits wait for preceding retention, and reclaim requests stop before the next item.
Near/far terrain still hands coverage over only after its summaries and uploads are ready.
Outgoing handoffs recheck the latest radius and player centre before withdrawing coverage.
Discarded detail invalidates pending meshes; retained handoffs regain lighting and its halo.
Eviction clears interrupted membership so replacement columns refresh through the entering cursor.
Repeated restoration cancellations retain the original refresh checkpoint while keeping
published members eligible for later edits.
Poll reports count maintenance work and its maximum item time; trace spans record commit
sizes and reclaim waits.

Incomplete: matched release flight and teleport captures must establish reclaim max and
p99 on target hardware. Synthetic CPU benchmarks do not close a frame-pacing or parity gate.


## Repeated container delivery acknowledgements

Repeated opens of the current non-dynamic chest retain its contents and send the warning
acknowledgement used by server menu dispatchers before subsequent clicks. Compatibility
tests cover transport pressure and a dispatcher timeout. Incomplete: version-matched
vanilla capture of the warning's diagnostic text and live PMMP acceptance remain pending;
these tests do not close a Bedrock parity gate.


## Bevy-free client UI

`client-ui` has no Bevy dependency. It reads `semantic-input` keyboard keys and wheel units,
`glam` math, `tracing` logs and a camera eye position. `client-shell` owns the ECS wrappers
for UI authority and presentation, and maps Bevy device codes to semantic keys and HID usages.
`app` owns gameplay cursor capture, FOV input collection, screen input suppression and the
prepared publication resource. `client-presentation` borrows UI state from those app systems.
This move has not been compiled or executed and closes no parity gate.


## Flowerbed stem tint

Flowerbed stems use biome grass tint while petals retain their sprite colours.
Original-art compiler coverage checks every registered flowerbed state and the
carrier roundtrip. The focused check and local BDS rendering with a rebuilt
carrier pass against the official client's green stems. Broader terrain parity
and hardware performance gates stay open.


## Liquid surfaces beneath opaque roofs

Liquid tops below opaque roofs remain visible when any surface corner is below
the roof boundary. Stacked compatible liquids and fully raised surfaces against
opaque roofs retain their culling. Original-art meshing regressions cover water,
lava, sloped surfaces and shared side insets. Local BDS rendering now matches the
closed surfaces observed with the official Android client. Broader liquid parity
and hardware performance gates remain open.


## World day length

`world::TICKS_PER_DAY` is the single day length for sky and moon-phase math, the HUD
days-played count and their tests. The guest SDK keeps its time-override bound because
it cannot depend on `world`; a compile-time check in `render` keeps the two equal.
Values are unchanged; compilation and test execution remain unverified until CI.


## Client-side chunk generation

- Login claims explicitly decline client-side chunk generation. The client relies on
  server-supplied terrain; FeatureRegistry consumption and client-side terrain generation
  remain unsupported and incomplete. Server-routing parity remains open.


## Totem activation

- Local totem activation retains the equipped item before later inventory or equipment updates
  consume it, including when world publication is delayed.
  Offhand takes precedence, retriggers replace the image, and a session change retires it.
- The camera overlay spins and scales the flat item icon for forty client ticks. It samples
  the current admitted item atlas, including texture replacements, reloads and removals.
- Client time includes the partial tick and bounds elapsed time during stalls. An activation
  starts on the current frame even when event publication precedes the UI clock update. Screen overlays
  stay above the effect; HUD safe-area insets do not move or crop the camera animation.
- Local activation starts the pack-authored Totem emitter at the player's native position,
  following the player during emission. Remote status events do not create this emitter.
- Deterministic activation regressions and full-client default/custom recordings pass,
  including main-hand fallback, retriggering and remote/empty exclusions. Broader item action,
  particle emission and performance gates remain open.


## Graphics API startup recovery

- A shared policy selects Windows Vulkan/DX12, Linux Vulkan, Android Vulkan,
  and macOS Metal. Each API has its own instance and enumeration sentinel. Known vendor
  defaults order probes; fresh installs use platform order until hardware is identified.
- Precedence is `WGPU_BACKEND`, the restart-only Video override, vendor defaults and the
  driver blocklist, then crash exclusions. Explicit overrides bypass automatic filters.
  A failed saved Video choice retries Auto for that launch; only the environment override is strict.
  GL/GLES is unavailable, saved GL choices become Auto, and Android requires Vulkan.
  The Video control is hidden on macOS.
- All eligible adapters are ranked by device type (discrete, integrated, virtual, other, CPU),
  then vendor backend order. Low-power preference puts integrated hardware before discrete.
  Windows Intel adapters prefer DX12 on ties; other multi-API defaults prefer Vulkan. A requested adapter name restricts candidates across every API.
- Candidates must support terrain base-vertex draws, vertex and fragment storage, alternate
  texture views, non-power-of-two mipmaps, sample-rate shading, unaligned buffer bindings and
  the required storage limits.
  Startup tries each ranked candidate; returned device failures advance to the next candidate.
  Crash exclusions identify only the backend and survive successful fallback. Client version
  changes and Graphics API selections, including reselecting Auto, clear exclusions at startup.
  If no non-excluded candidate succeeds, startup clears allowed exclusions for one more attempt pass.
  Startup log boundaries flush the file directly without waiting for terminal output.
- The driver blocklist has one data source and bounded numeric version rules. Crash capture
  attributes the renderer attempt pushed to the crash monitor and keeps normal OS crash handling.
- Diagnostics Phase 3, crash monitor and window: the client re-runs itself as
  `--crash-monitor` before startup; `crash-handler` faults and panics request a `minidumper`
  dump, and the monitor writes the folder (dump, report, log sections read after the client
  died), prunes to 8 and shows the crash window (shared `bootstrap-ui` window stack) or a native
  dialog. Game state (renderer attempt, session phase, server kind, dimension, server packs) is
  pushed on change; exits without a dump report `kind: abort`; GUI-build `main` failures show
  `kind: startup`. In-process folders remain the fallback before connection and on Android.
  Verified locally on Windows: panic, access violation, stack overflow, abort (fail-fast) and
  external kill each give one folder of the right kind (dump where native/panic), clean exit gives
  none and the monitor exits with its client. Incomplete: Linux and macOS fixtures run only in
  CI; the window and native dialog are not visually verified on macOS or Linux; dumps are not
  yet walked with `minidump-stackwalk`; next-launch notices for unshown reports and the optional
  user comment remain open.
- Diagnostics Phase 4, hang watchdog: the client maps a small heartbeat file in its crash
  directory that the monitor also maps; the main thread ticks a frame counter once per frame in
  `First` (one relaxed load and store), and a loading count covers each world join until two
  in-world frames finished. Nothing counts before the first frame, so carrier loading, first-run
  preparation and renderer setup are never hangs; pack compilation runs off the main thread and
  needs no loading phase. The monitor samples ten times per 10 s window; no progress outside
  loading writes one `kind: hang` folder (report, log sections, minidump of every thread taken
  from the monitor side) and keeps the client running, without a window. Progress then a new
  stall writes a new folder; killing a hung client shows its hang folder instead of an abort
  folder. A client in a debugger, one being dumped for a crash, and gaps in the monitor's own
  sampling (system sleep) are not hangs. Verified locally on Windows: detection unit tests on a
  manual clock, and fixture clients where a stall then kill gives exactly one hang folder with a
  dump holding the watched thread, a loading stall gives none, and recovery then a second stall
  gives two. Incomplete: Linux (`ptrace` dump after `PR_SET_PTRACER`) and macOS (report without
  a dump; the task port needs privileges) fixtures run only in CI; a real client hang has not
  been reproduced; a Windows title-bar drag or resize that blocks frames for 10 s would be
  reported as a hang.
- Diagnostics Phase 5, core process wiring: every bedrock-core spawn (game, account, sign-in
  and catalog helpers) gets `-log-level` from the `core` log category (trace and debug map to
  debug, off to error) and `-crash-output` in the session's crash staging directory (`core/`,
  pruned with it once the client is gone). A reader thread per process
  labels stderr lines without a `process=` attribute with role and pid and queues them for one
  `core.log` writer (a per-session family with the `CORE_SESSION_START` header and the client
  log's mid-session size cap); a full queue drops and counts lines, never blocking a core.
  Exit statuses are recorded; an exit nobody asked for other than status 0 or the core's own
  `core failed` status 1, or any Go crash output, writes a `kind: core` folder with the client
  and core log sections, the staged packet and flight attachments and `core-crash.txt`, without
  a crash window; the staged file is
  deleted afterwards. The account core restarts with `RestartBackoff` (0.5 s doubling to 8 s,
  giving up after five quick exits until sign-in changes or a refresh). Verified locally on
  Windows with a Go stand-in core: a panic gives one folder with the traceback, clean and status
  1 exits and kills give none, helper stderr reaches the log labelled. Incomplete: Linux and
  macOS run only in CI; no live client run with a crashing real core.
- Startup recovery files have an exclusive OS lease through the first completed frame;
  sentinels include PID and process start time. Window closure and handled SIGINT/SIGTERM
  cancel unfinished startup without recording a GPU failure. Termination revokes later sentinel
  writes, while panics and native faults retain crash evidence. Saved Unix handlers receive the
  original signal information and machine context; default hardware faults repeat their original
  instruction. Another client's saved choice cannot change this process's crash attribution.
- Target-device crash reproduction, Android lifecycle acceptance
  and Video screenshots remain incomplete. This work closes no parity or performance gate.


## Optional spatial anti-aliasing and cutout coverage

- Capability-aware MSAA remains the primary setting with its existing default. Spatial
  anti-aliasing is an independent Off/SMAA option, defaulting to Off.
- Spatial SMAA uses world-depth discontinuities and continuous-slope rejection before world
  text, hands and HUD drawing. It uses no temporal samples or colour edge detection.
- Multisampled cutouts integrate nearest alpha contours across a one-pixel band. Covered RGB stays
  nearest; newly covered transparent texels use the nearest covered neighbour's colour.
  Single-sample thresholds, opaque/translucent passes, shadows and entity depth passes are unchanged.
- Ordinary world leaves retain their alpha cutoff and opaque output under MSAA, so fractional
  distant mip alpha does not leave background samples visible through a dense canopy.
- Actor cutout taps stay within each triangle's front/back UV texel rectangles; centroid
  interpolation keeps multisample UVs inside covered geometry. Rectangular chart boundaries
  and interior contours have GPU regression coverage. Shared nonrectangular mesh-chart
  association remains incomplete and does not close shader parity.
- Alpha-contour reconstruction is an extra visual policy, labelled incomplete for exact-version
  vanilla coverage parity. Matched hardware performance and native foliage comparison remain
  incomplete; no parity or performance gate closes from these changes.


## Optional camera motion blur

- Video offers Off (default), Low, Medium and High camera exposure. This is an owner-authorized
  visual extension; classic Bedrock has no motion blur. Gameplay and network state do not change.
- Depth reprojection blurs camera rotation/translation before nametags, projected UI, hands and HUD.
  Exposure uses elapsed frame time; explicit reanchors suppress the discontinuity frame.
  Spatial anti-aliasing follows exposure so filtered silhouette colour retains the actor's sharp edge.
- Depth-writing actors retain their previous rendered root and interpolated bone pose for object
  exposure. Actor texels establish visible motion footprints in the background while
  retaining stationary actor edges and nearer occluders. The motion pass preserves camera depth ranges.
  Mixed MSAA texels with nearer non-actor samples retain their resolved colour at the source;
  separating the actor's colour for exposure remains incomplete.
- Per-object blur remains incomplete for blended actors and other world renderers. Hidden background
  reconstruction, competing object velocities in a search neighbourhood, and matched flight acceptance
  remain open. Enabled views retain exact per-pixel motion/depth resolves and row/tile velocity targets.
  Repeated exposure taps share those resolves; each reduction scans at most one tile edge per invocation.
  Full-scene hardware frame-budget acceptance remains open; no performance gate closes.


## Item frames

- All six facing states retain their outward direction, placing the frame against its support.
  Ordinary sprites sit just ahead of the backing and use the floor/ceiling face's upright basis.
- Float item rotations retain degrees, including fractional angles; legacy integral turns
  remain supported. Offline GPU fixtures cover mounting and clockwise turns with original art.
- Ordinary frames have a birch rim and recessed normal/glow backing, with block-scale UV
  crops and shading in the mounted world direction. Glow frames retain brightness in darkness.
- Map/photo presentation, special items, three-dimensional block items and custom texture-alias
  bindings and complete material/lighting parity remain incomplete. No broad item-frame parity gate closes.


## Movement and input audit fixes

Ability updates retain the winning Flying value and its local-change authority
independently of NoClip. Collision bypass uses the selected travel physics and
survives correction replay. Higher fixed flight denials prevent local re-entry;
NoClip alone grants no hunger or sprint exemption. Incomplete: matched trajectories,
outgoing flight requests, hunger/sprint contexts, unloaded terrain, mounted movement
and complete block-contact behavior still need acceptance; these changes do not close
a movement parity gate.

Server-enabled flight also remains active in ordinary modes without MayFly;
that permission still controls starting flight locally. A native Android
1.26.50.4 Swim.gg post-death comparison verifies upward flight for this server
combination. A fresh fixed-client death cycle also verifies ascent and hovering
after jump release. Other toggle edges and the full movement parity gate remain
incomplete.

Spectator mode now keeps forced flight without the ordinary flight-toggle grant,
passes through solid terrain without ground contact or anchor depenetration, and
suppresses block interactions and first-person hands. Player render queries follow
the committed mode; runtime skins receive authored visibility before equipment is
attached, and authored blended layers multiply texture opacity by controller opacity.
The inside-block overlay respects no-clip while server camera fades remain active.
Owning regressions cover mode transitions, stale ability grants, solid and unloaded
terrain, embedded anchors, render queries, hands, overlays, runtime skin visibility,
authored material overrides, and GPU opacity. The built-in spectator blend fallback
is provisional until the version-matched stock material contract is verified. Full
spectator parity remains incomplete pending exact native trajectory and platform
comparison, custom controller geometry, and persona coverage;
these changes do not close the movement, camera, or actor parity gates.

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
  Slime and bed rebounds apply only the gravity remaining after impact, then
  vertical drag; negligible clipped travel keeps ordinary gravity.
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
  Snapped MovePlayer teleports suppress travel for one completed tick, retaining
  destination contact, jump cooldown and independent motion updates. The captured
  input preserves that tick during correction replay. Pending anchor recovery
  precedes resumed travel, including catch-up ticks within the same frame.
- Actor crouch updates preserve held ability-flight descent during live input
  and correction replay; older walking-pose updates stop at the flight transition.
  Focused regressions reproduce the upward rewind caused by a server sneak clear.
  Scoped native-server descent, release, landing and flight-toggle acceptance passed.
- Provisional streaming-boundary handling keeps ability-flight ticks advancing
  beside entirely absent columns, using solid collision barriers with retained
  absence identities and replay snapshots. Missing sections in partially received
  columns still wait for authoritative data. Streaming-boundary server, input and
  rendered acceptance remain incomplete; this does not close creative-flight parity.
- The flight double-tap counts presses from the per-tick `JumpDown`/`Jumping`
  pair the server replays, every ability update overwrites local flight, flight
  persists on the ground, and only the double-tap sends `StartFlying`/`StopFlying`.
  The flying-on-ground lift-off nudge is not modelled; creative-flight parity stays open.
- All movement modes sample block effects at body contact instead of the swept
  volume; post-move effects use the inset destination body, and solid collision
  still checks the full move. Query limits retain physics authority without
  accumulating tick debt. High ability speeds, 500-block/s actor motion and
  flight-to-walking transitions have regression coverage. CI and rendered
  acceptance remain pending, and the existing collision-query extent limit still
  applies. This does not close the creative-flight parity gate.
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

- Eligible custom blocks predict their admitted state in both palette modes. Enabled
  placement traits apply the click face, vertical half, body-facing direction and yaw
  offset while preserving authored property values. Server placing handlers retain
  authority. Prediction checks the selected state's collision bounds and session IDs.
  Incomplete: custom placement filters, unrepresented traits and unresolved permutation
  physics still defer to the server; complete placement parity remains open.

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
- Repeat deadlines retain sub-millisecond clock precision so the first eligible
  simulation tick can attempt placement.
- Presses resolve when they arrive and held repeats only on frames that simulate a tick,
  both before the frame's physics and from the last completed tick's end state, so the next
  tick's movement collides with a placed block. Incomplete: a frame
  that simulates two ticks resolves build actions only before the first.
- Deterministic gameplay and wire tests cover ledges, jump bridging, towering,
  backward sneak bridging, failed attempts and refused transport.
- Placement and block-use classification read compiled block facts and numeric identity
  handles. Slab and sign counterparts come from the checked placement catalog. Support,
  interaction membership and replacement admission remain provisional; this migration
  does not close the placement parity gate.
- Provisional: slot or item changes preserve the held placement line and repeat
  schedule. Native slot-change callbacks and their effect on retained history,
  orientation and timing remain unverified; this does not close the hotbar-switch gate.
- Incomplete: complete runtime block-property and custom block-placer admission,
  item-specific replacement/state rules and live vanilla/server acceptance remain open. No
  complete placement parity or performance gate is closed by these tests.
- Vanilla rules: [held block placement](../../docs/reference/held-block-placement.md).


## Camera packets and aim assist

- Packet admission covers spline registries/instructions, aim presets, commands
  and actor-priority updates. Semantic errors are skipped and counted; malformed
  framing remains fatal. See the [Vanilla rules](../../docs/reference/camera.md).
- Preset inheritance, starting values, offsets, target tracking, camera collision,
  easing, fade, shake, FOV, listener and presentation capabilities have regressions.
  Named and inline splines have independent progress/rotation tracks. Aim selection
  uses retained physics-tick poses, item categories, priorities and visibility,
  with separate interaction direction and action-triggered rotation.
- Attachments belong to individual boom rigs. Selecting first person after an
  attached tutorial returns to the player; attaching alone does not select a
  camera. Clear and detach release only the selected rig's attachment.
- Server input locks commit in packet order independently of camera selection.
  Camera, movement, jump, sneak, dismount and directional locks filter input;
  raw movement vectors remain available and unlocking restores held controls.
  Mount permission routing and exact attachment actor-loss behavior remain incomplete.
  Dimension, respawn, game-mode and initialization interactions, plus the full
  current-version aim-assist and presentation-effects audit, remain unverified.
- Incomplete parity: animated attachment anchors, definition-authored actor hitboxes,
  complete native block tags, touch pick-range remapping, equal-score actor order,
  historical remote geometry during catch-up, and fire-resistance lava fog remain
  owner limitations. Legacy education photography output is unsupported. Packaged
  boom/shake/collision defaults and highlight sampler need pinned-version witnesses.
  The gameplay FOV multiplier follows vanilla (speed ratio, slowness, flying, bow,
  spyglass); the swim-speed factor and underwater narrowing remain incomplete.
  Sprint transitions recalculate movement speed from attribute defaults and other
  modifiers, preventing FOV overshoot after attribute resends and sprint restarts.
  Server Speed effect changes refresh that value while sprint stays held.
  Local Speed modifier expiry without a server update remains incomplete.
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


## Bedrock 1.12 combat visuals

Bedrock 1.12 combat visuals are a deliberate default and accepted deviation from
1.26.50. There is no user setting. `render_api::DEFAULT_COMBAT_VISUALS` retains the
Modern profile as a one-line code choice. The combat profile owns body and worn
armour hurt red, glint, camera bob/hurt, and ordinary Bedrock player attacks.
The legacy attack uses lookup trig for third-person joints, the bare first-person
arm and fallback main-hand items. Item use and special movement retain their
authored poses. Java 1.7 takes precedence for body, arm and item poses.
First-person hand sway deliberately uses Java tick smoothing in both combat
profiles, independently of the Java animation option.
Sway retains complete camera yaw turns through tick smoothing and frame interpolation,
including turns larger than a half turn in one input frame.
Leaving freelook restores the actor heading with the nearest equivalent yaw winding,
so full camera revolutions do not add artificial hand motion on release.
Activating or leaving the sleeping camera reanchors its presentation history, including
when sleep begins before held freelook is released; stable sleeping frames keep their history.
Movement bob uses the physics fraction; hurt and death use the actor fraction.
Remote movement keeps pitch in the signed half-turn range before strike poses read it.
Frame interpolation takes the shortest pitch arc across the signed boundary in Java mode.
Regression coverage samples repeated remote swings in Bedrock and Java modes with equivalent
signed and wrapped pitch values.
Page-level body dye masks retain zero-alpha RGB and depth independently of armour cutout.

Implementation and deterministic swing, sway, camera, overlay, equipment and shader fixtures
are present. Full combat-profile visual comparison remains incomplete; this does
not close a parity or visual acceptance gate.


## Java 1.7 animations

- Player poses and items are selected live from Video › Animations (persisted): Java 1.7 or
  Bedrock. Saved toggle choices migrate without changing the selected mode. Rules and frame mapping:
  [Java 1.7 animations](../../docs/reference/java-1-7-animations.md).
- First person: Java's item stacks (swing; equip with the old item through the dip and the
  re-equip after a placement; eat/drink; sword block with block-hitting; bow draw and pull
  frames; rods), the empty-hand arm, sneak eye height and arm sway. The shared combat
  profile takes precedence for view bob, hurt/death motion and glint. Third person: Java's
  biped pose, body yaw, limb swing with the hurt flail, held-item grips, the cape's chasing
  swing and the sneak drops; armour flashes red, held items do not.
- Local eating and bow draw pair gameplay use ticks with the physics frame fraction;
  bow artwork reads the same counter. Regression coverage checks frames between ticks,
  differing actor fractions, stopping use and retained items. Compilation, test execution
  and rendered motion validation for this correction remain incomplete.
- Every hurt event immediately resets the limb boost, including consecutive hits; movement
  still contributes to the phase, and death alone does not trigger a flail.
  Fourteen focused hurt tests pass. macOS/Metal captures at 1920×1080, DPI 1, GUI scale 2
  verify single and repeated hits, walking while hurt, and recovery to idle.
- Cape rotation is bounded at the shoulder in both animation modes: Java tilt is capped at
  90° and sideways swing at ±90°; Bedrock bounds the cape's actual direction after bind
  retargeting and preserves hanging authored rotations. These are deliberate presentation
  limits. Regression coverage includes vertical and fast horizontal
  flight, frame interpolation, local/remote actors and stopping. Remote teleport and interpolation
  resets now discard both Java chase endpoints; unflagged steps beyond ten blocks re-anchor
  without old lag. Remote Bedrock flap uses completed tick displacement instead of retained
  packet speed and ignores those large steps. Local flap retains per-tick velocity through
  catch-up frames. Regressions cover remote knockback, repeated hits, teleports, interpolation
  resets, local catch-up frames, hanging yaw/roll and upward directions near horizontal.
  Compilation, test execution and rendered-frame validation remain incomplete; the exact
  Bedrock flap calculation also remains incomplete. This change
  closes no parity or performance gate.
- Worn elytra retains authored wing poses and glint in both modes, suppresses the separate cape,
  and uses the cape texture when present. Native controller blend composition stays intact.
- Cape pixels are resampled and hashed in bounded appearance-worker batches. Equal source
  content shares a prepared layer across wire allocations; presentation reads ready layers
  without waiting. Cold capes appear after preparation, and session changes retire their
  queues. Rendered-frame and hardware performance acceptance remain incomplete.
- The HUD cape now uses the evaluated player cape bone and the shared world-cape attachment
  calculation instead of a resting menu mesh. Worn wings suppress its separate cape layer.
  Missing and hidden cape bones omit collapsed faces while keeping the body visible; flattened
  capes retain their visible faces. Filtered HUD, cape geometry and wing-routing tests pass.
  Before/after rendered validation remains incomplete.
- Golden tests assert composed stacks and projected arm, item and cape points against Java's
  calls.
- Review corrections: local head sampling stays in the current render frame; cape and camera
  motion use native velocity, health and riding state. Retained items keep their own use
  clocks through swaps, authored first-person rigs retain ownership, and raster depth stays
  one sixteenth at any texture size. Emote skin layers follow the sampled body pose.
  Unchanged render layers sharing geometry retain their own completed pose allocations.
  Held-item hand routing uses the owner's admitted definition, so rejected pack bows
  retain the startup bow's Java placement in both views.
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


## Status-effect column

- Vanilla's `mob_effects_renderer` draws one column at the right edge, below the
  top band. Effects run in id order, wrap leftward, and blink at vanilla's rate.
- The built-in Java pack keeps its Java rows at the top-right through its own
  `java_mob_effects_renderer`. Packs that place vanilla's control get the column.
- Incomplete: there is no slide-in or slide-out animation. The safe-zone and
  screen-position offsets are taken as zero. The top band ignores riding hearts and
  pocket-profile rules.


## Controller state time and pack queries

- `query.state_time`, `query.max_trade_tier` and `query.position` now compile.
  Unsupported queries had dropped whole controller transitions and script
  statements, leaving looping weapon recoil states and misplacing scripted markers.
- Controller weights, clip clocks and bone channels read the elapsed time of
  their own state, including separate outgoing and incoming epochs during blends.
  Position queries require one axis and read the native actor origin.
- Incomplete: `query.state_time` advances in actor ticks, like the existing
  finished-animation queries, rather than per rendered frame.


## Block selection outlines and visual bounds

- A nearer selectable actor suppresses the block outline behind it, using the
  same rounded-intercept comparison as interactions. Blocks in front of actors
  retain their outline. Local cow comparison matches Android Bedrock 1.26.50.4;
  this bounded correction does not close the remaining outline parity gates.
- Block shape compilation now belongs to `assets` and `asset-compiler`. The
  physics carrier stores final movement boxes, independent selection boxes,
  eligibility, outlines, tags, and typed context facts for both runtime ID spaces.
  Gameplay indexes these facts; ray, collision and outline queries do not parse
  canonical-state JSON or match block names.
- Runtime exceptions are paired doors, neighbour support and portal axes, vine
  and chorus outlines, deterministic column offsets, and piston actor inputs.
  Geometry-bearing actor updates retire pick revisions and historical snapshots
  retain their original facts, including remote piston-owner dependencies.
- Shape compilation reads block traits, model families, and validated shape
  memberships from the registry policy. Numeric bounds stay in the geometry
  compiler; the expansion records provisional memberships without closing a
  parity gate. Thin connected blocks share the pane collision recipe.
- Incomplete parity: client piston animation timing, legacy moving-block state
  migration, animated shulker outlines, remaining material/support bindings, and
  newer block identifiers using compatibility recipes. These do not close the
  interaction or rendering parity gates.

- At the owner's request, untouched and older settings select the native black
  outline instead of the filled highlight. Explicit Outline Selection choices
  retain the native two-mode rendering contract.
- Regular outlines retain the twelve unexpanded bounds edges in a dedicated
  opaque, unlit, depth-writing pass. Unchanged targets retain their geometry and
  upload revision across camera movement.
- The live outline was too faint. A Metal rendering diagnostic confirms FXAA
  dilutes thin black lines more strongly on diagonals. GPU expansion once supplied
  two-physical-pixel coverage as provisional compensation. The native outline is a
  twelve-edge line list, which rasterizes one pixel wide (a thicker outline is
  built only by the deferred renderer below 100% upscaling), so the stroke is now
  one physical pixel. Matching vanilla's anti-aliasing of it remains incomplete.
- Fence and wall selection use visual bounds independently of taller movement
  collision. Wall selection retains its state-dependent visual height and insets.
- Incomplete parity: the matching-version material depth offset, scaled deferred
  outlines and platform-specific factory defaults
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


## Configured inventory hotbar swaps

- User-requested inventory shortcut: the configured hotbar key swaps the hovered cell directly with that hotbar slot.
- Keyboard and mouse remaps share gameplay's saved bindings; replaced number keys no longer perform swaps.
- Local prediction updates both cells immediately without an inventory transition animation or a server round trip.
- Focused text fields retain input ownership. Installed Windows input acceptance is pending.


## Inventory close buttons and quick-move

- Engine-drawn container screens, the personal and creative inventory included, set
  `#close_button_visible`, so the pack's own close button shows (`common_panel`'s, or the
  inventory top bar's `close_button_panel`). Pressing it (`button.menu_exit`) closes the screen as
  the inventory key does, and in browsers that click recaptures the mouse.
- Shift with either mouse button quick-moves a stack between the open container or creative
  catalog and the player inventory, through the existing whole-stack item stack requests.
- Provisional: the pack routes `button.menu_auto_place` to `button.container_auto_place`, but no
  capture confirms which desktop clicks raise it; the Shift+right binding follows the
  maintainer's report of vanilla. Whether desktop vanilla shows the close button on every
  container screen is likewise unconfirmed by capture.


## Inventory authority and cursor cleanup

- Ordinary container updates use the open window ID and its manager indices. UI-window
  updates use raw UI slots, and registered containers use their full key and relative
  indices. Invalid normal transactions publish no valid sibling updates.
- Full player snapshots retire removed slots. Accepted request corrections use their owned
  snapshots, skip unrelated containers, and retain corrected names and damage in item data.
  Supported typed item data compares independently of compound and restriction-list order.
- Closing a screen returns its active temporary inputs and cursor, drops what does not fit,
  and keeps unrelated inactive inputs from blocking world interaction. Named stacks can
  merge and drag; a drag clamps contributions after an authoritative decrease.
- World-use selection reads player backing separately from screen predictions and recovery.
  Local backing mutations and hotbar changes still wait for transport admission.
- Held books read their identity and contents from player backing. Opening admits an item-use
  transaction, including initialized and written books; initialization only creates absent user data.
- A normal close acknowledgement retires that screen's unanswered requests without
  committing their predictions. HUD mining requests retain their separate ownership,
  and HUD/crafting observers follow the admitted packet and backing state.
  Enchant selection leaves item mutation unpredicted until its complete result pipeline
  is implemented; it does not keep an unchanged input as a predicted result.
- Manual crafting fills compatible destinations through repeated bounded result scopes.
  Each scope declares its per-craft output once. Explicit counts remain atomic when they
  do not fit; craft-all stops at available destination space.
- Ordinary recipe ingredients match their admitted item identity, aux and tags independently
  of unrelated item data. Crafting preserves remaining input data and keeps consumption
  permissions and output merging as separate checks.
- Secondary creative catalog clicks create one item with an empty cursor and destroy one
  with a held selection. Typed item locks apply to ordinary player removal, storage and
  station placement, drops, destruction and consumption.
- Inventory commands capture the actor's current effective permissions. Accepted ability
  packets replace prior layer definitions, and later mode assignments update their owned
  layers. Binding armor removal follows actor Creative mode; item creation follows
  Instabuild. Creative catalog handlers reject stale hits after a mode change, and empty
  Survival recipe-book clicks preserve the cursor selection.
- Server UI result previews remain admitted for existing screen consumers. Moving their
  ownership to locally generated station previews is incomplete and must preserve result
  display and taking together.
- Brewing bottle cells use a one-item limit derived from their named window run. Decoded
  potion and container mix records retain their wire order and allocation credit separately
  from ordinary recipe IDs. Runtime configured membership and routing remain incomplete.
  Ingredient and fuel cells use admitted item capacity.
- Loom banner inputs check the admitted item identity and typed root Type. Grindstone inputs
  require an enchantment List or admitted damageability without a true Unbreakable Byte.
  Valid durability components retain a zero Int maximum as damageable, without a durability bar
  or positive repair maximum. Inventory and world registry admission share that maximum rule.
  Registry changes retire placements or fallback routes whose
  admission changes. Cancelling unsent gestures restores the sparse owners they replaced,
  including empty cursor ownership; server rejection keeps its separate response behavior.
  Registry-dependent malformed component loading and remaining station membership checks
  are incomplete.
- Incomplete: book-edit publication and replacement ordering, crafting remainders and multiple results, enchanting prediction and availability,
  remaining station membership rules, ability updates beyond retained evidence,
  complete registry/model coverage, legacy response keys and alias-resolved item data, and complete live
  pickup, reconnect and close-lifetime acceptance. These changes do not close the inventory
  parity gate.


## Item stack sizes

- DataDriven entries use component stack sizes even for known item names. They use
  `item_properties.max_stack_size`, or the item default when no capacity is declared. Legacy
  and unversioned entries retain the known item's capacity; other names use the component
  flag to choose between a component item and a plain item. Unknown item versions bind by
  name in the same way. Unavailable component facts leave capacity unresolved unless the
  payload is the canonical empty compound.
- Provisional: a component item's top-level `minecraft:max_stack_size` component is read when
  it has no `item_properties`, and plain or vanilla items carrying that component leave their
  capacity unsettled. Parity verification of these layouts remains incomplete.


## Eating and drinking presentation

- While a player uses an admitted food or drink, the client sets `variable.use_item_startup_progress` and
  `variable.use_item_interval_progress`, so the pack's third-person `use_item_progress` pose
  raises the arm to the mouth and bobs it, for remote players and the local third-person body.
- The local player's crumbs follow its own eating sound steps: in front of a first-person camera,
  else from the mouth other players see. A server's eat event for the local player is skipped so
  bites do not double; other players' crumbs come from feed events for admitted foods.
- Arm poses, sounds and crumbs use the effective consumption after server and pack overrides.
  A food overridden to drink plays drinking presentation without emitting crumbs.
- The progress follows the use's remaining ticks: the arm rises until 25 ticks remain, then
  bobs in quarter steps of the remaining ticks modulo four, the 1.12-era formula.
- Provisional: no version-matched witness of the current engine's values is recorded, and the
  first-person crumb point (0.45 ahead, 0.15 below the camera) is an approximation.


## Headless chunk cost baselines

- Criterion exercises production palette/column decode, light solves, cube/biome
  meshing, bounded ingress-to-CPU-publication bursts, idle polls and metadata scans.
- Fixtures validate decoded cells, lighting, exposed faces and drained stream state.
- Biome lattices pack u16 tint indices and integer sample counts into four words per
  point, retaining full-width indices when needed. Record reuse is bounded and scoped
  to immutable biome, tint, pack and dimension sources. CPU and shader oracles compare
  decoded weights and colours with the original nine-word representation.
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


## Menu frame passes and retained memory

- The HUD composites after FXAA, inside the output pass; FXAA is off with no world drawn.
- HUD sprites use the shared nearest sampler across pack changes. Centered crosshairs snap
  to physical pixels after positioning; larger pack textures keep their source texels.
- The panorama draws opaque in the main pass; a static menu frame is two fullscreen passes.
- Font pages are coverage bytes on CPU and GPU; the transparent ref buffer grows on demand;
  JSON-UI caches evict least recently used entries.
- Incomplete visual acceptance: a rendered menu and HUD frame (text sharpness, panorama,
  crosshair, screen overlays) on the target platform is pending.


## Held cube item consistency

- Cube admission now ignores unrelated gameplay flags and terrain occlusion.
- Authored carried faces fall back to ordinary pack faces; transparent cube templates
  and cutout/blended sheets retain block geometry in both hands.
- First-person material modes preserve ice blending and cutout holes.
- Opaque and blended held blocks draw only their outward faces, so a held stained glass block
  hides its far side as in vanilla; cutout blocks (glass, leaves) render both sides. Session
  custom cubes keep both sides until their alpha mode reaches the mesh build.
- Regressions reproduce the previous gameplay-flag, alpha and missing-sheet failures.
- Rebuilt macOS/Metal frames show cube geometry; the user confirmed the live fix.
- Non-cube block items the inventory draws in 3D (slabs, stairs, fences, walls, anvils) hold
  as the same template geometry, from the state their inventory model shows; items the
  inventory draws flat (torches, plants, panes) keep their sprite.
- Chests (normal, trapped, ender and copper), shulker boxes, decorated pots and conduits hold
  as their placed block-entity models, closed and centred. Provisional: their held facing and
  the native item renderer's pose for them are unmeasured. Banners still hold as sprites;
  beds are sprites in vanilla too.
- Held block-entity artwork follows the prepared resource-pack generation and restores startup
  bindings after disconnect. Missing pot base or side artwork retains the sprite fallback.
- Animated and high-resolution carried geometry parity remains open.
- Vanilla rules: [held block items](../../docs/reference/held-block-items.md).


## Barrier selection visibility

- User-requested correction: suppress barrier highlights/outlines outside Creative.
- Preserve barrier collision, picking and normal block selection in both ID spaces.
- Incomplete native parity: selection eligibility inspected; remaining comparison
  service unavailable. Windows rendered barrier acceptance pending.


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
its additive RGB, and speed scales elapsed time before the texture scrolling phases.
The sampled glint appearance and phase periods remain provisional; rendered
item, world-item, and entity-glint parity remain incomplete. Current option
defaults/ranges remain open.


## Server primitive shapes

The retained renderer and packet pipeline implement the six Script API debug shapes. See
[the Vanilla rules](../../docs/reference/primitive-shapes.md) for packet patches, geometry, text,
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



## Server pack compatibility

Manifest format numbers accept integral decimal and exponent notation, including
`2.0`, within the existing supported range. Fractional and unsupported formats
remain rejected. Native Android 1.26.50.4 admits the observed decimal-format pack;
the fixed client reaches the Motaz world and the Veltrix rules form with their
required packs applied. Broader admission and rendering parity remain pending.

Resource-pack admission accepts corrected chunk-transfer sizes within its archive
and selected-stack byte limits. Repeated offer identities consume the count and
byte budgets once, retaining the first entry's size estimate. Required packs retain
their acquired archive and actual size through the session handoff. Incomplete:
unrestricted pack-size parity and exact native admission comparisons remain open.

Galaxite's full-block geometry, composite Battle Pass models, translucent podium
glows, state-filtered path borders, correctly lit benches, custom hotbar/held items
and source-pixel form borders render in a 1920×1080, DPI 1 macOS/Metal hidden-client
pass. Custom geometry preserves explicit absorption and its legacy type-light flag.
Item registries retain numeric aliases and populated definitions accompanying empty declarations.
Zeqa equipment sources and dynamic UI textures retain bounded native dimensions;
rejected UI publications preserve the previous catalog and retry pending artwork.

Ordinary bodies and first-person hands sample collision-box top and bottom light;
equipment inherits body illumination. Samples beyond the admitted dimension height
use its default light pair without clamping to an edge cell.

Incomplete: merging multiple different populated component definitions, unrestricted
pack-size parity, special actor volume lighting, exact native frame comparisons
and release hardware budgets remain open. Two terrain texture keys absent from the
served Galaxite stack still report diagnostic textures; this compatibility work does not close those parity gates.

Follow-up compatibility covers client-authoritative inventory opening and transfers,
authored entity visibility bounds, filtered HUD control messages, and multipart
custom-block collision admission. Collision lists retain at most 256
primitives. Per-state collision and selection overrides now resolve once at session
admission and register in both network-ID spaces. Captured bottom/top slab fixtures
match their authored geometry heights and retain stationary support for 100 ticks.
A state's authored `minecraft:transformation` now rotates, scales and translates its
explicit collision and selection boxes about the block centre through the same
transform the overlay applies to geometry, so face-attached blocks collide on their
authored face. Rotation and scale pivots are still ignored for geometry and boxes,
so pivoted transformations remain incomplete. The definition tree
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

Chat trigger filters now survive built-in layout restoration when a pack attaches
its visibility binding inside a derived HUD row or the open chat history label.
Pack variables resolve before the binding transfers, and effect factories retain
the full message stream. Private-use bitmap icons preserve their sheet colors and
text opacity; missing icons retain the normal replacement glyph tint.
Incomplete: several active derived labels with different `#visible` filters are
flattened onto the one built-in label, so the last filter wins instead of their union.
Full native pixel matching, supplementary-plane emoji routing and hardware frame
budgets also remain open. These changes do not close general chat parity.

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
Lunge moves the fixed client without movement input. Hotbar shading now follows
admitted attack-category cooldowns, including other displayed stacks in the same
category. The complete matched-version native comparison remains incomplete. See
[spear actions](../../docs/reference/spear-actions.md).


## Third-person held attachables

Incomplete: held attachables publish one selected layer per hand. Authored
dissolve depth/color passes use a drawable ordinary-material fallback, retaining
overlay suppression; coupled multi-pass coverage remains unsupported in both
first and third person. This does not close a material or held-item parity gate.

Selected tint, overlay, UV animation and lighting modifiers are retained in both
view submissions. Incomplete: the first-person hand shader does not yet consume
those fields. Projection regressions verify published values; first-person styled
rendering remains incomplete and does not close a material parity gate.

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
  See [local placement rules](../../docs/reference/block-placement-prediction.md).
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
recipe discovery remain incomplete; the full furnace parity gate stays open.

2026-10-17 inventory preferences: protocol 2193 inventory-option updates enter
ordered startup/play ingress and remain session-owned. Classic personal inventories
and crafting tables snapshot the complete tuple, resolve their own active layout,
and preserve the inactive layout. Tabs use explicit UI/wire conversion. Changed
controls save immediately without echoing incoming updates; queued older choices
cannot suppress a later selection. Filtering defaults to false, with a creative
model bypass that preserves the saved toggle. The carrier renders Classic's wide
recipe list, and right-tab selection bindings are supplied. Pocket presentation,
input-profile selection, persistent profile storage, recipe-unlock category guards,
full right-panel presentation, additional search-related saves, and stable-client
visual acceptance remain incomplete;
these changes do not close the inventory parity gate.

2026-10-17 furnace preferences: protocol 2193 furnace-option updates enter ordered
inventory ingress and retain separate player choices for furnace, blast furnace
and smoker. Each open controller snapshots its saved tab, filtering and layout;
later server updates change retained preferences without replacing that snapshot.
Changed controls save and queue one options packet, unchanged controls send none,
and a full queue leaves the choice available to retry. Furnace filtering no
longer uses the crafting filter, and the inventory tab is independent of layout.
Stored unspecified layouts resolve to the recipe-book layout on opening. The
profile-specific forced-layout branch, exact stable-client gameplay acceptance,
recipe discovery, ghost rendering and other station rules remain incomplete.

2026-10-17 recipe category facts: item registry components retain the legacy
numeric category with its typed Int and low-byte rules. A valid properties
compound defaults that fact to zero when the field is absent or has another
type; ambiguous paths remain unresolved. Recipe-unlock category masks and
their UI consumers, built-in category defaults, and complete stable-client
acceptance remain incomplete.


## Entity interaction and emote starts

Entity-use presses now send the selected stack and fresh actor hit in the input
frame, with block occlusion and bounded queue retries. A live cow interaction
produces a server-confirmed milk bucket. Selecting an equipped custom emote starts
local playback and reports its catalog identifier and phase over the authenticated
Cinnabar API. Stops, visibility and bounded leases retire shared playback without
sending custom identifiers to a Bedrock server. Marketplace clip ownership/playback
and the complete villager trade UI remain incomplete. These producer fixes do not
close those parity gates.

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


## Keyboard & Mouse command macros

- Keyboard & Mouse settings hold ten command macro slots below every other
  keybind, each with a "Command Macro N" label, an Unassigned binding button and
  a command box. The section header matches the shipped wording. A press on a
  binding button captures one desktop key or mouse button; Alt supplies the
  modifier by default. Cinnabar's Macro Modifier setting applies to all slots and
  offers Alt, Ctrl, Shift, Super or None; None runs the bound control alone.
  This customization is an intentional extension to vanilla. Macro rows are
  keyboard and mouse only and never join the gamepad layout.
- Commands persist in settings alongside the bindings, bounded to one outbound
  chat packet, including the command slash, and free of control characters.
  Oversized drafts remain editable with an inline error and do not replace the
  saved command. A slot with a binding but no command stays silent, and an empty
  command box returns that slot to untouched.
  A keyboard layout reset clears macro rows while retaining their commands.
- The selected modifier plus a bound control while playing queues the command as
  Enter would, consuming that control even if the queue refuses the send, without
  resetting other held controls or pointer motion. Mouse buttons use the same macro dispatch. A leading
  slash is added for commands typed without one; macros do nothing while a menu,
  chat box, inventory, sign editor or emote wheel owns input. Keyboard macros
  fire once per physical press; consumed controls stay suppressed until release.
  Clicking or tapping a native command box places its caret at the selected
  character.
- Incomplete parity: Bedrock's exact per-slot key names, default bindings and
  command-box editing affordances are unverified, as is macro behaviour over the
  JSON-UI settings fallback. Macros run client-side, so a server rejecting the
  command is indistinguishable from the client's own feedback. No native parity
  gate closes from these changes.

### Shipped font grids and coverage (visual acceptance pending)

The Sans carrier includes every mapped source scalar at its 18-pixel raster em. The four
generated faces use a 1000-unit em and committed pixel cells with individual widths and
heights. The generator traces those cells into fresh outlines and writes declared advances,
bearings and pair spacing, then keeps only each face's Latin, Greek, Cyrillic and symbol
ranges; Sans draws every other scalar. Fonts and source manifests reproduce byte for byte.

The Sans carrier preserves partial edge coverage and glyph advances in 1/64 atlas texels,
excludes legacy character maps from Unicode lookups, and retains source bearings. Face
bundles hold a 52 px FreeType distance field and exact 1-9 px coverage with the same advance
precision. The runtime loader applies generated pair spacing at each raster em; layout
includes it before wrapping. The carrier table owns line metrics. Sans pixel-grid text
rounds final unrotated glyph edges onto the device grid; face rasters are filtered. Five and Five Bold
are available as roles; current screens still use Seven and Ten.

Missing regular, bold, italic and monospace mathematical Latin letters and digits use
original grid-based variants built from the shipped Latin artwork.
The Sans carrier also supplies original monochrome bed-symbol artwork when the source
outline map lacks it, so MOTDs can display that symbol without a replacement box.
Unrestricted Unicode, script shaping and native fallback rendering remain open. Owner visual acceptance and
live text rendering across scales remain open gates.


## Session biome climate and environment

Admitted live biome identifiers and climate now travel with the session tint registry. Weather, fog precipitation, camera temperature and environment routes read that registry, including custom biome routes. Registry replacement clears removed routes.

Incomplete parity: exact custom-biome fallback profiles and regional or height-adjusted temperature behavior remain open.


## Compiled item armour

- Protection and player slots live in the entity carrier; HUD capture reads numeric registry facts resolved at admission. Server wearable components replace protection without per-frame parsing.
- Inventory quick moves use the pinned armour table and shared head identities. Loom banners, smithing inputs and horse armour accept negotiated item tags with pinned membership as a fallback. Registry resends retire pending quick moves whose input role changed. Custom head items follow their declared wearable slot. Request-target regression coverage uses the retail registry and admitted server declarations. Local execution and rendered acceptance remain incomplete.
- Incomplete: the full armour identifier/class inventory, missing-member defaults, body slots and later component versions still need parity verification. Unverified items and malformed declarations retain explicit unresolved facts.

Effective item durability is shared through required entity-carrier descriptors and session registry admission. Unknown entries and invalid declared maxima stay unresolved; UI and elytra gameplay read the admitted numeric-ID fact. This gate remains incomplete: the full damageable item inventory, native class bindings, numeric component bounds and all network replacement behavior still need verification. The pinned base table preserves the existing positive values and its coverage guard checks their membership in the independent retail inventory.

Compiled entity interaction traits remain incomplete. The entity carrier admits separate
pick, placement-obstruction and aim dispositions, checks every independently collected
entity identity, and retains the existing class routes as provisional compatibility data.
Actor admission resolves advertised bases once, preserves the first advertisement,
refreshes earlier actors, and caches numeric dispositions for frame and ray consumers.
Unadvertised identities and unadmitted nonempty bases remain unresolved; attack picking
still admits them through their collision box or server hitboxes, while aim assist skips
them. Empty advertised bases retain an explicit provisional generic-mob disposition. Complete native constructor,
mutable component, spectator pick/obstruction and class inventories are still open;
the generic-mob disposition remains provisional and does not close a parity gate.

Committed asset tables are checked against pinned block, item, actor and pack-name
vocabularies. Retained compatibility exceptions name the exact field and fail when
unused. Every pinned actor has an explicit interaction disposition; unsupported
entries remain unresolved, and identifier coverage does not establish behavior parity.

Compiled seat descriptors are incomplete. Every independently collected entity has a seat admission, including unresolved definitions. Authored base layouts, bounded rider counts, literal rotations and Molang rotation references are compiled before runtime; component-group names no longer select layouts. Actual group activation and rotation-expression evaluation remain unresolved, so those layouts retain streamed poses. Passenger ordering, mutable rideable components, session behavior overlays and complete native layout coverage remain open. Static count selection preserves the existing compatibility order and is provisional.

### Compiled intrinsic shine (incomplete)

Intrinsic shine admission remains incomplete. The versioned item descriptor retains the existing
four admitted positive choices as provisional compatibility data and gives each independently
admitted retail identity an explicit disposition. Written books are absent from this inventory
and remain unresolved until their admission is independently established. Unsupported identities remain unresolved. Full native class,
metadata and component-version coverage remains open; compiler facts alone do not close these
parity gates. Numeric session admission and shared world/UI consumers are implemented; their
focused tests pass. Full visible rendering, metadata and native coverage verification remain open.


## Held-item grip admission (incomplete parity)

Item grip facts are compiled into entity carrier 21, checked against the separate
retail identity inventory and copied through numeric item admission into equipment
input. Unknown names cannot borrow a tool grip. Explicit session flags, including
false, replace the compiled upright choice. The existing built-in grip and rod-art
choices remain provisional compatibility data. Complete native, metadata, version
and custom base-item coverage is incomplete. Optional Java animation name routing
remains a separate incomplete admission path. This does not close the complete
equipment parity gate. `make assets` rebuilds carriers that lack these records.

Canonical empty component data preserves the compiled native item grip even when
an item registry marks the item component based. Explicit hand flags still override it;
malformed, missing nonempty definitions and unknown versions remain unresolved.
Original regressions cover upright and mirrored art plus retained equipment refresh.
A local fishing-rod check restores the upright grip and first-person art direction.
Incomplete: a third-person rod rotation difference remains against native Bedrock.


## Spawner display actors

Spawner displays now use client-owned actor lifetimes and the ordinary actor
evaluator, render controllers, sampled poses and active pack artwork. Configured
entities no longer depend on a fixed mob list. Displays retire when the block or
world changes, and pack replacement rebinds existing lifetimes.

Incomplete parity: display factory metadata defaults, exact native renderer bounds,
activation edge cases and the full entity gallery are unverified. Frame and GPU
budgets remain open; these changes do not close those acceptance gates.

Item-use animation routes and charged projectile drawing roles are typed admissions.
Local and remote equipment read numerical registry facts, including generation replacements;
item names do not select bow or crossbow use frames. Unsupported charged projectile metadata
has no admitted drawing frame. Complete native item/stack metadata coverage and stack-NBT
cache acceptance remain incomplete. Authored and session use cooldown fractions share the
provisional component-precision conversion; native cooldown initialization is unverified.

## Loom banner previews

JSON-UI banner controls draw ordered front-cloth layers from the active artwork. Loom pattern choices and results publish bounded pattern and dye bindings, including existing banner layers.

Incomplete parity: recipe eligibility and exact pattern-choice styling remain open.


## Custom fence connection states

Custom fences use the `minecraft:connection` trait
(`minecraft:cardinal_connections`): four neighbor-derived booleans
(`minecraft:connection_north/east/south/west`) that read as ordinary block
states in permutations and bone visibility. The trait parser expands them to
four boolean palette axes (sixteen states), so each state hides only its
disconnected rail bones and collision permutations resolve per state.
Connection queries without the trait stay unevaluated and keep the base
visual, locked by the connection permutation regression.

Incomplete: the axis order (north, east, south, west) and their placement
after placement traits are provisional with no wire capture yet; hashed
sessions hash sorted names and are unaffected, but sequential palettes need
the server order confirmed. Rendered native comparison (isolated post
against connected arms) and cross-block behavior (`connection_rule`,
vanilla fences connecting to tagged custom fences) remain unverified.



## Compiled item facts

A hash-checked item-facts carrier joins the pinned retail IDs, tags, capacities,
armour, durability, fuel, glint, grips and use profiles. Startup requires its exact
fingerprint; `make assets` rebuilds it. Registry admission caches server component
facts by negotiated numeric ID and retires them on replacement or reconnect.
Inventory equipment, brewing, loom, smithing, beacon and horse routes, enchantment
slots and anvil repair now read admitted item facts. Beacon payment icons share the
compiled payment roles and renderer keys. Enchantment rows use checked protocol IDs.
Cartography map routing retains its legacy fallback because filled-map identities are
absent from the pinned retail carrier. Native compatibility rules remain provisional;
missing identities, animal armor enchantment slots and complete repair membership
remain unresolved. Other consumer migrations and gameplay or rendering parity gates
remain incomplete.
Registered native filled maps retain provisional offhand admission at session admission;
explicit component permissions take precedence.


## Generated block-entity routing

The active registry now generates one block-entity table for backing membership,
NBT identity, renderer selection and sign/banner mounts. Renderer admission,
spawner displays, portal scans and visual diagnostics share that table; state
identities come from the pinned registry rather than numeric ranges.

Incomplete: the additional renderers for brushable blocks, trial spawners and
vaults remain unsupported. New logical routes are provisional, and the existing
renderer, visual and performance acceptance gates remain open.


## Runtime visual fact consumers

Actor contexts retain numeric item facts for both hands and worn armour. Tag and
material queries use those facts; engine variables and particle math share symbol
owners. Native particle routes use typed handles and retain pack override precedence.
Hardcore heart roles and fixed native atlas paths have one shared definition. Java
held-item presentation reads the admitted upright grip.

Incomplete: event routes remain provisional. Authored material metadata, predicted
audio cues, dynamic texture associations, particle filters and biome ambience need
further admission work. Rendered comparison and performance acceptance remain open.


## Custom block component bounds

Explicit default collision and selection components retain their bounds before state transformations. Empty pixel boxes remain empty instead of becoming full cubes, asymmetric X bounds follow the block coordinate convention, and collision boxes retain their supported taller Y range. Movement, targeting and outlines keep their independent component bounds in both palette modes.

Incomplete parity: the original report does not identify the affected block definitions. In-world acceptance of the component cases remains open.


## Inventory opening while mounted

Inventory input resolves a retained mount through live actor authority and sends its runtime
ID instead of the local player's ID. On-foot and unresolved mounts retain the personal
request. The chosen runtime ID survives transport delay. Supported server window opens
are handled without request correlation, including replies received after cancellation,
timeout, or a changed target. An open settles the pending input without retaining reply
history or sending a rejection close. Personal opens supersede storage; storage opens
keep the active personal screen guard.
Open timeouts release local admission without closing a guessed window ID.
Item requests owned by a screen retire when it is destroyed.
Admitted user closes retain their generation so late replies cannot close a replacement.
Full mount container, vehicle input and platform-specific inventory-opening guards
remain incomplete.

## Selected view radius during startup

The join captures the saved render distance and uses it in the first chunk-radius
request after StartGame. Startup and live updates share the existing request
builder, so the selected radius stays separate from the advertised capability.
Regression coverage exercises saved preferences with a hidden menu and the
core-to-client spawn sequence, including a selection above the advertised byte.
The focused startup and join suites, saved-setting checks and both owner checks
pass, including a join selection above the advertised byte. Platform-specific
scaling, duplicate update timing and full startup packet parity remain incomplete.

### Latency reply batching

Ordinary latency echoes retain FIFO admission and join the current outbound frame.
A full command queue retains the echo and submits the reserved frame delimiter,
then retries the echo without allowing later packets to pass it. Exact async
receive/present phase, compression representation and transport jitter remain
incomplete.

2026-10-17 autumn foliage rendering (implemented, uncommitted): behavior-pack
material instances now bind stateless builtin crosses, including red shrubs,
with inset front/back planes and authored alpha/AO policy. Numeric leaf facts
select species-authored falling particles through the pinned vocabulary; their
pack motion and tint remain authoritative. Static particle texture UVs clamp
within their own atlas rectangle, preserving atlas-relative terrain particles.
Sugar cane uses narrower full-height cross planes shared by every growth state,
with no random segment offsets. Its shared tint facts now select the grass biome
colour, which remains attached when the compiler imports fallback alpha policy.

The optimized client and world carrier were rebuilt. Hidden local-BDS captures
confirm textured shrub crosses replacing diagnostic cubes, falling poplar leaves,
and narrower cane geometry. Three focused compiler regressions passed; the added
leaf-fact and GPU sampling regressions remain unrun. Specialized leaf emission
probability still requires current-client confirmation; broad foliage parity
remains INCOMPLETE. The cane tint regression failed before the correction and
passes afterward for every growth state in both ID modes. The registry compiler
reproduces the complete facts expansion with unchanged block/light carriers;
local daylight captures confirm the corrected biome colour.


**Short grass and shelf mushrooms (2026-10-17, local changes).** Short grass and
fern use narrow crossed planes and a deterministic, column-seeded three-axis
displacement shared by rendering and picking. Extensionless terrain textures
prefer TGA over PNG in the offline compiler and runtime pack loader; explicit
filenames retain their selected format. This restores the brighter short-grass
art supplied alongside the darker PNG. Shelf mushroom states are admitted from
the source registry, with bounded behavior-pack permutation resolution and
shared geometry parsing for their growth sizes, facing, face UVs and material
policy. Selection retains the oriented collision footprint and selects the cap.
This does not close the general vegetation, lighting or component-model parity
gates; arbitrary component permutations and mushroom movement responses remain
incomplete.



## Held-use progress events

Accepted eating and drinking uses report due progress before their movement
inputs, using the negotiated item identity. Progress admission survives an input
retry without sending the event twice; release, reselection and life changes end it.

Focused gameplay tests cover timing, cancellation and admission retries. Progress
pauses while a release awaits retry, and non-consumable cancellation stays with
the item-use lifecycle. Other animation kinds, block-backed item uses,
relevant-user-data cancellation, mounted phases and special movement catch-up
need further parity work. This does not close the complete serverbound parity gate.


## Correction sync posture (incomplete)

Correction-sync packets combine received actor metadata with the completed local
sneak and sprint states. Undefined attributes retain their wire defaults and refused
sends remain pending. Other locally predicted flags, bounding-box updates, history
loss and exact send phase remain open; this does not close movement parity.


## Local actor presentation flags (incomplete)

Local animation flags are retained separately from received gameplay metadata.
Visual held-item use and locomotion poses cannot change correction-sync flags.
Nametags, player previews, hands and the paperdoll read the rendering state.

Accepted local held-use and glide/swim/crawl projection, replay authority and exact
send phase remain incomplete. This separation does not close movement parity.

## Near-contact collision faces

Swept collision clipping and overlap recovery share the same face-contact tolerance.
Tiny overlaps no longer create a landing against an adjacent wall or an unsolicited
separation move. Larger penetrations still resolve. This bounded geometry correction
does not close complete collision-engine, terrain or current-version movement parity.

### Reject item releases after a selected count change

Releasing after a selected count change now ends the old use without sending a
release for the replacement stack. Held updates retain their start snapshot, an
unchanged count still releases, and an already-observed release retains its retry
policy. Full item identity, unavailable-selection, deferred-release and use-phase
parity remain incomplete.
### Keep custom playback out of native emote input

Custom emotes continue through their render overlay and Cinnabar API. They no
longer start or cancel the native movement-input emote duration. An adapter
regression checks the serialized next input during local playback and restarts.
Native Marketplace admission, identity, cancellation and timing parity remain
incomplete.
### Retain an active loading screen across dimension replacements

Overlapping dimension requests from the same player reuse the active loading
screen. Only a present replacement ID updates that screen; pending replacements
coalesce before the update. A new player or session starts a separate lifetime.
The final report retains the active ID through transport retries. Full loading
screen, respawn, startup and presentation timing parity remain incomplete.


### Keep mouse boating requests independent of forward motion

Mouse boating no longer derives both paddle request bits from forward motion.
Side steering retains its existing requests. The other device modes retain
their provisional mapping. Independent simultaneous digital paddle requests,
input locks, mapping transitions, correction replay and full current-version
vehicle input parity remain incomplete. This change closes only the bounded
forward-motion flag mismatch; it is not a full vehicle parity gate.
