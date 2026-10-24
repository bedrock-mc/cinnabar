# Entity masks

[Personal mods](mods/personal.md) use `cinnabar:world-draw/masks@0.1.0` to retain
actor selections across successful callbacks. User grants apply in local and
remote worlds. Server grants do not authorize this interface.

| Operation | Required grants/behavior |
| --- | --- |
| `create(test)` | `entity_highlight`; scene-depth-tested coverage |
| `create(always)` | Also `render_through_walls` |
| `entity` | `entity_observe` and `render_through_walls`; resolves an authorized runtime ID without exposing pose/mesh data |
| `select` | Atomically replaces membership; empty selection clears coverage |
| `set-style` | Host outline/glow; no arbitrary shader permission |
| `register-effect` | Also `render_post`; binds only this owner's explicit live masks |
| `remove` | Releases the mask; remove referencing render passes first or get `in-use` |

Each component owns its handles and quota. Foreign, removed and replaced actors
return `stale-handle`. Session changes, transfers, reloads, traps and revocation
clear masks. Numeric bounds live in [`mod_api::drawing`](../crates/mod-api/src/drawing.rs).

Coverage is the union of admitted body/equipment draws, preserving alpha cutouts
and blended-material alpha. Shared edges have no internal boundary. Missing
pipelines, artwork or geometry fail closed. No unadmitted attachment is enumerated.
Exceeding the aggregate attachment budget disables masks for that frame.

Style colours are sRGB `[0, 1]`, including alpha; radii are physical pixels.
Alpha zero disables a layer; `none` removes style. One mask gives intersecting
actors one exterior outline; use separate masks for different colours. Identical
permitted unions share coverage and blur work. Failed glow allocation retains the
crisp outline. Composition follows grading/post effects and precedes UI; it is
independent of world HDR emission and bloom.

Effects read declared selections with `mask(index, uv)`; undeclared indices read
zero. Missing coverage skips the whole effect. No graphics resources or global
entity-ID buffer are exposed. See the [WIT](../crates/mod-api/wit/deps/world-draw/drawing.wit)
and [host checks](../crates/mod-host/src/masks.rs).
