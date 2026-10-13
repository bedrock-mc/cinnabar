struct Exposure {
    previous_clip_from_clip: mat4x4<f32>,
    strength: vec4<f32>,
    viewport: vec4<f32>,
}
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<uniform> exposure: Exposure;
@group(0) @binding(4) var actor_velocity: texture_2d<f32>;
@group(0) @binding(5) var swept_velocity: texture_2d<f32>;

const MIN_BLUR_SPEED: f32 = 0.5;
const MAX_BLUR_SPEED: f32 = 96.0;

fn scene_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(depth, pixel, 0);
}

fn nearer_surface_in_footprint(position: vec2<f32>, z: f32) -> bool {
    let texel = position - vec2(0.5);
    let corner = vec2<i32>(floor(texel));
    let weight = fract(texel);
    return scene_depth(corner) > z
        || (weight.x > 0.0 && scene_depth(corner + vec2(1, 0)) > z)
        || (weight.y > 0.0 && scene_depth(corner + vec2(0, 1)) > z)
        || (weight.x > 0.0 && weight.y > 0.0 && scene_depth(corner + vec2(1, 1)) > z);
}

fn swept_motion(pixel: vec2<i32>) -> vec4<f32> {
    if exposure.strength.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    return textureLoad(swept_velocity, pixel, 0);
}

// Multisampled views resolve actor motion across covered samples, including silhouette cutouts.
// Mixed actor/world pixels retain the covered actor's motion instead of smearing its silhouette.
fn object_motion(pixel: vec2<i32>) -> vec4<f32> {
    if exposure.strength.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    return textureLoad(actor_velocity, pixel, 0);
}

// A linear scene tap can include up to four texels, each with its own actor coverage.
fn stationary_actor(pixel: vec2<i32>) -> bool {
    let actor = object_motion(pixel);
    return actor.z >= 0.5 && length(actor.xy * exposure.viewport.zw * exposure.strength.x) < MIN_BLUR_SPEED;
}

fn stationary_actor_in_footprint(position: vec2<f32>) -> bool {
    if exposure.strength.z <= 0.0 {
        return false;
    }
    let texel = position - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(texel));
    let weight = fract(texel);
    // Zero-weight neighbours neither contribute colour nor need an in-bounds motion lookup.
    return stationary_actor(corner)
        || (weight.x > 0.0 && stationary_actor(corner + vec2<i32>(1, 0)))
        || (weight.y > 0.0 && stationary_actor(corner + vec2<i32>(0, 1)))
        || (weight.x > 0.0 && weight.y > 0.0 && stationary_actor(corner + vec2<i32>(1, 1)));
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let base = textureLoad(scene, pixel, 0);
    if exposure.strength.x <= 0.0 {
        return base;
    }
    let uv = (position.xy - exposure.viewport.xy) / exposure.viewport.zw;
    let surface_actor = object_motion(pixel);
    let actor = select(swept_motion(pixel), surface_actor, surface_actor.z >= 0.5);
    var velocity = actor.xy * exposure.viewport.zw * exposure.strength.x;
    if actor.z < 0.5 {
        let z = textureLoad(depth, pixel, 0);
        // Reverse-Z sky pixels reconstruct a direction at infinity, excluding camera translation.
        let previous = exposure.previous_clip_from_clip * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
        if previous.w <= 0.00001 {
            return base;
        }
        let previous_uv = previous.xy / previous.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
        velocity = (uv - previous_uv) * exposure.viewport.zw * exposure.strength.x;
    }
    let speed = length(velocity);
    if speed < MIN_BLUR_SPEED {
        return base;
    }
    velocity *= min(1.0, MAX_BLUR_SPEED / speed);
    let dimensions = vec2<f32>(textureDimensions(scene));
    let lower = exposure.viewport.xy + vec2<f32>(0.5);
    let upper = exposure.viewport.xy + exposure.viewport.zw - vec2<f32>(0.5);
    let count = u32(exposure.strength.y);
    var color = vec4<f32>(0.0);
    var accepted = 0u;
    for (var i = 0u; i < count; i++) {
        let step = f32(i) / f32(count - 1u) - 0.5;
        let sample_position = clamp(position.xy + velocity * step, lower, upper);
        // Protect screen-stationary actors while allowing moving silhouettes to blur.
        if stationary_actor_in_footprint(sample_position) {
            continue;
        }
        // A moving actor cannot gather a nearer occluder's colour into its trail.
        let actor_depth = select(actor.w, scene_depth(pixel), surface_actor.z >= 0.5);
        if actor.z >= 0.5 && nearer_surface_in_footprint(sample_position, actor_depth) {
            color += base;
        } else {
            color += textureSampleLevel(scene, scene_sampler, sample_position / dimensions, 0.0);
        }
        accepted += 1u;
    }
    if accepted == 0u {
        return base;
    }
    return color / f32(accepted);
}
