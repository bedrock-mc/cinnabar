# Local terrain reads

[Personal mods](mods/personal.md) with the default-denied `terrain` grant can call
`terrain.read-region` and read local pose from `gameplay.read-frame`. Other players
need their own grants. Screens suspend reads. Limits are `MAX_TERRAIN_*` in
[`mod-api`](../crates/mod-api/src/lib.rs).

## Query lifecycle

A query has inclusive minimum, exclusive maximum and requested collision/selection
geometry (either, both or neither). Extents and distance from the player's feet
must fit the SDK limits. Errors are `denied`, `unavailable`, `invalid-query`,
`out-of-range`, `budget-exhausted` or `limit-exceeded`.

Queries read in a callback replace its subscriptions. Deferred/skipped callbacks
retain them. New queries first return `pending`; the client refreshes subscriptions
before callbacks. Pass the previous stamp as `after` to receive `unchanged(stamp)`
without copying unchanged terrain. Stamps compare only within the same query,
session, dimension and registry generation.

Subscriptions refresh when their session, dimension, registry or read columns
change. A shared round-robin frame-cell budget defers remaining refreshes as
`pending`. Oversized geometry/metadata/responses return `limit-exceeded`; shrink
the query. Sampling is host work; guest pathfinding and caches still consume the
ordinary frame fuel and should be incremental.

## Region layout

| Field | Interpretation |
| --- | --- |
| Cell index | `((x * size-z) + z) * size-y + y`, relative to minimum |
| `loaded[cell / 64]` | Bit `cell % 64` marks known cells; unknown is not air |
| `states[l * cells + cell]` | Palette index at layer `l`; 65535 means no layer |
| `palette` | Runtime ID, identifier, canonical state JSON, tags and available physics; missing facts stay absent |
| `shape-offsets`, `shape-boxes` | Shared block-local boxes; shape 0 is known-empty |
| `overrides` | Per-cell geometry, sorted by slot |
| `chunks` | Every consulted column and its revision, including neighbor geometry reads; none means absent terrain |

A known section without storage is air in layer 0. Geometry may be `fixed`,
`not-requested`, `per-cell` or `unavailable`. Position-dependent shapes include
neighbors, block actors and native offsets. `unavailable` means required facts
are missing. Selection uses the default interaction context. See the
[terrain WIT](../crates/mod-api/wit/extension.wit).
