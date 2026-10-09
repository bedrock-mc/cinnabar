//! Ordinary native Fancy actor shading shared by UI previews and world shaders.

/// Y, squared X/Z, ambient, and hurt-alpha coefficients, in evaluation order.
/// Lighting constants used by the ordinary Actor/Entity shaders.
pub const ACTOR_SHADE_COEFFICIENTS: [f32; 5] = [0.275, -0.1, 0.1, 0.45, 0.35];

/// Samples the environment lightmap and applies directional actor shading.
pub const ACTOR_LIGHT_WORLD: u32 = 1 << 31;
/// Applies directional actor shading with white illumination instead of the lightmap.
pub const ACTOR_LIGHT_DIRECTIONAL: u32 = 1 << 30;

/// The caller supplies a normalized posed world normal and dimension-adjusted Y.
pub fn fancy_actor_shade([x, y, z]: [f32; 3], overlay_alpha: f32) -> f32 {
    let [y_scale, x_scale, z_scale, ambient, hurt_scale] = ACTOR_SHADE_COEFFICIENTS;
    (((1.0 + y) * y_scale + x * x * x_scale) + z * z * z_scale)
        + ambient
        + overlay_alpha * hurt_scale
}
