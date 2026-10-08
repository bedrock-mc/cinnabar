# Lighting solver work

The solver now reuses validated dense indices across block, prior-light, provenance and output
buffers. Pending increases for the same cell share one queue entry and merge direct-sky
provenance before visiting neighbours. Membership clears on pop so later increases can revisit
the cell. Bounds checks, raw light-value validation and trusted halo handling remain in place.

## Deterministic evidence

Two three-cell regressions fail on the original solver with five visits instead of four: one
upgrades a queued block-light source; the other upgrades queued direct-sky provenance. An
independent whole-field relaxation oracle checks every light value and provenance bit across
96 edited scenes. Coverage includes filters, unknown blocks, trusted halos, partial and negative
bounds, coordinate limits, queue limits and scratch reuse after interrupted solves.

| Workload | Original increase visits | Revised visits | Peak pending entries, original → revised |
| --- | ---: | ---: | ---: |
| Recorded 24-section output fixture | 263,425 | 239,032 | 31,173 → 31,173 |
| Sky, fresh | 98,304 | 98,304 | 256 → 256 |
| Sky, retained | 98,304 | 98,304 | 98,304 → 98,304 |
| Mixed, fresh | 145,555 | 124,387 | 49,851 → 37,575 |
| Mixed, retained | 109,132 | 109,132 | 98,304 → 98,304 |

The recorded fixture retains its exact light/provenance fingerprint and enforces reduced work
ceilings. With pending queues already merged, indexed cache access reduces its coordinate
conversions from 4,948,745 to 1,676,400; the smaller 16³ fixture falls from 178,438 to 59,436.
A test-only counter guards this reduction. Each increase channel retains one membership byte
per cell, and each queued entry adds a dense index; scratch-retention tests cover these buffers.

## Runtime diagnostic

M3 Pro, 2026-10-08; dev profile with `world` opt-level 1 and external dependencies opt-level 3.
Original base: `9e0db66fb`. Preserved original and revised executables ran in ABBA order, with
eight warmups then 256 individually timed solves per case/pass: 512 samples per case/version.
All columns contain 24 sections. Worker scratch is warm. Timing includes output destruction
and excludes fixture construction, warmup and CSV reporting. Quantiles use floor(p × n).

**Shared-host compilation was active. These are standalone diagnostics, not a live streaming
or client frame-budget gate.** Both revised median passes beat both original passes in every
case, by 27.7–31.0% pooled. Mixed-fresh tail outliers occurred in revised pass one and original
pass two; their exact cause is unassigned, so the tail difference is not a qualified improvement.

| Case | Original p50 / p99 / max (ms) | Revised p50 / p99 / max (ms) |
| --- | ---: | ---: |
| Sky, fresh | 20.266 / 20.714 / 28.445 | 13.975 / 14.263 / 14.843 |
| Sky, retained | 33.026 / 33.560 / 33.837 | 22.962 / 23.642 / 25.501 |
| Mixed, fresh | 24.384 / 64.024 / 141.642 | 16.966 / 38.227 / 107.106 |
| Mixed, retained | 32.402 / 32.875 / 39.060 | 23.416 / 24.150 / 24.488 |

Run medians in ABBA order (ms): sky fresh 20.270, 13.983, 13.971, 20.250; sky retained
32.947, 22.944, 22.971, 33.116; mixed fresh 24.417, 16.877, 16.998, 24.324; mixed retained
32.398, 23.394, 23.447, 32.412.

Native samples compare the earlier candidate with indexed queue entries against the final
version that also reuses indices across cached fields. The ABBA table compares the original
base with the final version. Summed running-stack sample weights fall from 28,093 to 19,321 ms
for the same fixed workload. Coordinate mapping self weights fall from
7,679 to 2,405 ms. Output packing remains about 2.4 seconds of sample weight (12.2% of revised
samples). Only the sampled-stack table was exported. These weights estimate where CPU work
occurs; they are neither exact execution times nor measurements of wall-time tails.

## Verification and reproduction

Focused lighting tests pass: 21 unit and 23 integration tests, with one pre-existing ignored
timing benchmark. New work regressions fail before their fixes. The independent oracle and
output fingerprints pass both versions. The dev benchmark builds; `cargo check --locked
--tests -p world` checks the touched crate. CI asserts work and output, never milliseconds.

Route Cargo through `cslot` with `CBUILD_SLOTS=3 CARGO_BUILD_JOBS=4`:

```text
cslot cargo test --locked -p world light --no-fail-fast
cslot cargo check --locked --tests -p world
cslot cargo bench --locked --profile dev -p world --bench chunk_costs --no-run
```

Preserve the executable printed by Cargo for each version, then run each with `--light-samples`
in ABBA order. It emits individual samples as CSV and deterministic work on stderr. Comparable
client streaming captures against the [hardware requirements](../agents/live-testing.md) remain
necessary before closing the performance gate.
