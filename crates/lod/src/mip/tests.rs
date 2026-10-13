use std::collections::HashMap;

use world::ChunkKey;

use super::*;
use crate::{
    biome_grid::UNKNOWN_BIOME,
    classify::TableClassifier,
    exposure::Neighbourhood,
    fixtures::{biome_column, blocks, fixture_classifier, terrain_column},
    geometry::{SECTION_VOXELS, voxel_index},
    mip_policy::{GroupTraits, MipRule},
    sampler::{ColumnSnapshot, sample_column},
};

pub(super) fn material(id: u32) -> MaterialId {
    MaterialId::new(id).expect("fixture id")
}

pub(super) fn block(id: u32, light: Light) -> Voxel {
    Voxel::new(
        material(id),
        light,
        crate::voxel::FULL_COVERAGE,
        id == blocks::WATER,
    )
}

/// A 2×2×2 group: `top` fills octants with y = 1, `bottom` those with y = 0.
fn group(bottom: [MipChild; 4], top: [MipChild; 4]) -> [MipChild; 8] {
    [
        bottom[0], bottom[1], bottom[2], bottom[3], top[0], top[1], top[2], top[3],
    ]
}

fn child(id: u32, class: BlockClass, exposed: bool) -> MipChild {
    let voxel = if class == BlockClass::Air {
        Voxel::air(Light::FULL_SKY)
    } else {
        block(id, Light::DARK)
    };
    MipChild {
        voxel,
        class,
        exposed,
    }
}

fn air() -> MipChild {
    child(blocks::AIR, BlockClass::Air, false)
}

fn decided(children: [MipChild; 8]) -> (Voxel, MipRule) {
    match mip_group(&children) {
        GroupDecision::Voxel(voxel, rule) => (voxel, rule),
        GroupDecision::HiddenFallback(_) => panic!("unexpected hidden fallback"),
    }
}

#[test]
fn policy_table_rows() {
    let stone = |exposed| child(blocks::STONE, BlockClass::Opaque, exposed);
    let grass = |exposed| child(blocks::GRASS, BlockClass::Opaque, exposed);
    let dirt = |exposed| child(blocks::DIRT, BlockClass::Opaque, exposed);
    let water = |exposed| child(blocks::WATER, BlockClass::Water, exposed);
    let leaves = |exposed| child(blocks::LEAVES, BlockClass::Leaves, exposed);
    let hidden = |exposed| MipChild {
        voxel: Voxel::HIDDEN,
        class: BlockClass::Opaque,
        exposed,
    };
    let rows: [(&str, [MipChild; 8], Voxel, MipRule); 11] = [
        (
            "all air",
            [air(); 8],
            Voxel::air(Light::FULL_SKY),
            MipRule::AllAir,
        ),
        (
            "one child keeps the voxel solid",
            group([stone(true), air(), air(), air()], [air(); 4]),
            Voxel::new(material(blocks::STONE), Light::FULL_SKY, 1, false),
            MipRule::ExposedMajority,
        ),
        (
            "water surface wins over the seabed",
            group([stone(true); 4], [water(true), water(true), air(), air()]),
            Voxel::new(material(blocks::WATER), Light::new(8, 0), 5, true),
            MipRule::WaterSurface,
        ),
        (
            "one water block does not outvote exposed solid neighbours",
            group(
                [dirt(false); 4],
                [water(true), grass(true), grass(true), grass(true)],
            ),
            Voxel::new(material(blocks::GRASS), Light::DARK, 7, true),
            MipRule::ExposedMajority,
        ),
        (
            "water with as many solid top children is not a surface",
            group(
                [dirt(false); 4],
                [water(true), water(true), grass(true), grass(true)],
            ),
            Voxel::new(material(blocks::GRASS), Light::DARK, 7, true),
            MipRule::ExposedMajority,
        ),
        (
            "bottom water is only a candidate",
            group(
                [water(true), water(true), stone(false), stone(false)],
                [air(); 4],
            ),
            Voxel::new(material(blocks::WATER), Light::new(10, 0), 4, true),
            MipRule::ExposedMajority,
        ),
        (
            "leaves are a material but not see-through",
            group([leaves(true); 4], [air(); 4]),
            Voxel::new(material(blocks::LEAVES), Light::FULL_SKY, 4, false),
            MipRule::ExposedMajority,
        ),
        (
            "unexposed children do not vote",
            group(
                [stone(false), stone(false), stone(false), grass(true)],
                [grass(true), air(), air(), air()],
            ),
            Voxel::new(material(blocks::GRASS), Light::FULL_SKY, 4, false),
            MipRule::ExposedMajority,
        ),
        (
            "ties go to the higher child",
            group(
                [dirt(true), dirt(true), air(), air()],
                [grass(true), grass(true), air(), air()],
            ),
            Voxel::new(material(blocks::GRASS), Light::FULL_SKY, 4, false),
            MipRule::ExposedMajority,
        ),
        (
            "enclosed groups take the majority",
            group(
                [stone(false), stone(false), dirt(false), stone(false)],
                [hidden(false); 4],
            ),
            Voxel::new(material(blocks::STONE), Light::DARK, 7, false),
            MipRule::EnclosedMajority,
        ),
        (
            "hidden groups stay hidden",
            [hidden(false); 8],
            Voxel::HIDDEN,
            MipRule::AllHidden,
        ),
    ];
    for (name, children, voxel, rule) in rows {
        assert_eq!(decided(children), (voxel, rule), "{name}");
    }
    let mut same_height = group([dirt(true), grass(true), air(), air()], [air(); 4]);
    assert_eq!(
        decided(same_height).0.material(),
        material(blocks::DIRT),
        "lower id wins a full tie"
    );
    same_height[1] = stone(true);
    assert_eq!(decided(same_height).0.material(), material(blocks::STONE));
}

#[test]
fn exposed_hidden_children_request_the_fallback() {
    let hidden = MipChild {
        voxel: Voxel::HIDDEN,
        class: BlockClass::Opaque,
        exposed: true,
    };
    let decision = mip_group(&group([hidden; 4], [air(); 4]));
    assert_eq!(
        decision,
        GroupDecision::HiddenFallback(GroupTraits {
            light: Light::FULL_SKY,
            coverage: 4,
            water: false,
        })
    );
}

#[test]
fn light_mean_rounds_by_the_named_constant() {
    let mut children = [air(); 8];
    for (index, child) in children.iter_mut().enumerate() {
        child.voxel = Voxel::air(Light::new(
            if index < 4 { 15 } else { 14 },
            if index < 1 { 3 } else { 0 },
        ));
    }
    let (voxel, _) = decided(children);
    assert_eq!(
        voxel.light().sky(),
        crate::mip_policy::LIGHT_MEAN_ROUNDING.mean(15 * 4 + 14 * 4, 8) as u8
    );
    assert_eq!(voxel.light(), Light::new(15, 0));
}

/// Fine section with `fill` everywhere and `cells` overridden.
fn fine_section(fill: Voxel, cells: &[([usize; 3], Voxel)]) -> DenseSection {
    let mut section = DenseSection::filled(fill, 0);
    for &([x, y, z], voxel) in cells {
        section.set(x, y, z, voxel);
    }
    section
}

fn mip_whole(child: &DenseSection, classes: &TableClassifier) -> (DenseSection, MipOutcome) {
    let mut parent = DenseSection::filled(Voxel::AIR, 0);
    let outcome = mip_into_parent(
        child,
        &Neighbourhood::UNKNOWN,
        Octant::ALL[0],
        VoxelBox::FULL,
        classes,
        &mut parent,
    );
    (parent, outcome)
}

/// Hidden voxels exposed by a water neighbour: a listed hidden-fallback case.
fn exposed_hidden_fixture(extra: &[([usize; 3], Voxel)]) -> DenseSection {
    let water = block(blocks::WATER, Light::FULL_SKY);
    let mut cells = vec![([2, 1, 1], water), ([2, 0, 1], water)];
    cells.extend_from_slice(extra);
    fine_section(Voxel::HIDDEN, &cells)
}

#[test]
fn hidden_fallback_uses_the_parent_group_first() {
    let classes = fixture_classifier();
    let grass = block(blocks::GRASS, Light::FULL_SKY);
    let fine =
        exposed_hidden_fixture(&[([3, 3, 3], Voxel::air(Light::FULL_SKY)), ([3, 2, 3], grass)]);
    let (parent, outcome) = mip_whole(&fine, &classes);
    assert_eq!(parent.get(0, 0, 0).material(), material(blocks::GRASS));
    assert!(outcome.hidden_fallbacks > outcome.unresolved_hidden);
}

#[test]
fn hidden_fallback_then_searches_the_column() {
    let classes = fixture_classifier();
    let dirt = block(blocks::DIRT, Light::FULL_SKY);
    let fine = exposed_hidden_fixture(&[
        ([1, 21, 0], Voxel::air(Light::FULL_SKY)),
        ([1, 20, 0], dirt),
    ]);
    let (parent, outcome) = mip_whole(&fine, &classes);
    assert_eq!(parent.get(0, 0, 0).material(), material(blocks::DIRT));
    assert!(outcome.hidden_fallbacks > outcome.unresolved_hidden);
}

#[test]
fn unresolved_hidden_fallback_stays_hidden_and_is_counted() {
    let classes = fixture_classifier();
    let (parent, outcome) = mip_whole(&exposed_hidden_fixture(&[]), &classes);
    assert_eq!(parent.get(0, 0, 0), Voxel::HIDDEN);
    assert!(outcome.unresolved_hidden >= 1);
}

#[test]
fn coarse_occupancy_contains_fine_occupancy_exactly() {
    let classes = fixture_classifier();
    let mut state = 0x9e37_79b9_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let palette = [
        Voxel::air(Light::FULL_SKY),
        Voxel::HIDDEN,
        block(blocks::STONE, Light::DARK),
        block(blocks::WATER, Light::new(12, 0)),
        block(blocks::LEAVES, Light::new(9, 2)),
        block(blocks::GLASS, Light::new(15, 4)),
    ];
    for density in [1, 3, 6] {
        let mut fine = DenseSection::filled(Voxel::AIR, 0);
        for voxel in fine.voxels_mut().iter_mut() {
            *voxel = if next() % 8 < density {
                palette[1 + next() as usize % 5]
            } else {
                palette[0]
            };
        }
        let (parent, _) = mip_whole(&fine, &classes);
        for cy in 0..CHILD_SPAN {
            for cz in 0..CHILD_SPAN {
                for cx in 0..CHILD_SPAN {
                    let any_child = Octant::ALL.iter().any(|o| {
                        !fine
                            .get(cx * 2 + o.x(), cy * 2 + o.y(), cz * 2 + o.z())
                            .is_air()
                    });
                    assert_eq!(
                        !parent.get(cx, cy, cz).is_air(),
                        any_child,
                        "density {density} at {cx},{cy},{cz}"
                    );
                }
            }
        }
    }
}

/// Synthetic world: chunks `0..8` × `0..8`, sub-chunks `0..8`, i.e. one level-2 section.
pub(super) struct World {
    pub(super) level0: HashMap<SectionPos, DenseSection>,
    columns: Vec<((i32, i32), crate::sampler::ColumnSample)>,
}

/// Samples the world's columns, with `sub_chunks(cx, cz)` choosing each column's
/// sub-chunk Ys and `biomes(cx, cz)` its per-sub-chunk biomes from Y 0.
fn sample_world_with(
    sub_chunks: impl Fn(i32, i32) -> std::ops::Range<i32>,
    biomes: impl Fn(i32, i32) -> Vec<u32>,
    edit: impl Fn(i32, i32) -> bool,
) -> World {
    let mut classifier = fixture_classifier();
    let mut level0 = HashMap::new();
    let mut columns = Vec::new();
    for cx in 0..8 {
        for cz in 0..8 {
            let mut column = terrain_column(cx, cz, sub_chunks(cx, cz));
            if edit(cx, cz) {
                let flattened = crate::fixtures::uniform_sub_chunk(blocks::SAND);
                column.insert(2, std::sync::Arc::new(flattened));
            }
            let biomes = biome_column(0, &biomes(cx, cz));
            let snapshot = ColumnSnapshot {
                chunk: ChunkKey::new(0, cx, cz),
                sub_chunks: &column,
                biomes: &biomes,
                default_biome: 0,
                light: None,
            };
            let sample = sample_column(&snapshot, &mut classifier).expect("column has a key");
            for sub in &sample.sub_chunks {
                let section = level0.entry(sub.section()).or_insert_with(unknown_section);
                sub.write_into(section);
            }
            columns.push(((cx, cz), sample));
        }
    }
    World { level0, columns }
}

pub(super) fn sample_world(edit: impl Fn(i32, i32) -> bool) -> World {
    sample_world_with(|_, _| 0..8, |_, _| vec![1, 1, 2, 2, 3, 3, 4, 4], edit)
}

fn root() -> SectionPos {
    SectionPos::new(2, 0, 0, 0).expect("root")
}

/// Sections and child masks by position, as a store would hold them.
#[derive(Default)]
struct Store {
    sections: HashMap<SectionPos, DenseSection>,
    masks: HashMap<SectionPos, ChildMask>,
}

/// Level-1 and level-2 sections and masks rebuilt from the world's level-0 sections.
fn from_scratch(world: &World, classes: &TableClassifier) -> Store {
    let mut store = Store::default();
    let child = |pos: SectionPos, octant| pos.child(octant).expect("child");
    for octant in Octant::ALL {
        let pos = child(root(), octant);
        let children = Octant::ALL.map(|o| match world.level0.get(&child(pos, o)) {
            Some(section) => ChildInput::Present {
                section,
                state: ChildState::of_level_zero(section),
            },
            None => ChildInput::Unknown,
        });
        // A reused scratch section: nothing of an earlier re-mip may survive.
        let mut out = DenseSection::filled(block(blocks::GLASS, Light::DARK), 9);
        let outcome =
            remip_from_children(children, &[Neighbourhood::UNKNOWN; 8], classes, &mut out);
        store.sections.insert(pos, out);
        store.masks.insert(pos, outcome.children);
    }
    let children = Octant::ALL.map(|o| {
        let pos = child(root(), o);
        ChildInput::Present {
            section: &store.sections[&pos],
            state: store.masks[&pos].summary(),
        }
    });
    let mut level2 = unknown_section();
    let outcome = remip_from_children(children, &[Neighbourhood::UNKNOWN; 8], classes, &mut level2);
    store.sections.insert(root(), level2);
    store.masks.insert(root(), outcome.children);
    store
}

/// Replays every sampled sub-chunk through the local chain, in the given order.
fn incremental(
    samples: &[&crate::sampler::SampledSubChunk],
    classes: &TableClassifier,
    store: &mut Store,
) -> ChainOutcome {
    let mut total = ChainOutcome::default();
    for sub in samples {
        let positions = [sub.section(), sub.section().parent().expect("l1"), root()];
        let mut chain: Vec<DenseSection> = positions
            .iter()
            .map(|pos| store.sections.remove(pos).unwrap_or_else(unknown_section))
            .collect();
        let mut masks: Vec<ChildMask> = positions
            .iter()
            .map(|pos| store.masks.get(pos).copied().unwrap_or(ChildMask::UNKNOWN))
            .collect();
        sub.write_into(&mut chain[0]);
        let outcome = mip_chain(
            &mut chain,
            &mut masks,
            &[Neighbourhood::UNKNOWN; 2],
            sub.section(),
            sub.region(),
            classes,
        );
        total.updated_levels = total.updated_levels.max(outcome.updated_levels);
        total.hidden_fallbacks += outcome.hidden_fallbacks;
        for ((pos, section), mask) in positions.into_iter().zip(chain).zip(masks) {
            store.sections.insert(pos, section);
            store.masks.insert(pos, mask);
        }
    }
    total
}

fn assert_levels_match(store: &Store, world: &World, classes: &TableClassifier) {
    let expected = from_scratch(world, classes);
    for (pos, section) in &expected.sections {
        let actual = &store.sections[pos];
        let mismatch = (0..SECTION_VOXELS).find(|&i| actual.voxels()[i] != section.voxels()[i]);
        assert_eq!(mismatch, None, "level {} {pos:?}", pos.level());
        assert_eq!(actual.biomes(), section.biomes(), "biomes {pos:?}");
        assert_eq!(store.masks[pos], expected.masks[pos], "mask {pos:?}");
    }
}

fn all_sub_chunks(world: &World) -> Vec<&crate::sampler::SampledSubChunk> {
    world
        .columns
        .iter()
        .flat_map(|(_, c)| c.sub_chunks.iter())
        .collect()
}

#[test]
fn local_chain_equals_remip_from_children_in_any_order() {
    let classes = fixture_classifier();
    let world = sample_world(|_, _| false);
    let mut forward = all_sub_chunks(&world);
    let mut store = Store::default();
    let outcome = incremental(&forward, &classes, &mut store);
    assert_eq!(outcome.updated_levels, 2);
    assert_levels_match(&store, &world, &classes);

    forward.reverse();
    let mut reversed = Store::default();
    incremental(&forward, &classes, &mut reversed);
    assert_levels_match(&reversed, &world, &classes);
}

#[test]
fn partial_columns_and_vertical_biome_changes_mip_equally_in_any_order() {
    let classes = fixture_classifier();
    // Columns reach different heights, so upper sections are known only in some columns,
    // and each column's biomes differ from its neighbours'.
    let world = sample_world_with(
        |cx, cz| 0..[2, 4, 6, 8][((cx + cz) % 4) as usize],
        |cx, cz| {
            (0..8)
                .map(|y| 10 + (cx * 3 + cz) as u32 % 5 + y as u32)
                .collect()
        },
        |_, _| false,
    );
    let mut order = all_sub_chunks(&world);
    let mut store = Store::default();
    incremental(&order, &classes, &mut store);
    assert_levels_match(&store, &world, &classes);
    order.reverse();
    let mut reversed = Store::default();
    incremental(&order, &classes, &mut reversed);
    assert_levels_match(&reversed, &world, &classes);
    let biome = |store: &Store| store.sections[&root()].biomes().get(0, 0);
    assert_ne!(biome(&store), UNKNOWN_BIOME);
}

#[test]
fn local_chain_stays_exact_after_edits_and_stops_on_unchanged_parents() {
    let classes = fixture_classifier();
    let world = sample_world(|_, _| false);
    let all = all_sub_chunks(&world);
    let mut store = Store::default();
    incremental(&all, &classes, &mut store);

    let resend = incremental(&all[..1], &classes, &mut store);
    assert_eq!(
        resend.updated_levels, 0,
        "an unchanged sub-chunk changes no parent"
    );

    let is_edited = |cx: i32, cz: i32| (cx + cz) % 3 == 0;
    let edited = sample_world(is_edited);
    let changed: Vec<_> = edited
        .columns
        .iter()
        .filter(|((cx, cz), _)| is_edited(*cx, *cz))
        .flat_map(|(_, c)| c.sub_chunks.iter())
        .collect();
    incremental(&changed, &classes, &mut store);
    assert_levels_match(&store, &edited, &classes);
}

#[test]
fn the_chain_records_known_children_even_when_no_voxel_changes() {
    let classes = fixture_classifier();
    // A dark cave: every voxel equals the unknown fill, so no parent word changes.
    let cave = DenseSection::filled(UNKNOWN_CHILD_FILL, UNKNOWN_BIOME);
    let base = SectionPos::new(0, 0, 0, 0).expect("pos");
    let mut chain = vec![cave, unknown_section(), unknown_section()];
    let mut masks = vec![ChildMask::UNKNOWN; 3];
    let outcome = mip_chain(
        &mut chain,
        &mut masks,
        &[Neighbourhood::UNKNOWN; 2],
        base,
        VoxelBox::FULL,
        &classes,
    );
    assert_eq!(outcome.updated_levels, 1, "only the parent's mask changed");
    assert_eq!(masks[1].get(base.octant_in_parent()), ChildState::Empty);
    assert_eq!(
        masks[1].summary(),
        ChildState::Unknown,
        "seven unexplored siblings keep the parent unknown"
    );
    assert_eq!(masks[2].get(Octant::ALL[0]), ChildState::Unknown);
    assert_eq!(chain[1], unknown_section());

    let again = mip_chain(
        &mut chain,
        &mut masks,
        &[Neighbourhood::UNKNOWN; 2],
        base,
        VoxelBox::FULL,
        &classes,
    );
    assert_eq!(again.updated_levels, 0);

    chain[0].set(1, 1, 1, block(blocks::STONE, Light::DARK));
    let filled = mip_chain(
        &mut chain,
        &mut masks,
        &[Neighbourhood::UNKNOWN; 2],
        base,
        VoxelBox::voxel(1, 1, 1).expect("box"),
        &classes,
    );
    assert_eq!(filled.updated_levels, 2);
    assert_eq!(masks[2].summary(), ChildState::NonEmpty);
}

#[test]
fn remip_fills_absent_children_and_reports_their_state() {
    let classes = fixture_classifier();
    let stone = fine_section(block(blocks::STONE, Light::DARK), &[]);
    let air = DenseSection::filled(Voxel::air(Light::FULL_SKY), 0);
    let mut children = [ChildInput::Unknown; 8];
    children[1] = ChildInput::Empty;
    children[2] = ChildInput::Present {
        section: &stone,
        state: ChildState::NonEmpty,
    };
    children[3] = ChildInput::Present {
        section: &air,
        state: ChildState::Empty,
    };
    // Unknown fill air whose own mask says nothing is known stays unknown.
    children[4] = ChildInput::Present {
        section: &air,
        state: ChildState::Unknown,
    };
    let mut out = DenseSection::filled(block(blocks::GLASS, Light::DARK), 7);
    let outcome = remip_from_children(children, &[Neighbourhood::UNKNOWN; 8], &classes, &mut out);
    let state = |index: usize| outcome.children.get(Octant::ALL[index]);
    assert_eq!(state(0), ChildState::Unknown);
    assert_eq!(state(1), ChildState::Empty);
    assert_eq!(state(2), ChildState::NonEmpty);
    assert_eq!(state(3), ChildState::Empty);
    assert_eq!(state(4), ChildState::Unknown);
    assert_eq!(out.get(0, 0, 0), UNKNOWN_CHILD_FILL);
    assert_eq!(out.get(16, 0, 0), EMPTY_CHILD_FILL);
    assert_eq!(out.get(0, 0, 16).material(), material(blocks::STONE));
    assert_eq!(
        out.voxels()[voxel_index(16, 0, 16)],
        Voxel::air(Light::FULL_SKY)
    );
    assert_eq!(
        out.biomes().get(16, 0),
        UNKNOWN_BIOME,
        "absent children give no biome and nothing of `out` survives"
    );
    assert_eq!(out.biomes().get(0, 16), 0);
}

#[test]
#[should_panic(expected = "passes the top level")]
fn chains_past_the_top_level_panic() {
    let classes = fixture_classifier();
    let mut chain = vec![
        DenseSection::filled(Voxel::AIR, 0),
        DenseSection::filled(Voxel::AIR, 0),
    ];
    let mut masks = vec![ChildMask::UNKNOWN; 2];
    let top = SectionPos::new(crate::geometry::MAX_LEVEL, 0, 0, 0).expect("pos");
    mip_chain(
        &mut chain,
        &mut masks,
        &[Neighbourhood::UNKNOWN],
        top,
        VoxelBox::FULL,
        &classes,
    );
}

#[test]
fn materials_classified_as_air_keep_their_occupancy() {
    struct AirForEverything;
    impl MaterialClasses for AirForEverything {
        fn class(&self, _: MaterialId) -> BlockClass {
            BlockClass::Air
        }
    }
    let stone = block(blocks::STONE, Light::DARK);
    let mut fine = DenseSection::filled(Voxel::air(Light::FULL_SKY), 0);
    fine.set(5, 6, 7, stone);
    let mut parent = unknown_section();
    mip_into_parent(
        &fine,
        &Neighbourhood::UNKNOWN,
        Octant::ALL[0],
        VoxelBox::FULL,
        &AirForEverything,
        &mut parent,
    );
    assert_eq!(parent.get(2, 3, 3).material(), material(blocks::STONE));
    let mut children = [air(); 8];
    children[3] = MipChild {
        voxel: stone,
        class: BlockClass::Air,
        exposed: true,
    };
    assert!(!decided(children).0.is_air());
}

#[test]
fn section_faces_expose_surfaces_but_not_hidden_voxels() {
    let classes = fixture_classifier();
    // Grass on the section's top plane over dirt: the air above lies in the next section.
    let grass = block(blocks::GRASS, Light::DARK);
    let dirt = block(blocks::DIRT, Light::DARK);
    let mut fine = DenseSection::filled(Voxel::HIDDEN, 0);
    for z in 0..SECTION_SIDE {
        for x in 0..SECTION_SIDE {
            fine.set(x, 31, z, grass);
            fine.set(x, 30, z, dirt);
        }
    }
    // One sand block over a small cave: exposed inside the section.
    fine.set(8, 30, 8, block(blocks::SAND, Light::DARK));
    fine.set(8, 29, 8, Voxel::air(Light::DARK));
    let (parent, _) = mip_whole(&fine, &classes);
    assert_eq!(
        parent.get(4, 15, 4).material(),
        material(blocks::GRASS),
        "grass under the next section's air outvotes the cave's sand"
    );
    assert_eq!(
        parent.get(4, 3, 4),
        Voxel::HIDDEN,
        "hidden voxels on section faces stay enclosed"
    );
}
