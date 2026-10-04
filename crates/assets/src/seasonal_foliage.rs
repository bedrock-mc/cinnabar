//! Native seasonal foliage palette, independent of GPU colour packing.
//!
//! 1.26.50.26 `SeasonsRenderer` palette generation
//! creates covered evergreen/birch/default columns, then their exposed
//! counterparts. Snow blending precedes the half-intensity RGBA8 storage;
//! the world material doubles stored RGB without clipping before multiplying
//! the texture. The CPU particle consumer instead clamps it.

use crate::{
    BLOCK_VISUAL_VARIANT_TOP_SNOW, BlockFlags, MATERIAL_FLAG_BIRCH_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, ResolvedBlock, TintMapId,
    VisualKind,
};

pub const SEASONAL_FOLIAGE_EXPOSED_OFFSET: usize = 3;
pub const SEASONAL_FOLIAGE_COUNT: usize = SEASONAL_FOLIAGE_EXPOSED_OFFSET * 2;
const COVERED_MAPS: [TintMapId; SEASONAL_FOLIAGE_EXPOSED_OFFSET] =
    [TintMapId::Evergreen, TintMapId::Birch, TintMapId::Foliage];
/// LeavesBlock::getRenderLayer cold limit.
pub const SEASONAL_FOLIAGE_COLD_THRESHOLD: f32 = 0.15;
pub const SEASONAL_FOLIAGE_SNOW_RGB: f32 = 1.8;

/// Species selection shared by CPU particle colours and generated GPU constants.
#[must_use]
pub fn seasonal_foliage_palette_index(material_flags: u32, exposed: bool) -> usize {
    let map = match material_flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK {
        MATERIAL_FLAG_EVERGREEN_FOLIAGE => TintMapId::Evergreen,
        MATERIAL_FLAG_BIRCH_FOLIAGE => TintMapId::Birch,
        _ => TintMapId::Foliage,
    };
    COVERED_MAPS
        .iter()
        .position(|&candidate| candidate == map)
        .expect("seasonal species map")
        + if exposed {
            SEASONAL_FOLIAGE_EXPOSED_OFFSET
        } else {
            0
        }
}

pub(crate) fn seasonal_palette_map(index: usize) -> TintMapId {
    COVERED_MAPS[index % SEASONAL_FOLIAGE_EXPOSED_OFFSET]
}

/// The source-backed facts needed by the native seasonal shelter query.
/// Unlike rendering contributors, these retain their original storage order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeasonalFoliageBlock {
    pub flags: BlockFlags,
    pub kind: VisualKind,
    pub variant: u32,
}

impl SeasonalFoliageBlock {
    pub const AIR: Self = Self {
        flags: BlockFlags::AIR,
        kind: VisualKind::Invisible,
        variant: 0,
    };
}

impl From<ResolvedBlock> for SeasonalFoliageBlock {
    fn from(block: ResolvedBlock) -> Self {
        Self {
            flags: block.flags(),
            kind: block.kind(),
            variant: block.variant(),
        }
    }
}

/// Native ClientLeavesSeasonColorUtils
/// skips the main block's air/leaves properties before canBeBuiltOver.
/// TopSnow's override delegates to a non-air extra block
/// first; only with air extra does its height decide replaceability.
///
/// `None` main means unavailable and conservatively shelters. `None` extra
/// means no extra storage (air). Unknown blocks use Diagnostic facts and
/// shelter; callers must not turn an unloaded/unknown extra into `None`.
#[must_use]
pub const fn seasonal_foliage_cell_shelters(
    main: Option<SeasonalFoliageBlock>,
    extra: Option<SeasonalFoliageBlock>,
) -> bool {
    let Some(main) = main else {
        return true;
    };
    if matches!(main.kind, VisualKind::Diagnostic) {
        return true;
    }
    if main
        .flags
        .intersects(BlockFlags::AIR.union(BlockFlags::LEAF_MODEL))
    {
        return false;
    }
    if main.variant & BLOCK_VISUAL_VARIANT_TOP_SNOW != 0 {
        if let Some(extra) = extra
            && !extra.flags.contains(BlockFlags::AIR)
        {
            // The main leaves property shortcut is not an extra block's
            // canBeBuiltOver predicate: an extra leaf still needs replacement.
            return matches!(extra.kind, VisualKind::Diagnostic)
                || !extra.flags.contains(BlockFlags::SEASONAL_REPLACEABLE);
        }
        // The native compiler represents heights 0..6 as bounded models and
        // height 7 as a full cube; no numeric height is duplicated here.
        return !matches!(main.kind, VisualKind::Model);
    }
    !main.flags.contains(BlockFlags::SEASONAL_REPLACEABLE)
}

pub(crate) fn seasonal_palette_colour(rgb: u32, snow: f32) -> [f32; 4] {
    let snow = snow.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let ordinary = ((rgb >> shift) & 255) as f32 / 255.0;
        let mixed = ordinary * (1.0 - snow) + SEASONAL_FOLIAGE_SNOW_RGB * snow;
        let stored = (mixed * 0.5 * 255.0) as u8;
        // RenderChunk's seasonal material does not clamp doubled palette RGB.
        // Keep extended linear colour here: normalised RGB10 packing would
        // silently turn the native 1.8 snow tint into 1.0 before texture lookup.
        let decoded = f32::from(stored) * 2.0 / 255.0;
        if decoded <= 0.040_45 {
            decoded / 12.92
        } else {
            ((decoded + 0.055) / 1.055).powf(2.4)
        }
    };
    [channel(16), channel(8), channel(0), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn block(flags: BlockFlags, kind: VisualKind, variant: u32) -> SeasonalFoliageBlock {
        SeasonalFoliageBlock {
            flags,
            kind,
            variant,
        }
    }

    #[test]
    fn main_property_skips_are_not_inferred_from_crossed_or_liquid_geometry() {
        for (main, shelters) in [
            (SeasonalFoliageBlock::AIR, false),
            (
                block(
                    BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
                    VisualKind::Cube,
                    0,
                ),
                false,
            ),
            (
                block(BlockFlags::SEASONAL_REPLACEABLE, VisualKind::Cross, 0),
                false,
            ),
            (block(BlockFlags::empty(), VisualKind::Cross, 0), true),
            (
                block(BlockFlags::SEASONAL_REPLACEABLE, VisualKind::Liquid, 0),
                false,
            ),
            (block(BlockFlags::empty(), VisualKind::Liquid, 0), true),
            (
                block(BlockFlags::SEASONAL_REPLACEABLE, VisualKind::Diagnostic, 0),
                true,
            ),
        ] {
            assert_eq!(
                seasonal_foliage_cell_shelters(Some(main), None),
                shelters,
                "{main:?}"
            );
        }
    }

    #[test]
    fn snow_delegates_non_air_extra_before_height_and_without_main_property_skips() {
        let replaceable = block(BlockFlags::SEASONAL_REPLACEABLE, VisualKind::Cross, 0);
        let flower = block(BlockFlags::empty(), VisualKind::Cross, 0);
        let extra_leaf = block(
            BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            VisualKind::Cube,
            0,
        );
        let diagnostic = block(BlockFlags::empty(), VisualKind::Diagnostic, 0);
        for kind in [VisualKind::Model, VisualKind::Cube] {
            let snow = Some(block(
                BlockFlags::empty(),
                kind,
                BLOCK_VISUAL_VARIANT_TOP_SNOW,
            ));
            assert_eq!(
                seasonal_foliage_cell_shelters(snow, None),
                kind == VisualKind::Cube
            );
            assert_eq!(
                seasonal_foliage_cell_shelters(snow, Some(SeasonalFoliageBlock::AIR)),
                kind == VisualKind::Cube
            );
            assert!(!seasonal_foliage_cell_shelters(snow, Some(replaceable)));
            assert!(seasonal_foliage_cell_shelters(snow, Some(flower)));
            assert!(seasonal_foliage_cell_shelters(snow, Some(extra_leaf)));
            assert!(seasonal_foliage_cell_shelters(snow, Some(diagnostic)));
        }
    }

    #[test]
    fn missing_main_shelters_but_extra_does_not_override_main_property_skip() {
        let stone = block(BlockFlags::CUBE_GEOMETRY, VisualKind::Cube, 0);
        assert!(seasonal_foliage_cell_shelters(None, None));
        assert!(seasonal_foliage_cell_shelters(None, Some(stone)));
        assert!(!seasonal_foliage_cell_shelters(
            Some(SeasonalFoliageBlock::AIR),
            Some(stone)
        ));
        assert!(!seasonal_foliage_cell_shelters(
            Some(block(
                BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
                VisualKind::Cube,
                0
            )),
            Some(stone),
        ));
    }

    #[test]
    fn seasonal_species_indices_share_palette_generation_order() {
        for flags in [
            0,
            MATERIAL_FLAG_EVERGREEN_FOLIAGE,
            MATERIAL_FLAG_BIRCH_FOLIAGE,
        ] {
            let covered = seasonal_foliage_palette_index(flags, false);
            let exposed = seasonal_foliage_palette_index(flags, true);
            assert_eq!(exposed, covered + SEASONAL_FOLIAGE_EXPOSED_OFFSET);
            assert!(exposed < SEASONAL_FOLIAGE_COUNT);
            assert_eq!(seasonal_palette_map(covered), seasonal_palette_map(exposed));
            assert_eq!(
                seasonal_palette_map(covered),
                match flags {
                    MATERIAL_FLAG_EVERGREEN_FOLIAGE => TintMapId::Evergreen,
                    MATERIAL_FLAG_BIRCH_FOLIAGE => TintMapId::Birch,
                    _ => TintMapId::Foliage,
                }
            );
        }
    }

    #[test]
    fn world_snow_palette_retains_native_overbright_rgb_before_texture_multiplication() {
        let colour = seasonal_palette_colour(0x547938, 1.0);
        let decoded = (SEASONAL_FOLIAGE_SNOW_RGB * 0.5 * 255.0).floor() * 2.0 / 255.0;
        let expected = ((decoded + 0.055) / 1.055).powf(2.4);
        assert!(expected > 1.0);
        assert_eq!(colour, [expected, expected, expected, 1.0]);
    }

    #[test]
    fn native_half_intensity_storage_truncates_odd_palette_bytes() {
        let expected = crate::biome::rgb_to_linear(0x123456);
        assert_eq!(seasonal_palette_colour(0x133557, 0.0), expected);
    }

    #[test]
    fn partial_snow_blends_before_palette_quantization() {
        // RGBA8 stores floor((channel / 255 * .5 + 1.8 * .5) * .5 * 255).
        let expected = crate::biome::rgb_to_linear(0xe4e4e4);
        assert_eq!(seasonal_palette_colour(0, 0.5), expected);
        assert_eq!(seasonal_palette_colour(0, -1.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            seasonal_palette_colour(0, 2.0),
            seasonal_palette_colour(0, 1.0)
        );
    }
}
