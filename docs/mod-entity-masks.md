# Entity masks

Personal mods use the user's approved permissions in both local and remote worlds.
Server negotiation and server grants do not control these capabilities.

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
Accounting includes physical dimensions, sample count, resolve targets and private companion
depth. A frame exceeding the aggregate attachment budget disables all masks for that frame. Unavailable pipelines,
artwork or changed geometry fail closed, and every retained target is cleared before reuse.

## Outline and glow

`set-style` accepts optional host-owned appearance. Colours are sRGB display values in
`[0, 1]`, including alpha. Radii are physical pixels: the outline expands in eight directions
up to `MAX_MASK_OUTLINE_RADIUS`; the halo uses one of `MASK_BLUR_LEVELS` downsample levels,
bounded by `MAX_MASK_GLOW_RADIUS`. Alpha zero disables a layer; `none` removes styling.
Intersecting selected actors share one exterior outline. Use separate masks for separate colours.

The host shares coverage and the blur pyramid for identical permitted unions with the same
depth policy. All effects using that union reuse the pyramid. The halo runs at progressively
smaller resolutions. If its allocation or aggregate budget fails, the crisp outline remains.
No full-resolution blur is allocated per component.

Composition runs after grading and authored mod post effects, before UI. It mixes the already
graded scene and the requested colours in display space, then returns linear output to the
target. This selection halo is independent of world HDR emission and Enhanced bloom. It does
not change the existing `bloom()` helper, post-pass timing, parameters or authored pass order.

`register-effect` also requires `render_post`. Its explicit mask list binds only this owner's
live selections, exposed through `mask(index, uv)`; an undeclared index reads zero. Effects
cannot declare graphics resources or read a global entity-ID buffer. Missing coverage skips
the complete effect. Remove referencing effects through `render.remove-pass` before removing
a mask, or removal returns `in-use`. Host styling requires no arbitrary shader permission.

To inspect the original actor fixture, run the filtered
`mod_render::composition::tests::outlines_and_halos_render_from_actor_coverage_in_ldr_and_hdr_targets`
render test with `CINNABAR_SELECTION_CAPTURE_DIR` set to a scratch directory. It writes the
ordinary frame, coverage, outlined and glowing frames, plus HDR and MSAA variants. In a local
world, a component can resolve an admitted actor, select it and call `set-style` to see the
same composition on the world mesh.
