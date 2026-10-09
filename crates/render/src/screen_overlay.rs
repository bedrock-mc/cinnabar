//! Scene description for full-screen camera overlays; the GPU side lives in `screen_overlay_render`.

use std::sync::Arc;

use bevy::{
    prelude::{Mat4, Resource},
    render::extract_resource::ExtractResource,
};

pub const MAX_SCREEN_OVERLAY_LAYERS: usize = 8;
/// Both overlay textures are square RGBA8 of this side.
pub const SCREEN_OVERLAY_TEXTURE_SIDE: u32 = 256;
const TEXTURE_BYTES: usize =
    (SCREEN_OVERLAY_TEXTURE_SIDE * SCREEN_OVERLAY_TEXTURE_SIDE * 4) as usize;

/// Shader pattern selector for one layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenOverlayKind {
    Flat = 0,
    PowderSnow = 1,
    Fire = 2,
    Portal = 3,
    PumpkinBlur = 4,
    SpyglassScope = 5,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenOverlayLayer {
    pub kind: ScreenOverlayKind,
    pub rgb: [f32; 3],
    pub alpha: f32,
}

/// Pumpkin-blur and spyglass-scope pixels, RGBA8, exactly [`SCREEN_OVERLAY_TEXTURE_SIDE`] square.
#[derive(Debug, PartialEq, Eq)]
pub struct ScreenOverlayTextures {
    pumpkin: Vec<u8>,
    spyglass: Vec<u8>,
}

impl ScreenOverlayTextures {
    /// Rejects any payload that is not exactly one RGBA8 layer per texture.
    pub fn new(pumpkin: Vec<u8>, spyglass: Vec<u8>) -> Option<Self> {
        (pumpkin.len() == TEXTURE_BYTES && spyglass.len() == TEXTURE_BYTES)
            .then_some(Self { pumpkin, spyglass })
    }

    pub(crate) fn layer_major(&self) -> Vec<u8> {
        [self.pumpkin.as_slice(), self.spyglass.as_slice()].concat()
    }
}

/// The frame's overlay stack, back to front. `textures_revision` must change whenever `textures` does.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
pub struct ScreenOverlayScene {
    pub(crate) layers: Vec<ScreenOverlayLayer>,
    pub(crate) clock_seconds: f32,
    pub(crate) textures: Option<Arc<ScreenOverlayTextures>>,
    pub(crate) textures_revision: u64,
    pub(crate) fire: Option<Arc<crate::ScreenFireTexture>>,
    pub(crate) fire_revision: u64,
    pub(crate) fire_projection: [f32; 2],
    /// Direction transform for the native unit-cube portal overlay.
    pub(crate) portal_from_clip: Mat4,
}

impl ScreenOverlayScene {
    /// Replaces the layer stack; layers beyond the cap and non-finite layers are dropped.
    pub fn set_layers(
        &mut self,
        layers: impl IntoIterator<Item = ScreenOverlayLayer>,
        clock_seconds: f32,
    ) {
        self.layers = layers
            .into_iter()
            .filter(|layer| {
                layer.alpha.is_finite() && layer.rgb.iter().all(|value| value.is_finite())
            })
            .filter(|layer| layer.alpha > 0.0)
            .take(MAX_SCREEN_OVERLAY_LAYERS)
            .collect();
        self.clock_seconds = if clock_seconds.is_finite() {
            clock_seconds
        } else {
            0.0
        };
    }

    pub fn set_textures(&mut self, textures: Option<Arc<ScreenOverlayTextures>>) {
        self.textures = textures;
        self.textures_revision = self.textures_revision.wrapping_add(1);
    }

    pub fn set_fire_texture(&mut self, fire: Option<Arc<crate::ScreenFireTexture>>) {
        self.fire = fire;
        self.fire_revision = self.fire_revision.wrapping_add(1);
    }

    /// Native fire follows the active perspective projection, including FOV changes.
    pub fn set_fire_projection(&mut self, vertical_fov: f32, aspect: f32) {
        self.fire_projection = [
            (vertical_fov * 0.5).tan() * aspect,
            (vertical_fov * 0.5).tan(),
        ];
    }

    pub fn set_portal_from_clip(&mut self, matrix: Mat4) {
        self.portal_from_clip = if matrix.is_finite() {
            matrix
        } else {
            Mat4::IDENTITY
        };
    }

    /// Perspective half-extents used to project this frame's fire overlay.
    #[must_use]
    pub const fn fire_projection(&self) -> [f32; 2] {
        self.fire_projection
    }

    /// Maps this frame's clip coordinates to the portal overlay's world directions.
    #[must_use]
    pub const fn portal_from_clip(&self) -> Mat4 {
        self.portal_from_clip
    }

    #[must_use]
    pub fn layers(&self) -> &[ScreenOverlayLayer] {
        &self.layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(alpha: f32) -> ScreenOverlayLayer {
        ScreenOverlayLayer {
            kind: ScreenOverlayKind::Flat,
            rgb: [0.0; 3],
            alpha,
        }
    }

    #[test]
    fn layers_are_bounded_and_sanitized() {
        let mut scene = ScreenOverlayScene::default();
        scene.set_layers(
            std::iter::once(layer(f32::NAN))
                .chain(std::iter::once(layer(0.0)))
                .chain(std::iter::repeat_n(layer(0.5), 20)),
            f32::INFINITY,
        );
        assert_eq!(scene.layers().len(), MAX_SCREEN_OVERLAY_LAYERS);
        assert_eq!(scene.clock_seconds, 0.0);
    }

    #[test]
    fn textures_require_exact_layer_sizes() {
        assert!(
            ScreenOverlayTextures::new(vec![0; TEXTURE_BYTES], vec![0; TEXTURE_BYTES]).is_some()
        );
        assert!(ScreenOverlayTextures::new(vec![0; 4], vec![0; TEXTURE_BYTES]).is_none());
    }

    #[test]
    fn texture_swaps_advance_the_revision() {
        let mut scene = ScreenOverlayScene::default();
        scene.set_textures(None);
        assert_eq!(scene.textures_revision, 1);
    }
}
