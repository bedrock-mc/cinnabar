fn shade(input: MaterialInput) -> Surface {
    let gradient = clamp(input.uv.x, 0.0, 1.0) * clamp(input.params0.w, 0.0, 1.0);
    var color = mix(input.color.rgb, input.params0.rgb * input.color.rgb, gradient);
    if input.params1.x != 0.0 || input.params1.y != 0.0 {
        let hue = fract(phase() * input.params1.x + input.uv.x * input.params1.y);
        let rainbow = clamp(abs(fract(vec3<f32>(hue) + vec3<f32>(0.0, 0.6666667, 0.3333333)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
        color *= rainbow;
    }
    let pulse = mix(clamp(input.params1.w, 0.0, 1.0), 1.0, 0.5 + 0.5*cos(fract(phase()*input.params1.z)*6.283185307));
    var coverage = input.color.a * pulse;
    if input.params2.x > 0.0 {
        let dash = fract(input.distance / input.params2.x - phase()*input.params2.z);
        coverage *= select(0.0, 1.0, dash < clamp(input.params2.y, 0.0, 1.0));
    }
    return Surface(color, coverage, color * max(input.params2.w, 0.0));
}
