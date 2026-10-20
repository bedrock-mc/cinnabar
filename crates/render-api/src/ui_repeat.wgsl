// Decode tile coordinates into source texels; z is zero for fragments another page owns.
fn ui_repeat_uv(coords: vec2<f32>, bounds: vec4<f32>, flags: u32, content: vec4<f32>, size: vec2<f32>) -> vec3<f32> {
    let axes = vec2<bool>((flags & STYLE_REPEAT_X) != 0u, (flags & STYLE_REPEAT_Y) != 0u);
    var uv = bounds.xy + select(coords, fract(coords), axes) * bounds.zw;
    if (flags & STYLE_BILINEAR) != 0u {
        let inset = min(abs(bounds.zw) * 0.5, vec2<f32>(0.5));
        let low = min(bounds.xy, bounds.xy + bounds.zw) + inset;
        let high = max(bounds.xy, bounds.xy + bounds.zw) - inset;
        uv = clamp(uv, low, high);
    }
    if (flags & STYLE_REPEAT_PAGE) != 0u {
        let lo = content.xy;
        let hi = content.zw;
        if any(size <= vec2<f32>(0.0)) || (uv.x < lo.x && lo.x > 0.0) || (uv.y < lo.y && lo.y > 0.0) || (uv.x >= hi.x && hi.x < size.x) || (uv.y >= hi.y && hi.y < size.y) {
            return vec3<f32>(0.0);
        }
        uv = clamp(uv, vec2<f32>(0.5), max(vec2<f32>(0.5), size - vec2<f32>(0.5)));
    }
    return vec3<f32>(uv, 1.0);
}
