# Zeqa frame pacing and native dispatch

The supplied 185-second gameplay capture contains 18,918 render-app frames.
Its render-app median includes surface acquisition. These elapsed spans overlap;
they are not CPU execution measurements or additive GPU costs.

| Span | Count | p50 ms | p90 ms | p99 ms | Maximum ms |
|---|---:|---:|---:|---:|---:|
| Render app | 18,918 | 8.226 | 15.948 | 17.963 | 291.821 |
| Surface preparation | 18,918 | 5.593 | 12.127 | 15.277 | 53.641 |
| Render app minus its contained surface preparation | 18,918 | 2.822 | 5.112 | 10.100 | 291.777 |
| Render graph system | 18,919 | 1.342 | 2.428 | 3.862 | 33.072 |
| Pipeline cache processing | 18,919 | 0.000542 | 0.000875 | 0.001917 | 4.908 |
| Present call | 18,919 | 0.012750 | 0.034750 | 0.070375 | 4.131 |

Quantiles here use the sorted observation at floor(p × count). The residual row
subtracts each frame's actual contained surface span before computing quantiles.
Surface preparation includes drawable acquisition and CPU/API overhead, so its
elapsed duration cannot alone distinguish display pacing, GPU backpressure, or
thread scheduling. Existing Metal plots report summed overlapping pass latencies;
they do not establish GPU busy time or identify the receipt frame's GPU cost.

At trace time 402.666902375 seconds, the render app starts a 291.820500 ms frame.
Extraction commands finish at 402.667003417. Render workers complete small asset
systems by 402.667028917, then no render system runs until main-thread activity
resumes. The next render system is the native graphics metadata probe at
402.957494125, taking only 0.001334 ms. Surface preparation then takes 0.043792 ms
and the render graph 0.454292 ms. The extract-to-probe gap is 290.490708 ms.

The probe is a non-send system on macOS. Its body returned early for disabled or
already-published diagnostics, but scheduling that no-op still required the main
thread before view preparation could proceed. It was dispatched 18,919 times in
the capture. This identifies a client dependency that propagates a main-thread
pause into rendering; it does not identify the operating-system reason for the
main-thread pause.

The deterministic regression counts native dispatches for disabled diagnostics,
pending startup publication, successful publication, and retry. It contains no
wall-clock threshold. Native surface creation/capability queries and transparent
resource retirement have separate optional Tracy spans. Bevy already supplies
executor, pipeline-cache, surface-preparation, graph-submit and present spans.

## Local diagnostics

The original client reproduced a separate, sustained actor-publication problem
with 128 synthetic remote players: the 128-entry normalization cache also served
the local hand, causing repeated expansion to 512×512. Across three 20-second
appearance bursts, populated five-second windows averaged about 164–165 ms in
actor publication. Idle windows averaged 0.026 ms. These diagnostic captures
ran while compilation was active; they identify work, not a qualified speedup.

A separate two-second Metal diagnostic of the hidden terrain scene recorded
342 complete GPU frame groups. Merging overlapping recorded execution intervals
within each group gave 5.362 ms median, 6.550 ms p99 and 6.713 ms maximum. Across
2.746 seconds of recorded GPU observations, their union covered 62.1% of elapsed
time. The target submitted 25 command buffers per frame. These are native
execution intervals, not the additive delayed pass plots, and profiler overhead
and concurrent compilation exclude them from acceptance. The hidden surface
provided no display-swap or client-buffer-wait rows, so this capture cannot
quantify displayed intervals or separate every cause of drawable waiting.

The native-dispatch regression failed before the scheduling guard (one disabled
probe dispatch) and passed afterward (zero). All five metadata lifecycle tests
passed, and `cargo check --tests -p render --features tracy,bevy/debug` passed.
The instrumented client build and matched before/after captures are pending;
no frame-budget or 120 Hz acceptance claim is made.
