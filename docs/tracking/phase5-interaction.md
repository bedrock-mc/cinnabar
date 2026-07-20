# Phase 5 combat, block interaction, hotbar, and inventory tracker

Current audited progress: **30%** at `main` commit `fe698f5`.

This estimate uses equal contract, runtime, production integration/presentation, deterministic-verification/review, and live/native/performance gates. Packet and state scaffolding does not count as end-to-end interaction.

Gate scores: contract 65%, runtime 45%, production integration/presentation 15%, deterministic verification/review 25%, live/native/performance 0%; arithmetic mean 30%.

## Landed foundations

- [x] Bounded inventory/item packet normalization and canonical item identities.
- [x] Inventory authority/router scaffolding with session/FIFO/reset tests.
- [x] Selected-slot and equipment state foundations.
- [x] Server block-crack event normalization, sequencing, and bounded retention.
- [x] Atomic camera targeting-ray and world/actor identity carriers.

## Remaining block interaction

- [ ] Send vanilla block-breaking transactions and reconcile server-authoritative crack progress.
- [ ] Render crack overlays and handle cancellation, tool/slot changes, corrections, and unloaded boundaries.
- [ ] Encode and reconcile block placement and item-use transactions.

## Remaining combat

- [ ] Implement reviewed vanilla actor bounding boxes and pose-dependent targeting.
- [ ] Ray-test nearest valid actor with solid-block occlusion and native game-mode reach.
- [ ] Encode exact protocol-1001 `UseItemOnEntityActionAttack` transactions.
- [ ] Present native miss/swing/hit behavior without client-authoritative damage or knockback.
- [ ] Cover stale/removed actors, backpressure, selected-item changes, and session replacement.

## Remaining inventory/UI

- [ ] Implement the canonical inventory journal, request/response reconciliation, and rollback.
- [ ] Complete survival/creative inventory, hotbar stack presentation, and selected-slot authority.
- [ ] Implement chest, furnace, crafting, and other container screens and interactions.

## Closure gates

Exact packet fixtures, deterministic ray/occlusion/reach tests, inventory rollback tests, local BDS and authenticated server witnesses, native behavior comparison, stable resource/performance evidence, and independent review are required. Lunar modules are neither used nor required.

## Historical references

The Phase 5 task8/task10/rawtext/native-HUD backups preserve partial experiments. None is a safe wholesale merge candidate; selectively port only reviewed contracts and tests onto this fresh branch.
