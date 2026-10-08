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
main-thread pause. Native surface creation already has its own configuration-needed
condition, so it does not impose this recurring idle dispatch.

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

A separate two-second Metal diagnostic of the earlier hidden terrain build recorded
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
The instrumented client build also passed. Six interleaved 120-second captures
used the same hidden 1920×1080 scene, 12-chunk view, original 30-player fixture,
uncapped frame setting, and immediate presentation mode. Other builds were active.
The earlier baseline has diagnostic actor spans beyond the pinned revision. Its
original shared-cache build log was not retained, so these historical comparisons
do not provide the isolated source provenance of the later scheduling experiment.

| Comparison | Build | Proxy intervals | p50 ms | p99 ms | Maximum ms | >16.7 ms |
|---|---|---:|---:|---:|---:|---:|
| Three runs each | Baseline | 43,232 | 8.365 | 9.765 | 33.127 | 5 |
| Three runs each | Dispatch guard | 41,954 | 8.308 | 19.029 | 161.299 | 567 |
| Subsequent pair | Baseline | 14,173 | 8.390 | 14.232 | 139.677 | 110 |
| Subsequent pair | Dispatch guard | 14,408 | 8.262 | 9.718 | 18.291 | 3 |

The proxy measures consecutive frame-marker system entries, not display intervals.
The subsequent pair held this task's compilation and benchmarks idle, but other
host work remained uncontrolled. Its reversed tail association, and long gaps in
both builds, prevent attributing a consistent runtime gain or regression to the
guard. The recurring metadata dispatch is deterministically eliminated; the
underlying main-thread pause and all remaining long gaps are not explained.

A separate native scheduler sample observed main-thread running priority 46 and
compute frame workers predominantly at 31. A ten-second Time Profiler sample
that overlapped Tracy did not reproduce a >100 ms stall. Neither sample establishes
the native cause of the original pause. No frame-budget or 120 Hz acceptance
claim is made.


## macOS frame scheduling

Frame compute workers started at the default native scheduling class while the
main application thread ran at interactive priority. The render executor and
compute pool now request the class intended for UI and animation work. The render
policy runs once, exclusively on that executor's own thread; IO, async compute and
Rayon keep their existing policies. This follows Apple's
[interactive-work guidance](https://developer.apple.com/documentation/Dispatch/DispatchQoS/userInteractive).

A separate native scheduler sample confirms compute workers predominantly running
at priority 31 before and 46 after; the main thread runs at 46 in both samples.
The unnamed rendering thread also changes from predominantly 31 to 46, but its
identity is inferred from the workload, not a matched native thread identifier.
The samples do not overlap an original long stall. Four focused thread-budget
tests pass, including requested-class readback, unchanged background policies,
and once-only execution on the render thread. The two policy regressions failed
before the change. The isolated client test check and capture build pass.

Eight further lobby captures compare the dispatch guard with guard plus frame
QoS, in A–B–B–A order per workload. Each has 40 seconds of settling. Runs without
added load record 120 seconds each; runs with four owned CPU workers record 60
seconds each. All retain 30 actors before and after recording. Other host work is
uncontrolled. The guard build log records fresh compilation of the relevant local
crates; the QoS candidate uses a worktree-specific artifact namespace after a
shared intermediate cache exposed another worktree's APIs. No source workaround
was made for those foreign artifacts.

| Added workers | Build | Residual p50 ms | Residual p99 ms | Proxy p99 ms | Proxy max ms | >12.5 ms | >16.7 ms |
|---|---|---:|---:|---:|---:|---:|---:|
| 0 | Guard | 1.895 | 2.559 | 9.714 | 22.161 | 10 / 28,820 | 4 / 28,820 |
| 0 | Guard + QoS | 1.827 | 4.205 | 10.149 | 36.029 | 88 / 28,773 | 32 / 28,773 |
| 4 | Guard | 2.450 | 3.584 | 9.903 | 42.526 | 14 / 14,411 | 10 / 14,411 |
| 4 | Guard + QoS | 1.696 | 2.060 | 9.460 | 25.339 | 11 / 14,423 | 7 / 14,423 |

Residual means render-app elapsed time minus its contained surface-preparation
span, not pure CPU execution. The pooled loaded median is 30.8% lower and p99
42.5% lower. The result repeats per run: loaded guard medians are 2.473/2.429 ms
and p99s 3.487/3.661 ms; candidate medians are 1.706/1.686 ms and p99s 2.018/2.141 ms.
Both candidate passes beat both controls on those measures.

The unloaded tail is adverse evidence, concentrated in one candidate run with
32 intervals above 16.7 ms and residual p99 5.637 ms. Unexplained gaps remain
between short render systems. Shared-host variability prevents assigning those
gaps to the policy or excluding them as OS preemption. The loaded result supports
more frame-work headroom under contention; it does not establish consistent hitch
reduction, smoother display presentation, or completion of the frame-pacing gate.
