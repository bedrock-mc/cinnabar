// Signed distance to a rounded rectangle, followed by a Gaussian survival approximation.
fn ui_shadow_coverage(point: vec2<f32>, shape: vec4<f32>, pixel: f32) -> f32 {
    let q = abs(point) - shape.xy + vec2<f32>(shape.z);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - shape.z;
    if shape.w <= 0.0 {
        return clamp(0.5 - distance / max(pixel, 0.000001), 0.0, 1.0);
    }
    let footprint = max(pixel, 0.000001);
    let upper = ui_shadow_integral(-distance + footprint * 0.5, shape.w);
    let lower = ui_shadow_integral(-distance - footprint * 0.5, shape.w);
    return clamp((upper - lower) / footprint, 0.0, 1.0);
}

// Integrate the Gaussian CDF across a physical pixel, including positive subpixel blur.
fn ui_shadow_integral(position: f32, sigma: f32) -> f32 {
    let x = position / sigma;
    let t = 1.0 / (1.0 + 0.2316419 * abs(x));
    let tail = 0.3989423 * exp(-0.5 * x * x) * t
        * (0.31938153 + t * (-0.35656378 + t * (1.7814779 + t * (-1.8212559 + t * 1.3302745))));
    let cdf = select(tail, 1.0 - tail, x >= 0.0);
    return position * cdf + sigma * 0.3989423 * exp(-0.5 * x * x);
}
