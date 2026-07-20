# Phase 4.4 actor authority and live interpolation tracker

Current audited progress: **66%** at implementation head `5492459` (authority primitives and the acceptance-only correlation pipeline are landed; production renderer correction, independent re-review, and live/native gates remain open).

This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates.

Gate scores: contract 95%, runtime 90%, production presentation 65%, deterministic verification/review 80%, live/native/performance 0%; arithmetic mean 66%. No binding live/native/performance witness has passed, and the material correlation fix still requires independent re-review after integration with the production renderer correction.

## Landed

- [x] Bounded actor lifecycle/store, roster, skins, metadata, and foreign-player movement routing.
- [x] Exact actor packet-origin and network-offset normalization.
- [x] Oomph-style three-tick player convergence.
- [x] Separate adjacent-frame renderer interpolation.
- [x] Teleport/replacement snaps plus finite, frustum, distance, and capacity controls.
- [x] Deterministic ordinary-move, teleport, origin, and interpolation tests.
- [x] Retain and resolve `AddPlayer` game mode against the authoritative default (`29b47bb`).
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly (`29b47bb`).
- [ ] Apply spectator and metadata-invisible filtering in the production rig renderer before culling and the 128-instance cap. The store predicate is landed, but the PR-head renderer hookup still needs correction.
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport (`29b47bb`).
- [x] Add bounded packet-to-store-to-presented-frame correlation (`5492459`): capture is disabled during normal gameplay; timed acceptance runs correlate an exact committed identity across two consecutive GPU-presented frames, reset at session/dimension boundaries, record rejected/drop counts, and emit at most 64 parsed witnesses per session.

## Remaining production authority

- [x] Retain and resolve `AddPlayer` game mode against the authoritative default.
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly.
- [ ] Correct production renderer eligibility filtering before culling/capacity; the predicate and deterministic store coverage are present, but the production rig path is not yet fixed at `5492459`.
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport.
- [x] Add bounded packet-to-store-to-presented-frame correlation.

## Deterministic verification at `5492459`

- Actor witness, movement-authority, and Windows make-target regressions pass.
- Strict `client-world` and `bedrock-client` Clippy with warnings denied passes.
- Architecture enforcement and the full acceptance PowerShell dry-run suite pass.
- The prior `actor_store/tests.rs` and `asset_startup.rs` file-size violations are split without policy exemptions.

## Live/native acceptance

- [ ] Authenticate to LBSG and capture spawn, ordinary movement, rotation, and teleport in one valid witness.
- [ ] Prove feet remain on the same ground plane without a 1.6-block jump.
- [ ] Compare matched native actor movement/interpolation.
- [ ] Pass release performance/resource gates and final independent review.

## Historical references

- `origin/backup/completion-phase4-20260719` contains the old authority/filter experiments.
- `origin/backup/completion-phase4-f9-integration-20260719`, `origin/backup/phase44-ground-contact-convergence-20260719`, and `origin/phase44-presented-ground-contact` contain witness history.

These references must be selectively reimplemented and reviewed on fresh `main`; they must not be bulk merged or rebased.
