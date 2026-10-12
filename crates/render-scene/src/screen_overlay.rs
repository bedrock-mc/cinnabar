//! Scene description for full-screen camera overlays; the GPU side lives in `screen_overlay_render`.

use std::sync::Arc;

use bevy::{
    prelude::{Mat4, Resource},
    render::extract_resource::ExtractResource,
};

pub const MAX_SCREEN_OVERLAY_LAYERS: usize = 8;
/// Both overlay textures are square RGBA8 of this side.
pub const SCREEN_OVERLAY_TEXTURE_SIDE: u32 = 256;
pub const PUMPKIN_MASK_PATH: &str = "textures/misc/pumpkinblur.png";
pub const SPYGLASS_MASK_PATH: &str = "textures/ui/spyglass_scope.png";
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

/// Available pumpkin and spyglass masks, each RGBA8 and [`SCREEN_OVERLAY_TEXTURE_SIDE`] square.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenOverlayTextures {
    pumpkin: Option<Vec<u8>>,
    spyglass: Option<Vec<u8>>,
}

impl ScreenOverlayTextures {
    /// Rejects any payload that is not exactly one RGBA8 layer per texture.
    pub fn new(pumpkin: Vec<u8>, spyglass: Vec<u8>) -> Option<Self> {
        Self::from_layers(Some(pumpkin), Some(spyglass))
    }

    /// Admits independently available masks, retaining procedural art for absent layers.
    pub fn from_layers(pumpkin: Option<Vec<u8>>, spyglass: Option<Vec<u8>>) -> Option<Self> {
        let valid = |pixels: &Option<Vec<u8>>| {
            pixels
                .as_ref()
                .is_none_or(|pixels| pixels.len() == TEXTURE_BYTES)
        };
        (valid(&pumpkin) && valid(&spyglass) && (pumpkin.is_some() || spyglass.is_some()))
            .then_some(Self { pumpkin, spyglass })
    }

    /// Returns the pumpkin pixels, when that layer has an admitted image.
    pub fn pumpkin(&self) -> Option<&[u8]> {
        self.pumpkin.as_deref()
    }

    /// Returns the spyglass pixels, when that layer has an admitted image.
    pub fn spyglass(&self) -> Option<&[u8]> {
        self.spyglass.as_deref()
    }

    /// Packs both GPU layers, filling an absent layer with transparent pixels.
    pub fn layer_major(&self) -> Vec<u8> {
        let mut pixels = vec![0; TEXTURE_BYTES * 2];
        if let Some(pumpkin) = self.pumpkin() {
            pixels[..TEXTURE_BYTES].copy_from_slice(pumpkin);
        }
        if let Some(spyglass) = self.spyglass() {
            pixels[TEXTURE_BYTES..].copy_from_slice(spyglass);
        }
        pixels
    }
}

/// The frame's overlay stack, back to front. `textures_revision` must change whenever `textures` does.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct ScreenOverlayScene {
    pub layers: Vec<ScreenOverlayLayer>,
    pub clock_seconds: f32,
    pub textures: Option<Arc<ScreenOverlayTextures>>,
    pub textures_revision: u64,
    pub fire: Option<Arc<crate::ScreenFireTexture>>,
    pub fire_revision: u64,
    pub fire_projection: [f32; 2],
    /// Direction transform for the native unit-cube portal overlay.
    pub portal_from_clip: Mat4,
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
    fn independent_masks_preserve_pixels_and_leave_absent_layers_transparent() {
        let pumpkin = vec![0; TEXTURE_BYTES];
        let masks = ScreenOverlayTextures::from_layers(Some(pumpkin.clone()), None).unwrap();
        assert_eq!(masks.pumpkin(), Some(pumpkin.as_slice()));
        assert!(masks.spyglass().is_none());
        assert_eq!(masks.layer_major(), vec![0; TEXTURE_BYTES * 2]);
        assert!(ScreenOverlayTextures::from_layers(None, None).is_none());
        assert!(ScreenOverlayTextures::from_layers(Some(vec![0; 4]), None).is_none());
    }

    #[test]
    fn texture_swaps_advance_the_revision() {
        let mut scene = ScreenOverlayScene::default();
        scene.set_textures(None);
        assert_eq!(scene.textures_revision, 1);
    }
}
