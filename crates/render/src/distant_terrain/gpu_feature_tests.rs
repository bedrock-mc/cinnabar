//! On a native adapter, the production kernels and pipelines draw the crack list the
//! neighbour's level needs, tint faces by the biome under each fragment, fade toward the fog,
//! clamp full lists and count what they drop, start again from nothing after a visible-set
//! reset, hide coarse sections over resident chunk columns and fill the statistics.

use std::sync::Arc;

use bevy::math::Vec3;
use lod::{
    colour::{BiomeTints, FaceColour, MaterialColours, TintClass},
    draw::{NeighbourLevel, NeighbourLevels},
    fixtures::blocks,
    geometry::{Face, SectionPos},
    mesh::output::CLUSTER_QUADS,
    select::shown::DrawnSection,
};
use render_scene::distant_terrain::{DistantTerrainColourTables, ResidentColumnMask};

use super::{
    gpu_fixture::{EYE, Fixture, SIDE, STONE, close, section, target, update},
    layout::POOL_UNIT_BYTES,
    test_meshes::{mesh, mesh_beside, section_of},
};

fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
    SectionPos::new(level, x, y, z).expect("position")
}

#[test]
fn crack_lists_draw_only_toward_the_neighbour_level_that_needs_them() {
    let Some(mut fixture) = Fixture::new("distant terrain crack lists") else {
        return;
    };
    // Ground below y 8 beside a solid same-level section across +X, under air one level
    // coarser there: its +X faces are only in the to-coarser list.
    let fine = pos(0, 1, 0, 0);
    let ground = |_: usize, y: usize, _: usize| (y < 8).then_some(blocks::STONE);
    let solid = |_: usize, _: usize, _: usize| Some(blocks::STONE);
    let air = |_: usize, _: usize, _: usize| None;
    let around = [
        section_of(pos(0, 2, 0, 0), solid, |_, _| 1),
        section_of(pos(1, 1, 0, 0), air, |_, _| 1),
    ];
    let mesh = mesh_beside(&section_of(fine, ground, |_, _| 1), &around);
    // East of the section, level with the ground, facing its +X side; every other face
    // turns away from this eye.
    let eye = [100, 4, 16];
    let look = Vec3::new(60.0, 4.0, 16.0);
    for (level, drawn) in [
        (NeighbourLevel::None, false),
        (NeighbourLevel::Same, false),
        (NeighbourLevel::Finer, false),
        (NeighbourLevel::Coarser, true),
    ] {
        fixture.reset();
        let section = DrawnSection {
            pos: fine,
            mesh: Arc::clone(&mesh),
            neighbours: NeighbourLevels::NONE.with(Face::PosX, level),
        };
        let frame = fixture.frame(Some(update(vec![section])), eye, look, None);
        assert_eq!(
            frame.counters[0] > 0,
            drawn,
            "{level:?}: {:?}",
            frame.counters
        );
        assert_eq!(frame.centre() != [0, 0, 0], drawn, "{level:?}");
    }
}

#[test]
fn faces_take_the_tint_of_the_biome_under_each_fragment() {
    let Some(mut fixture) = Fixture::new("distant terrain biome tints") else {
        return;
    };
    let mut materials = MaterialColours::new();
    let white = FaceColour::from_linear([1.0; 3], TintClass::Grass, 1.0);
    for _ in 1..=blocks::SAND {
        assert!(materials.push([white; 6]));
    }
    let mut red = [[1.0; 3]; TintClass::COUNT];
    red[TintClass::Grass.index()] = [1.0, 0.0, 0.0];
    let mut blue = [[1.0; 3]; TintClass::COUNT];
    blue[TintClass::Grass.index()] = [0.0, 0.0, 1.0];
    let tints = BiomeTints::new(
        Box::new([0, 1, 2]),
        Box::new([[[1.0; 3]; TintClass::COUNT], red, blue]),
    );
    fixture.set_colours(&DistantTerrainColourTables {
        materials: Arc::new(materials),
        biomes: Some(Arc::new(tints)),
    });
    // Raw biome 1 west of x 16 and 2 east of it, under ground seen from the south.
    let origin = pos(0, 0, 0, 0);
    let ground = section_of(
        origin,
        |_, y, _| (y < 4).then_some(blocks::STONE),
        |x, _| if x < 16 { 1 } else { 2 },
    );
    let drawn = DrawnSection {
        pos: origin,
        mesh: mesh_beside(&ground, &[]),
        neighbours: NeighbourLevels::NONE,
    };
    let frame = fixture.frame(Some(update(vec![drawn])), EYE, target(), None);
    let [left, right] = [SIDE * 3 / 8, SIDE * 5 / 8].map(|x| frame.pixel(x, SIDE / 2));
    let red = |pixel: [u8; 3]| pixel[0] > 200 && pixel[2] < 10;
    let blue = |pixel: [u8; 3]| pixel[2] > 200 && pixel[0] < 10;
    assert!(
        (red(left) && blue(right)) || (blue(left) && red(right)),
        "each side takes its own biome's tint: {left:?} {right:?}"
    );
}

#[test]
fn faces_fade_toward_the_fog_and_a_hard_fog_edge_stays_finite() {
    let Some(mut fixture) = Fixture::new("distant terrain fog") else {
        return;
    };
    const FOG: [u8; 3] = [255, 0, 255];
    let fog_colour = [1.0, 0.0, 1.0];
    let origin = pos(0, 0, 0, 0);
    let draw = |fixture: &mut Fixture, start: f32, end: f32| {
        fixture.fog = (fog_colour, start, end);
        fixture.reset();
        let ground = section(origin, 4, 0);
        fixture
            .frame(Some(update(vec![ground])), EYE, target(), None)
            .centre()
    };
    // The ground's centre lies about 49 blocks from the eye.
    let clear = draw(&mut fixture, 1.0e6, 1.0e6);
    assert!(
        close(clear, STONE),
        "start == end beyond the ground: {clear:?}"
    );
    let hidden = draw(&mut fixture, 1.0, 1.0);
    assert!(
        close(hidden, FOG),
        "start == end before the ground: {hidden:?}"
    );
    let half = draw(&mut fixture, 0.0, 98.0);
    assert!(!close(half, STONE) && !close(half, FOG), "{half:?}");
    assert!(half[1] < STONE[1] && half[2] > STONE[2], "{half:?}");
}

#[test]
fn full_lists_clamp_and_count_what_they_drop() {
    // A pool of 256 cluster slots, a quarter of them waiting to fill.
    let pool = 16_383 * POOL_UNIT_BYTES;
    let Some(mut fixture) = Fixture::with_pool("distant terrain list overflow", pool) else {
        return;
    };
    let capacity = fixture.capacity_clusters();
    // A row of sections, each with a block at either end of one row of voxels: their ±X
    // clusters span the eye's X, so four of each section's six clusters face it.
    let sections: Vec<DrawnSection> = (0..42)
        .map(|z| {
            let at = pos(0, 0, 0, z);
            DrawnSection {
                pos: at,
                mesh: mesh(at, |x, y, z| {
                    (y == 0 && z == 16 && (x == 0 || x == 31)).then_some(blocks::STONE)
                }),
                neighbours: NeighbourLevels::NONE,
            }
        })
        .collect();
    let eye = [16, 8, -40];
    let look = Vec3::new(16.0, 0.0, 200.0);
    let first = fixture.frame(Some(update(sections)), eye, look, None);
    let [visible, newly, _, dropped] = first.counters;
    assert!(
        visible + newly > capacity,
        "the fixture overfills the shared list: {:?}",
        first.counters
    );
    assert_eq!(dropped, visible + newly - capacity, "{:?}", first.counters);
    let second = fixture.frame(None, eye, look, None);
    assert_eq!(
        second.counters,
        [visible, 0, 0, 0],
        "pass A holds every visible cluster"
    );
}

#[test]
fn a_visible_set_reset_forgets_what_was_visible_last_frame() {
    let Some(mut fixture) = Fixture::new("distant terrain visible-set reset") else {
        return;
    };
    let origin = pos(0, 0, 0, 0);
    let first = fixture.frame(
        Some(update(vec![section(origin, 4, 0)])),
        EYE,
        target(),
        None,
    );
    let visible = first.counters[0];
    assert!(visible > 0);
    let second = fixture.frame(None, EYE, target(), None);
    assert_eq!(second.counters[1], 0, "drawn in pass A");
    // The same section in the same slots after a reset is new again: drawn in pass B.
    fixture.reset();
    let third = fixture.frame(
        Some(update(vec![section(origin, 4, 0)])),
        EYE,
        target(),
        None,
    );
    assert_eq!(third.counters[..2], [visible, visible]);
    assert!(close(third.centre(), STONE), "{:?}", third.centre());
}

#[test]
fn coarse_sections_over_resident_columns_are_hidden_whole() {
    let Some(mut fixture) = Fixture::new("distant terrain coarse over resident") else {
        return;
    };
    // A level-1 section spans chunk columns 0..4 on both axes.
    let coarse = pos(1, 0, 0, 0);
    let eye = [32, 40, -60];
    let look = Vec3::new(32.0, 4.0, 32.0);
    let drawn = fixture.frame(Some(update(vec![section(coarse, 4, 0)])), eye, look, None);
    assert!(drawn.counters[0] > 0);
    let elsewhere = ResidentColumnMask::new([0, 0], [[10, 10]]);
    let frame = fixture.frame(None, eye, look, Some(&elsewhere));
    assert!(
        frame.counters[0] > 0,
        "a column outside its footprint hides nothing"
    );
    let under = ResidentColumnMask::new([0, 0], [[3, 3]]);
    let frame = fixture.frame(None, eye, look, Some(&under));
    assert_eq!(
        frame.counters[0], 0,
        "one resident column under it hides it all"
    );
    assert_eq!(frame.bins.sections.iter().sum::<u32>(), 0);
    // Pass A drew last frame's list before culling saw the column; from the next frame
    // nothing draws.
    let next = fixture.frame(None, eye, look, Some(&under));
    assert!(next.pixels.chunks(4).all(|pixel| pixel[..3] == [0, 0, 0]));
}

#[test]
fn visible_sections_and_clusters_fill_the_size_bins() {
    let Some(mut fixture) = Fixture::new("distant terrain statistics") else {
        return;
    };
    let origin = pos(0, 0, 0, 0);
    let frame = fixture.frame(
        Some(update(vec![section(origin, 4, 0)])),
        EYE,
        target(),
        None,
    );
    let visible = frame.counters[0];
    assert!(visible > 0);
    assert_eq!(
        frame.bins.sections.iter().sum::<u32>(),
        1,
        "{:?}",
        frame.bins
    );
    assert_eq!(frame.bins.clusters.iter().sum::<u32>(), visible);
    assert_eq!(
        frame.bins.sections[0], 0,
        "the section spans more than a few pixels"
    );
    // Flat ground merges into a few large quads, so most slots are empty.
    let slots = visible * CLUSTER_QUADS as u32;
    assert!(frame.bins.degenerate > slots / 2 && frame.bins.degenerate < slots);
    // The next frame starts its statistics afresh.
    let away = fixture.frame(None, EYE, Vec3::new(16.0, 20.0, -100.0), None);
    assert_eq!(away.counters[0], 0);
    assert_eq!(away.bins, Default::default());
}
