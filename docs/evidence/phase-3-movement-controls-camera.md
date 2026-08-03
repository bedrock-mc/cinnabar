# Phase 3 movement, controls, and camera evidence

Status: candidate implementation focused tests and fixture-only evidence-harness validation are green. The broad deterministic matrix, live/native acceptance, independent review, and the separate production-enable gate remain open.

## Implemented contract

- Physics remains `ProductionDisabled` unless the explicit Phase 3 candidate flag is present. The `CandidatePhysics` scenario adds `--phase3-candidate-physics` and never adds `--auto-fly`; the independent `FreeCameraSilence` scenario adds `--auto-fly` and never enables candidate Physics.
- Completed fixed physics ticks enter one bounded FIFO. Successful command admission stages the complete immutable per-tick evidence context (FIFO and pose generations, dimension, perspective, camera/avatar state, look input, authority, and queue counters) beside the exact tick input. Only the later successful socket-write acknowledgement publishes that staged record; render-frame state changes while the write is pending cannot rewrite its evidence. A render update that catches up multiple ticks publishes one evidence frame for every acknowledged tick, with non-retrograde pose generations permitted across ticks admitted from the same rendered pose.
- Replay corrections are transactional and retain exact collision-world identity. Teleports, MovePlayer controls, and dimension reanchors use the snap path. A snap aligns to `max(server_tick + 1, existing_next_tick)`, including missing or negative MovePlayer source ticks, so the local ticker cannot move backward.
- Correction evidence is emitted only after a successful candidate correction. Each record names `replayed` or `snapped`, records corrected/replayed ticks, and carries a finite bounded correction magnitude.
- `InteractionOriginSnapshot` atomically copies session, FIFO, physics tick, pose generation, perspective, collision identity, eye origin, and look direction from `LocalPlayerFrameCarrier`. Correction/session/dimension invalidation removes the outbound ray.
- Frame evidence records exact input mode, held/start/repeated/released jump state, grounded state before and after the tick, outbox depth/drops, perspective, camera-blocked/fallback outcomes, and local-avatar visibility. Failed, cancelled, stale, duplicate, or out-of-order socket acknowledgements cannot publish a frame.
- Any production evidence violation, queue drop, free-camera movement packet, invalid correction, or contradictory camera/avatar state is fail closed. At the acceptance deadline Physics stops producing or admitting new commands, then the runtime keeps processing already-admitted worker acknowledgements for at most the existing two-second acceptance grace. A healthy final write therefore reaches `Drained` before exactly one terminal record is emitted. Timeout or transport failure emits an explicit non-`Drained` terminal and exits in error. The terminal binds the session, active source, packet counts, final pending-outbox depth, and typed reconciliation state. `SocketPending`, `BudgetDeferred`, `TransportRestored`, and `FullRestored` are typed diagnostic states, but none can pass the `CandidatePhysics` terminal gate; only zero-depth `Drained` passes. `FreeCameraSilence` requires `NotAuthoritative`, zero pending input, zero Physics packets, zero free-camera packets, and no movement frames or events.

## Identity and orchestration

`scripts/acceptance/Phase3Launcher.ps1` maps the reviewed targets exactly:

- Lunar: `pvp.lunarbedrock.com:19134`
- Zeqa: `zeqa.net:19132`
- LBSG: `play.lbsg.net:19132`
- Zeno: `zenomc.org:19197`
- BDS: supplied endpoint, defaulting to `127.0.0.1:19132` (the BDS process must already be available)

The launcher refuses tracked dirty source, embeds the exact clean `HEAD` and `RUST_MCBE_SOURCE_DIRTY=false` in the client build, builds and hashes the core and client, and creates a run ID. Its run directory must be newly created or empty. Before launching the core it proves that the unique socket directory contains no `game.addr`; afterward it accepts only a fresh publication while that exact core process remains alive and binds the witness to that core PID. It then launches the selected client scenario. On Windows the reviewed executable is always `target/debug/bedrock-client.exe`; the launcher does not copy or rename it. In-process identity markers bind the build, run ID, upstream endpoint, PREG/BREG, core SHA/process ID, and app process ID. A copied stale binary cannot claim the current clean `HEAD`, and a binary without the explicit clean-source compile marker cannot emit candidate identity.

Lunar, Zeqa, LBSG, and Zeno runs require an existing authentication cache beneath `.local`, pass its resolved path to the core as `-auth-cache`, and reject durations below 300 seconds. BDS remains the only offline target and may use a shorter local diagnostic duration.

Operating caveats to record and review before a live run:

- The wall, corner, and ceiling manifest checks only prevent weakening the three named controlled scenarios. They do not prove those geometries were exercised: a valid aggregate is not evidence that the scenarios occurred, so the generated artifacts require manual review.
- `phase3-final.json` has no dedicated server-version field. Record the server-reported version separately during each live run.
- Static inspection does not verify Zeno's designation as official BDS. Confirm that designation, endpoint reachability, and authentication on the first Zeno live run.
- `FastTransferWitness` remains LBSG-only, requires at least 600 seconds and the compiled vanilla asset carrier, and requires the operator to run `/transfer sm3`.

Example from a clean committed branch:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/Phase3Launcher.ps1 -Target Lunar -DurationSeconds 300 -Scenario CandidatePhysics -AuthCache .local/auth/microsoft-token.json
```

Run the network-silence witness separately; it cannot borrow authorized Physics frames from the candidate run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/Phase3Launcher.ps1 -Target Lunar -DurationSeconds 300 -Scenario FreeCameraSilence -AuthCache .local/auth/microsoft-token.json
```

During each `CandidatePhysics` run, produce every required marker-backed witness declared by `scenario-manifest.json`: keyboard/mouse and gamepad input; the exact First/Back/Front/First perspective wrap; replay and snap corrections; a grounded flat walk of at least `0.25` magnitude; a grounded diagonal walk with at least `0.25` on each axis; one non-repeated grounded single-jump start distinct from the held-space landing/re-jump; release before landing; camera obstruction and fallback; and both hidden and visible local-avatar states. By owner decision Touch is an explicitly named `Deferred` witness that does not gate the scenario verdict; the final aggregate records its disposition, observed state, and deferral reason, and this does not close touch parity. Walk witnesses exclude held/start/repeated/released jump frames, and the aggregate rejects missing or all-zero movement evidence. The validator rejects a run if any required marker witness is absent; it does not infer one from elapsed time.

The same manifest and final aggregate also carry the complete required controlled matrix: sprint, sneak/ledge, slabs/stairs, ladder, water and lava, cobweb/slime/bed/soul-sand/honey/bubble-column surfaces, knockback, teleport, dimension change, focus loss, controller disconnect, 30/60/144 frame caps, targeting-ray invariance, the exact movement/jump minima above, and three separate third-person camera outcomes: `camera_wall_outcome=WallBlocked`, `camera_corner_outcome=CornerBlocked`, and `camera_ceiling_outcome=CeilingBlocked`. The validator rejects any manifest that disables, omits, or weakens one; `FreeCameraSilence` requires zero movement/jump minima and `NotRequired` for all three camera outcomes.

The wall, corner, and ceiling outcomes are deliberately not aliases for the generic `camera_blocked` or `camera_fallback` marker bits. The marker harness cannot automatically identify which world geometry caused those bits, so the three named fields preserve distinct controlled live scenarios without claiming fixture-derived proof. These fields are named `required_controlled_matrix`: they record what the controlled live run must execute, not an automatic claim that a fixture or elapsed session performed it. Scenario-specific completion still requires reviewed live artifacts for each geometry and the native comparison listed below.

`scripts/acceptance/Phase3.ps1` validates the bounded marker stream and writes `phase3-final.json`. The deterministic aggregate contains:

- build/target/endpoint/run/process/core/app and PREG/BREG identities;
- selected scenario, candidate and production-default flags;
- input modes, physics tick range/count, correction/replay/snap counts, maximum correction magnitude, outbox high-water/drops, free-camera packet count, marker-backed flat/diagonal walk counts, the distinct non-repeated single-jump count, held landing/re-jump, and release-before-landing results;
- exact perspective-wrap sequence, camera blocked/fallback counts, and avatar visible/hidden counts;
- the full `required_controlled_matrix` execution contract without treating it as an observed-result substitute;
- frame timing, resource counters, timeout state, and process exits.

Validation rejects dirty or mismatched identity, stale hashes, wrong endpoint/process/run/scenario, duplicate/retrograde/gapped ticks except an exactly correlated dimension reanchor, non-finite or oversized values, queue drops, free-camera packets, violation markers, missing scenario witnesses, terminal-count mismatches, any Candidate terminal state other than zero-depth `Drained` (including typed `SocketPending`), authority faults, timeouts, and nonzero app exits.

## Current deterministic record

Focused Rust tests green on 2026-07-18:

```text
phase3_evidence_emits_one_frame_for_every_completed_catch_up_tick: 1 passed
phase3_correction_evidence_records_only_bounded_successful_replay_and_snap_outcomes: 1 passed
phase3_candidate_identity_rejects_dirty_or_unattributed_builds: 1 passed
stale_explicit_snap_preserves_monotonic_ticker_and_physics_alignment: 1 passed
multi_tick_catch_up_exposes_every_completed_tick_to_evidence_before_send: 1 passed
phase3_terminal_binds_candidate_and_free_camera_packet_silence: 1 passed
bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full: 1 passed
acceptance_terminal_runs_after_the_authoritative_network_send_stage: 1 passed
```

The affected broad suites are green with `cargo test --locked -p sim -p semantic-input -p bedrock-client`. The scoped strict lint gate is also green: `cargo clippy --locked -p sim -p semantic-input -p bedrock-client --all-targets --all-features -- -D warnings`. The acceptance shutdown system uses a bounded Bevy `SystemParam`; no lint suppression was added.

Fixture-only PowerShell validation is green:

```text
scripts/tests/acceptance/Phase3.Tests.ps1: 93 passed, 0 failed
```

These tests cover authenticated target launcher flags and minimum durations, the stable debug path, new-or-empty run directories, fresh core-attributed endpoint publication, equal-pose catch-up, replay/snap aggregation, the full required-matrix schema, exact perspective wrap, authoritative terminal queue reconciliation, an independent zero-frame FreeCamera terminal, exact schema/correlation, dirty and mismatched run/process/core rejection, and isolated fail-closed negative gates. The negatives independently reject missing flat/diagonal movement, all-zero movement, a missing distinct single jump, weakened movement/jump minima, and weakened wall/corner/ceiling outcomes. They validate the harness contract and synthetic marker fixtures only; they do not claim that a live controlled matrix, geometry-specific camera collision scenario, or native comparison has run. Debug-client timing in a generated aggregate is diagnostic evidence, not a release-performance acceptance claim.

## Gates still required before closure

- Broad Rust, Clippy, format, architecture, and independent-review gates on the integrated branch.
- Controlled local BDS movement/correction/single-jump/held-jump/perspective matrix, with separate wall, corner, and ceiling third-person camera-collision artifacts.
- Separate five-minute authenticated `CandidatePhysics` and `FreeCameraSilence` runs on Lunar, Zeqa, LBSG, and Zeno.
- Native Bedrock comparison for movement cadence, special surfaces, camera collision, local-avatar visibility, and controller equivalence. Touch parity remains open but is owner-deprioritized and does not gate Phase 3 acceptance.
- Review of each generated `phase3-final.json` before the separate production Physics enable change.

Until those gates are attached and reviewed, `P3-MOVEMENT`, `P3.4-INPUT-CAMERA`, and Phase 3 remain open.
