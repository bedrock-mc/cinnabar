# Phase 3 physics, movement, controls, and camera tracker

Current audited progress: **67%** at `3a06b15` on PR #6.

This estimate uses five equal gates: authoritative contract, bounded runtime/state, production integration, deterministic verification/review, and live/native/performance acceptance. It is not a phase-closure claim.

Gate scores: contract 95%, runtime/state 90%, production integration 55%, deterministic verification/review 95%, live/native/performance 0%; arithmetic mean 67%. Production integration is unchanged rather than raised: this tranche closed simulator and binding defects but also established two previously unrecorded touch/controller reachability gaps. The evidence harness is preparation and does not earn acceptance credit before a binding witness passes.

## Landed

- [x] Protocol-1001 `PlayerAuthInput` snapshots and bounded 20 Hz outbound FIFO.
- [x] Prediction/correction replay bound to collision-world identity.
- [x] BREG/PREG collision plumbing and terrain, fluid, and special-surface simulation.
- [x] Semantic keyboard, mouse, controller, and touch input foundation.
- [x] Held-Space repeat jumping and focus/session release barriers.
- [x] First-person, rear-third-person, and front-third-person camera modes.
- [x] Configurable FOV authority, third-person collision, targeting identity, and local-avatar/F5 carrier.
- [x] Candidate-physics evidence harness and substantial deterministic test coverage.
- [x] Climb, slime, bed, and soul-sand strata promoted from an unsupported ledger to
      pinned-bedsim conformance, with the four simulator parity defects they exposed fixed.
- [x] Every default keyboard, mouse, and gamepad binding proven reachable from the app's
      physical translation layer. Touch bindings are explicitly **not** covered: a binding can
      name a valid hit ID that no on-screen region ever assigns, which that guard cannot see.
- [x] Production soul-sand movement corrected from the Java `0.4` to the pinned bedsim Bedrock
      `0.543`, with the physics carrier regenerated and its pinned SHA-256 updated.
- [x] CI runs `tools/bedsimtrace` regeneration and provenance tests as an isolated `GOWORK=off`
      module, enforced by a workflow-contract test.
- [x] The complete `225b17e..3a06b15` candidate range received an independent Claude Opus
      4.8/xhigh `APPROVE` verdict after all three Important findings were fixed; no Critical or
      Important findings remain.

## Bedsim conformance ledger

The generator now supplies bedsim's `BlockSemanticsProvider` (name, friction, climbable) from
each scenario's PREG facts, which is the "authoritative PREG-to-bedsim environment query" the
previous ledger recorded as missing. Observed scripts rose from 8 to 18 and observed steps from
16 to 36; explicitly unsupported scripts fell from 19 to 12.

Still unsupported, each because `bedsim v0.1.3` genuinely lacks the oracle rather than because
the generator is incomplete:

- Fluids (`water_enter`, `water_swim`, `water_exit`, `lava`): bedsim has no fluid physics and
  marks any liquid intersection as an unreliable simulation.
- `bubble_up` / `bubble_down`: no bubble-column stratum.
- `scaffolding`, `honey`: no corresponding stratum.
- `cobweb`: bedsim senses cobweb through `BlockCollisions` geometry, which a passable scenario
  block cannot supply without also blocking movement.
- `slab_step` / `stair_step`: bedsim loses grounded state after the deliberate Phase 3 step
  correction.
- `unloaded_boundary`: the Rust fail-closed error contract is not a bedsim `TickResult`.

Because both sides model a scenario world as homogeneous, these fixtures witness each stratum's
force law, not the block-sampling extent. bedsim samples climbability at the feet block while
`crates/sim` unions flags across the swept volume; that difference is unwitnessed here. bedsim
also re-resolves the supporting block after collision before applying `walkOnBlock`, while
`crates/sim` retains the start-of-tick sampled surface response. Homogeneous scenario worlds
cannot witness that ordering difference either, so it remains part of native/BDS acceptance.

### Movement constants that remain unproven

- **Honey** keeps `horizontal_speed_factor = 0.4`, a Java Edition value, because bedsim v0.1.3
  implements no honey stratum and no Bedrock oracle exists to correct it against. It is named
  `unprovenHoneySpeedQ1E8` in `tools/registrygen/physics.go` and is deliberately not aliased
  onto soul sand's corrected value. Replace it once an authoritative Bedrock reference is
  identified.
- **Soul sand** previously carried the same Java `0.4`. It is now `0.543` from bedsim's
  `simulation.go`, applied at the identical point in the force law. This changed the compiled
  carrier, so `crates/assets/data/block-physics-v1001.sha256` moved from `1e903b92…` to
  `fab9ea3d…`. The pre-change pipeline was first shown to reproduce the old hash exactly, so the
  new hash reflects only this constant.

### Correction and replay collision semantics

Retained axis collisions now gate a discrete climb branch, so a correction cannot treat them as
ordinary interpolable state. They describe the motion that produced a position and cannot be
recomputed from an anchor. The rule is:

- A **hard reanchor** (StartGame, session/dimension replacement, teleport, un-replayable
  correction) clears them along with all other history.
- A **replayed correction** retains them only when the server echoes back the exact `f32`
  network position this client sent for that tick, which confirms the motion behind it. Any
  other correction clears them. The comparison is exact and in sent-network space, because that
  is the only representation the server actually acknowledged.
- Either way the loss is bounded to the first replayed tick; `Simulator::tick` re-derives
  collisions for every tick after it.

## Remaining features

- [ ] Execute and approve the binding local BDS physics matrix.
- [ ] Run >=5-minute CandidatePhysics and FreeCameraSilence sessions on Lunar `19134`, Zeqa `19132`, and LBSG `19132`.
- [ ] Compare movement, jumping, camera collision, F5 avatar, touch, and controller behavior against matching vanilla Bedrock.
- [ ] Produce release resource/timing evidence at 30, 60, and 144 FPS caps.
- [x] Complete independent review of the final integrated candidate range through `3a06b15`.
- [ ] Land a separate reviewed change enabling normal-session physics.

## Open production-integration gaps established from code

These were established by reading the implementation, not inferred from the tracker, and are
deliberately left unfixed because each needs an authoritative vanilla reference this tranche
does not have:

- `app/src/ui_runtime/gameplay_touch.rs` assigns only movement, `JUMP`, `USE`, and the four look
  axes. `SNEAK`, `SPRINT`, `ATTACK`, `PERSPECTIVE`, `MENU`, and all nine hotbar hit IDs have
  default bindings but no on-screen region, so they are unreachable by touch. Placing them
  requires a version-matched vanilla touch-layout reference and a rendered-frame acceptance pass.
- `app/src/semantic_controls/physical.rs` gates the whole device frame on
  `camera::input_is_active`, which requires a locked, hidden cursor. A touch-only or
  controller-only session never locks the pointer, so neither device can deliver input. The fix
  is bounded but changes the focus/release model, which should be decided against vanilla
  behaviour rather than chosen here.

## Important current behavior

Production deliberately starts with `PhysicsAuthorityGate::ProductionDisabled`; normal gameplay therefore remains FreeCamera unless the attributable candidate flag is used. Tests for candidate physics do not mean production physics is enabled. Nothing in this tranche changes that gate.

## Historical references

- `origin/backup/completion-phase3-20260719` contains older patch-equivalent work already represented on `main`.
- `origin/backup/phase3-physics-foundation-20260719` is sparse-residency WIP requiring fresh audit, not a merge candidate.

## Closure gates

All movement/surface/correction/teleport/focus/controller cases, strict workspace tests and Clippy, architecture policy, independent review, native comparison, live server authority, and release performance evidence must pass before production enablement or Phase 3 closure.
