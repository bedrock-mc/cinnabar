# Phase 5 forms, menus, settings, controls, and parity tracker

Current audited progress after the JSON UI scope expansion: **20%** at `main` commit `fe698f5`.

This estimate uses equal contract, runtime, production UI integration, deterministic-verification/review, and live/native/performance gates.

Gate scores: contract 50%, runtime 25%, production UI integration 5%, deterministic verification/review 20%, live/native/performance 0%; arithmetic mean 20%.

## Landed foundations

- [x] Bounded ModalFormRequest packet normalization.
- [x] Typed user settings for FOV, perspective, controls, and selected render behavior.
- [x] Runtime settings replacement authority and semantic control bindings.
- [x] Keyboard, mouse, controller, and touch action foundations.
- [x] Deterministic tests for several settings, camera, focus, and input transitions.

## Remaining forms

- [ ] Parse, retain, and present modal, menu, and custom JSON forms.
- [ ] Validate fields, bounds, cancellation, replacement, and session lifetime.
- [ ] Route exact responses and support keyboard, mouse, controller, and touch navigation.
- [ ] Validate Lunar ClickUI compatibility without Lunar-specific client behavior.

## Resource-pack JSON UI

- [ ] Load the active Bedrock resource-pack stack's `ui/*.json`, `_ui_defs.json`, `_global_variables.json`, textures, glyphs, and version metadata with deterministic precedence and bounded caches.
- [ ] Parse the JSON-with-comments/trailing-commas dialect used by Bedrock UI documents.
- [ ] Implement namespaces, control/template inheritance, variables, bindings, collections, factories, grids, anchors, sizing expressions, layers, focus/navigation, toggles/radio groups, and lifecycle-safe local state.
- [ ] Implement vanilla `modifications` patch operations so packs can extend or replace HUD, inventory, chat, pause, and form controls without hard-coded per-pack UI code.
- [ ] Implement UI animations plus textures, nine-slice, flipbooks, fonts, formatting, and sound hooks required by reviewed packs.
- [ ] Apply server resource-pack updates transactionally across session/transfer boundaries; reject malformed or over-budget UI without corrupting the currently active UI.
- [ ] Render server simple/modal/custom forms and resource-pack-customized JSON UI forms with exact response index/type identity.
- [ ] Add version-pinned catalog/compatibility tests and live third-party resource-pack witnesses.

[`schphe/jui`](https://github.com/schphe/jui) is a useful implementation and tooling reference: it models TSX-to-Bedrock-JSON-UI compilation, screen patches, HUD overlays, server-form routing, custom-form skins, bindings, collections, animations, a vanilla-pack-backed preview, and HUD/inventory/form goldens. Treat its vanilla-accuracy claims as hypotheses requiring Cinnabar's own native validation. Its workspace declares Apache-2.0, but pin and complete a license/provenance audit before copying or vendoring any code; architectural ideas may be independently reimplemented.

## Remaining menus and settings

- [ ] Implement in-game pause, controls, video, audio, and accessibility screens.
- [ ] Expose configurable FOV, perspective, sensitivity, GUI scale, graphics, and bindings through strict vanilla Bedrock UI.
- [ ] Persist settings safely and apply/reset them across startup, focus, session, and device changes.
- [ ] Complete held-input behavior, navigation, rebinding conflicts, and device-disconnect recovery.

## Parity and performance

- [ ] Compare supported resolutions, DPI/scales, aspect ratios, focus states, and input devices against matching vanilla Bedrock.
- [ ] Verify menus/forms do not leak gameplay input.
- [ ] Prove bounded retained memory and stable frame time with forms, menus, HUD, chat, scoreboards, and inventory active together.
- [ ] Complete independent review and cross-platform live acceptance.

## Historical references

Existing Phase 5 backup refs contain UI scaffolds and experiments, but no stale branch represents a complete forms/settings implementation. Build remaining behavior from current `main` and consult backups only for bounded tests or contracts.

Java Edition visual behavior may inform the separately approved Java-style gameplay HUD through clean-room observation. Decompiled proprietary Java source must not be copied, translated, vendored, or treated as source code for this JSON UI/runtime implementation.
