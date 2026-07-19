# Semantic keyboard-tap preservation handoff

Branch: `fix/semantic-tap-preservation`

Base: `completion-performance-profile` at `924a655`, which already contains
`origin/phase2-textures` `d026417`.

Status: implementation complete and independently approved; commit/push pending
at the time this note was written.

## Behavior

The physical keyboard sampler now unions currently held keys with keys pressed
since the previous frame, then sorts and deduplicates HID usages. A complete
press/release between render samples therefore produces one semantic input frame
instead of being lost. Held behavior is unchanged and the next transient-clear
boundary releases the pulse without repeating it. Mouse and modifier behavior is
unchanged in this bounded fix.

This was discovered when native automation's complete F5 tap was invisible to the
old `get_pressed()`-only sampler, preventing reproducible third-person actor
acceptance. The same fix protects genuine short keyboard taps during low frame
rate or event batching.

## Evidence

- Red: `captured_f5_tap_between_frames_still_cycles_perspective_once` remained
  in FirstPerson before the implementation.
- Green: focused regression passed after the implementation.
- `cargo test --locked -p bedrock-client --lib`: 336/336 passed.
- `cargo clippy --locked -p bedrock-client --lib -- -D warnings`: passed.
- Independent review initially found the focused harness did not model the
  transient-clear boundary. The test now clears `ButtonInput` at that exact
  boundary; fresh re-review returned APPROVE with no findings.

After checkout on another machine, rerun the focused test and a native F5
first/rear/front/first cycle before composing this commit into the integration
branch.
