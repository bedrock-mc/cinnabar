# Drawing materials

Retained drawings use vertex linear RGBA and UV, an instance tint, and a fixed
`materials::PARAMS` parameter block. `material: None` selects the shared material.
The Rust SDK supplies `materials::Preset::{rainbow, pulse, gradient, dashed}`;
pass `preset.0.to_vec()` to the generated WIT instance's `params`. An empty block
is solid vertex colour. Parameter updates replace only instance data.

The shared gradient multiplies RGB from white to the requested colour across UV.x.
Dashes repeat over local polyline distance; transforming a line also transforms
its dash length. Rainbow, pulse and dash scrolling run in the fragment shader.
They do not require geometry updates or guest callbacks each frame.

The host clock starts at zero for a new connection and advances with real time,
including while gameplay is paused. `phase()` is in [0, 1), wrapping after
`materials::ANIMATION_WRAP_SECONDS`. SDK periods are rounded to whole cycles per
wrap. Custom effects must also use whole cycle counts for continuous wrapping.

## Custom procedural fragments

`create-material` requires both `world_draw` and `render_material`. The returned
handle belongs to that mod incarnation and session. All operations are
transactional. Referenced materials reject removal; removed materials remain
charged until renderer references retire. Validation attempts, retained material
count and native pipeline compilation are bounded by the `mod-api` limits.
Creation validates source; native compilation can remain pending or fail, in which
case that material's instances skip drawing.

```wgsl
fn shade(input: MaterialInput) -> Surface {
    let wave = 0.5 + 0.5 * cos(phase() * 6.283185307);
    return Surface(input.color.rgb, input.color.a, vec3<f32>(wave));
}
```

`MaterialInput` supplies `uv`, `local_position`, `color` (vertex RGBA times instance
tint), cumulative local line `distance`, and four flat `params0`–`params3` vectors.
`param(input, index)` reads a scalar from that block. `Surface` contains linear
RGB `color`, scalar `coverage`, and linear RGB `emission`. The host bounds colour
and coverage to [0, 1], emission to `MAX_MATERIAL_EMISSION`, and premultiplies output.

Materials cannot declare resources, module variables, overrides or entry points.
There is no texture sampling, vertex output, depth output, storage writing,
recursion or looping. The validator counts expanded call costs. Instance depth
and blend remain host-owned; a material grants no through-wall authority.

World emission reaches the world HDR pipeline and participates in Enhanced bloom
when enabled. It does not promise a halo in every render mode. Halos independent
of render mode belong to the separate mask path. Existing post-pass effects keep
their current timing and shader contract.
