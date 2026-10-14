# Local terrain reads

The `terrain` grant, denied by default, lets local components and packages read nearby
terrain through `terrain.read-region` in the extension world. It also admits the local pose
from `gameplay.read-frame`, without other players. Limits live in `mod_api`
(`MAX_TERRAIN_*`).

## Queries and polling

A query names an inclusive minimum, an exclusive maximum (each extent 1 to
`MAX_TERRAIN_EXTENT` blocks) and the `geometry` it needs: `collision`, `selection`, both or
neither. It must lie within `MAX_TERRAIN_DISTANCE` blocks of the player's feet on each axis.
A callback may stage `MAX_TERRAIN_QUERIES` distinct queries. Errors are typed: `denied`,
`unavailable` (no gameplay frame), `invalid-query`, `out-of-range`, `budget-exhausted` and
`limit-exceeded`.

The queries a callback reads become its subscriptions; the next callback that runs replaces
them, so a query it no longer reads is dropped. A callback that is deferred or skipped keeps
them. Before each frame's callbacks the client refreshes every mod's subscriptions, so a new
query first reads `pending`. Pass the stamp of the last region received for a query as
`after`: an unchanged sample returns `unchanged(stamp)` without copying any terrain, and a
changed one returns the whole region. A stamp compares equal only within one query, session,
dimension and registry generation.

A subscription is re-sampled only when its session, dimension, registry or a column it read
changed. Re-sampling shares one per-frame cell budget (`MAX_TERRAIN_FRAME_CELLS`) across all
mods, taken round-robin from where the previous frame stopped; a stale subscription past the
budget reads `pending` until its turn, within a few frames. Regions over the box, metadata or
response-size limits fail with `limit-exceeded`; ask for a smaller region.

## Region layout

Cell `((x * size-z) + z) * size-y + y`, relative to the minimum, is Y-contiguous. Bit
`cell % 64` of `loaded[cell / 64]` is set for known cells; unknown cells are never air.
Layer `l` of a cell is `states[l * cells + cell]`, an index into `palette`, or 65535 when the
cell has no such layer. A known section without storage is air in layer 0.

Each palette entry is one runtime ID with its identifier, canonical state JSON, tags and
physics (friction, speed factors, fluid height, physics flags and surface response). Each
is absent when the registry lacks it, never inferred.

Shapes are shared: shape `s` is boxes `shape-offsets[s]` to `shape-offsets[s + 1]` of
`shape-boxes`, six block-local `f64` values each, with native position offsets applied.
Shape 0 is known-empty. A state binds collision and selection to `fixed` geometry,
`not-requested`, or `per-cell` when geometry depends on position: paired doors, random
offsets, neighbour-dependent or block-actor selection, and coordinates beyond single
precision. Every per-cell occurrence has an entry in `overrides`, sorted by slot.
`unavailable` geometry needs a neighbour or registry fact the client lacks. Selection uses
the default interaction context.

`chunks` lists every column read, including columns consulted for neighbour or actor
geometry, with its revision; `none` is absent terrain.

## Computation

Sampling runs in the client, outside guest fuel, and is bounded by the cell budget. Callbacks
of terrain-authorized mods have the ordinary frame fuel; pathfinding, raycasts and caching
belong to the mod and should stay incremental. Screens suspend reads.
