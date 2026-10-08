struct Exposure {
    previous_clip_from_clip: mat4x4<f32>,
    strength: vec4<f32>,
    viewport: vec4<f32>,
}
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var depth: DEPTH_TEXTURE_TYPE;
@group(0) @binding(3) var<uniform> exposure: Exposure;

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
    let z = textureLoad(depth, pixel, 0);
    // Reverse-Z sky pixels reconstruct a direction at infinity, excluding camera translation.
    let previous = exposure.previous_clip_from_clip * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
    if previous.w <= 0.00001 {
        return base;
    }
    let previous_uv = previous.xy / previous.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    var velocity = (uv - previous_uv) * exposure.viewport.zw * exposure.strength.x;
    let speed = length(velocity);
    if speed < 0.5 {
        return base;
    }
    velocity *= min(1.0, 96.0 / speed);
    let dimensions = vec2<f32>(textureDimensions(scene));
    let lower = exposure.viewport.xy + vec2<f32>(0.5);
    let upper = exposure.viewport.xy + exposure.viewport.zw - vec2<f32>(0.5);
    let count = u32(exposure.strength.y);
    var color = vec4<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let step = f32(i) / f32(count - 1u) - 0.5;
        let sample_position = clamp(position.xy + velocity * step, lower, upper);
        color += textureSampleLevel(scene, scene_sampler, sample_position / dimensions, 0.0);
    }
    return color / f32(count);
}
