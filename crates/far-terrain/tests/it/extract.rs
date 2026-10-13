use far_terrain::{SurfaceCell, SurfaceContext, summarise_column};

use crate::fixture::{
    AIR, CAPPED, FLOWER, FOLIAGE_TINT, GRASS, GRASS_TINT, LEAF_RGB, LEAVES, NUB, SAND, SAND_RGB,
    SEASONAL_MATERIAL, SLAB, TORCH, WATER, column, context, section, tints,
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
