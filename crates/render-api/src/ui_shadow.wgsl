// Signed distance to a rounded rectangle, followed by a Gaussian survival approximation.
fn ui_shadow_coverage(point: vec2<f32>, shape: vec4<f32>, pixel: f32) -> f32 {
    let q = abs(point) - shape.xy + vec2<f32>(shape.z);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - shape.z;
    if shape.w <= 0.0 {
        return clamp(0.5 - distance / max(pixel, 0.000001), 0.0, 1.0);
    }
    let x = distance / shape.w;
    let t = 1.0 / (1.0 + 0.2316419 * abs(x));
    let tail = 0.3989423 * exp(-0.5 * x * x) * t
        * (0.31938153 + t * (-0.35656378 + t * (1.7814779 + t * (-1.8212559 + t * 1.3302745))));
    return select(1.0 - tail, tail, x >= 0.0);
}
