//! Exposure shader specialization, kept free of Bevy so shader validation includes it directly.

/// Pixel edge of one reduced motion tile.
pub(super) const TILE_SIZE: u32 = 48;

/// Injects the tile size and, for multisampled targets, per-sample depth and velocity reads.
pub(super) fn source(raw: &str, multisampled: bool) -> String {
    let source = raw.replace("MOTION_TILE_SIZE", &TILE_SIZE.to_string());
    if multisampled {
        source
            .replace("texture_depth_2d", "texture_depth_multisampled_2d")
            .replace(
                "let covered_depth = textureLoad(depth, pixel, 0);",
                r#"var covered_depth = 0.0;
    for (var sample = 0u; sample < textureNumSamples(depth); sample++) {
        if textureLoad(actor_velocity, pixel, i32(sample)).z >= 0.5 {
            covered_depth = max(covered_depth, textureLoad(depth, pixel, i32(sample)));
        }
    }"#,
            )
            .replace(
                "return textureLoad(depth, pixel, 0);",
                r#"var nearest = 0.0;
    for (var sample = 0u; sample < textureNumSamples(depth); sample++) {
        nearest = max(nearest, textureLoad(depth, pixel, i32(sample)));
    }
    return nearest;"#,
            )
            .replace(
                "var actor_velocity: texture_2d<f32>",
                "var actor_velocity: texture_multisampled_2d<f32>",
            )
            .replace(
                "return textureLoad(actor_velocity, pixel, 0);",
                r#"var velocity = vec2<f32>(0.0);
    var covered = 0u;
    let samples = textureNumSamples(actor_velocity);
    for (var sample = 0u; sample < samples; sample++) {
        let motion = textureLoad(actor_velocity, pixel, i32(sample));
        if motion.z >= 0.5 {
            velocity += motion.xy;
            covered += 1u;
        }
    }
    if covered == 0u {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(velocity / f32(covered), 1.0, f32(covered) / f32(samples));"#,
            )
    } else {
        source
    }
}
