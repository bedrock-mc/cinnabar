use assets::{
    BLOCK_VISUAL_VARIANT_TOP_SNOW, BlockFlags, CompiledBiomeAssets, MATERIAL_FLAG_BIRCH_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_TINT, SeasonalFoliageBlock, VisualKind,
    seasonal_foliage_cell_shelters, seasonal_foliage_palette_index,
};

use super::{column_exposed, seasonal_tint};

fn cell(kind: VisualKind, flags: BlockFlags, variant: u32) -> SeasonalFoliageBlock {
    SeasonalFoliageBlock {
        flags,
        kind,
        variant,
    }
}

#[test]
fn leaf_particle_gamma_colour_uses_each_shared_covered_and_exposed_species_column() {
    let tints = CompiledBiomeAssets::diagnostic().resolve_live(&[]).unwrap();
    let mut record = tints.records[0];
    record.seasonal_foliage =
        std::array::from_fn(|index| [(index + 1) as f32 / 8.0, 0.25, 0.5, 1.0]);
    for flags in [
        MATERIAL_FLAG_FOLIAGE_TINT,
        MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        MATERIAL_FLAG_BIRCH_FOLIAGE,
    ] {
        for exposed in [false, true] {
            let index = seasonal_foliage_palette_index(flags, exposed);
            let gamma = seasonal_tint(&record, flags, exposed);
            assert_eq!(
                gamma[0],
                super::linear_to_srgb(record.seasonal_foliage[index][0])
            );
            assert_eq!(gamma[3], 1.0);
        }
        assert_ne!(
            seasonal_tint(&record, flags, false),
            seasonal_tint(&record, flags, true)
        );
    }
}

#[test]
fn cpu_particle_colour_clamps_the_world_palettes_overbright_channels_like_native() {
    // Vanilla's CPU seasonal tint clamps doubled palette RGB, whereas
    // its chunk seasonal shader multiplies it into the texture unclamped.
    let tints = CompiledBiomeAssets::diagnostic().resolve_live(&[]).unwrap();
    let mut record = tints.records[0];
    record.seasonal_foliage = [[3.5, 2.0, 1.25, 1.0]; assets::SEASONAL_FOLIAGE_COUNT];
    for exposed in [false, true] {
        let colour = seasonal_tint(&record, 0, exposed);
        // The sRGB transfer can round white one ULP below 1. Keep the
        // clamping assertion strict without requiring exact powf rounding.
        for channel in &colour[..3] {
            assert!(*channel <= 1.0 && (1.0 - *channel).abs() <= f32::EPSILON);
        }
        assert_eq!(colour[3], 1.0);
    }
}

#[test]
fn native_upward_colour_scan_preserves_raw_snow_extra_delegation_and_unknown_shelter() {
    let leaf = cell(VisualKind::Cube, BlockFlags::LEAF_MODEL, 0);
    let thin = cell(
        VisualKind::Model,
        BlockFlags::empty(),
        BLOCK_VISUAL_VARIANT_TOP_SNOW,
    );
    let full = cell(
        VisualKind::Cube,
        BlockFlags::empty(),
        BLOCK_VISUAL_VARIANT_TOP_SNOW,
    );
    let grass = cell(VisualKind::Cross, BlockFlags::SEASONAL_REPLACEABLE, 0);
    let flower = cell(VisualKind::Cross, BlockFlags::empty(), 0);
    let mut column = [
        (Some(leaf), None),
        (Some(thin), None),
        (Some(SeasonalFoliageBlock::AIR), None),
    ];
    let exposed = |cells: &[(Option<SeasonalFoliageBlock>, Option<SeasonalFoliageBlock>)]| {
        column_exposed(0, 0, cells.len() as i32, |y| {
            let (main, extra) = cells[y as usize];
            seasonal_foliage_cell_shelters(main, extra)
        })
    };
    assert!(exposed(&column));
    column[1] = (Some(full), Some(grass));
    assert!(
        exposed(&column),
        "full snow first delegates to replaceable extra"
    );
    for extra in [flower, leaf] {
        column[1] = (Some(thin), Some(extra));
        assert!(
            !exposed(&column),
            "thin snow extra is not a main-block property skip"
        );
    }
    column[1] = (Some(full), None);
    assert!(!exposed(&column));
    column[1] = (None, None);
    assert!(!exposed(&column));
    assert!(!column_exposed(-1, 0, 3, |_| false));
    assert!(!column_exposed(3, 0, 3, |_| false));
}
