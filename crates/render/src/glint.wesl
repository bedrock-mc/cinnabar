// Shared glint coordinates: normalize padded atlas UVs before either rotation.
fn glint_layer_uvs(uv: vec2<f32>, origin: vec2<f32>, scale: vec2<f32>, scroll: vec2<f32>, item_extent: f32) -> array<vec2<f32>, 2> {
    let item = item_extent > 0.0;
    let extent = select(vec2(1.0), vec2(item_extent), item);
    let center = extent * 0.5;
    let normalized_uv = (uv - select(vec2(0.0), origin, item)) * select(vec2(1.0), scale, item) - center;
    let rotation_scale = select(scale, vec2(1.0), item);
    let first = (vec2(0.9396926 * normalized_uv.x + 0.34202015 * normalized_uv.y, -0.34202015 * normalized_uv.x + 0.9396926 * normalized_uv.y) + center) * rotation_scale + vec2(scroll.x, 0.0);
    let second = (vec2(0.17364818 * normalized_uv.x - 0.9848077 * normalized_uv.y, 0.9848077 * normalized_uv.x + 0.17364818 * normalized_uv.y) + center) * rotation_scale + vec2(scroll.y, 0.0);
    return array<vec2<f32>, 2>(first, second);
}
