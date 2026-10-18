// Comparisons reject NaN and negative values before bounding guest arithmetic.
fn bounded(value: f32, upper: f32) -> f32 {
    if value >= 0.0 { return min(value, upper); }
    return 0.0;
}
@fragment fn material_fragment(input: MaterialInput) -> @location(0) vec4<f32> {
    let surface = shade(input);
    let color = vec3<f32>(bounded(surface.color.x, 1.0), bounded(surface.color.y, 1.0), bounded(surface.color.z, 1.0));
    let emission = vec3<f32>(bounded(surface.emission.x, MAX_EMISSION), bounded(surface.emission.y, MAX_EMISSION), bounded(surface.emission.z, MAX_EMISSION));
    let alpha = bounded(surface.coverage, 1.0);
    return vec4<f32>((color + emission) * alpha, alpha);
}
