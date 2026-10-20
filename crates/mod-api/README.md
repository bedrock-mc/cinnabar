# World drawing helpers

`mod-api` is the Rust SDK for personal mods. `drawing::api` exposes the generated
`cinnabar:world-draw/drawing` WIT interface. `drawing::shapes` builds its geometry
records directly, without another geometry schema or graphics dependency.

The builders return a list of independent local-coordinate polylines:

| Helper | Geometry |
| --- | --- |
| `box_wireframe` | Twelve independent edges from minimum and maximum bounds |
| `entity_bounding_box` | The same twelve edges from caller-supplied authorized bounds |
| `circle` | One closed XZ ring, starting at +X towards +Z |
| `sphere_wireframe` | Three closed great circles in XZ, XY and YZ order |
| `arrow` | A shaft and two independent head wings |
| `polyline` | An open path or closed line, preserving repeated points |
| `path_gradient` | An open path with linear RGBA interpolated by distance |
| `grid` | An XZ grid including its boundary, with caller-selected subdivisions |
| `chunk_border` | Twelve edges from caller-supplied local chunk bounds |

Closed rings use the contract's `closed` flag without repeating the first vertex.
The host computes cumulative line distances. UV and custom attributes default to
zero. Line width, joins and caps belong to each instance. Builders reject invalid
dimensions, non-finite values, out-of-range colors and shapes that exceed an empty
owner's geometry or callback budget. Limits come from `mod-api`; host validation
also accounts for the owner's other resources and instance transforms.
Uploading each geometry uses one import. Other drawing and render imports in the
same callback share that allowance.

For example, these statements can run during an authorized mod callback:

```rust,no_run
use mod_api::drawing::{api, instance, shapes};

let shape = shapes::circle(2.0, 32, [0.2, 0.6, 1.0, 1.0]).expect("valid circle");
let geometry = api::create_geometry(&shape[0]).expect("drawing geometry admitted");
let mut draw = instance(geometry, api::Anchor::World(api::WorldAnchor {
    dimension: 0,
    origin: (0.0, 64.0, 0.0),
}));
draw.width = 2.0;
draw.width_mode = api::WidthMode::Pixels;
let drawing = api::create(&draw).expect("drawing instance admitted");
```

Retain both handles in the mod's state and update the instance to move it, without
rebuilding geometry. Remove instances before their geometry. For callback-only
output, pass instances referencing retained geometry to `api::replace_frame_group`.
Groups expire on the next successful callback; explicit instances persist until
removed or revoked. The example traps on an admission failure so the host rolls
back the whole callback. Returning normally after a handled error still commits
earlier successful calls.

`instance` defaults to depth testing, premultiplied blending, white tint, identity
placement, world-unit width, bevel joins and butt caps. Helpers do not query
entities or blocks and do not grant observation, entity attachment or through-wall
drawing. Check `api::capabilities` and handle host errors. Availability of renderer
and anchor support is separate from building a geometry record.

The existing `render::draw`, `render::draw_lines` and position-box APIs remain
available with their existing output and callback lifetime. Their compatibility
paths currently remain in place:

| Existing primitive | Required behavior beyond the retained unlit contract |
| --- | --- |
| `draw_lines` | Existing feathered coverage, dash/dot phase and pixel expansion |
| Decals | Ground lift and procedural, animated coverage |
| Ribbons | Taper, head-to-tail fade and emissive coverage |
| Beams | Animated core, halo and intensity |
| Billboards | Camera-facing/upright placement and procedural patterns |
| Position boxes | Translucent faces plus the existing feathered edge coverage |

These paths must keep their compatibility implementation until retained presets
can preserve that output. A wireframe bounding box is a new shape convenience;
it does not replace the translucent position-box primitive.

Experience client parts continue to use `experience-sdk` and their separate
server-experience capability interfaces. The current drawing helpers target the
personal-mod Rust/WIT interface. A future TypeScript author package should generate
records, operations, limits and feature detection from the same contract after
component packaging is available. No TypeScript bridge is supplied here.

Player HUD consumers can use `player-state.read-revisions` to observe equipment and effects independently. Import only changed families with `read-equipment` and `read-effects`; effect ticks do not invalidate equipment. The records retain session/dimension authority and the existing player-state read budget. The original `read-snapshot` and `read-revision` APIs retain their exact-content semantics.
