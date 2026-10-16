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

@group(0) @binding(5) var motion_tiles: texture_2d<f32>;
@group(0) @binding(6) var resolved_motion: texture_2d<f32>;
@group(0) @binding(7) var resolved_depth: texture_2d<f32>;
@group(0) @binding(8) var motion_rows: texture_2d<f32>;

const TILE_SIZE: i32 = MOTION_TILE_SIZE;
const MAX_BLUR_SPEED: f32 = f32(TILE_SIZE * 2);
const MIN_BLUR_SPEED: f32 = 0.5;

// Multisampled views resolve actor motion across covered samples, including silhouette cutouts.
// Mixed actor/world pixels retain the covered actor's motion instead of smearing its silhouette.
fn source_motion(pixel: vec2<i32>) -> vec4<f32> {
    if exposure.strength.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    return textureLoad(actor_velocity, pixel, 0);
}

// Clamp actor exposure once, using the same bound for tile selection and sample reach.
fn actor_velocity_pixels(motion: vec4<f32>) -> vec2<f32> {
    let velocity = motion.xy * exposure.viewport.zw * exposure.strength.x;
    return velocity * min(1.0, MAX_BLUR_SPEED / max(length(velocity), MIN_BLUR_SPEED));
}

// The nearest reverse-Z surface owns occlusion, including partially covered MSAA pixels.
fn source_scene_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(depth, pixel, 0);
}

// Only covered samples provide an actor's depth; neighbouring foreground samples do not.
fn source_actor_depth(pixel: vec2<i32>) -> f32 {
    let covered_depth = textureLoad(depth, pixel, 0);
    return covered_depth;
}

struct ResolvedSurface {
    @location(0) motion: vec4<f32>,
    @location(1) depths: vec2<f32>,
}

// Resolve coverage and both depth owners once, before any tile or exposure taps read them.
@fragment
fn resolve_fragment(@builtin(position) position: vec4<f32>) -> ResolvedSurface {
    let pixel = vec2<i32>(position.xy);
    let motion = source_motion(pixel);
    var covered_depth = 0.0;
    if motion.z >= 0.5 {
        covered_depth = source_actor_depth(pixel);
    }
    return ResolvedSurface(motion, vec2<f32>(source_scene_depth(pixel), covered_depth));
}

// Repeated gathers read exact resolved values instead of resolving MSAA at every tap.
fn object_motion(pixel: vec2<i32>) -> vec4<f32> {
    if exposure.strength.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    return textureLoad(resolved_motion, pixel, 0);
}

// Keep scene occlusion and actor-covered depth separate at mixed MSAA edges.
fn scene_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(resolved_depth, pixel, 0).x;
}

// Reads only the depth of samples covered by an actor.
fn actor_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(resolved_depth, pixel, 0).y;
}

// Each row invocation reads at most one tile edge, retaining full precision until selection ends.
@fragment
fn row_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let origin = vec2<i32>(i32(position.x) * TILE_SIZE, i32(position.y));
    let lower = vec2<i32>(exposure.viewport.xy);
    let upper = min(lower + vec2<i32>(exposure.viewport.zw), vec2<i32>(textureDimensions(resolved_motion)));
    var best = vec2<f32>(0.0);
    for (var x = 0; x < TILE_SIZE; x++) {
        let pixel = origin + vec2<i32>(x, 0);
        if any(pixel < lower) || any(pixel >= upper) {
            continue;
        }
        let motion = object_motion(pixel);
        let velocity = actor_velocity_pixels(motion);
        if motion.z >= 0.5 && dot(velocity, velocity) > dot(best, best) {
            best = velocity;
        }
    }
    return vec4<f32>(best, 0.0, 0.0);
}

// Row winners retain the original row-major tie order without a square serial scan.
@fragment
fn tile_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let origin = vec2<i32>(i32(position.x), i32(position.y) * TILE_SIZE);
    let height = i32(textureDimensions(motion_rows).y);
    var best = vec2<f32>(0.0);
    for (var y = 0; y < TILE_SIZE && origin.y + y < height; y++) {
        let velocity = textureLoad(motion_rows, origin + vec2<i32>(0, y), 0).xy;
        if dot(velocity, velocity) > dot(best, best) {
            best = velocity;
        }
    }
    return vec4<f32>(best, 0.0, 0.0);
}

// Half the maximum exposure fits in one tile, so only the surrounding nine tiles can reach us.
fn neighbouring_velocity(pixel: vec2<i32>) -> vec2<f32> {
    if exposure.strength.z <= 0.0 {
        return vec2<f32>(0.0);
    }
    let tile = pixel / TILE_SIZE;
    let size = vec2<i32>(textureDimensions(motion_tiles));
    var best = vec2<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let candidate = tile + vec2<i32>(x, y);
            if any(candidate < vec2<i32>(0)) || any(candidate >= size) {
                continue;
            }
            let velocity = textureLoad(motion_tiles, candidate, 0).xy;
            if dot(velocity, velocity) > dot(best, best) {
                best = velocity;
            }
        }
    }
    return best;
}

// A tile supplies a search direction; each real actor texel still proves its reach and visibility.
fn actor_reaches(source: vec2<i32>, destination: vec2<f32>, z: f32) -> bool {
    let motion = object_motion(source);
    let velocity = actor_velocity_pixels(motion);
    let speed_squared = dot(velocity, velocity);
    let covered_depth = actor_depth(source);
    // Resolved colour can contain nearer non-actor samples that must stay at their source.
    if motion.z < 0.5 || speed_squared < MIN_BLUR_SPEED * MIN_BLUR_SPEED
        || covered_depth < z || scene_depth(source) > covered_depth {
        return false;
    }
    let offset = destination - (vec2<f32>(source) + vec2<f32>(0.5));
    let time = clamp(dot(offset, velocity) / speed_squared, -0.5, 0.5);
    return length(offset - velocity * time) <= 1.0;
}

// Reconstruct bilinear actor colour without pulling unrelated background or foreground colours.
fn spreading_sample(sample_position: vec2<f32>, destination: vec2<f32>, z: f32, base: vec4<f32>) -> vec4<f32> {
    let texel = sample_position - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(texel));
    let weight = fract(texel);
    var color = base;
    for (var y = 0; y <= 1; y++) {
        for (var x = 0; x <= 1; x++) {
            let amount = select(1.0 - weight.x, weight.x, x == 1) * select(1.0 - weight.y, weight.y, y == 1);
            if amount > 0.0 && actor_reaches(corner + vec2<i32>(x, y), destination, z) {
                color += (textureLoad(scene, corner + vec2<i32>(x, y), 0) - base) * amount;
            }
        }
    }
    return color;
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

// A continuous moving surface may slope toward the camera without becoming its own occluder.
fn nearer_occluder(pixel: vec2<i32>, z: f32, velocity: vec2<f32>) -> bool {
    let nearest = scene_depth(pixel);
    if nearest <= z {
        return false;
    }
    let motion = object_motion(pixel);
    return motion.z < 0.5 || nearest > actor_depth(pixel)
        || length(actor_velocity_pixels(motion) - velocity) >= MIN_BLUR_SPEED;
}

// Every contributing texel must reject nearer occluders, including fractional edge taps.
fn nearer_occluder_in_footprint(position: vec2<f32>, z: f32, velocity: vec2<f32>) -> bool {
    let texel = position - vec2(0.5);
    let corner = vec2<i32>(floor(texel));
    let weight = fract(texel);
    return nearer_occluder(corner, z, velocity)
        || (weight.x > 0.0 && nearer_occluder(corner + vec2(1, 0), z, velocity))
        || (weight.y > 0.0 && nearer_occluder(corner + vec2(0, 1), z, velocity))
        || (weight.x > 0.0 && weight.y > 0.0 && nearer_occluder(corner + vec2(1, 1), z, velocity));
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// Gather the destination surface's own motion while protecting stationary actor samples.
fn surface_gather(position: vec2<f32>, velocity: vec2<f32>, base: vec4<f32>) -> vec4<f32> {
    let speed = length(velocity);
    if speed < MIN_BLUR_SPEED {
        return base;
    }
    let bounded = velocity * min(1.0, MAX_BLUR_SPEED / speed);
    let dimensions = vec2<f32>(textureDimensions(scene));
    let lower = exposure.viewport.xy + vec2<f32>(0.5);
    let upper = exposure.viewport.xy + exposure.viewport.zw - vec2<f32>(0.5);
    let count = u32(exposure.strength.y);
    var color = vec4<f32>(0.0);
    var accepted = 0u;
    for (var i = 0u; i < count; i++) {
        let step = f32(i) / f32(count - 1u) - 0.5;
        let sample_position = clamp(position + bounded * step, lower, upper);
        if stationary_actor_in_footprint(sample_position) {
            continue;
        }
        if object_motion(vec2<i32>(position)).z < 0.5 {
            // Keep the world's camera blur while actor exposure follows its own motion and depth.
            let texel = sample_position - vec2<f32>(0.5);
            let corner = vec2<i32>(floor(texel));
            let weight = fract(texel);
            var tap = base;
            for (var y = 0; y <= 1; y++) {
                for (var x = 0; x <= 1; x++) {
                    let source = corner + vec2<i32>(x, y);
                    let amount = select(1.0 - weight.x, weight.x, x == 1) * select(1.0 - weight.y, weight.y, y == 1);
                    if amount > 0.0 && object_motion(source).z < 0.5 {
                        tap += (textureLoad(scene, source, 0) - base) * amount;
                    }
                }
            }
            color += tap;
        } else {
            if nearer_occluder_in_footprint(sample_position, actor_depth(vec2<i32>(position)), bounded) {
                color += base;
            } else {
                color += textureSampleLevel(scene, scene_sampler, sample_position / dimensions, 0.0);
            }
        }
        accepted += 1u;
    }
    if accepted == 0u {
        return base;
    }
    return color / f32(accepted);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let base = textureLoad(scene, pixel, 0);
    if exposure.strength.x <= 0.0 {
        return base;
    }
    let uv = (position.xy - exposure.viewport.xy) / exposure.viewport.zw;
    let actor = object_motion(pixel);
    let z = scene_depth(pixel);
    var velocity = actor_velocity_pixels(actor);
    if actor.z < 0.5 {
        // Reverse-Z sky reconstructs a direction at infinity, excluding camera translation.
        let previous = exposure.previous_clip_from_clip * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
        if previous.w > 0.00001 {
            let previous_uv = previous.xy / previous.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
            velocity = (uv - previous_uv) * exposure.viewport.zw * exposure.strength.x;
        }
    }
    // Followed actors and nearer non-actor MSAA samples retain their resolved colour.
    if actor.z >= 0.5 && (length(velocity) < MIN_BLUR_SPEED || z > actor_depth(pixel)) {
        return base;
    }
    let background = surface_gather(position.xy, velocity, base);
    let neighbour = neighbouring_velocity(pixel);
    if actor.z >= 0.5 || length(neighbour) < MIN_BLUR_SPEED {
        return background;
    }
    let lower = exposure.viewport.xy + vec2<f32>(0.5);
    let upper = exposure.viewport.xy + exposure.viewport.zw - vec2<f32>(0.5);
    let count = u32(exposure.strength.y);
    var color = vec4<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let step = f32(i) / f32(count - 1u) - 0.5;
        let sample_position = clamp(position.xy + neighbour * step, lower, upper);
        color += spreading_sample(sample_position, position.xy, z, background);
    }
    return color / f32(count);
}
