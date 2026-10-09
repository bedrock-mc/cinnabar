use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::SerializedSkinRef;

use super::{MAX_PLAYER_LIST_SKIN_BYTES, MAX_STANDARD_SKIN_SIDE};

mod alpha;
pub use alpha::{normalize_classic_skin_rgba8, normalize_custom_skin_rgba8};
mod animation;
pub use animation::{SkinAnimation, SkinAnimationKind};
pub use render_api::{
    CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE, MAX_SKIN_ANIMATION_LAYERS, SkinRgba8,
    expand_legacy_skin_rgba8,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardSkin {
    pub width: u32,
    pub height: u32,
    pub rgba8: SkinRgba8,
    /// The skin's cape image when it carries a valid one; counts toward the skin byte budget.
    pub cape: Option<CapeImage>,
    /// The skin's own model inputs when it may name a non-default geometry.
    pub geometry: Option<Arc<SkinGeometrySource>>,
}

/// The resource patch and geometry JSON a skin carries; parsed by the actor runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinGeometrySource {
    pub resource_patch: Arc<str>,
    pub geometry_data: Arc<str>,
    pub animations: Arc<[SkinAnimation]>,
}

impl SkinGeometrySource {
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.resource_patch.len()
            + self.geometry_data.len()
            + self
                .animations
                .iter()
                .map(|image| image.rgba8.len())
                .sum::<usize>()
    }
}

/// Model input bytes one skin may retain; larger models fall back to the default geometry.
pub const MAX_SKIN_GEOMETRY_SOURCE_BYTES: usize = 1024 * 1024;

/// Keeps the resource patch even without model data, so classic slim skins select their model.
fn geometry_source(
    skin: &SerializedSkinRef,
    retained_bytes: &mut usize,
) -> Option<Arc<SkinGeometrySource>> {
    let bytes = skin.resource_patch.len() + skin.geometry_data.len();
    let next = retained_bytes.checked_add(bytes)?;
    if skin.resource_patch.is_empty()
        || bytes > MAX_SKIN_GEOMETRY_SOURCE_BYTES
        || next > MAX_PLAYER_LIST_SKIN_BYTES
    {
        return None;
    }
    *retained_bytes = next;
    Some(Arc::new(SkinGeometrySource {
        resource_patch: skin.resource_patch.as_str().into(),
        geometry_data: skin.geometry_data.as_str().into(),
        animations: animation::normalize(&skin.animated_image_data, retained_bytes),
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapeImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

impl CapeImage {
    pub fn is_valid(&self) -> bool {
        CAPE_DIMENSIONS.contains(&(self.width, self.height))
            && self.rgba8.len() == self.width as usize * self.height as usize * 4
    }
}

/// Cape image sizes Bedrock skins use, as `(width, height)`.
pub const CAPE_DIMENSIONS: [(u32, u32); 4] = [(64, 32), (128, 64), (256, 128), (1024, 512)];

fn normalize_cape(
    image: &valentine::bedrock::version::v1_26_51::SkinImage,
    retained_bytes: &mut usize,
) -> Option<CapeImage> {
    let (width, height) = (image.width, image.height);
    if !CAPE_DIMENSIONS.contains(&(width, height)) {
        return None;
    }
    let expected = usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4;
    let next = retained_bytes.checked_add(expected)?;
    if image.image_bytes.len() != expected || next > MAX_PLAYER_LIST_SKIN_BYTES {
        return None;
    }
    *retained_bytes = next;
    Some(CapeImage {
        width,
        height,
        rgba8: Arc::from(image.image_bytes.as_slice()),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSkinUnavailable {
    InvalidDimensions,
    InvalidByteLength,
    RetainedBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSkin {
    Standard(StandardSkin),
    Unavailable(PlayerSkinUnavailable),
}

pub(super) fn normalize_player_skin(
    mut skin: SerializedSkinRef,
    retained_bytes: &mut usize,
) -> PlayerSkin {
    // Serialized persona payloads carry the assembled model, base raster and animation atlases.
    let (width, mut height) = (skin.image_data.width, skin.image_data.height);
    let classic = CLASSIC_SKIN_SIDE as u32;
    let legacy = (width, height) == (classic, classic / 2);
    let limit = if skin.is_persona {
        MAX_STANDARD_SKIN_SIDE
    } else {
        MAX_CLASSIC_SKIN_SIDE as u32
    };
    if !legacy && (width != height || !width.is_power_of_two() || width < classic || width > limit)
    {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    }
    let Some(expected_bytes) = usize::try_from(width)
        .ok()
        .and_then(|width| usize::try_from(height).ok().map(|height| (width, height)))
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    if skin.image_data.image_bytes.len() != expected_bytes {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidByteLength);
    }
    if legacy && !skin.is_persona {
        skin.image_data.image_bytes =
            expand_legacy_skin_rgba8(&skin.image_data.image_bytes, width as usize);
        height = width;
        skin.image_data.height = height;
    }
    let expected_bytes = skin.image_data.image_bytes.len();
    let Some(next_bytes) = retained_bytes.checked_add(expected_bytes) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    };
    if next_bytes > MAX_PLAYER_LIST_SKIN_BYTES {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    }
    *retained_bytes = next_bytes;
    let cape = normalize_cape(&skin.cape_image_data, retained_bytes);
    let geometry = geometry_source(&skin, retained_bytes);
    alpha::normalize(&mut skin);
    PlayerSkin::Standard(StandardSkin {
        width,
        height,
        rgba8: SkinRgba8::from(skin.image_data.image_bytes),
        cape,
        geometry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_geometry_keeps_the_captured_slim_resource_patch() {
        let skin = SerializedSkinRef {
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.customSlim"}}"#.into(),
            geometry_data: "null".into(),
            ..Default::default()
        };
        let mut bytes = 0;
        let source = geometry_source(&skin, &mut bytes).expect("named classic model survives");
        assert_eq!(source.resource_patch.as_ref(), skin.resource_patch);
        assert_eq!(source.geometry_data.as_ref(), "null");
        assert_eq!(bytes, source.byte_len());
    }

    #[test]
    fn classic_rejects_large_rasters_while_persona_accepts_the_bounded_larger_atlas() {
        for side in [MAX_CLASSIC_SKIN_SIDE as u32 * 2, MAX_STANDARD_SKIN_SIDE] {
            let mut source = SerializedSkinRef {
                image_data: valentine::bedrock::version::v1_26_51::SkinImage {
                    width: side,
                    height: side,
                    image_bytes: vec![255; (side * side * 4) as usize],
                },
                ..Default::default()
            };
            assert_eq!(
                normalize_player_skin(source.clone(), &mut 0),
                PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions)
            );
            source.is_persona = true;
            assert!(matches!(
                normalize_player_skin(source, &mut 0),
                PlayerSkin::Standard(_)
            ));
        }
    }

    #[test]
    fn legacy_alpha_is_validated_after_left_limbs_are_expanded() {
        let side = CLASSIC_SKIN_SIDE as u32;
        let source = SerializedSkinRef {
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#.into(),
            image_data: valentine::bedrock::version::v1_26_51::SkinImage {
                width: side,
                height: side / 2,
                image_bytes: vec![0; (side * side / 2 * 4) as usize],
            },
            ..Default::default()
        };
        let PlayerSkin::Standard(skin) = normalize_player_skin(source, &mut 0) else {
            panic!("legacy skin");
        };
        assert_eq!(skin.height, side);
        assert_eq!(skin.rgba8[(52 * CLASSIC_SKIN_SIDE + 16) * 4 + 3], 255);
        assert_eq!(skin.rgba8[(48 * CLASSIC_SKIN_SIDE) * 4 + 3], 0);
    }
}
