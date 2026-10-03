# Offline frame-stutter investigation

The supplied Zeqa tail has sampled frame intervals of 7–9 ms and 25–28 ms on an
Apple M3 Pro, release build, Metal and FIFO. It contains no full frame trace or
packet replay. The measurements below use local release builds, the installed
vanilla carriers, and the four supplied server pack archives. No server or
account was used. These are CPU measurements, not a live Zeqa FPS acceptance.

## Unchanged player skins

`SkinLayerPack::pack` compared a vector of `Arc<[u8]>` skin layers to its previous
vector every frame. On the pinned Rust toolchain, equality of these unsized byte
slices scans their contents even when both Arcs share the same allocation. The
128-player warm-cache benchmark compares 134,217,728 bytes per frame despite
reusing the packed output and doing only one vector allocation. The initial
release measurement was 5.732 ms median and 25.040 ms p99. An independent
optimized check of 64 shared layers measured 2,038.053 us per comparison, versus
0.025 us with an explicit allocation-identity check.

Three adjacent, interleaved pre/post release runs isolate the cache under the
same current local build load. Each packs 128 shared layers (128 MiB) once, then
measures 200 unchanged calls:

| Run | Before median / p99 (ms) | After median / p99 (ms) |
| --- | --- | --- |
| 1 | 2.810 / 7.835 | 0.012 / 0.013 |
| 2 | 2.656 / 6.428 | 0.008 / 0.011 |
| 3 | 2.624 / 3.076 | 0.011 / 0.016 |

The unchanged path is 234–332 times faster by median, with one vector allocation
in both versions and the same packed output allocation. Cold copying still
costs about 19–23 ms and is unchanged; it is required when pixels actually change.
The initial 25 ms p99 varies with machine load, but the redundant work and its
removal reproduce independently of that tail.

The fix checks `Arc::ptr_eq` before byte equality for skin packing and skin-family
selection. Different allocations still use exact byte equality, so deduplication,
texture revisions and output pixels retain their previous behavior. Equal
replacement layers also become the cache's current source allocations, avoiding
repeated comparisons against obsolete allocations. Existing regressions cover
equal copies, pixel changes, order and removal; a new ownership regression covers
equal replacements releasing the old source.

Adjacent pre/post runs of the production animation, preparation and publication
path also improve. Each uses 600 frames after 60 warm-up frames, the installed
vanilla rigs and the supplied Zeqa archive. Thread CPU separates measured work
from scheduling delays caused by other local builds:

| Player fixture | Before CPU median / p95 (ms) | After CPU median / p95 (ms) | Rig/publication wall median before → after (ms) |
| --- | --- | --- | --- |
| 64 idle | 1.460 / 2.071 | 0.075 / 0.611 | 1.389 → 0.034 |
| 128 moving | 3.012 / 4.096 | 0.173 / 1.217 | 1.503 → 0.070 |

Animation p95 is unchanged within noise (0.475 → 0.487 ms and
0.978 → 0.973 ms). The evaluated operation counts stay at 4,836,352 and
10,093,184, with no invalid geometry, frozen actors or budget exhaustion. Both
runs assert 64 / 128 body instances are published. The optimization removes skin scans;
it does not reduce animation work or drop actors.

## Other measured candidates

| Offline workload | Release result |
| --- | --- |
| Full HUD publication, two Zeqa UI overlays, installed font, 500 idle frames at 2560×1440 / DPI 2 | median 0.114 ms, p99 0.123 ms; one bind/layout pass |
| Same HUD with an action-bar change every frame | median 0.795 ms, p99 1.286 ms; 501 bind/layout passes |
| Forced JSON-UI bind/layout, first UI overlay, 500 frames | median 2.036 ms, p99 2.912 ms |
| Forced JSON-UI bind/layout, second UI overlay, 500 frames | median 1.967 ms, p99 4.101 ms |
| Settled mixed roof/emitter/transparency fixture, 216 subchunks, 2,000 production polls | median 167 ns, p99 250 ns, max 750 ns; no light or mesh jobs/publications |
| Fast-frame recorder, 32 stage timers and frame boundaries, 10,000 frames | disabled median 0.041 us / p99 0.042 us; always-on median 1.417 us / p99 1.542 us |

The settled lighting fixture preserves its exact block, sky and direct-sky light
hash, `8406a83a02b26fbd`. It does not exhibit autonomous light/mesh churn. No
lighting behavior was changed. These fixed fixtures do not cover the server's
actual NPC population, ongoing packet workload, chunk geometry or GPU cost.

## Corrected telemetry

The tail repeats visibility generation 1219 with 764 resident meshes / 256
submitted meshes. Contemporary world-ready records instead show 8,376 resident
subchunks / 971 rendered subchunks and active decode work. Normal play stops the
startup visibility probe, but telemetry kept printing its retained snapshot.
The fix excludes that inactive witness, keeps current stream/upload counters,
and logs null visibility identities with `visibility_snapshot_valid=false`.
Active acceptance snapshots retain their existing schema.

Across the 15 supplied publication samples, accepted light jobs rise by 192,
value-changing jobs by 129 and uploads by 2,982,316 bytes. The 409.7465 ms light
worker maximum never changes: it is historical, not a worker measured during
these frames. The final 17.179 / 24.890 ms samples occur after light and upload
counters stop. Sampled zero publication work is not evidence of no work between
samples.

## Always-on frame attribution

`RUST_MCBE_SLOW_FRAME` reports a preceding start-to-start interval or main update
lasting 20 ms or more, at most once per second. It prints `frame_ms`, `main_ms`,
`between_updates_ms`, focus, occlusion, suppressed slow-frame count, and sorted
`main_stages` / `window_stages` durations in milliseconds. Named spans include
world/network work, actor animation/preparation/rig building, UI preparation and
publication, telemetry, drawable preparation and CPU render/present submission.
`main_stages` includes spans completed during the main update, including
concurrent render and worker spans; it is not restricted to main-thread CPU.
Nested and concurrent stages overlap; a worker's whole span is counted when it
completes, so spans crossing a boundary can exceed the frame interval. These
numbers are attribution, not additive CPU/GPU timings. Fast
frames do not format strings, write traces or take aggregate-sample locks.

## Reference contracts

- Lens was searched source-first in reconstructed client 1.26.50.26. Its verified
  source-backed brightness callback, artifact 6 / RVA `0x01ee9ca0`, dirties the
  changed block's ±1 halo. This agrees with `R:r/RenderChunkCoordinator.cpp:914–968`
  in the supplied 26.30 by-owner reconstruction. Valid light invalidation was
  preserved.
- `R:d/DataBindingComponent.cpp:553–638` identifies conditional controller binding
  and retained binding state. The vanilla pack's `ui/hud_screen.json` supplies the
  authored HUD bindings and factories used by the offline UI runs.
- The pinned vanilla pack's `entity/player.entity.json:16–43` selects
  `geometry.humanoid.custom`, initialization, pre-animation and the player root
  animation controller. The synthetic actor witness uses these installed
  definitions through production publication, with distinct player skins.
- The skin optimization changes cache lookup only. It does not change skin
  geometry, texture pixels, actor animation or any vanilla presentation rule.

The pack paths come from the worktree's read-only `.local` link and the source
manifest. Extracted server packs, scratch carriers and executables remain under
`/private/tmp`; none of their payloads are committed.

## Repeating the offline witnesses

Run Cargo through the supplied `scratchpad/cslot`, with `--release --locked` and
the ignored test filters below. Already-built test executables can run directly;
the app executable must run from `app/` so the existing carrier harness resolves
the worktree's `.local` link. All asset-dependent witnesses skip if their required
local inputs are absent.

- `-p bedrock-client --lib frame_cost_bench_skin_packing_shared_layers -- --ignored --nocapture`
  isolates the production skin cache, with one cold pack followed by 200 warm
  calls. It asserts that the packed output allocation stays unchanged.
- `-p bedrock-client --lib synthetic_player_lobby_bench -- --ignored --nocapture`
  uses `CINNABAR_RENDER_PACK` pointing to the supplied archive renamed from its
  manifest as `<uuid>_<version>.mcpack`. `CINNABAR_LOBBY_PLAYERS=128` and
  `CINNABAR_LOBBY_MOVING=1` select the moving population. No socket is opened.
- `-p bedrock-client --lib offline_server_hud_publication_cost -- --ignored --nocapture`
  uses `CINNABAR_FORM_PACK_DIR` with the two extracted directories containing
  `ui/hud_screen.json`, separated by `:`. It publishes actual painted vertices.
- `-p render --lib fast_frame_attribution_cost -- --ignored --nocapture`
  compares a disabled recorder against always-on recording without aggregate
  samples or a trace file.

Wall-clock tails vary under concurrent local builds. The actor witness reports
thread CPU separately; its fixed population does not reproduce the owner's
packet stream or FIFO presentation. The supplied trace does not establish that
the skin scan explains every long frame. The next normal session's always-on
attribution can identify any remaining CPU stage or inter-update/render wait.
