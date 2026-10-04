# Prediction world retention

`bf1d08f9` added immutable collision worlds to every prediction frame, first
integrated on the original first-parent range by `f0f92337`. It copied every
column's subchunk index, loaded-column set, subchunk authority map and collision
revision map on every tick, including ticks with an unchanged world.

The store now keeps one weak reference to its current immutable collision
snapshot. Unchanged ticks share that store. Loaded authority changes, palette
mutations and column evictions invalidate the reference. History alone owns old
snapshots; clearing history releases them. Each column's subchunk index is also
shared and copied only when its palette entries change. Full-column updates that
only change biomes or block entities preserve that index.

This changes ownership, not collision queries or correction behavior. Snapshots
still retain the original registry, palette pages, loaded authority and collision
revisions after the live world changes or unloads. A changed snapshot still copies
the global column, authority and revision indexes. Very frequent mutations in a
large world remain more expensive than an unchanged world.

## Offline measurements

The isolated harness populates 1,000 columns with 16 nonempty subchunks each and
runs the same 10,000 stationary prediction ticks. It counts live allocation bytes
and samples local `ps` RSS. All builds use the same optimized development profile
through `scratchpad/cslot`. These are memory measurements, not release frame-time
acceptance or a reproduction of the player's machine crash.

| History | Baseline added live bytes | Before fix | Fixed | Final RSS baseline / before / fixed, KiB |
| --- | ---: | ---: | ---: | --- |
| 32 | 8 | 27,235,592 | 293,936 | 13,136 / 52,320 / 13,360 |
| 1,000 | 8 | 851,112,008 | 402,352 | 13,184 / 933,616 / 13,696 |

Over 10,000 ticks, allocated bytes drop from 8,556,681,036 to 47,011,380;
the baseline allocates 45,561,036 bytes. With one distant palette mutation every
100 ticks and a 1,000-tick history, fixed added retention stabilizes at 3,313,232
bytes from tick 2,000 through tick 10,000, with final RSS 16,816 KiB.

Raw harnesses and sampled logs are in `/private/tmp/cinnabar-growth/`. The source
bisect log identifies `bf1d08f9`; the original measured current was `18f3509a`.

## Regression coverage and references

The world crate tests 10,000 unchanged captures, 10,000 column changes with a
32-snapshot history, release after history eviction, and authority and palette
immutability across changes and unloads. The simulation crate's historical-world
replay tests still exercise corrections after a chunk is evicted and its live
registry is changed.

Vanilla actor prediction retains shared movement history items. This does not
assert that vanilla uses Cinnabar’s history capacities or world snapshots.
No movement formula or history capacity changes in this fix.
