# Actor burst preparation

A full remote-player draw set plus the local hand exceeded the old normalized-skin
cache. Repeated expansion and later downsampling made unchanged crowds expensive.
Square skins now retain their native pixels; legacy expansion accounts for both
retained source keys and results.

Cold custom models are prepared by one worker batch at a time. Pointer admission
uses [the shared per-pass limit](../../crates/client-world/src/actor_animation/skin/queue.rs);
hashing, parsing, rest poses, animated layers and meshes run off-thread. Existing
actors retain their last complete profile, including body pixels and cape, until
the replacement model is ready for an animation tick. New appearances remain gated.
Session-owned completion channels prevent obsolete work from becoming visible.
Constructed meshes retain their validation and vertex fingerprints. Both catalogs
reuse that work, and exact page sharing transfers the witness to canonical storage.

## Deterministic evidence

The focused tests failed on the original implementation and passed after the fixes.
The table records inspected candidates, not elapsed time; the after column is the
passing upper bound asserted by each regression.

| Regression fixture | Before | Passing ceiling |
| --- | ---: | ---: |
| Append 256 geometry pages beside 128 retained pages | 65,408 | 1,536 |
| Replace 32 resident skins | 1,024 | 128 |
| Repack 128 skins | 8,128 | 512 |
| Register and reuse 128 skin models | 16,384 | 512 |
| Register and locate 128 animated-layer images | 16,384 | 512 |

Additional passing contracts:

- [Native raster tests](../../crates/render-model/src/actor/skin/native.rs) preserve
  texels across supported sizes, retain legacy working sets, and reject malformed
  lengths. Render tests verify warm allocation-free assignments and hand uploads.
- [Worker queue tests](../../crates/client-world/src/actor_animation/skin/queue_tests.rs)
  verify bounded non-inline admission, shared mesh accounting, replacement capacity,
  exact-content reuse and stale-owner rejection. Profile tests retain the old complete
  appearance and prevent zero-tick publication of new pixels with an old model.
- [Artwork tests](../../crates/render/src/actor_render/artwork_reuse_tests.rs) verify
  texture/glint reuse across repeated publication. Equipment/entity pixels and meshes
  are prepared by the existing pack worker; catalog content interning retains a
  worker-owned vertex allocation.
- [Prepared geometry tests](../../crates/render-model/src/actor/rig/preparation_tests.rs)
  reduce repeated hand/body validation from 72 inspected vertices to zero.
  [Catalog tests](../../crates/render/src/actor/rig/catalog/tests.rs) reduce crowd
  admission hashing from 786,432 vertex bytes to zero. Public mutation still forces
  validation, and canonical sharing releases duplicate source allocations.
- Existing Java body/cape/persona, native hand readiness, pack publication and steady
  actor allocation tests pass. One existing carrier-dependent animation test and one
  existing catalog benchmark remain ignored.

The touched-crate check with isolated worktree artifacts passes. The application
rerun passes 18 animation, 12 presentation, 3 pack-publication and 1 allocation
tests; the existing carrier-dependent animation test remains ignored. The capture
client build also passes with developer control and Tracy enabled. The focused
geometry follow-up passes 3 model, 14 catalog and 23 rig tests, its touched-crate
check, and a fresh client build. Formatting checks pass for all touched Rust files.

## Runtime comparison

Matched local fixtures ran headlessly on an M3 Pro at 1920 × 1080, radius 12,
using dev opt-level 1 with dependencies at 3 and Tracy enabled. Baseline and final
clients used isolated build artifacts; the baseline has attribution spans only.
Each scenario has one before and one after capture on the shared host. The warm
window begins one second after the burst command and ends at its clear command;
cold maxima cover that first second. These are CPU publication timings, not FPS.

All 128 actors remain rendered in each burst. Warm `prepare_actor_render_frame`
durations are milliseconds, shown as median / p99 / maximum:

| Burst | Before | After | Cold maximum before → after |
| --- | ---: | ---: | ---: |
| Shared skin and model | 179.185 / 183.883 / 184.073 | 0.442 / 0.644 / 1.301 | 174.239 → 0.544 |
| Distinct skin pixels | 182.125 / 255.201 / 286.857 | 0.463 / 0.833 / 1.250 | 184.783 → 0.623 |
| Distinct models | 182.065 / 185.179 / 186.839 | 0.486 / 0.569 / 0.836 | 183.053 → 2.401 |
| Complex models | 182.152 / 186.663 / 188.157 | 0.859 / 0.975 / 1.166 | 180.971 → 1.590 |

Complex-model cold actor advancement falls from 16.435 to 2.178 ms. Its 128 sources
run in 16 worker batches, at most 8 per batch: worker occupancy peaks at 6.205 ms,
while main completion handling peaks at 0.0053 ms. No final capture enters the
vertex-fingerprint fallback. Complex catalog append peaks at 0.311 ms and retained
validation at 0.000416 ms. A follow-up retaining prepared metadata reduced the
intermediate candidate's 22.628 ms cold publication maximum to the final 1.590 ms.

Cold artwork session geometry commit falls from 6.733 to 0.532 ms. Equipment and
entity preparation runs on the pack worker; elapsed worker duration does not
measure time blocking the frame thread. Repeated-publication texture reuse is
covered by deterministic tests, not a repeated-publication runtime scenario.
Paired shared, complex, artwork and first-person hand screenshots retain the
expected body/hand appearance and fixture layout, apart from water/cloud phase.

## Remaining limits

The complex capture has no frame-interval proxies above 16.7 ms, but three exceed
12.5 ms; its maximum is 14.628 ms. Those frames include pipeline creation at
3.595 ms and GPU actor preparation at 6.402 and 4.331 ms. The other captures still
have frame gaps:

| Full capture | Proxies above 16.7 ms | Maximum proxy |
| --- | ---: | ---: |
| Shared/distinct player scenarios | 51 | 80.558 ms |
| Cold artwork publication | 7 | 103.327 ms |
| First-person hand | 1 | 18.169 ms |

The artwork join still includes GPU transfer at 18.869 ms, pipeline creation at
48.995 ms and UI publication at 24.332 ms. The largest player gap contains only
0.035 ms of actor publication, with time between render/main systems; host
preemption is not proven. The hand gap contains 0.023 ms of actor publication and
0.011 ms of GPU actor preparation, and its cause remains unproven.

First GPU transfers, pipeline creation, UI publication, exact page comparisons and
first-use persona half-height expansion remain cold costs. A bounded admission
count is not a frame-time guarantee. Readiness-gated first appearance is still
provisional; these synthetic captures do not close whole-join, no-pop-in, native
presentation or release frame-budget gates in [the performance plan](../../plan.md).

Use the [original local-server fixtures](../../tools/localserver/actor-burst.md)
with the [headless MCP workflow](../agents/client-mcp.md). No remote server or owner
account is needed. Raw captures and assets do not belong in git.
