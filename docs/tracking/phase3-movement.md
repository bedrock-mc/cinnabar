# Phase 3 physics, movement, controls, and camera tracker

Current audited progress: **64%** at `main` commit `fe698f5`.

This estimate uses five equal gates: authoritative contract, bounded runtime/state, production integration, deterministic verification/review, and live/native/performance acceptance. It is not a phase-closure claim.

Gate scores: contract 90%, runtime/state 85%, production integration 55%, deterministic verification/review 90%, live/native/performance 0%; arithmetic mean 64%. The evidence harness is preparation and does not earn acceptance credit before a binding witness passes.

## Landed

- [x] Protocol-1001 `PlayerAuthInput` snapshots and bounded 20 Hz outbound FIFO.
- [x] Prediction/correction replay bound to collision-world identity.
- [x] BREG/PREG collision plumbing and terrain, fluid, and special-surface simulation.
- [x] Semantic keyboard, mouse, controller, and touch input foundation.
- [x] Held-Space repeat jumping and focus/session release barriers.
- [x] First-person, rear-third-person, and front-third-person camera modes.
- [x] Configurable FOV authority, third-person collision, targeting identity, and local-avatar/F5 carrier.
- [x] Candidate-physics evidence harness and substantial deterministic test coverage.

## Remaining features

- [ ] Execute and approve the binding local BDS physics matrix.
- [ ] Run >=5-minute CandidatePhysics and FreeCameraSilence sessions on Lunar `19134`, Zeqa `19132`, and LBSG `19132`.
- [ ] Compare movement, jumping, camera collision, F5 avatar, touch, and controller behavior against matching vanilla Bedrock.
- [ ] Produce release resource/timing evidence at 30, 60, and 144 FPS caps.
- [ ] Complete independent review of the final integrated candidate range.
- [ ] Land a separate reviewed change enabling normal-session physics.

## Important current behavior

Production deliberately starts with `PhysicsAuthorityGate::ProductionDisabled`; normal gameplay therefore remains FreeCamera unless the attributable candidate flag is used. Tests for candidate physics do not mean production physics is enabled.

## Historical references

- `origin/backup/completion-phase3-20260719` contains older patch-equivalent work already represented on `main`.
- `origin/backup/phase3-physics-foundation-20260719` is sparse-residency WIP requiring fresh audit, not a merge candidate.

## Closure gates

All movement/surface/correction/teleport/focus/controller cases, strict workspace tests and Clippy, architecture policy, independent review, native comparison, live server authority, and release performance evidence must pass before production enablement or Phase 3 closure.
