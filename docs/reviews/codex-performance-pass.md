# Cinnabar performance pass

Reviewed `review/codex-perf` at base `e92dd67ad86d2fa588420aff8586c3a38a1eb9ac`
on 2026-09-29. Changes preserve the existing math, draw ordering, semantic
rejection, simulation ticks, and pack bindings. Item icon resolution, inventory
ledger handling, and equipment presentation were not edited.

## Evidence from the owner's busy-server log

The 316 stage-profile intervals in `.local/logs/client.log` average 247.862 ms
of `main_frame` per second. `ui_publication` averages 101.304 ms/s (40.9%),
`actor_publication` 17.659 ms/s, and `block_entities` 15.544 ms/s. Stage totals
can overlap; they are attribution, not additive frame time.

| Log line | Main frame maximum | Correlated stage maximum | Interpretation |
| --- | ---: | ---: | --- |
| 224 | 131.223 ms | actor 72.677 ms; UI 52.034 ms | Join/pack and new-actor work share the spike. |
| 2667 | 93.803 ms | UI 92.221 ms | UI dominates this spike; actor and world-stream work are small. |
| 4376 | 34.651 ms | UI 33.009 ms | Another UI-dominated frame. |
| 5270 | 59.855 ms | thousands of pending chunk jobs nearby | Backlog correlation does not prove which thread blocked. |
| 6125 | 74.154 ms | network ingestion 73.292 ms | Ingestion dominates; world-stream maximum is only 0.267 ms. |

`CachedScreen::render` in
`app/src/ui_runtime/presentation/forms/hud.rs` already retains HUD resolve and
layout results until model/catalog/viewport/scale changes. Moving nametags still
rebuild the composed retained presentation and its GPU input. Variable inheritance,
repeated measurements, and unchanged GPU bytes are therefore useful targets
without assuming a completely uncached HUD.

The reviewed base already negatively caches failed skin geometry (64 entries),
caches skin normalization, batches rig registration, reuses completely static
block-entity scenes, quantizes nametag text scale, and decodes audio on workers.
The log has 15 failed-model warnings, not evidence of an every-frame retry loop.
Lighting, meshing, and chunk decode use Rayon workers; the 178 ms lighting solve
is worker work. Mesh dispatch and GPU uploads already have bounded budgets.

## Measurement conditions and limits

All Cargo invocations use `CARGO_PROFILE_DEV_DEBUG=0` and this worktree's `target`.
There was one heavy Cargo invocation at a time in this worktree and no release
build. Benchmarks use the repository's test profile (workspace opt level 1,
dependencies opt level 3) on the Apple M3 Pro. Old/new reference paths run in the
same test executable where possible. Counters and exact output comparisons guard
the optimizations independently of noisy elapsed times.

These are local test-profile measurements, not release budget acceptance or a
live frame-rate result. Host load varied substantially: a second sequential
actor sample reversed its timing result. The actor benchmark now alternates
old/new order across eleven batches and reports medians. The changing-HUD fixture
also varied (initial untouched run: 1.067 ms bind + 2.373 ms layout/emission;
a later pre-layout-cache run: 2.327 + 4.687 ms; optimized run: 1.074 + 1.687 ms).
The final workspace fixture was 0.299 + 0.510 ms.
Those sequential runs are useful diagnostics but do not isolate causality.
The label request-count test gives the reliable layout result.

## Optimizations and guards

| Change | Before | After | Guard |
| --- | --- | --- | --- |
| Share inherited JSON-UI variables until a declaration changes them | 38.202 ms / 1,000 scopes with 500 variables | 0.004 ms | Read-only scopes share storage; changed declarations isolate parent and siblings; same-value writes retain sharing, while scalar and nested signed zero preserve substitution spelling. |
| Reuse natural and child measurements within one layout | 5 text measurements for one label (4 unwrapped, 1 wrapped) | 2 (1 per width) | Counter fails against the original implementation, then passes with changed text, viewport, and environment; existing layout/HUD/form fixtures pass. |
| Retain static mesh fragments in a scene containing animated block entities | 0.228 ms / frame | 0.097 ms / frame | Every layer matches a complete rebuild; 400 static rebuilds over 200 frames, rather than 80,000; reorder, light/model changes, assets, dynamic atlas, and vertex-limit rejection are covered. |
| Refresh actor-bound particle emitters in one traversal | 0.288 ms / frame at 768 attachments | 0.001 ms / frame | One callback per live attachment; missing actors stop emission while particles finish; subsequent draws match the old path. |
| Append actor matrices to final bone arenas | 0.127 ms median / frame | 0.068 ms median / frame; temporary pose allocations 100 → 0 per 50-actor frame | Complete frames match; invalid late bones and identity rejection restore both arenas; previous maximum-vertex-count behavior remains. |
| Upload exact changed UI buffer spans | 312,000 bytes / moving-tag fixture | 24,000 bytes (92.3% less) | Exact byte reconstruction including signed zero; grow/shrink, unchanged inputs, block boundaries, and real render preparation/reallocation are tested. |

The table uses the final workspace benchmark run (exit 0), with eleven
alternating batches for the actor timing and sequential pairs for the other
fixtures. Repeats under changing load were 0.953 → 0.457 ms for mixed block entities
and 1.211 → 0.001 ms for attachments; the counters are unchanged. The initial
actor pair was 0.134 → 0.072 ms, followed by the noisy reversed sample mentioned
above. UI upload planning initially added 0.027 ms CPU cost (0.004 → 0.031 ms)
when comparing each vertex; block comparisons reduced the repeat to 0.005 →
0.007 ms; the final pair was 0.004 → 0.006 ms. This trades a small CPU scan for fewer GPU queue writes and bytes;
no native GPU/frame-time improvement is claimed.

Run the new paired benchmarks with:

```sh
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_TARGET_DIR="$PWD/target"
cargo test -p json-ui --locked frame_cost_bench -- --ignored --nocapture --test-threads=1
cargo test -p render --lib --locked frame_cost_bench -- --ignored --nocapture --test-threads=1
```

The local vanilla HUD fixture reads ignored assets; it skips when they are absent.
The synthetic optimization guards and render benchmarks do not require those
assets.

## Deferred work

- Incremental subtree binding and layout: custom packs can bind across controls,
  change factories/visibility, and couple size expressions to siblings. A complete
  dependency/invalidation contract is needed before changing publication behavior.
- Moving initial HUD resolution or new-skin registration to workers: preparation,
  readiness, catalog ownership, and scene admission must stay atomic. Current tests
  do not prove unchanged first-frame timing and artwork readiness for every pack.
- Teleport eviction: `crates/client-world/src/stream/residency.rs:43` scans several
  full subchunk maps for each column, and `evict_all_resident` repeats it for all
  columns. This is a plausible ingestion spike source. Bulk eviction needs a
  reference fixture for invalidation, dirty-neighbor order, and cohort generations;
  the log alone does not establish attribution.
- Completion-drain limits and lighting-batch splitting: queued work is already off
  the main thread. New limits must preserve FIFO committed frontiers, publication
  permits, fairness, and light dependencies. A naive per-frame cap could stall
  authoritative updates or starve initial lighting.
- Actor catch-up and particle substeps: skipping ticks changes simulation and
  animation behavior. They were not capped as part of a behavior-preserving pass.
- Nametag wall-ray reuse: camera, actor, block, and collision-generation changes
  need exact invalidation; stale occlusion would change visible output.
- Audio queue caps and actor artwork replacement are bugs reported separately;
  they were not fixed under the report-only instruction.

## Verification and delivery

Every Cargo command below used `CARGO_PROFILE_DEV_DEBUG=0`, this worktree's
`target`, and the shared build-slot wrapper. The final Clippy result includes
the signed-zero correction and alternating actor benchmark.

| Required command | Real exit | Result |
| --- | ---: | --- |
| `cargo fmt --all --check` | 0 | Green. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | Green. |
| `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml` | 0 | Green. |
| `cargo test --workspace --locked` | 101 | Three existing remote-image fixtures cannot bind `127.0.0.1:0` in the sandbox. |

A complete serial continuation (`--no-fail-fast -- --test-threads=1`) with
`GOCACHE=/private/tmp/codex-perf-go-cache` produced **5,051 passed, 6 failed,
30 ignored**, exit **101**. All six failures are denied socket binds:

- `cinnabar`: `remote_images_download_in_the_background`,
  `path_and_url_button_images_resolve_like_vanilla`, and
  `vanilla_form_button_images_resolve`. They share the unchanged TCP fixture at
  `app/src/ui_runtime/presentation/forms/remote_images.rs:145`.
- `bridge`: `go_status_endpoint_returns_the_strict_initialized_snapshot`,
  `live_go_status_compatibility_outcome_decodes_as_schema_v1`, and
  `go_frame_echo_round_trips_binary_payloads_and_cleans_up`. Their Go helpers
  report `listen unix ...: bind: operation not permitted`.

The first complete parallel continuation also hit the existing single-entry
`OVERLAY_CACHE` test assertion (2 compiles instead of 1), and Go writes to
`~/Library/Caches/go-build` were denied. Serial execution passed the cache
assertion; using a writable Go cache passed both physics registry install tests
and allowed the Go bridge helpers to compile. The remaining socket tests were
not edited, skipped, or marked ignored. Thus full workspace verification is
**not green** and needs an unrestricted local run.

The focused JSON-UI suite and renderer library suite passed; the renderer result
was 443 passed, 4 ignored. The original-layout run of the new measurement guard
deliberately failed (Cargo exit 101), proving the counter detects repeated work;
that baseline source was restored immediately afterward. The final workspace
ignored frame-cost benchmark command exited **0**, including the existing
skin-normalization, rig-registration, static-scene, audio first-play, and hidden
preview benchmarks. Those existing optimizations are not attributed to this pass.

Full local command logs are `/private/tmp/codex-perf-workspace-test.log`,
`/private/tmp/codex-perf-workspace-test-all.log`,
`/private/tmp/codex-perf-workspace-test-serial.log`,
`/private/tmp/codex-perf-final-bench.log`, `/private/tmp/codex-perf-clippy.log`,
`/private/tmp/codex-perf-architecture.log`, and `/private/tmp/codex-perf-fmt.log`. They remain outside Git.

Git staging failed with exit 128 because the sandbox denies creation of
`/Users/hashim/Downloads/cinnabar/.git/worktrees/codex-perf/index.lock`.
No local commit or push was made. The change groups and messages are prepared
for local application outside that restriction in
`/private/tmp/codex-perf-commit-plan.sh`. The script checks the branch/base and
requires all four verification commands to pass before creating eight small
commits. Every message ends with the requested co-author trailer. It never pushes. All work remains on
`review/codex-perf`; no other branch was modified.

A target-platform rendered-frame pass and measured release run have not been
performed. Under `docs/agents/live-testing.md`, these changes remain local and
are not cleared for visual/performance acceptance or pushing.
