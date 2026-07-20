# Phase 4.4 actor authority and live interpolation tracker

Current audited progress: **69%** at integrated head `5316ca3` (production authority + correlation landed; live/native still open).

This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates.

Gate scores: contract 95%, runtime 90%, production presentation 75%, deterministic verification/review 85%, live/native/performance 0%; arithmetic mean 69%. No binding live/native/performance witness has passed.

## Landed

- [x] Bounded actor lifecycle/store, roster, skins, metadata, and foreign-player movement routing.
- [x] Exact actor packet-origin and network-offset normalization.
- [x] Oomph-style three-tick player convergence.
- [x] Separate adjacent-frame renderer interpolation.
- [x] Teleport/replacement snaps plus finite, frustum, distance, and capacity controls.
- [x] Deterministic ordinary-move, teleport, origin, and interpolation tests.
- [x] Retain and resolve `AddPlayer` game mode against the authoritative default (`2c7019c`).
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly (`2c7019c`).
- [x] Filter spectator and metadata-invisible actors before culling and the 128-instance cap (`2c7019c`).
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport (`2c7019c`).
- [x] Add bounded packet-to-store-to-presented-frame correlation (`5316ca3`).

## Remaining production authority

- [x] Retain and resolve `AddPlayer` game mode against the authoritative default.
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly.
- [x] Filter spectator and metadata-invisible actors before culling and the 128-instance cap.
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport.
- [x] Add bounded packet-to-store-to-presented-frame correlation.

## Live/native acceptance

- [ ] Authenticate to LBSG and capture spawn, ordinary movement, rotation, and teleport in one valid witness.
- [ ] Prove feet remain on the same ground plane without a 1.6-block jump.
- [ ] Compare matched native actor movement/interpolation.
- [ ] Pass release performance/resource gates and final independent review.

## Historical references

- `origin/backup/completion-phase4-20260719` contains the old authority/filter experiments.
- `origin/backup/completion-phase4-f9-integration-20260719`, `origin/backup/phase44-ground-contact-convergence-20260719`, and `origin/phase44-presented-ground-contact` contain witness history.

These references must be selectively reimplemented and reviewed on fresh `main`; they must not be bulk merged or rebased.
