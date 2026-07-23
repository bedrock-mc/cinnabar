# Phase 4.3 rigs, animation, and mob rendering tracker

Current audited progress: **65%** at `main` commit `fe698f5`.

The master-plan prose understates the landed implementation. This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates.

Gate scores: contract 95%, runtime 85%, production presentation 65%, deterministic verification/review 80%, live/native/performance 0%; arithmetic mean 65%. No binding live/native/performance witness has passed.

## Landed

- [x] Deterministic entity, geometry, animation, controller, render-controller, texture, and item compilation.
- [x] Typed bounded Molang subset, controller state, query inputs, transitions, and reset budgets.
- [x] Runtime rig resolution and adjacent completed bone poses.
- [x] Shared skeletal GPU actor renderer and application rig publication.
- [x] Local F5-avatar preservation/alignment foundation.
- [x] Deterministic compiler, assets, client-world, render, and application tests.

## Remaining features

- [ ] Route non-player entity families through reviewed geometry/material/texture/render-controller presentation.
- [ ] Render mobs; current `player_route_and_skin()` explicitly selects `NoDraw` for non-player actors.
- [ ] Complete classic/legacy skins, outer layers, persona/custom geometry, name tags, and equipment integration.
- [ ] Prove vanilla limb, look, walk, action, and controller parity for players and mobs.
- [ ] Run hardware-pipeline, live multiplayer, bounded 128-actor, bone-arena, and texture-resource acceptance.
- [ ] Complete the final integrated independent review and native animated galleries.

## Historical references

- `origin/backup/phase43-render-integration-20260719` is superseded by newer `main` equivalents.
- `origin/backup/completion-phase4-canonical-20260719` contains stale presentation diagnostics and is reference-only.

## Closure gates

Compiler provenance and malformed-data negatives, controller/pose reset tests, actual hardware rendering, matched vanilla player/mob scenes, live presented-frame evidence, release resource ceilings, and fresh independent approval are required.
