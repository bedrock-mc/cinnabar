# Phase 4.4 actor authority and live interpolation tracker

Current audited progress: **53%** at `main` commit `fe698f5`.

This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates.

Gate scores: contract 80%, runtime 70%, production presentation 60%, deterministic verification/review 55%, live/native/performance 0%; arithmetic mean 53%. No binding live/native/performance witness has passed.

## Landed

- [x] Bounded actor lifecycle/store, roster, skins, metadata, and foreign-player movement routing.
- [x] Exact actor packet-origin and network-offset normalization.
- [x] Oomph-style three-tick player convergence.
- [x] Separate adjacent-frame renderer interpolation.
- [x] Teleport/replacement snaps plus finite, frustum, distance, and capacity controls.
- [x] Deterministic ordinary-move, teleport, origin, and interpolation tests.

## Remaining production authority

- [ ] Retain and resolve `AddPlayer` game mode against the authoritative default.
- [ ] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly.
- [ ] Filter spectator and metadata-invisible actors before culling and the 128-instance cap.
- [ ] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport.
- [ ] Add bounded packet-to-store-to-presented-frame correlation.

## Live/native acceptance

- [ ] Authenticate to LBSG and capture spawn, ordinary movement, rotation, and teleport in one valid witness.
- [ ] Prove feet remain on the same ground plane without a 1.6-block jump.
- [ ] Compare matched native actor movement/interpolation.
- [ ] Pass release performance/resource gates and final independent review.

## Historical references

- `origin/backup/completion-phase4-20260719` contains the old authority/filter experiments.
- `origin/backup/completion-phase4-f9-integration-20260719`, `origin/backup/phase44-ground-contact-convergence-20260719`, and `origin/phase44-presented-ground-contact` contain witness history.

These references must be selectively reimplemented and reviewed on fresh `main`; they must not be bulk merged or rebased.
