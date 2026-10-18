# Entity masks

The experimental `cinnabar:world-draw/masks@0.1.0` interface retains selections across
successful callbacks. `create(test)` defaults to scene-depth-tested coverage; `create(always)`
also requires `render_through_walls`. Both require `entity_highlight`. Each component owns its
tokens and quota. Session changes, transfers, reloads, traps and revocation clear all masks.

Resolve an authorized runtime ID with `entity`, then atomically replace membership with
`select`. This lookup requires `entity_observe` and `render_through_walls` because the current
candidate source has no occlusion-filtered observations. It returns no pose or mesh data.
An empty selection clears coverage. `remove` releases a mask. Foreign, removed or replaced
actor tokens return `stale-handle`. The numeric limits live in `mod_api::drawing`.

Coverage is the union of the selected actors' admitted body and equipment draws. Shared
edges have no internal boundary. The mask uses the scene's actual vertex, skinning, artwork,
culling and alpha-cutout paths. Blended materials contribute their clamped alpha, combined
by maximum coverage; non-blended materials contribute opaque surviving samples. Multisample
cutouts retain the scene's alpha-to-coverage rule. Dissolve depth-only companions contribute
no colour coverage. No unadmitted attachment or off-screen mesh is enumerated.

Attachments are allocated only for nonempty selections, per view, and retired on removal.
Accounting includes physical dimensions, sample count and resolve targets. A frame exceeding
the aggregate attachment budget disables all masks for that frame. Unavailable pipelines,
artwork or changed geometry fail closed, and every retained target is cleared before reuse.
