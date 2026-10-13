struct Indirect {
    vertices: atomic<u32>,
    instances: atomic<u32>,
    first_vertex: atomic<u32>,
    first_instance: atomic<u32>,
}
@group(0) @binding(6) var<storage, read_write> positions: array<u32>;
@group(0) @binding(7) var<storage, read_write> indirect: Indirect;

fn nearest_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(depth, pixel, 0);
}

fn covered_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(depth, pixel, 0);
}

// Compact only visible, moving actor pixels; background pixels generate no sweep geometry.
@compute @workgroup_size(ACTOR_COMPACTION_SIDE, ACTOR_COMPACTION_SIDE)
fn compact(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if all(invocation == vec3<u32>(0u)) {
        atomicStore(&indirect.vertices, 12u);
    }
    let pixel = vec2<u32>(exposure.viewport.xy) + invocation.xy;
    let dimensions = textureDimensions(actor_velocity);
    if any(invocation.xy >= vec2<u32>(exposure.viewport.zw)) || any(pixel >= dimensions) {
        return;
    }
    let motion = object_motion(vec2<i32>(pixel));
    let velocity = motion.xy * exposure.viewport.zw * exposure.strength.x;
    if motion.z >= 0.5 && length(velocity) >= MIN_BLUR_SPEED {
        let index = atomicAdd(&indirect.instances, 1u);
        positions[index] = pixel.y * dimensions.x + pixel.x;
    }
}

struct SweepOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) motion: vec4<f32>,
}

// The six corners are the exact Minkowski sum of a pixel square and its exposure segment.
@vertex
fn sweep_vertex(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> SweepOutput {
    let dimensions = textureDimensions(actor_velocity);
    let index = positions[instance];
    let pixel = vec2<i32>(i32(index % dimensions.x), i32(index / dimensions.x));
    let motion = object_motion(pixel);
    var velocity = motion.xy * exposure.viewport.zw * exposure.strength.x;
    velocity *= min(1.0, MAX_BLUR_SPEED / length(velocity));
    let sign = select(vec2(-1.0), vec2(1.0), velocity >= vec2(0.0));
    let half_motion = velocity * 0.5;
    let corners = array<vec2<f32>, 6>(
        -half_motion - sign * 0.5,
        -half_motion + sign * vec2(0.5, -0.5),
        half_motion + sign * vec2(0.5, -0.5),
        half_motion + sign * 0.5,
        half_motion + sign * vec2(-0.5, 0.5),
        -half_motion + sign * vec2(-0.5, 0.5)
    );
    let triangle = vertex / 3u;
    let corner = select(select(triangle + 2u, triangle + 1u, vertex % 3u == 1u), 0u, vertex % 3u == 0u);
    let position = vec2<f32>(pixel) + vec2(0.5) + corners[corner];
    let z = covered_depth(pixel);
    var output: SweepOutput;
    output.position = vec4(position / vec2<f32>(dimensions) * vec2(2.0, -2.0) + vec2(-1.0, 1.0), z, 1.0);
    output.motion = vec4(motion.xy, 1.0, z);
    return output;
}

@fragment
fn sweep_fragment(input: SweepOutput) -> SweepFragment {
    var output: SweepFragment;
    output.motion = input.motion;
    output.depth = input.motion.w;
    return output;
}

struct SweepFragment {
    @location(0) motion: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// Seed from the nearest original sample so a nearer scene surface blocks actor trails.
@fragment
fn seed_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return nearest_depth(vec2<i32>(position.xy));
}
