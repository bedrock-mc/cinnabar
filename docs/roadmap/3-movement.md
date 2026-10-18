# Phase 3 — Movement and the local player `P3-MOVEMENT`

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


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
  collision shapes, and failed replay queries stop prediction instead of guessing.
  Collision scans floor their inclusive upper bounds so fractional queries do not suspend
  movement over an extra, unavailable cell. Resolved collision faces survive rounded
  center updates and replay, so stacked column faces cannot fabricate support or
  block a jump; pose changes retain horizontal contact and relocations rebuild it.
  Required missing collision data still stops
  prediction; full unloaded-boundary behavior remains incomplete.
  A generator pinned to bedsim v0.1.3 records a checksum-bound JSONL trace, and the Rust conformance test
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
    `cargo test -p cinnabar --locked` (lib 420, assets 41, hud_assets 9,
    inventory_router 3, physics_assets 2, doctest 1; all passed);
    `cargo test -p protocol --locked` (21 test binaries, all passed);
    `cargo test -p client-world --locked` (lib 236 passed / 1 ignored,
    entity_runtime 11, item_actions 14; all passed);
    `cargo fmt --all -- --check` passed;
    `git diff --check` passed;
    `cargo clippy --workspace --all-targets --locked -- -D warnings` passed with zero
    warnings and is CI's exact Clippy command; and
    `Invoke-Pester -Script 'scripts/tests/acceptance/MovementEvidence.Tests.ps1' -PassThru` passed
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
  - `MovementLauncher.ps1` now supports authenticated `Zeno` runs at
    `zenomc.org:19197`, following the same candidate/free-camera scenario and five-minute
    minimum used by the other external targets. Zeno is the low-population official-BDS
    server-authority target for movement rejection and correction checks.
  - Pre-existing, out-of-scope observation: adding `--all-features` to Clippy failed in the
    since-removed vendored client session crate because its optional dependencies were not
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
    `CINNABAR_WORLD_READY` never fires (`app/src/acceptance/mutation.rs:270-272`), the
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
    harness lanes that add `--movement-candidate-physics`; `FreeCameraSilence` remains
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
    cinnabar tests, strict cinnabar Clippy, and all 106 Phase 3/FastTransfer
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
  Dry low-space entry now selects crawling from standing when neither standing
  nor sneaking fits; water contact selects a fitting swimming pose. Teleport
  recovery previews the destination pose before probing overlap, preserving a
  fitting low-space feet anchor. Controller tests
  cover crawl travel, eye height, start/stop input edges and correction replay.
  Headless fixture captures confirm trapdoor closure, crawl travel, exit, and
  actual pearl relocation; broader crawl and correction parity gates stay open.
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
  separately; numeric regressions alone close no acceptance gate. Wire edges for
  swim/glide/crawl/fly and `PersistSneak` still need complete native input comparisons.
  Glide travel, firework glide boosts (replayed from their stamped tick), glide start/stop and
  the seven-tick flight double-tap follow the identified vanilla systems; the held-jump glide
  lift gated by an unidentified movement ability and geyser boosts remain incomplete.
  Rocket launch audio has a first-tick actor request with packet echo pairing. Integrated
  audio acceptance, offhand air use and a comparison of boost timing and flight speed remain
  incomplete; numerical travel regressions alone do not close these gates.
  Water and lava travel read the underwater and lava movement attributes, and a swimmer's
  dolphin boost scales speed and drag as vanilla does. Riptide charge and release now feed an
  additive level-dependent launch, retained release facing, spin lifetime and start/stop input flags.
  Ordinary and assisted releases retain eligibility at the release edge while waiting for facing
  input, including movement FIFO backpressure before interaction sampling, without extending charge
  time, and keep use slowdown through that facing tick. Accepted uses retain their facing fence
  while pending inventory gestures hide the selected stack. The first release observation retains
  its selected item, eligibility and effective action facing for both that input and the launch;
  later look and aim targets cannot replace it. Its sprint gate expires
  on the same fixed tick even when a frame runs several ticks. Admitted releases retain
  their launch through snaps and replay fallback until its movement input is written; discarded
  releases cancel their pending and replayed launch. Cancellation replay rebuilds interpolated
  camera eyes, retained aim and liquid attachment heights from the retained pose state.
  Locomotion, travel and replay share the preceding spin's compact liquid probe, including glide entry.
  Respawn search and ready anchors discard
  retained releases, active item-use charges, temporary use modifiers and pending launches from
  the old life, including their reserved input flags and facing. Item-use producers wait for
  respawn completion before accepting new uses. Each distinct ready control retains a completion
  before later server controls, including repeated positions; transport retries do not duplicate it.
  Full respawn input suppression and tick continuity remain incomplete.
  Travel-suppressed spin ticks use the compact
  body for block contact. Impulses beyond the
  collision-query budget are skipped and logged instead of stalling prediction; this safety
  bound is a known divergence for unsupported enchantment levels.
  Riptide parity remains incomplete: positional rain temperature, precise rain occlusion,
  actor-hit termination, server spin metadata, launch audio and integrated acceptance remain open.
  Incomplete safety bound (a known divergence; vanilla is unbounded): Depth Strider's water-speed
  blend target is clamped so the steady water velocity stays inside the simulator's block-sampling
  budget. Land movement speed is not clamped.
  Honey jump/slide, soul speed and depth
  strider coefficients are provisional (honey and soul speed have no public value). Riding
  suspends player physics and streams steering input with boat paddle flags. Server-defined
  seat offsets follow the mount's actor origin and yaw for local and remote riders without
  multiplying the offset by entity scale. Boat jump and other vehicle sneak presses send a
  vehicle-targeted dismount request; the server's removal link releases the rider.
  Local movement includes solid actor bounds from compiled definition defaults and synced
  collision flags, excluding the player's own mount. Prediction snapshots retain the actor
  bounds used by each frame after later movement or removal. Landing, side contact, step-up,
  size updates and retained support have deterministic coverage. Controlled live acceptance
  of boat, chest-boat and shulker collisions remains incomplete. Shulker attachment and
  lid-dependent bounds still need their specialized actor-shape implementation.
  Mounted prediction retains the seated posture while an exit request is pending, including
  during correction replay; the held sneak input resumes crouching after the server unlinks.
  Incomplete: authored seat fallbacks and special horse/camel/parrot placement, predictive
  dismount exit placement, client-predicted vehicles (`IsInClientPredictedVehicle`) and
  horse jump wire signalling remain open. Sweet berry bush slowdown is written in `tools/registrygen`
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
  `ClientMovementPredictionSync` becomes due 200 completed client ticks after a
  server correction. Undefined base attributes send zero; undefined friction,
  bounciness and air drag modifiers send 1.0/0.0/1.0. All three metadata flag
  words are retained, with completed local sneak/sprint overlaid. Accepted server
  posture updates supersede that sample until a new local tick completes, including
  retries on frames without a completed tick. Standard unscaled walking height
  follows the same sneak state. Transport refusal leaves the sync due. Other
  predicted flags, custom/scaled and horizontal-pose sizes, history loss, exact
  send phase and live server acceptance remain incomplete.
  `IsInClientPredictedVehicle` is deliberately never set: the public notes state riding does
  not imply prediction and no vehicle simulator exists, so all rides send ordinary player
  input. The horse jump has no dedicated packet in the pinned gophertunnel; it rides the raw
  jump flags and the mount's jump strength.
