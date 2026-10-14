// The Hi-Z occlusion test shared by terrain and distant terrain culling. The including shader
// declares `view` with `clip_from_rel` (clip from camera-relative blocks), `viewport` and
// `depth` (width, height, pyramid mips, Hi-Z enabled), and binds the pyramid as `hiz`.

// Visible unless every point of the camera-relative box lies behind the farthest pyramid depth
// it covers. `strict` also keeps a box reaching past the viewport, whose hidden part this depth
// never saw.
fn hiz_box_test(low: vec3<f32>, high: vec3<f32>, strict: bool) -> bool {
    if (view.depth.w == 0u) {
        return true;
    }
    var ndc_min = vec2(1.0e30);
    var ndc_max = vec2(-1.0e30);
    var nearest = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
        let clip = view.clip_from_rel * vec4(select(low, high, pick), 1.0);
        if (!(clip.w > 1.0e-4)) {
            return true;
        }
        let ndc = clip.xyz / clip.w;
        ndc_min = min(ndc_min, ndc.xy);
        ndc_max = max(ndc_max, ndc.xy);
        nearest = max(nearest, ndc.z);
    }
    if (strict && (any(ndc_min < vec2(-1.0)) || any(ndc_max > vec2(1.0)))) {
        return true;
    }
    let lo = clamp(ndc_min, vec2(-1.0), vec2(1.0));
    let hi = clamp(ndc_max, vec2(-1.0), vec2(1.0));
    let vp = view.viewport;
    let left = vp.x + (lo.x * 0.5 + 0.5) * vp.z;
    let right = vp.x + (hi.x * 0.5 + 0.5) * vp.z;
    let top = vp.y + (0.5 - hi.y * 0.5) * vp.w;
    let bottom = vp.y + (0.5 - lo.y * 0.5) * vp.w;
    let limit = vec2<f32>(view.depth.xy) - vec2(1.0);
    // One extra pixel each side absorbs rasteriser rounding.
    let p0 = vec2<u32>(clamp(floor(vec2(left, top)) - vec2(1.0), vec2(0.0), limit));
    let p1 = vec2<u32>(clamp(floor(vec2(right, bottom)) + vec2(1.0), vec2(0.0), limit));
    // The finest level where the rectangle spans at most 4x4 texels.
    var level = 0u;
    loop {
        let shift = level + 1u;
        let span = (p1 >> vec2(shift)) - (p0 >> vec2(shift));
        if ((span.x <= 3u && span.y <= 3u) || level + 1u >= view.depth.z) {
            break;
        }
        level++;
    }
    let shift = level + 1u;
    let last = textureDimensions(hiz, level) - vec2(1u);
    let t0 = min(p0 >> vec2(shift), last);
    let t1 = min(p1 >> vec2(shift), last);
    var farthest = 1.0;
    for (var y = t0.y; y <= t1.y; y++) {
        for (var x = t0.x; x <= t1.x; x++) {
            farthest = min(farthest, textureLoad(hiz, vec2(x, y), i32(level)).r);
        }
    }
    return nearest >= farthest;
}
