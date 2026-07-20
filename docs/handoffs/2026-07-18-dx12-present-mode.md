# DX12 presentation-policy handoff

Branch: `performance-dx12-present-mode`

Component base: `origin/phase2-textures` at `d026417`

Canonical integration: `origin/phase2-textures` at `2fc7a33`

Status: component implementation `5e01528` is pushed to
`performance-dx12-present-mode`, independently approved, merged through the
combined candidate, and shipped on canonical `phase2-textures`. Repairs through
`2fc7a33` preserve the cross-platform policy resource and schedule ordering
while compiling the surface probe only on Windows. Canonical CI run
`29671070071` is green overall: Linux and Windows passed the full workspace,
strict Clippy, acceptance, and Go gates, while macOS passed physics bootstrap
and carrier verification. Native acceptance remains pending.

## Evidence and intended policy

On Radeon RX 570, driver `31.0.21924.61`, the same release client and live scene
measured roughly 6-8 FPS with DX12 FIFO and roughly 190-250 FPS with DX12
Immediate on Lunar/LBSG/Zeqa. GPU work was not saturated. Vulkan surface
capability discovery failed on this system, so Vulkan is not a fallback.

The implementation is deliberately exact-match only:

- backend must be DX12;
- adapter must be `Radeon RX 570 Series`;
- driver must be `31.0.21924.61`;
- Immediate must be reported as supported;
- explicit vsync/no-vsync and acceptance/metrics intent must win;
- runtime video-setting changes replace Auto rather than being silently
  overridden.

The render world caches the capability decision and publishes a shared remedy.
The main world applies that remedy to the authoritative `Window` once, so the
next extraction remains Immediate and does not reconfigure the surface every
frame. A pending runtime settings generation is consumed only after a unique
primary window exists and the setting is applied. The render metadata probe is
ordered after policy resolution; main-world adoption is ordered before metrics
and title publication.

The exact Auto match first emits a deterministic structured `state=pending`
warning containing `preference=Auto`, startup/requested FIFO, recommended
Immediate, and the exact adapter and driver. It does not claim an effective
mode. Only a later extraction that observes Immediate on the same primary
window advances the pure lifecycle to `state=proven` and emits
`effective=Immediate`; that proof is one-shot. A different window, unchanged
FIFO extraction, or an explicit preference cannot prove the pending remedy.

A failed temporary surface probe clears the shared recommendation and uses a
capped 4/8/16/32/60-frame retry backoff. Retries continue indefinitely at the
60-frame cap, while a changed window/preference/request key retries immediately.
Attributable acceptance and metrics runs remain locked to FIFO unless
`--no-vsync` is explicitly supplied, so the automatic remedy cannot relabel
formal FIFO evidence. Explicit `--vsync` also locks FIFO.

## Local verification

- `cargo test --locked -p render --test present_mode_policy -- --nocapture`
- `cargo test --locked -p render present_mode -- --nocapture`
- `cargo test --locked -p render --test plugin -- contracts::graphics_runtime_metadata_waits_for_extracted_diagnostics_before_surface_probe --nocapture`
- `cargo test --locked -p bedrock-client present_mode -- --nocapture`
- `cargo test --locked -p bedrock-client args::tests -- --nocapture`
- `cargo test --locked -p render -p bedrock-client --all-targets --all-features`
- `cargo clippy --locked -p render -p bedrock-client --all-targets --all-features -- -D warnings`
- `cargo fmt --all -- --check`
- `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml`
- `git diff --check`

All listed local checks passed after the final-review fix. The present-mode
focused tests cover the exact driver/capability boundary, explicit preference
precedence, pending-to-proven lifecycle identity and one-shot proof, bounded
eventual surface-probe retry, runtime setting transitions, missing-window retry,
and two consecutive automatic applications with no second `Window` change.

## Remaining native gates

- Build the stable canonical path and repeat Lunar
  `pvp.lunarbedrock.com:19134`, then Zeqa
  `zeqa.net:19132`, confirming effective mode, frame pacing, chunk readiness,
  input latency, and no regression for explicit vsync.
- Validate normal Auto interactively. Use explicit `--no-vsync` only as the
  attributable performance control; formal FIFO evidence remains separate.
- Do not generalize the driver match without new measured evidence.
