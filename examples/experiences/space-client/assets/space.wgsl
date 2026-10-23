fn shade(input: MaterialInput) -> Surface {
    if input.params0.x > 0.5 {
        let cell = floor(input.uv * 180.0);
        let noise = fract(sin(dot(cell, vec2<f32>(127.1, 311.7))) * 43758.5453);
        let star = select(0.0, 0.8, noise > 0.993);
        let color = vec3<f32>(0.002, 0.004, 0.015) + vec3<f32>(star);
        return Surface(color, 1.0, color * 0.3);
    }
    // The common translucent pipeline keeps depth writes off; hide this convex sphere's far side.
    if dot(input.local_position, input.params0.yzw - input.local_position) <= 0.0 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let light = 0.18 + 0.82 * max(dot(normalize(input.local_position), normalize(vec3<f32>(-0.5, 0.8, 1.0))), 0.0);
    let color = input.color.rgb * light;
    return Surface(color, 1.0, color * 0.1);
}
