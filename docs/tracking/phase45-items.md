# Phase 4.5 held items, actions, dropped items, and viewmodel tracker

Current audited progress: **37%** at `main` commit `fe698f5`.

This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates. Data/state foundations are real, but player-visible presentation is largely absent.

Gate scores: contract 80%, runtime 55%, production presentation 15%, deterministic verification/review 35%, live/native/performance 0%; arithmetic mean 37%. No binding live/native/performance witness has passed.

## Landed foundations

- [x] Canonical item identity, visual routes, display transforms, and compiler support.
- [x] Bounded ItemRegistry, MobEquipment, Animate, and AnimateEntity normalization.
- [x] AddPlayer held-stack canonicalization.
- [x] Remote equipment/action timelines with bounded fallback state.
- [x] Local-versus-remote equipment routing and selected-equipment retention.

## Remaining features

- [ ] Render remote held items with correct hand, transform, and equipment lifetime.
- [ ] Drive arm and item swing/use/recover/critical poses from retained action timelines.
- [ ] Add the item sprite and block-item GPU presentation path.
- [ ] Normalize and retain AddItemEntity-specific dropped-item lifecycle.
- [ ] Render bounded dropped-item bob/spin and removal/reset behavior.
- [ ] Implement the first-person arm and held-item viewmodel with independent depth/FOV.
- [ ] Enforce exact first/rear/front viewmodel visibility of 1/0/0.
- [ ] Reconcile presentation with the final Phase 5 selected-stack, inventory, and rollback authority.

## Validation gates

Packet/store/reset tests, presented-frame equipment/action tests, handedness and depth tests, dropped-item capacity tests, BDS two-client and LBSG evidence, matching vanilla held/swing/use/drop/viewmodel scenes, release 128-held/4096-dropped ceilings, and independent review are required.

## Historical references

The broad Phase 4 backup branches overlap stale implementations. They are contract/test references only; no preserved Phase 4.5 branch is safer than implementing the missing presentation from current `main`.
