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

/// Accessibility factors, the actor glint clock in seconds, and the glint layer UV scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorGlint {
    pub time_seconds: f32,
    pub strength: f32,
    pub speed: f32,
    /// `GLINT_UV_SCALE`: 1 for equipment; items use [`ITEM_GLINT_UV_SCALE`] over their sprite.
    pub uv_scale: [f32; 2],
    pub texture: GlintTexture,
}

impl Default for ActorGlint {
    fn default() -> Self {
        Self {
            time_seconds: 0.0,
            strength: 1.0,
            speed: 1.0,
            uv_scale: [1.0; 2],
            texture: GlintTexture::Actor,
        }
    }
}

impl ActorGlint {
    /// Item glint over a sprite spanning `span` of its texture in UV units, as vanilla scales
    /// atlas sprites: the glint covers each sprite at [`ITEM_GLINT_UV_SCALE`].
    #[must_use]
    pub fn item(time_seconds: f32, span: [f32; 2]) -> Self {
        Self {
            time_seconds,
            uv_scale: span.map(|span| {
                if span.is_finite() && span > 0.0 {
                    ITEM_GLINT_UV_SCALE / span
                } else {
                    ITEM_GLINT_UV_SCALE
                }
            }),
            texture: GlintTexture::Item,
            ..Default::default()
        }
    }
}

impl ActorGlint {
    /// Native glint layer words: both scroll offsets divided by the UV scale (as vanilla
    /// divides them before scaling), strength, the UV scale and the raster selector.
    pub(crate) fn parameters(self) -> [f32; 6] {
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
        let scale = self.uv_scale.map(|scale| {
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        });
        let millis = ((time * 1000.0) as i32 as f32 * speed) as i32;
        [
            (millis % 1375) as f32 / -1375.0 / scale[0],
            (millis % 3750) as f32 / 3750.0 / scale[0],
            strength,
            scale[0],
            scale[1],
            match self.texture {
                GlintTexture::Actor => 0.0,
                GlintTexture::Item => 1.0,
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
        let glint = ActorGlint::item(3.0, [0.25, 0.125]);
        let [first, second, strength, scale_u, scale_v, texture] = glint.parameters();
        assert_eq!(
            [scale_u, scale_v],
            [2.0, 4.0],
            "0.5 across a quarter-wide sprite"
        );
        assert_eq!(first, (3000 % 1375) as f32 / -1375.0 / 2.0);
        assert_eq!(second, 3000.0 / 3750.0 / 2.0);
        assert_eq!(strength, 1.0);
        assert_eq!(texture, 1.0, "items sample the item glint raster");
        assert_eq!(ActorGlint::item(0.0, [1.0; 2]).uv_scale, [0.5; 2]);
    }
}
