use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    AnimatedImageData, EnumspersonaAnimatedTextureType, EnumspersonaAnimationExpression,
};

use crate::MAX_PLAYER_LIST_SKIN_BYTES;

/// Named geometry and texture slots used by the persona render controllers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinAnimationKind {
    Face,
    Body32,
    Body128,
}

impl SkinAnimationKind {
    /// Returns the stable atlas slot for this animation kind.
    pub const fn slot(self) -> usize {
        match self {
            Self::Face => 0,
            Self::Body32 => 1,
            Self::Body128 => 2,
        }
    }
    /// Returns the resource-patch geometry slot for this animation image.
    pub const fn geometry_key(self) -> &'static str {
        match self {
            Self::Face => "animated_face",
            Self::Body32 => "animated_32x32",
            Self::Body128 => "animated_128x128",
        }
    }
}

/// A transmitted persona animation atlas, without resampling or alpha modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinAnimation {
    pub kind: SkinAnimationKind,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
    pub frames: u32,
    pub blinking: bool,
}

/// Retains valid transmitted atlases within the same budget as the base skin.
pub(super) fn normalize(
    images: &[AnimatedImageData],
    retained: &mut usize,
) -> Arc<[SkinAnimation]> {
    let mut output: Vec<SkinAnimation> = Vec::new();
    for image in images {
        let kind = match image.animated_texture_type {
            EnumspersonaAnimatedTextureType::Face => SkinAnimationKind::Face,
            EnumspersonaAnimatedTextureType::Body32X32 => SkinAnimationKind::Body32,
            EnumspersonaAnimatedTextureType::Body128X128 => SkinAnimationKind::Body128,
            _ => continue,
        };
        let raster = &image.skin_image;
        let Some(bytes) = (raster.width as usize)
            .checked_mul(raster.height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            continue;
        };
        let previous = output.iter().position(|image| image.kind == kind);
        let released = previous.map_or(0, |index| output[index].rgba8.len());
        let candidate_bytes = retained
            .checked_sub(released)
            .and_then(|retained| retained.checked_add(bytes));
        if raster.width == 0
            || raster.height == 0
            || bytes != raster.image_bytes.len()
            || !image.frames.is_finite()
            || image.frames < 1.0
            || image.frames > raster.height as f32
            || image.frames.fract() != 0.0
            || candidate_bytes.is_none_or(|total| total > MAX_PLAYER_LIST_SKIN_BYTES)
        {
            continue;
        }
        if let Some(previous) = previous {
            output.remove(previous);
        }
        *retained = candidate_bytes.expect("validated animation byte total");
        output.push(SkinAnimation {
            kind,
            width: raster.width,
            height: raster.height,
            rgba8: raster.image_bytes.as_slice().into(),
            frames: image.frames as u32,
            blinking: image.animation_expression == EnumspersonaAnimationExpression::Blinking,
        });
    }
    output.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_51::SkinImage;

    #[test]
    fn review_animation_replacement_refunds_its_previous_atlas_before_budgeting() {
        let image = |height, byte| AnimatedImageData {
            skin_image: SkinImage {
                width: 1,
                height,
                image_bytes: vec![byte; height as usize * 4],
            },
            animated_texture_type: EnumspersonaAnimatedTextureType::Body32X32,
            frames: 1.0,
            animation_expression: EnumspersonaAnimationExpression::Linear,
        };
        let mut retained = MAX_PLAYER_LIST_SKIN_BYTES - 8;
        let images = normalize(&[image(2, 1), image(1, 2)], &mut retained);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].height, 1);
        assert_eq!(images[0].rgba8.as_ref(), &[2; 4]);
        assert_eq!(retained, MAX_PLAYER_LIST_SKIN_BYTES - 4);
    }

    #[test]
    fn transmitted_persona_animation_keeps_its_actual_width_and_frame_count() {
        let input = AnimatedImageData {
            skin_image: SkinImage {
                width: 24,
                height: 512,
                image_bytes: vec![71; 24 * 512 * 4],
            },
            animated_texture_type: EnumspersonaAnimatedTextureType::Body32X32,
            frames: 16.0,
            animation_expression: EnumspersonaAnimationExpression::Linear,
        };
        let mut bytes = 0;
        let images = normalize(&[input], &mut bytes);
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].width, images[0].frames), (24, 16));
        assert_eq!(bytes, images[0].rgba8.len());
        assert_eq!(images[0].kind.geometry_key(), "animated_32x32");
    }
}
