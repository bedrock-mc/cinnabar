//! Animated actor glint constants shared by armor and player cape textures.

/// The raster a glint layer samples.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GlintTexture {
    /// `enchanted_actor_glint`, drawn over worn equipment.
    #[default]
    Actor,
    /// The brighter `enchanted_item_glint`, drawn over held and dropped items.
    Item,
}

/// Native item glint UV scale, applied across one item sprite.
pub const ITEM_GLINT_UV_SCALE: f32 = 0.5;

/// Render-owned profile bit, above the authored material state bits.
pub const LEGACY_COMBAT_MATERIAL_FLAG: u32 =
    assets::EntityRenderMaterialState::DISABLE_COLOR_WRITE << 1;
/// Per-draw dye masking keeps leather separate even when rasters share an artwork page.
pub const ACTOR_COLOR_MASK_MATERIAL_FLAG: u32 = LEGACY_COMBAT_MATERIAL_FLAG << 1;

/// Converts legacy raster UV through rotation and then offset.
pub fn legacy_glint_uv(uv: [f32; 2], offsets: [f32; 2], pass: usize) -> [f32; 2] {
    let rules = render_api::BEDROCK_112;
    let (sin, cos) = rules.glint_rotation_radians[pass].sin_cos();
    let [x, y] = uv.map(|component| component - 0.5);
    [
        cos * x + sin * y + offsets[pass] + 0.5,
        -sin * x + cos * y + 0.5,
    ]
}

/// Accessibility, combat profile, clock, and sprite-local glint coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorGlint {
    pub profile: render_api::CombatVisuals,
    pub time_seconds: f32,
    pub strength: f32,
    pub speed: f32,
    /// `GLINT_UV_SCALE`: 1 for equipment; items use [`ITEM_GLINT_UV_SCALE`] over their sprite.
    pub uv_scale: [f32; 2],
    /// Extent of the sprite-local item coordinates: one for attachables, half for atlas sprites.
    pub uv_extent: f32,
    pub texture: GlintTexture,
}

impl Default for ActorGlint {
    fn default() -> Self {
        Self {
            profile: Default::default(),
            time_seconds: 0.0,
            strength: 1.0,
            speed: 1.0,
            uv_scale: [1.0; 2],
            uv_extent: 1.0,
            texture: GlintTexture::Actor,
        }
    }
}

impl ActorGlint {
    /// Shared purple modulation for actor and UI item enchantment textures.
    pub const TINT_RGB: [f32; 3] = [0.38, 0.19, 0.608];

    /// Item glint over a sprite spanning `span` of its texture in UV units, as vanilla scales
    /// atlas sprites: the glint covers each sprite at [`ITEM_GLINT_UV_SCALE`].
    #[must_use]
    pub fn item(time_seconds: f32, span: [f32; 2]) -> Self {
        Self {
            profile: render_api::CombatVisuals::Modern,
            time_seconds,
            uv_scale: span.map(|span| {
                if span.is_finite() && span > 0.0 {
                    ITEM_GLINT_UV_SCALE / span
                } else {
                    ITEM_GLINT_UV_SCALE
                }
            }),
            texture: GlintTexture::Item,
            uv_extent: ITEM_GLINT_UV_SCALE,
            ..Default::default()
        }
    }
}

impl ActorGlint {
    /// Returns profile-selected scrolls at unit UV scale and the clamped strength.
    pub fn scroll(self) -> [f32; 3] {
        let time = if self.time_seconds.is_finite() {
            self.time_seconds.max(0.0)
        } else {
            0.0
        };
        let speed = if self.speed.is_finite() {
            self.speed.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let strength = if self.strength.is_finite() {
            self.strength.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let millis = ((time * 1000.0) as i32 as f32 * speed) as i32;
        let [first, second] = match self.profile {
            render_api::CombatVisuals::Modern => [1375, 3750],
            render_api::CombatVisuals::Legacy112 => render_api::BEDROCK_112.glint_period_ms,
        };
        [
            (millis % first) as f32 / -(first as f32),
            (millis % second) as f32 / second as f32,
            strength,
        ]
    }

    /// Packs scrolls divided by sprite scale, strength, scale and item extent (zero for actors).
    pub fn parameters(self) -> [f32; 6] {
        let [first, second, strength] = self.scroll();
        let scale = self.uv_scale.map(|scale| {
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        });
        [
            first / scale[0],
            second / scale[0],
            strength,
            scale[0],
            scale[1],
            match self.texture {
                GlintTexture::Actor => 0.0,
                GlintTexture::Item => {
                    if self.uv_extent.is_finite() && self.uv_extent > 0.0 {
                        self.uv_extent
                    } else {
                        1.0
                    }
                }
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_glint_wraps_each_layer_and_scales_speed_after_millisecond_quantization() {
        let factors = ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            time_seconds: 3.75,
            strength: 0.5,
            speed: 1.0,
            ..Default::default()
        };
        assert_eq!(
            factors.parameters(),
            [-1000.0 / 1375.0, 0.0, 0.5, 1.0, 1.0, 0.0]
        );
        let half_speed = ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            time_seconds: 0.0039,
            speed: 0.5,
            ..Default::default()
        };
        assert_eq!(
            half_speed.parameters(),
            [-1.0 / 1375.0, 1.0 / 3750.0, 1.0, 1.0, 1.0, 0.0]
        );
        assert_eq!(
            ActorGlint {
                time_seconds: f32::NAN,
                ..Default::default()
            }
            .parameters(),
            [0.0, 0.0, 1.0, 1.0, 1.0, 0.0]
        );
    }

    #[test]
    fn item_glint_covers_each_sprite_at_half_scale_and_scrolls_against_it() {
        let glint = ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            ..ActorGlint::item(3.0, [0.25, 0.125])
        };
        let [first, second, strength, scale_u, scale_v, texture] = glint.parameters();
        assert_eq!(
            [scale_u, scale_v],
            [2.0, 4.0],
            "0.5 across a quarter-wide sprite"
        );
        assert_eq!(first, (3000 % 1375) as f32 / -1375.0 / 2.0);
        assert_eq!(second, 3000.0 / 3750.0 / 2.0);
        assert_eq!(strength, 1.0);
        assert_eq!(texture, ITEM_GLINT_UV_SCALE, "atlas item coordinate extent");
        assert_eq!(ActorGlint::item(0.0, [1.0; 2]).uv_scale, [0.5; 2]);
        assert_eq!(
            ActorGlint::item(0.0, [1.0; 2]).profile,
            render_api::CombatVisuals::Modern
        );
        assert_eq!(
            ActorGlint {
                texture: GlintTexture::Item,
                ..Default::default()
            }
            .parameters()[5],
            1.0,
            "attachable item coordinates retain unit extent"
        );
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;
    use render_api::{BEDROCK_112, CombatVisuals, DEFAULT_COMBAT_VISUALS};

    /// Calculates a shared wrap boundary without another hand-maintained period.
    fn gcd(mut a: i32, mut b: i32) -> i32 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }

    #[test]
    fn default_and_each_legacy_clock_boundary_follow_the_shared_profile() {
        assert_eq!(ActorGlint::default().profile, DEFAULT_COMBAT_VISUALS);
        let [first, second] = BEDROCK_112.glint_period_ms;
        for milliseconds in [
            0,
            first - 1,
            first,
            second - 1,
            second,
            first * second / gcd(first, second),
        ] {
            let glint = ActorGlint {
                profile: CombatVisuals::Legacy112,
                time_seconds: milliseconds as f32 / 1000.0,
                ..Default::default()
            };
            let [a, b, strength] = glint.scroll();
            assert!((a + (milliseconds % first) as f32 / first as f32).abs() < 1e-6);
            assert!((b - (milliseconds % second) as f32 / second as f32).abs() < 1e-6);
            assert_eq!(strength, 1.0);
        }
        for profile in [CombatVisuals::Modern, CombatVisuals::Legacy112] {
            for strength in [0.0, 0.25, 0.5, 1.0] {
                let glint = ActorGlint {
                    profile,
                    time_seconds: 4.0,
                    strength,
                    speed: 0.0,
                    ..Default::default()
                };
                assert_eq!(glint.scroll(), [0.0, 0.0, strength]);
            }
        }
    }

    #[test]
    fn uv_rotation_preserves_pixel_origins_and_non_square_extents() {
        let offsets = [-0.25, 0.75];
        // These coordinates come from a deliberately off-origin rectangle on a non-square image.
        for pixel in [[13.0, 7.0], [35.0, 18.0]] {
            let uv = [pixel[0] / 64.0, pixel[1] / 32.0];
            for pass in 0..2 {
                let angle = BEDROCK_112.glint_rotation_radians[pass];
                let matrix = glam::Mat2::from_cols(
                    glam::Vec2::new(angle.cos(), -angle.sin()),
                    glam::Vec2::new(angle.sin(), angle.cos()),
                );
                let rotated = matrix * (glam::Vec2::from_array(uv) - glam::Vec2::splat(0.5));
                let expected = rotated + glam::Vec2::new(offsets[pass] + 0.5, 0.5);
                assert!(
                    glam::Vec2::from_array(legacy_glint_uv(uv, offsets, pass))
                        .abs_diff_eq(expected, 1e-6)
                );
            }
        }
    }

    #[test]
    fn profile_and_mask_flags_do_not_overlap_any_admitted_material_state() {
        let admitted = assets::EntityRenderMaterial::Glint
            .word(assets::EntityRenderMaterialState::from_word(u32::MAX));
        assert_eq!(
            admitted & (LEGACY_COMBAT_MATERIAL_FLAG | ACTOR_COLOR_MASK_MATERIAL_FLAG),
            0
        );
        assert_eq!(
            LEGACY_COMBAT_MATERIAL_FLAG & ACTOR_COLOR_MASK_MATERIAL_FLAG,
            0
        );
        let material = crate::ActorMaterial {
            color_mask: true,
            glint: ActorGlint {
                profile: CombatVisuals::Legacy112,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            material.gpu_word() & LEGACY_COMBAT_MATERIAL_FLAG,
            LEGACY_COMBAT_MATERIAL_FLAG
        );
        assert_eq!(
            material.gpu_word() & ACTOR_COLOR_MASK_MATERIAL_FLAG,
            ACTOR_COLOR_MASK_MATERIAL_FLAG
        );
    }
}
