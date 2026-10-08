//! Player skin rasters: normalization into the shared skin array and the default skin.
use std::sync::{Arc, OnceLock};

use render_api::SkinRgba8;

pub const MAX_RENDERED_PLAYERS: usize = 128;
/// Classic skin UV layouts use this many texels per side regardless of image resolution.
const CLASSIC_SKIN_SIDE: usize = render_api::CLASSIC_SKIN_SIDE;
/// The shared player array preserves every texel of every admitted skin resolution.
pub const STANDARD_SKIN_SIDE: usize = render_api::MAX_STANDARD_SKIN_SIDE as usize;
pub const STANDARD_SKIN_BYTES: usize = STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE * 4;
pub const DEFAULT_SKIN_PROVENANCE: &str = "locally generated Cinnabar Default skin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorSkinPixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: SkinRgba8,
}

/// Pack path of the player entity's default texture, the stand-in for skins that cannot load.
pub const DEFAULT_PLAYER_SKIN_PATH: &str = "textures/entity/steve.png";

static VANILLA_DEFAULT_SKIN: OnceLock<SkinRgba8> = OnceLock::new();

/// Installs the classic vanilla default texture once, packing it for the player array.
pub fn install_default_player_skin(skin: Arc<[u8]>) {
    let side = CLASSIC_SKIN_SIDE as u32;
    if let Some(skin) = normalize_actor_skin(&ActorSkinPixels {
        width: side,
        height: side,
        rgba8: skin.into(),
    }) {
        let _ = VANILLA_DEFAULT_SKIN.set(skin);
    }
}

/// The vanilla Steve skin once installed, else a generated diagnostic skin.
#[must_use]
pub fn default_actor_skin_rgba8() -> SkinRgba8 {
    static GENERATED: OnceLock<SkinRgba8> = OnceLock::new();
    VANILLA_DEFAULT_SKIN
        .get()
        .unwrap_or_else(|| GENERATED.get_or_init(|| generated_default_skin().into()))
        .clone()
}

/// The skin resampled to the standard raster; a standard-size source keeps its allocation and hash.
#[must_use]
pub fn normalize_actor_skin(skin: &ActorSkinPixels) -> Option<SkinRgba8> {
    let (side, height) = validated_skin_shape(skin)?;
    let expanded;
    let square: &[u8] = if height == side {
        &skin.rgba8
    } else {
        expanded = render_api::expand_legacy_skin_rgba8(&skin.rgba8, side);
        if side == STANDARD_SKIN_SIDE {
            return Some(expanded.into());
        }
        &expanded
    };
    if side == STANDARD_SKIN_SIDE {
        return Some(skin.rgba8.clone());
    }
    let mut normalized = vec![0; STANDARD_SKIN_BYTES];
    for y in 0..STANDARD_SKIN_SIDE {
        for x in 0..STANDARD_SKIN_SIDE {
            let source_x = x * side / STANDARD_SKIN_SIDE;
            let source_y = y * side / STANDARD_SKIN_SIDE;
            let source = (source_y * side + source_x) * 4;
            let target = (y * STANDARD_SKIN_SIDE + x) * 4;
            normalized[target..target + 4].copy_from_slice(&square[source..source + 4]);
        }
    }
    Some(normalized.into())
}

fn generated_default_skin() -> Vec<u8> {
    let skin_tone = [198, 134, 91, 255];
    let mut rgba8 = skin_tone.repeat(STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE);
    fill_rect(&mut rgba8, 16, 16, 24, 16, [42, 91, 99, 255]);
    fill_rect(&mut rgba8, 0, 16, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 16, 48, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 8, 8, 8, 8, [112, 72, 48, 255]);
    // The generated fallback has no authored second-layer clothing. Keep its
    // standard 64x64 overlay regions transparent so the shared outer-layer
    // geometry does not turn the diagnostic skin into an accidental jacket.
    for (x, y, width, height) in [
        (32, 0, 8, 8),
        (16, 32, 8, 12),
        (40, 32, 4, 12),
        (48, 48, 4, 12),
        (0, 32, 4, 12),
        (0, 48, 4, 12),
    ] {
        fill_rect(&mut rgba8, x, y, width, height, [0, 0, 0, 0]);
    }
    rgba8
}

fn fill_rect(rgba8: &mut [u8], x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
    let scale = STANDARD_SKIN_SIDE / CLASSIC_SKIN_SIDE;
    for py in y * scale..(y + height) * scale {
        for px in x * scale..(x + width) * scale {
            let offset = (py * STANDARD_SKIN_SIDE + px) * 4;
            rgba8[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    // A legacy 64x32 skin gains left limbs that mirror the right limbs face by face.
    #[test]
    fn legacy_half_height_skin_expands_with_mirrored_left_limbs() {
        let mut legacy = vec![0u8; 64 * 32 * 4];
        let texel = |x: usize, y: usize| (y * 64 + x) * 4;
        // Right leg front face, leftmost column.
        legacy[texel(4, 20)..texel(4, 20) + 4].copy_from_slice(&[1, 2, 3, 255]);
        let square = normalize_actor_skin(&ActorSkinPixels {
            width: 64,
            height: 32,
            rgba8: legacy.into(),
        })
        .expect("legacy skin normalizes");
        assert_eq!(square.len(), STANDARD_SKIN_BYTES);
        let scale = STANDARD_SKIN_SIDE / 64;
        let texel = |x: usize, y: usize| (y * scale * STANDARD_SKIN_SIDE + x * scale) * 4;
        // Left leg front face (20..24, 52..64) mirrors it into its rightmost column.
        assert_eq!(&square[texel(23, 52)..texel(23, 52) + 4], &[1, 2, 3, 255]);
        assert_eq!(&square[texel(4, 20)..texel(4, 20) + 4], &[1, 2, 3, 255]);
        assert!(
            normalize_actor_skin(&ActorSkinPixels {
                width: 64,
                height: 48,
                rgba8: vec![0; 64 * 48 * 4].into(),
            })
            .is_none()
        );
    }

    /// Narrow opaque texels in HD skins must not disappear when packed into the skin array.
    #[test]
    fn skin_packing_preserves_native_texels_between_old_downsample_points() {
        // Isolated texels from captured HD skins: the old nearest downsample missed both.
        for (side, x, y, pixel) in [
            (128usize, 96usize, 29usize, [91, 91, 91, 254]),
            (256, 5, 0, [231, 170, 57, 255]),
        ] {
            let mut rgba8 = vec![0; side * side * 4];
            let source = (y * side + x) * 4;
            rgba8[source..source + 4].copy_from_slice(&pixel);
            let packed = normalize_actor_skin(&ActorSkinPixels {
                width: side as u32,
                height: side as u32,
                rgba8: rgba8.into(),
            })
            .unwrap();
            let at = (y * STANDARD_SKIN_SIDE / side * STANDARD_SKIN_SIDE
                + x * STANDARD_SKIN_SIDE / side)
                * 4;
            assert_eq!(&packed[at..at + 4], &pixel, "{side}-pixel skin");
        }
    }
}

/// Validates the shared pixel contract before either native or standard-size preparation.
fn validated_skin_shape(skin: &ActorSkinPixels) -> Option<(usize, usize)> {
    if !skin.width.is_power_of_two()
        || skin.width < CLASSIC_SKIN_SIDE as u32
        || skin.width > render_api::MAX_STANDARD_SKIN_SIDE
        || (skin.height != skin.width && skin.height.checked_mul(2) != Some(skin.width))
    {
        return None;
    }
    let side = usize::try_from(skin.width).expect("bounded standard skin side");
    let height = usize::try_from(skin.height).expect("bounded standard skin height");
    if skin.rgba8.len() != side * height * 4 {
        return None;
    }
    Some((side, height))
}

mod native;
pub use native::{
    PLAYER_SKIN_BUDGET_BYTES, SKIN_CLASS_SIDES, actor_skin_side, prepare_actor_skin_cached,
};
