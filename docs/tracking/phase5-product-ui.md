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

- [ ] Target the official Mojang `bedrock-samples` resource pack pinned by `assets/vanilla-source.json`: tag `v1.26.30.32-preview`, commit `020f1cf4b2baef78e635d4ce7498eb16a429dcbb`, and archive SHA-256 `12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`. A version upgrade requires a new pin and compatibility run.
- [ ] Load the active Bedrock resource-pack stack's `ui/*.json`, `_ui_defs.json`, `_global_variables.json`, textures, glyphs, and version metadata with deterministic precedence and bounded caches.
- [ ] Parse the JSON-with-comments/trailing-commas dialect used by Bedrock UI documents.
- [ ] Initial control subset: panel, stack panel, label, image, button, toggle, grid, and scroll view; anchors, offsets, sizes, layers, alpha/color, textures, nine-slice, fonts, and clipping; namespaces/templates/variables; view and collection bindings; factories and grid collections; focus/button mappings; and lifecycle-safe local state.
- [ ] Initial `modifications` subset: insert front/back/before/after, replace, remove, move front/back, and swap, sufficient for the acceptance HUD, inventory, chat, pause, and form patches without hard-coded per-pack UI code.
- [ ] Initial animation subset: offset, alpha, size, color, wait, flip-book, and UV animation. Unsupported control/property/expression/patch types must produce a bounded diagnostic, reject only the affected candidate UI layer, keep the current validated UI active, and never corrupt or partially apply the pack stack.
- [ ] Apply server resource-pack updates transactionally across session/transfer boundaries; reject malformed or over-budget UI without corrupting the currently active UI.
- [ ] Decode Bedrock network simple/modal/custom form schemas independently of JSON UI, preserve exact response index/type identity, and present those forms through the active resource-pack JSON UI templates where provided.
- [ ] Enforce initial implementation safety ceilings: 4 MiB per UI document, 64 MiB decoded UI data per active pack stack, 4,096 controls per document, 32,768 controls per active stack, 128 control-tree depth, 32 template/inheritance depth, 4,096-byte binding expressions, 256 modifications per control, and 4,096 collection rows. These are Cinnabar safety bounds, not claims about vanilla limits; adjust only with measured evidence and focused adversarial tests.
- [ ] Pass the acceptance corpus: the pinned official sample pack; an independently authored Cinnabar conformance pack covering HUD, inventory, pause, chat, and simple/modal/custom-form skins; and one authorized, hash-pinned, local-only third-party server pack. Keep Mojang and third-party assets out of Git. `jui` goldens may inform tests but are not authoritative parity evidence.

[`schphe/jui`](https://github.com/schphe/jui) is an evaluation and tooling reference: it is a build-time TSX-to-Bedrock-JSON-UI compiler with a separate preview/devtools application, and its README explicitly says no runtime ships to the game. It is **not** a drop-in Cinnabar in-game runtime. Bounded parser, lowering, patching, catalog, or preview components may be considered only after architectural fit and a pinned license/provenance audit; Cinnabar still requires its own production resource-pack loader, reactive UI runtime, renderer integration, input/focus bridge, protocol-form bridge, and lifecycle/resource limits. Treat its vanilla-accuracy claims as hypotheses requiring Cinnabar's own native validation. Its workspace declares Apache-2.0, but no repository-root license file was observed during this audit, so do not copy or vendor code until that ambiguity is resolved; architectural ideas may be independently reimplemented.

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
