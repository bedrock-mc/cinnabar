use far_terrain::{SurfaceCell, summarise_column};

use crate::fixture::{
    AIR, FLOWER, FOLIAGE_TINT, GRASS, GRASS_TINT, LEAF_RGB, LEAVES, SAND, SAND_RGB, WATER, column,
    context, section,
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
