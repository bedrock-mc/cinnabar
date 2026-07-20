# Phase 5 forms, menus, settings, controls, and parity tracker

Current audited progress: **25%** at `main` commit `fe698f5`.

This estimate uses equal contract, runtime, production UI integration, deterministic-verification/review, and live/native/performance gates.

Gate scores: contract 50%, runtime 35%, production UI integration 15%, deterministic verification/review 25%, live/native/performance 0%; arithmetic mean 25%.

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
