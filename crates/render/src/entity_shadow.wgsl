#import bevy_render::view::View

// Mirrors render_model::EntityShadowParams.
struct ShadowParams {
    colour: vec4<f32>,
    // Top y, bottom y, then the side apothem as z + w * y, in caster radii.
    volume: vec4<f32>,
    normals: array<vec4<f32>, 7>,
    sides: vec4<u32>,
}

@group(0) @binding(0) var<uniform> view: View;
#ifdef MULTISAMPLED
@group(0) @binding(1) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var scene_depth: texture_depth_2d;
#endif
// Feet in xyz, radius in w.
@group(0) @binding(3) var<storage, read> casters: array<vec4<f32>>;
@group(0) @binding(4) var<uniform> params: ShadowParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) caster: u32,
}

@vertex
fn shadow_vertex(
    @location(0) corner: vec3<f32>,
    @builtin(instance_index) instance: u32,
) -> VertexOutput {
    let caster = casters[instance];
    var out: VertexOutput;
    // An eye inside this convex volume has no visible entry face to mark a receiver.
    if inside_volume((view.world_position - caster.xyz) / caster.w) {
        out.position = vec4(0.0, 0.0, 2.0, 1.0);
    } else {
        out.position = view.clip_from_world * vec4(caster.xyz + corner * caster.w, 1.0);
    }
    out.caster = instance;
    return out;
}

fn inside_volume(local: vec3<f32>) -> bool {
    if local.y > params.volume.x || local.y < params.volume.y {
        return false;
    }
    let apothem = params.volume.z + params.volume.w * local.y;
    for (var side = 0u; side < params.sides.x; side += 1u) {
        let pair = params.normals[side / 2u];
        let normal = select(pair.xy, pair.zw, side % 2u == 1u);
        if dot(local.xz, normal) > apothem {
            return false;
        }
    }
    return true;
}

@fragment
fn shadow_fragment(in: VertexOutput,
#ifdef MULTISAMPLED
    @builtin(sample_index) sample: u32,
#endif
) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(in.position.xy));
#ifdef MULTISAMPLED
    let depth = textureLoad(scene_depth, pixel, i32(sample));
#else
    let depth = textureLoad(scene_depth, pixel, 0);
#endif
    // Reverse depth: zero is the sky.
    if depth <= 0.0 {
        discard;
    }
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    let clip = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let view_point = view.view_from_clip * clip;
    // Camera-relative, so distant coordinates keep the precision the thin top needs.
    let from_eye = (view.world_from_view * vec4(view_point.xyz / view_point.w, 0.0)).xyz;
    let caster = casters[in.caster];
    if !inside_volume((from_eye - (caster.xyz - view.world_position)) / caster.w) {
        discard;
    }
    return params.colour;
}
