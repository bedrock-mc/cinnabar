# DX12 presentation-policy handoff

Branch: `performance-dx12-present-mode`

Base: `origin/phase2-textures` at `d026417`

Status: incomplete work-in-progress checkpoint; policy tests are green, final app
integration verification is not complete.

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

The render decision is cached and propagated through an atomic preference. Four
focused policy tests passed: exact match, mismatch fallback, explicit preference,
and shared update behavior.

## Remaining work

- Finish/repair app integration compilation and tests.
- Run strict Clippy, formatting, architecture, and independent review.
- Build release and repeat Lunar `pvp.lunarbedrock.com:19134`, then Zeqa
  `zeqa.net:19132`, confirming effective mode, frame pacing, chunk readiness,
  input latency, and no regression for explicit vsync.
- Do not generalize the driver match without new measured evidence.

