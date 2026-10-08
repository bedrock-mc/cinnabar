# Biome tint sampling evidence

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Lattice points | Seven points per axis, spaced four blocks apart, cover offsets -12 through +12 from the cache origin. |
| Biome counts | Each point counts 27 samples at offsets -4, 0, +4 in X, Y and Z and retains the four most frequent IDs. Divide each retained count by all 27 samples without renormalizing discarded IDs. |
| Sampling | For an integer `BlockPos`, choose the eight nearest points from 27 candidates and weight each by `1 / (distance + epsilon)`, divided by the weight sum. Equal distances retain X/Y/Z traversal order. Z indexing adds one lattice step, so candidates are symmetric on all axes. Cell division truncates toward zero relative to the cache origin. |
| Cache origin | The render chunk's minimum block position plus eight on each axis is passed to the snapshot and lattice builder. |
| Epsilon | **1.1920928955078125e-7**, exactly `f32::EPSILON`. |
| Snapshot | Four-block spacing within radius 16; entries start at zero and absent chunks leave a zero biome ID. |
| Cache selection | The builder installs the snapshot and lattice cache under a capability query whose mapping to a user graphics setting remains unresolved. |
| Grass | A direct colour path is used when the cache is disabled, otherwise the lattice sampler is used. A tint-method-5 caller samples at a block position; this does not establish every grass, foliage or water route. |
| Tint strategies | Strategies 1–4 use foliage palette callbacks, 5 grass, and 6 water. Grass and water query the lattice under the same capability test or read per-column colours. All four foliage kinds use the lattice unless seasonal tinting applies; the fallback averages an unresolved offset list. |
| Vines | Query tint once at the integer block position before writing vertex colours. |
| Separate grass average | A separate path averages 25 horizontal samples at four-block spacing and the supplied Y. Its graphics-mode selection remains unresolved. |

## Cinnabar change and limits

The render shader already used a provisional horizontal four-block lattice;
`biome.rs`'s radius-one helper only drove CPU diagnostics. Changing its radius
alone would not change the screenshot. The shader used separable linear
weights, ignored vertical neighbours, and clamped absent neighbour samples to
its own edge. Model tint queries also varied over model geometry.

The new record caches the 3D lattice counts, carries 27 neighbour identities,
uses biome zero for absent samples, and invalidates vertical and diagonal mesh
consumers on arrival, replacement or eviction. Query weights are generated once
from Rust for the 343 signed within-cell integer positions. Uniform records
skip the lattice allocation. A nine-point-per-axis scratch grid reduces packed
biome lookups from 9,261 to 729 per nonuniform record. Model vertices share their
own block's tint query. Foliage kinds select their palettes inside the average.

**Incomplete:** this ports the verified lattice cache, not a proven universal
rule for every vanilla graphics mode and tint type. Top-four ties involving
more than four equally frequent biome IDs, all per-corner tessellation routes, the
capability-to-setting mapping, and vanilla neighbour-arrival invalidation still
need reference evidence. The owner's screenshot has no retained biome records;
its live hard edge has not been reproduced or conclusively attributed.

The PNG gallery is a CPU palette preview with synthetic artwork, not a GPU
frame or native acceptance witness. The complete-halo before preview can already
show a gradient; it cannot prove that this change fixes the owner's live seam.
No live server connection is needed or used by these tests.
