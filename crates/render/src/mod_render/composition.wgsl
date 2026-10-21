// Host-owned display-space selection composition. Parameters are validated before publication.
fn display_from_linear(value: vec3<f32>) -> vec3<f32> {
    let positive = max(value, vec3<f32>(0.0));
    return select(1.055 * pow(positive, vec3<f32>(1.0 / 2.4)) - 0.055, positive * 12.92, positive <= vec3<f32>(0.0031308));
}
fn linear_from_display(value: vec3<f32>) -> vec3<f32> {
    return select(pow((value + 0.055) / 1.055, vec3<f32>(2.4)), value / 12.92, value <= vec3<f32>(0.04045));
}
fn effect(uv: vec2<f32>) -> vec3<f32> {
    let coverage = mask(0u, uv);
    let o = frame.resolution.zw * param(8u);
    let near = max(max(mask(0u, uv + vec2<f32>(o.x, 0.0)), mask(0u, uv - vec2<f32>(o.x, 0.0))),
                   max(mask(0u, uv + vec2<f32>(0.0, o.y)), mask(0u, uv - vec2<f32>(0.0, o.y))));
    let diagonal = max(max(mask(0u, uv + o), mask(0u, uv - o)),
                       max(mask(0u, uv + vec2<f32>(o.x, -o.y)), mask(0u, uv + vec2<f32>(-o.x, o.y))));
    let edge = clamp(max(near, diagonal) - coverage, 0.0, 1.0) * param(3u);
    let halo = clamp(mask(1u, uv) - coverage, 0.0, 1.0) * param(7u);
    let original = scene(uv);
    let fill = coverage * param(12u);
    if (edge == 0.0 && halo == 0.0 && fill == 0.0) { return original; }
    let with_fill = mix(display_from_linear(original), vec3<f32>(param(9u), param(10u), param(11u)), fill);
    let with_glow = mix(with_fill, vec3<f32>(param(4u), param(5u), param(6u)), halo);
    return linear_from_display(mix(with_glow, vec3<f32>(param(0u), param(1u), param(2u)), edge));
}
