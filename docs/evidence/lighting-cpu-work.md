# Lighting CPU work

The bounded CPU solver now stops retained-light support searches as soon as the old
value is supported. Sky searches start above the cell. Output packing visits each
section once, packs its two nibble channels directly, and constructs uniform channels
without allocating packed payloads. Light rules, provenance and propagation queues
are unchanged.

## Bottleneck and deterministic checks

A temporary phase-timed baseline on a synthetic 24-section retained-sky column spent
about 13.6 ms checking support, 9.0 ms propagating sky and 1.6 ms packing results.
These four-sample attribution diagnostics include instrumentation and are separate
from the repeated uninstrumented comparisons below. CPU sampling was unavailable
on this remote host (`perf_event_paranoid=4`).

Regressions first failed with six neighbour reads instead of zero for local support,
six instead of one for sufficient neighbour support, four instead of one for direct
sky, and one transient packed allocation instead of zero for a uniform sky section.
They now pass. A scalar fixed-point oracle checks every light value and provenance
bit across 256 edited scenes, filters, unknown cells, trusted boundaries, dimensions,
integer limits and section seams. A separate four-section sky test removes and restores
its source. Packing tests compare scalar writes, all nibble pairs and canonical storage,
including known darkness, unknown sections and partially intersected sections.

## Remote measurements

2026-10-08, remote Linux x86-64, Ryzen 7 9700X (16 visible logical CPUs, cgroup
quota 8 CPUs), Rust 1.93.1. Dev profile with opt-level 3 and debug symbols disabled;
this is not a release/LTO or target-hardware acceptance build. Production changes:
`312d335cc` → `21a6939a6`. No task-owned builds ran during the timed comparisons.

Each executable ran twice in ABBA order: four warmups and 64 individual samples per
case/pass, pooling 128 samples per build/case. Scratch is warm; time includes output
destruction. Allocation counters run separately; bytes are total allocation requests,
not peak memory. Both candidate passes beat both baseline medians in every case.
All 42 case/size pairs preserve output fingerprints and every recorded queue counter.
Median reductions range from 2.9% to 32.4% across 1, 10 and 24 sections.

| Synthetic case, 24 sections | Before p50 (ms) | After p50 (ms) | Reduction | Allocation calls before → after |
| --- | ---: | ---: | ---: | ---: |
| sky_top_fresh | 21.774 | 20.637 | 5.2% | 58 → 29 |
| sky_top_retained | 35.709 | 26.682 | 25.3% | 58 → 29 |
| sky_all_seeded_fresh | 22.406 | 21.486 | 4.1% | 58 → 29 |
| sky_all_seeded_retained | 35.212 | 23.871 | 32.2% | 58 → 29 |
| solid_dark_fresh | 11.692 | 10.975 | 6.1% | 34 → 29 |
| terrain_caves_filters_fresh | 20.302 | 19.167 | 5.6% | 69 → 53 |
| terrain_caves_filters_retained | 31.235 | 24.105 | 22.8% | 69 → 53 |
| sparse_emitters_fresh | 13.382 | 12.497 | 6.6% | 51 → 46 |
| sparse_emitters_retained | 16.982 | 14.878 | 12.4% | 51 → 46 |
| dense_emitters_fresh | 23.197 | 21.997 | 5.2% | 58 → 53 |
| dense_emitters_retained | 31.607 | 25.285 | 20.0% | 58 → 53 |
| removed_emitters_retained | 17.198 | 15.327 | 10.9% | 34 → 29 |
| trusted_halo_fresh | 21.941 | 20.745 | 5.4% | 60 → 31 |
| trusted_halo_retained | 36.046 | 26.813 | 25.6% | 60 → 31 |

Uniform 24-section sky allocates 102,144 bytes per independent result, down from
153,600. Packed channel allocations are eliminated; the result still owns its
provenance, section handles and map. Existing scratch-retention checks still pass.

[All aggregates](lighting-cpu-work.csv) include p50, nearest-rank p99, maximum,
individual-pass medians, allocation bytes, queue work and fingerprints. Tail samples
remain unqualified: 24-section fresh sky p99 rose from 21.876 to 22.396 ms, and its
maximum from 21.946 to 23.676 ms. The one-section retained-terrain maximum also rose.
Their causes are unattributed; no scheduler trace was available. Median savings do
not establish a tail-latency or live hitch improvement.

A separate, uncommitted composition with the indexed caches and coalesced queues of
#349 (`b49bbf4f4`) passed that branch's 45 focused tests and matched all 14 24-section
fingerprints. A second ABBA comparison measured another 9.0–34.0% median reduction
relative to #349 alone: top-sky retained 23.224 → 17.180 ms; retained terrain
20.776 → 15.577 ms. The CSV labels this diagnostic `pr349`. It is complementary work,
not included in this PR; composing the changed support helper requires preserving
#349's indexed cache reads. One fresh-terrain maximum worsened in this comparison too.

## Reproduction and limits

Build the benchmark at the base and candidate commits, using a separate Cargo target
per worktree and the shared build-slot limiter:

```sh
CARGO_PROFILE_DEV_OPT_LEVEL=3 CARGO_PROFILE_DEV_DEBUG=0 \
  cslot cargo bench --locked --profile dev -p world --bench light_work --no-run
```

Copy each executable printed by Cargo outside its target directory. Run the two
executables in baseline/candidate/candidate/baseline order, each with `--samples 64`.
Use the same benchmark files on both revisions. The standalone harness also accepts
`--sections 1,10,24`, `--warmups N` and `--case substring`. Redirect CSV and summaries
outside the repository. No assets, server or credentials are required.

The synthetic cases exercise whole-column sky, explicit all-cell sky seeds, solid
terrain, caves/overhangs/filters, sparse and dense emitters, source removal and trusted
halos. The all-cell sky case is a stress case; the pipeline normally seeds the world
top. Real scheduler shortcuts can bypass uniform solves entirely. These results omit
palette lookup, dispatch, meshing, GPU uploads and network delays; they are not native
terrain captures or a Dragonfly comparison.

The strongest measured changes here remove CPU work without changing output. Indexed
cache access and queue coalescing remain useful complementary work. Bit-parallel/GPU
propagation and shader-sampled light need separate measurements and output validation;
this investigation supplies no evidence for a blanket 5–20× claim. Matching the current
solver output does not close remaining vanilla parity or live performance gates in
[the hardware requirements](../agents/live-testing.md).
