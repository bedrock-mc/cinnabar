# Repository agent instructions

## Vanilla compliance

- Player-visible gameplay, UI, animation, controls, physics, and rendering must match unmodified vanilla Bedrock behavior and presentation for the pinned protocol/resource-pack version.
- Do not substitute debug text, raw protocol/JSON, placeholder geometry, duplicate HUD elements, numeric stand-ins, or invented behavior for a required vanilla surface.
- The only approved presentation exception is the compact Java-style bottom-left chat layout. Its message semantics, formatting, focus, input routing, bounds, and server-authored content must still remain correct.
- Server-specific clients or modules may be used only as diagnostic references. Do not copy or depend on their modifications; implement vanilla behavior independently.

## Player-visible validation gate

- Do not call player-visible work ready, complete, approved, or pushable from unit tests alone.
- Before pushing a player-visible increment, build the exact release candidate, run it through the production client/core path, capture the affected surface in a native window, and inspect the rendered result against a matching vanilla Bedrock reference.
- Exercise the real interaction that exposed the defect when one is known. Protocol/UI text changes must prove that human-readable text is rendered and raw JSON or protocol envelopes are not visible.
- Record visible defects as open failures. Do not approve or push an increment with a known relevant visual, behavioral, crash, warning, or strict-check failure.
- Review the final diff for unused imports and run formatting, affected tests, strict Clippy with warnings denied, architecture checks, and `git diff --check` before push.

## Integration discipline

- Preserve unrelated user changes and integrate only reviewed commits onto the current remote branch head.
- Re-review material fixes made in response to review findings.
- Push independently useful increments as soon as all applicable gates pass, but never mark a phase complete until every specified behavior and live/native acceptance gate for that phase passes.
