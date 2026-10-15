use far_terrain::{SurfaceCell, SurfaceContext, summarise_column};

use crate::fixture::{
    AIR, CAPPED, FLOWER, FOLIAGE_TINT, GRASS, GRASS_TINT, LEAF_RGB, LEAVES, NUB, SAND, SAND_RGB,
    SEASONAL_LEAVES, SEASONAL_MATERIAL, SLAB, TORCH, WATER, column, context, section, tints,
};

fn srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn tinted(base: [u8; 3], tint: [f32; 3]) -> [u8; 3] {
    std::array::from_fn(|channel| (f32::from(base[channel]) * srgb(tint[channel])).round() as u8)
}

#[test]
fn grass_top_is_tinted_and_plants_above_are_seen_through() {
    let ground = section(|_, y, _| match y {
        0..=3 => [GRASS, AIR],
        4 => [FLOWER, AIR],
        _ => [AIR, AIR],
    });
    let summary = summarise_column(&context(), &column([(4, ground)]));
    let cell = summary.cell(5, 9);
    assert_eq!(cell.height, 4 * 16 + 4);
    assert_eq!(cell.water_depth, 0);
    assert_eq!(cell.rgb, tinted([255; 3], GRASS_TINT));
}

#[test]
fn leaves_average_only_their_opaque_texels() {
    let canopy = section(|_, y, _| if y == 7 { [LEAVES, AIR] } else { [AIR, AIR] });
    let summary = summarise_column(&context(), &column([(0, canopy)]));
    assert_eq!(summary.cell(0, 0).height, 8);
    assert_eq!(summary.cell(0, 0).rgb, tinted(LEAF_RGB, FOLIAGE_TINT));
}

#[test]
fn water_records_its_depth_and_blends_toward_the_floor() {
    let lake = section(|_, y, _| match y {
        0..=4 => [SAND, AIR],
        5..=7 => [WATER, AIR],
        _ => [AIR, AIR],
    });
    let open = section(|_, _, _| [WATER, AIR]);
    let summary = summarise_column(&context(), &column([(0, lake), (-1, open)]));
    let shallow = summary.cell(3, 3);
    assert_eq!((shallow.height, shallow.water_depth), (8, 3));
    assert_ne!(shallow.rgb, SAND_RGB);

    let deep = summarise_column(&context(), &column([(0, section(|_, _, _| [WATER, AIR]))]));
    assert_eq!(deep.cell(0, 0).water_depth, 16);
}

#[test]
fn waterlogged_plants_read_as_water() {
    let pond = section(|_, y, _| match y {
        0 => [SAND, AIR],
        1 => [FLOWER, WATER],
        _ => [AIR, AIR],
    });
    let cell = summarise_column(&context(), &column([(0, pond)])).cell(15, 15);
    assert_eq!((cell.height, cell.water_depth), (2, 1));
}

#[test]
fn highest_section_wins_and_empty_columns_stay_empty() {
    let low = section(|_, _, _| [SAND, AIR]);
    let high = section(|x, y, _| {
        if x == 0 && y == 2 {
            [GRASS, AIR]
        } else {
            [AIR, AIR]
        }
    });
    let summary = summarise_column(&context(), &column([(0, low), (3, high)]));
    assert_eq!(summary.cell(0, 4).height, 3 * 16 + 3);
    assert_eq!(summary.cell(1, 4).height, 16);
    assert_eq!(summary.cell(1, 4).rgb, SAND_RGB);

    let air = section(|_, _, _| [AIR, AIR]);
    let empty = summarise_column(&context(), &column([(0, air)]));
    assert!(empty.is_empty());
    assert_eq!(empty.cell(0, 0), SurfaceCell::EMPTY);
}

/// Slabs and stairs are floors seen from above; torches are looked through.
#[test]
fn models_count_as_surface_only_when_their_tops_cover_the_block() {
    let floor = section(|x, y, _| match (x, y) {
        (_, 0) => [GRASS, AIR],
        (0, 1) => [SLAB, AIR],
        (1, 1) => [TORCH, AIR],
        _ => [AIR, AIR],
    });
    let summary = summarise_column(&context(), &column([(0, floor)]));
    assert_eq!(summary.cell(0, 0).height, 2, "the slab is the surface");
    assert_eq!(summary.cell(1, 0).height, 1, "the torch is looked through");
}

/// Downward faces are not seen from above, so a small floating cube is looked through.
#[test]
fn downward_model_faces_do_not_count_as_cover() {
    let floor = section(|x, y, _| match (x, y) {
        (_, 0) => [GRASS, AIR],
        (0, 1) => [NUB, AIR],
        _ => [AIR, AIR],
    });
    let summary = summarise_column(&context(), &column([(0, floor)]));
    assert_eq!(summary.cell(0, 0).height, 1);
}

/// Snow or another solid block on layer 1 under a plant is the surface.
#[test]
fn solid_second_layer_under_a_plant_is_the_surface() {
    let ground = section(|_, y, _| match y {
        0 => [GRASS, AIR],
        1 => [FLOWER, SAND],
        _ => [AIR, AIR],
    });
    let cell = summarise_column(&context(), &column([(0, ground)])).cell(3, 3);
    assert_eq!((cell.height, cell.rgb), (2, SAND_RGB));
}

/// Session air must be seen through even when the asset set does not know its ID.
#[test]
fn session_air_unknown_to_the_assets_is_seen_through() {
    const SESSION_AIR: u32 = 999;
    let ground = section(|_, y, _| match y {
        0 => [SAND, SESSION_AIR],
        _ => [SESSION_AIR, SESSION_AIR],
    });
    let context = SurfaceContext {
        air: SESSION_AIR,
        ..context()
    };
    let cell = summarise_column(&context, &column([(0, ground)])).cell(5, 5);
    assert_eq!((cell.height, cell.rgb), (1, SAND_RGB));
}

/// A model's top colour comes from its upward faces, not the material of its sides.
#[test]
fn models_take_their_top_material() {
    let ground = section(|_, y, _| match y {
        0 => [CAPPED, AIR],
        _ => [AIR, AIR],
    });
    let cell = summarise_column(&context(), &column([(0, ground)])).cell(2, 2);
    assert_eq!((cell.height, cell.rgb), (1, SAND_RGB));
}

/// Seasonal foliage in a seasonal biome takes the seasonal palette, as the terrain shader does.
#[test]
fn seasonal_foliage_uses_the_seasonal_palette() {
    let colours = context().colours;
    let mut seasonal = tints().records[0];
    seasonal.flags |= assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE;
    seasonal.seasonal_foliage =
        [[0.9, 0.9, 0.95, 1.0]; assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT];
    let mut plain = tints().records[0];
    plain.foliage = [0.9, 0.9, 0.95, 1.0];
    assert_eq!(
        colours.tinted(SEASONAL_MATERIAL, Some(&seasonal)),
        colours.tinted(SEASONAL_MATERIAL, Some(&plain))
    );
    assert_ne!(
        colours.tinted(SEASONAL_MATERIAL, Some(&seasonal)),
        colours.tinted(SEASONAL_MATERIAL, Some(&tints().records[0]))
    );
}

/// Surface seasonal leaves are open to the sky, so they take their exposed material.
#[test]
fn surface_seasonal_leaves_use_their_exposed_material() {
    let canopy = section(|_, y, _| match y {
        0 => [SEASONAL_LEAVES, AIR],
        _ => [AIR, AIR],
    });
    let cell = summarise_column(&context(), &column([(0, canopy)])).cell(4, 4);
    assert_eq!((cell.height, cell.rgb), (1, SAND_RGB));
}

/// Sliced extraction preserves synchronous bytes for tinted ground, plants, models and water.
#[test]
fn bounded_slices_match_full_extraction() {
    use far_terrain::{SurfaceScan, SurfaceSlice};
    let context = context();
    let source = column((-16..16).map(|section_y| {
        (
            section_y,
            section(move |x, y, z| {
                if section_y == -16 {
                    [if x % 2 == 0 { GRASS } else { SAND }, AIR]
                } else if section_y == -15 && x < 8 {
                    [WATER, AIR]
                } else if section_y == -14 && y == 0 && z < 3 {
                    [SLAB, AIR]
                } else {
                    [AIR, AIR]
                }
            }),
        )
    }));
    let expected = summarise_column(&context, &source).encode();
    let mut scan = SurfaceScan::default();
    let mut cursor = None;
    let mut steps = 0;
    loop {
        let slice = source.surface_slice(cursor, SurfaceSlice::MIN_BYTES);
        assert!(slice.retained_bytes() <= SurfaceSlice::MIN_BYTES);
        scan.apply(&context, &slice);
        steps += 1;
        cursor = slice.next();
        if cursor.is_none() || scan.is_complete() {
            break;
        }
        assert!(steps < 64);
    }
    assert_eq!(scan.finish().encode(), expected);
}

/// A copied slice owns only visible block layers and biomes, independent of source mutation.
#[test]
fn surface_slice_does_not_retain_the_source_sections() {
    use far_terrain::{ColumnSnapshot, SurfaceScan, SurfaceSlice};
    use std::{collections::BTreeMap, sync::Arc};
    let ground = section(|_, _, _| [GRASS, AIR]);
    let weak = Arc::downgrade(&ground);
    let source = ColumnSnapshot::new(Arc::new(BTreeMap::from([(0, ground)])), Vec::new());
    let expected = summarise_column(&context(), &source).encode();
    let slice = source.surface_slice(None, SurfaceSlice::MIN_BYTES);
    drop(source);
    assert!(weak.upgrade().is_none());
    let mut scan = SurfaceScan::default();
    scan.apply(&context(), &slice);
    assert_eq!(scan.finish().encode(), expected);
}

/// The minimum slice fits the decoder's maximum palette and packed word sizes.
#[test]
fn largest_decoded_surface_section_fits_the_minimum_slice() {
    use far_terrain::{ColumnSnapshot, SurfaceSlice};
    use std::{collections::BTreeMap, sync::Arc};
    use world::{
        BLOCKS_PER_SUB_CHUNK, DecodedBiomeColumn, MAX_PALETTE_ENTRIES, RawBiomeIds, RawBlockIds,
        SubChunk,
    };

    /// Encodes the non-negative signed varints used by the storage decoder.
    fn varint(out: &mut Vec<u8>, value: usize) {
        let mut value = (value as u32) << 1;
        while value >= 128 {
            out.push((value as u8) | 128);
            value >>= 7;
        }
        out.push(value as u8);
    }

    let mut storage = vec![33]; // Sixteen-bit network indices: two blocks per word.
    storage.resize(1 + BLOCKS_PER_SUB_CHUNK / 2 * std::mem::size_of::<u32>(), 0);
    varint(&mut storage, MAX_PALETTE_ENTRIES);
    for _ in 0..MAX_PALETTE_ENTRIES {
        varint(&mut storage, AIR as usize);
    }
    let mut blocks = vec![9, 2, 0];
    blocks.extend_from_slice(&storage);
    blocks.extend_from_slice(&storage);
    let section = Arc::new(SubChunk::decode(&blocks, &RawBlockIds { air: AIR }));
    let biome = DecodedBiomeColumn::decode(0, 1, &storage, &RawBiomeIds { default_biome: 0 });
    let source = ColumnSnapshot::new(
        Arc::new(BTreeMap::from([(0, section)])),
        vec![(0, biome.storage(0).unwrap())],
    );
    let slice = source.surface_slice(None, SurfaceSlice::MIN_BYTES);
    assert!(slice.retained_bytes() <= SurfaceSlice::MIN_BYTES);
    assert!(slice.next().is_none());
    let mut scan = far_terrain::SurfaceScan::default();
    scan.apply(&context(), &slice);
    assert_eq!(
        scan.finish().encode(),
        summarise_column(&context(), &source).encode()
    );
}
