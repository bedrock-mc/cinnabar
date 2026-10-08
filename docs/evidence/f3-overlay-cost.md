# F3 overlay CPU cost

## Scope and method

M3 Pro, macOS arm64, Metal, 1920×1080, DPI 1, GUI scale 4, 12-chunk radius, Immediate presentation and vsync disabled. All clients were headless and driven through developer-control on an owned local terrain server. The fixed scene contains 489 columns and no remote entities. Camera position, inspection heading, player feet and residency were checked at every phase boundary.

Before: optimized play-profile build preserving `3abbe9f94` overlay behavior, with matching timing zones and developer-control frame-count instrumentation (`developer-control,tracy`). After: optimized play-profile runtime from `5b826a364`, integrating dev `686fbb9e1`. Each Tracy comparison alternates three closed/open pairs of 120 seconds after settling. Conservative clock-anchor bounds select complete frames. The additive UI attribution uses disjoint main/UI and render/UI systems and CPU graph nodes; nested detail spans are reported separately. These are CPU-side elapsed spans across threads, not active CPU time or additive frame wall time.

This scene stays near 120 FPS despite uncapped Immediate presentation. It cannot establish the reported 500 FPS behavior. It also does not qualify the 30-player lobby, streaming or join budgets.

## Results

| UI CPU elapsed work (ms/frame) | Before | After |
| --- | ---: | ---: |
| F3 closed | 0.282 | 0.220 |
| F3 open | 1.165 | 0.363 |
| Open minus closed | 0.882 | 0.143 |

Added UI work fell by approximately 84%; every final pair is below the requested ~0.2 ms average per-frame target.

Per-pair added UI work: before 0.807, 0.865, 0.975 ms; after 0.139, 0.142, 0.148 ms.

| Open-frame detail (ms/frame; nested rows overlap) | Before | After |
| --- | ---: | ---: |
| Gather/format/snapshot total | 0.04438 | 0.00477 |
| World/chunk/spatial diagnostics subset | 0.03254 | 0.00324 |
| Inspection ray subset | 0.02456 | 0.00245 |
| Entity inspection subset | 0.00097 | 0.00013 |
| Retained paint total | 0.42009 | 0.06839 |
| Rebind/layout subset | 0.39474 | 0.06005 |
| Bindings/text measurement subset | 0.05428 | 0.00964 |
| Layout subset | 0.17506 | 0.02595 |
| UI geometry rebuild | 0.23767 | 0.03873 |
| Viewport upload API elapsed | 0.03249 | 0.00000 |
| Vertex upload API elapsed | 0.06968 | 0.00893 |
| Index upload API elapsed | 0.01444 | 0.00005 |
| Existing device poll | 0.06239 | 0.03435 |
| Existing readback processing | 0.00360 | 0.00286 |

Gather includes direct string formatting, diagnostic queries and CPU statistics copies. Static system labels use existing metadata. World includes spatial and entity subsets. Paint includes rebind and layout; these detail rows must not be added together. The whole UI-system difference above also includes UI preparation, render preparation, layering, binding, queueing and graph execution.

| Open-frame deterministic work | Before | After |
| --- | ---: | ---: |
| Gathers/frame | 1.00009 | 0.16820 |
| Rebind/layout calls/frame | 0.99392 | 0.16820 |
| Geometry rebuilds/frame | 0.99392 | 0.16820 |
| Viewport writes/frame | 1.00005 | 0.00000 |
| Vertex writes/frame | 0.99387 | 0.16818 |
| Index writes/frame | 0.49970 | 0.00199 |
| Font-atlas writes/frame | 0.00000 | 0.00000 |
| Viewport bytes/frame | 16 | 0 |
| Vertex bytes/frame | 191053 | 5207 |
| Index bytes/frame | 18757 | 75 |

| Collector-disconnected 120 s pair | Closed FPS / mean ms | Open FPS / mean ms |
| --- | ---: | ---: |
| Before | 119.34 / 8.379 | 118.29 / 8.454 |
| After | 119.71 / 8.354 | 119.99 / 8.334 |

## Work and cadence

Frame, GPU and UI counter text publishes once per second. World diagnostics publish at the shared simulation tick cadence; intermediate frames skip gathering and formatting. Hidden or covered overlays do not query world diagnostics. Reusable string buffers retain warmed capacities. Unchanged displayed rows skip rebind/measurement, retained glyph artwork is reused, and unchanged UI publications keep their geometry identity and require no vertex/index upload. The shared viewport uniform is retained when no active glint needs its clock; resize or changed settings publish it once, while active glint continues at the existing global phase. Font pages are already resident; changing numeric labels uploads no glyph atlas.

Baseline regression fixtures failed for the short frame-statistics window, per-frame gathering, covered-screen gathering, formatted-buffer allocations (including growth, shrinkage and regrowth) and measuring unchanged rows. The final targeted checks assert cadence, visibility, zero allocations on warm unchanged inputs, changed-row-only measurement, cache invalidation, empty upload plans and unchanged actual vertex/index/viewport queue-dispatch counters. The minimal warm producer fixture fell from 12 heap allocations per unchanged tick (120 over ten ticks) to zero. A separate growth regression reproduced two allocations on the next identical tick before buffer warming; growth, shrink and regrowth now remain allocation-free on that next tick. Timing is not asserted in CI.

## GPU and readback

F3 never enables timing collection, submits timestamp queries or requests readback. It reads the existing CPU snapshot at the frame-statistics cadence. The timing ring uses asynchronous mapping and nonblocking device polling, skipping unavailable slots. Existing collector work is present with F3 closed and open.

| Query-health pair | Frames | Readbacks | Ring skips | Resolved spans/readback | UI spans/readback | Mapping failures | Reversed UI samples |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Before closed-0 | 2908 | 2645 | 263 | 7 | 3 | 0 | 1851 |
| Before open-0 | 2748 | 2604 | 143 | 7 | 3 | 0 | 2214 |
| After closed-0 | 3042 | 3027 | 14 | 7 | 3 | 0 | 2867 |
| After open-0 | 3259 | 3248 | 12 | 7 | 3 | 0 | 3101 |

The separate final query-count capture uses the same final runtime. Both clocks were validated, and reporting intervals touching visibility transitions were excluded.

Metal's pass samples overlap, omit portions of the graph and include reversed UI timestamps. They cannot establish active GPU frame cost or a measurable-zero GPU difference. No added query spans or atlas uploads in this scene is a count result, not a GPU-duration claim.

An earlier candidate averaged 0.220 ms of added UI work, with pairs of 0.075 / 0.161 / 0.425 ms. Its third open phase spent 0.254 ms/frame in the shared viewport upload API, including repeated 10–20 ms calls; that uniform was uploaded every frame without glint. The final cache removes those redundant dispatches. These earlier stalls remain part of the investigation, and no operating-system cause is presumed.

## Frame intervals and hitches

| Build / phase | FPS | p50 / p99 / maximum ms | >12.5 ms | >100 ms |
| --- | ---: | ---: | ---: | ---: |
| Before closed-0 | 113.46 | 8.396 / 25.203 / 128.339 | 396 | 1 |
| Before open-0 | 118.82 | 8.388 / 10.798 / 98.888 | 77 | 0 |
| Before closed-1 | 117.73 | 8.377 / 12.794 / 151.564 | 147 | 2 |
| Before open-1 | 118.26 | 8.393 / 11.758 / 105.641 | 119 | 1 |
| Before closed-2 | 118.57 | 8.379 / 11.228 / 77.999 | 103 | 0 |
| Before open-2 | 117.68 | 8.388 / 12.973 / 74.526 | 159 | 0 |
| After closed-0 | 119.97 | 8.409 / 9.644 / 17.692 | 5 | 0 |
| After open-0 | 120.00 | 8.438 / 9.475 / 11.599 | 0 | 0 |
| After closed-1 | 118.46 | 8.445 / 11.650 / 99.344 | 114 | 0 |
| After open-1 | 117.39 | 8.439 / 16.365 / 96.641 | 271 | 0 |
| After closed-2 | 119.01 | 8.433 / 11.414 / 124.935 | 98 | 1 |
| After open-2 | 119.29 | 8.504 / 9.713 / 130.470 | 41 | 2 |

Seven second-pair hitches intersect 8.984–20.120 ms shared vertex-write API spans exceeding half their frame interval. These are elapsed API calls, without evidence identifying a GPU wait, lock owner or scheduler delay. The final two open frames above 100 ms contain only 0.004 and 0.671 ms of selected overlay/publication/geometry/upload spans; the remaining render time is unattributed.

Every interval above 12.5 ms remains in the reported distribution; no outlier is removed as presumed operating-system activity. Selected nested-span attribution is retained privately for every hitch. Unexplained time remains unexplained; this is not full-game performance qualification.

## Verification and visual result

Touched-crate `cargo check --tests`, 18 app and 22 UI overlay tests, 34 renderer unit tests and 63 renderer integration tests run through cslot, with focused follow-up dispatch and glide-fixture checks, including the retained half-tick correction fixture required by PR CI. Later changes add test-only dispatch counters and remove an unused glide-test calculation; they do not change the measured production runtime. Env-scrubbed Codex review reports no actionable findings. The final headless rendered frame passes legibility, geometry, clipping, layering, scaling, colors and endpoint-delivered F3 input at the stated platform and scale. Screenshots are attached to the PR outside git; raw captures are removed after selected timing exports.
