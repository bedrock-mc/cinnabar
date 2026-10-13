//! Mips with neighbour data across levels: the hidden-pass equivalence, face exposure and
//! the L5/L6 re-mip.

use std::collections::HashMap;

use super::{
    tests::{World, block, material, sample_world},
    *,
};
use crate::{
    exposure::{FacePlane, Neighbourhood},
    fixtures::{blocks, fixture_classifier, terrain_block},
    geometry::{
        MAX_LEVEL, MAX_SECTION_HORIZONTAL, MAX_SECTION_Y, MIN_SECTION_HORIZONTAL, MIN_SECTION_Y,
    },
    voxel::FULL_COVERAGE,
};

/// One level's sections by position.
type Level = HashMap<SectionPos, DenseSection>;

/// The planes across `pos`'s faces from its same-level sections in `level`.
fn neighbourhood(level: &Level, pos: SectionPos, classes: &impl MaterialClasses) -> Neighbourhood {
    Neighbourhood::of_section(pos, classes, |at| level.get(&at))
}

/// Fallback counts summed over one level's re-mips.
#[derive(Debug, Default, Clone, Copy)]
struct Fallbacks {
    resolved: u32,
    unresolved: u32,
}

/// Re-mips the parent of every section in `level`, reading each child's neighbours from
/// `level`; children missing from it are unknown.
fn mip_level(level: &Level, classes: &impl MaterialClasses) -> (Level, Fallbacks) {
    let mut parents: Vec<SectionPos> = level
        .keys()
        .map(|pos| pos.parent().expect("below the top level"))
        .collect();
    parents.sort();
    parents.dedup();
    let mut out = Level::new();
    let mut fallbacks = Fallbacks::default();
    for parent in parents {
        let child_pos = Octant::ALL.map(|octant| parent.child(octant).expect("child"));
        let children = child_pos.map(|pos| match level.get(&pos) {
            Some(section) => ChildInput::Present {
                section,
                state: ChildState::NonEmpty,
            },
            None => ChildInput::Unknown,
        });
        let neighbours = child_pos.map(|pos| neighbourhood(level, pos, classes));
        let mut section = unknown_section();
        let outcome = remip_from_children(children, &neighbours, classes, &mut section);
        fallbacks.resolved += outcome.hidden_fallbacks - outcome.unresolved_hidden;
        fallbacks.unresolved += outcome.unresolved_hidden;
        out.insert(parent, section);
    }
    (out, fallbacks)
}

/// True when a face neighbour of `at` exposes it, with the mip's rule: `around` decides
/// outside the section, and across unknown faces only non-hidden voxels are exposed.
fn exposed(
    section: &DenseSection,
    around: &Neighbourhood,
    classes: &impl MaterialClasses,
    at: [usize; 3],
) -> bool {
    let unknown_exposes = !section.get(at[0], at[1], at[2]).is_hidden();
    (0..3).any(|axis| {
        [false, true].into_iter().any(|positive| {
            let step = if positive {
                Some(at[axis] + 1).filter(|&value| value < SECTION_SIDE)
            } else {
                at[axis].checked_sub(1)
            };
            match step {
                Some(value) => {
                    let mut neighbour = at;
                    neighbour[axis] = value;
                    let [x, y, z] = neighbour;
                    voxel_class(classes, section.get(x, y, z)).exposes_neighbours()
                }
                None => {
                    let face = Face::of_axis(axis, positive);
                    around
                        .get(face)
                        .map_or(unknown_exposes, |plane| plane.exposes(face.in_plane(at)))
                }
            }
        })
    })
}

/// One fine child of a coarse voxel as the mip sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeenChild {
    voxel: Voxel,
    exposed: bool,
}

/// The eight fine children of coarse voxel `[x, y, z]` of `parent`, read from `level`.
fn seen_children(
    level: &Level,
    parent: SectionPos,
    [x, y, z]: [usize; 3],
    classes: &impl MaterialClasses,
) -> Option<[SeenChild; 8]> {
    let octant = Octant::from_halves(x >= CHILD_SPAN, y >= CHILD_SPAN, z >= CHILD_SPAN);
    let pos = parent.child(octant)?;
    let section = level.get(&pos)?;
    let around = neighbourhood(level, pos, classes);
    let base = [x, y, z].map(|value| value % CHILD_SPAN * 2);
    Some(Octant::ALL.map(|child| {
        let at = [
            base[0] + child.x(),
            base[1] + child.y(),
            base[2] + child.z(),
        ];
        SeenChild {
            voxel: section.get(at[0], at[1], at[2]),
            exposed: exposed(section, &around, classes, at),
        }
    }))
}

/// True when the hidden pass cannot change the coarse voxel over these children: no
/// exposed child is hidden, some exposed non-air child decides, and every exposed or water
/// child is the same voxel in both versions.
fn unaffected_group(hidden: &[SeenChild; 8], plain: &[SeenChild; 8]) -> bool {
    let decided = hidden
        .iter()
        .any(|child| child.exposed && !child.voxel.is_air() && !child.voxel.is_hidden());
    decided
        && hidden.iter().zip(plain).all(|(a, b)| {
            let water = |child: &SeenChild| child.voxel.has_water();
            !(a.exposed && a.voxel.is_hidden())
                && a.exposed == b.exposed
                && ((!a.exposed && !water(a) && !water(b)) || a.voxel == b.voxel)
        })
}

/// The world's level-0 sections with every hidden voxel restored to its terrain block, as
/// sampled without the hidden pass: enclosed opaque blocks are dark and never waterlogged.
fn without_hidden_pass(world: &World, classes: &impl MaterialClasses) -> (Level, u32) {
    let mut restored_count = 0;
    let level = world
        .level0
        .iter()
        .map(|(&pos, section)| {
            let mut restored = section.clone();
            let [bx, by, bz] = pos.min_block();
            for y in 0..SECTION_SIDE {
                for z in 0..SECTION_SIDE {
                    for x in 0..SECTION_SIDE {
                        if !section.get(x, y, z).is_hidden() {
                            continue;
                        }
                        let (id, waterlogged) = terrain_block(
                            (bx + x as i64) as i32,
                            (by + y as i64) as i32,
                            (bz + z as i64) as i32,
                        );
                        assert_eq!(classes.class(material(id)), BlockClass::Opaque);
                        let voxel = Voxel::new(material(id), Light::DARK, FULL_COVERAGE, false);
                        assert!(!waterlogged);
                        restored.set(x, y, z, voxel);
                        restored_count += 1;
                    }
                }
            }
            (pos, restored)
        })
        .collect();
    (level, restored_count)
}

/// With full neighbour data, L1–L6 agree with and without the level-0 hidden pass on
/// occupancy, light, water and coverage everywhere, and on every exposed voxel except the
/// listed fallback cases: groups with an exposed hidden child, groups no exposed child
/// decides, and groups whose children already differ.
#[test]
fn the_hidden_pass_changes_no_exposed_surface_from_l1_to_l6() {
    let classes = fixture_classifier();
    let world = sample_world(|_, _| false);
    let (mut without, restored) = without_hidden_pass(&world, &classes);
    assert!(restored > 10_000, "the fixture hides {restored} voxels");
    let mut with = world.level0.clone();
    let (mut listed_cases, mut total_compared) = (0, 0);
    for level in 1..=MAX_LEVEL {
        let (next_with, _) = mip_level(&with, &classes);
        let (next_without, plain_fallbacks) = mip_level(&without, &classes);
        assert_eq!(plain_fallbacks.resolved + plain_fallbacks.unresolved, 0);
        let mut compared = 0;
        for (&pos, hidden) in &next_with {
            let plain = &next_without[&pos];
            let around = neighbourhood(&next_with, pos, &classes);
            for y in 0..SECTION_SIDE {
                for z in 0..SECTION_SIDE {
                    for x in 0..SECTION_SIDE {
                        let (a, b) = (hidden.get(x, y, z), plain.get(x, y, z));
                        let at = format!("level {level} {pos:?} ({x}, {y}, {z})");
                        assert_eq!(a.is_air(), b.is_air(), "occupancy at {at}");
                        assert_eq!(a.light(), b.light(), "light at {at}");
                        assert_eq!(a.has_water(), b.has_water(), "water at {at}");
                        assert_eq!(a.coverage(), b.coverage(), "coverage at {at}");
                        if a.is_air() || !exposed(hidden, &around, &classes, [x, y, z]) {
                            continue;
                        }
                        let groups = (
                            seen_children(&with, pos, [x, y, z], &classes),
                            seen_children(&without, pos, [x, y, z], &classes),
                        );
                        match groups {
                            (Some(fine_hidden), Some(fine_plain))
                                if unaffected_group(&fine_hidden, &fine_plain) =>
                            {
                                compared += 1;
                                assert_eq!(a, b, "exposed voxel at {at}");
                            }
                            _ => listed_cases += 1,
                        }
                    }
                }
            }
        }
        assert!(compared > 0, "level {level} compares a surface");
        total_compared += compared;
        (with, without) = (next_with, next_without);
    }
    assert!(listed_cases > 0, "the fixture reaches the fallback cases");
    assert!(
        listed_cases * 100 < total_compared,
        "{listed_cases} listed cases against {total_compared} equal surface voxels"
    );
    assert_eq!(with.keys().next().map(|pos| pos.level()), Some(MAX_LEVEL));
}

/// A stone section with a hidden core below a dirt top layer.
fn buried_section() -> DenseSection {
    let mut section = DenseSection::filled(Voxel::HIDDEN, 0);
    let stone = block(blocks::STONE, Light::DARK);
    for a in 0..SECTION_SIDE {
        for b in 0..SECTION_SIDE {
            for face in Face::ALL {
                let [x, y, z] = face.voxel_at([a, b]);
                section.set(x, y, z, stone);
            }
        }
    }
    for z in 0..SECTION_SIDE {
        for x in 0..SECTION_SIDE {
            section.set(x, SECTION_SIDE - 1, z, block(blocks::DIRT, Light::DARK));
        }
    }
    section
}

/// Mips `child` whole into octant 0 of an unknown parent.
fn mip_with(child: &DenseSection, around: &Neighbourhood) -> (DenseSection, MipOutcome) {
    let classes = fixture_classifier();
    let mut parent = unknown_section();
    let outcome = mip_into_parent(
        child,
        around,
        Octant::ALL[0],
        VoxelBox::FULL,
        &classes,
        &mut parent,
    );
    (parent, outcome)
}

#[test]
fn known_neighbours_decide_exposure_across_section_faces() {
    let section = buried_section();
    let (unknown, _) = mip_with(&section, &Neighbourhood::UNKNOWN);
    assert_eq!(
        unknown.get(0, 15, 0).material(),
        material(blocks::DIRT),
        "unknown faces expose the top layer"
    );
    assert_eq!(unknown.get(0, 0, 0).material(), material(blocks::STONE));

    let mut closed = Neighbourhood::UNKNOWN;
    for face in Face::ALL {
        closed.set(face, Some(FacePlane::CLOSED));
    }
    let (enclosed, outcome) = mip_with(&section, &closed);
    assert_eq!(outcome.hidden_fallbacks, 0);
    assert_eq!(
        enclosed.get(0, 15, 0).material(),
        material(blocks::DIRT),
        "an enclosed group takes its enclosed majority"
    );
    assert!(enclosed.get(5, 5, 5).is_hidden());

    // Air above exposes the dirt; air beside exposes the stone shell, never the core.
    let mut open = closed;
    open.set(Face::PosY, Some(FacePlane::EXPOSING));
    open.set(Face::NegX, Some(FacePlane::EXPOSING));
    let (exposed_parent, outcome) = mip_with(&section, &open);
    assert_eq!(outcome.hidden_fallbacks, 0);
    assert_eq!(
        exposed_parent.get(3, 15, 3).material(),
        material(blocks::DIRT)
    );
    assert_eq!(
        exposed_parent.get(0, 3, 3).material(),
        material(blocks::STONE)
    );
    assert!(exposed_parent.get(5, 5, 5).is_hidden());
}

#[test]
fn a_known_open_face_sends_exposed_hidden_voxels_to_the_fallback() {
    // A hidden section with one stone voxel on its -X face, in the first 4×4×4 group.
    let mut section = DenseSection::filled(Voxel::HIDDEN, 0);
    section.set(0, 3, 3, block(blocks::STONE, Light::DARK));
    let (unknown, unknown_outcome) = mip_with(&section, &Neighbourhood::UNKNOWN);
    assert_eq!(unknown_outcome.hidden_fallbacks, 0);
    assert!(unknown.get(0, 0, 0).is_hidden());
    assert_eq!(unknown.get(0, 1, 1).material(), material(blocks::STONE));

    let mut open = Neighbourhood::UNKNOWN;
    open.set(Face::NegX, Some(FacePlane::EXPOSING));
    let (parent, outcome) = mip_with(&section, &open);
    assert!(outcome.hidden_fallbacks > outcome.unresolved_hidden);
    assert_eq!(parent.get(0, 0, 0).material(), material(blocks::STONE));
    assert!(
        parent.get(1, 1, 1).is_hidden(),
        "groups off the open face stay hidden"
    );
}

#[test]
fn neighbour_changes_remip_exactly_through_the_face_slab() {
    let classes = fixture_classifier();
    let section = buried_section();
    let mut around = Neighbourhood::UNKNOWN;
    for face in Face::ALL {
        around.set(face, Some(FacePlane::CLOSED));
    }
    let (mut parent, _) = mip_with(&section, &around);
    for face in [Face::PosX, Face::NegY, Face::PosZ] {
        let mut plane = FacePlane::CLOSED;
        plane.set([7, 9], true);
        plane.set([30, 2], true);
        around.set(face, Some(plane));
        mip_into_parent(
            &section,
            &around,
            Octant::ALL[0],
            face.slab(),
            &classes,
            &mut parent,
        );
        let (expected, _) = mip_with(&section, &around);
        assert_eq!(parent, expected, "after opening {face:?}");
    }
}

/// A varied section from `seed` in `biome`: terrain-like layers of four materials with air
/// above a seeded height, a few hidden voxels and some water.
fn varied_section(seed: u32, biome: u32) -> DenseSection {
    let ids = [blocks::STONE, blocks::DIRT, blocks::GRASS, blocks::SAND];
    let mut section = DenseSection::filled(Voxel::air(Light::FULL_SKY), biome);
    for z in 0..SECTION_SIDE {
        for x in 0..SECTION_SIDE {
            let hash = (x as u32 * 31 + z as u32 * 17 + seed * 101) % 97;
            let height = 4 + (hash % 24) as usize;
            for y in 0..height {
                let id = ids[(hash as usize + y) % ids.len()];
                let voxel = if y + 3 < height && (x + z) % 3 == 0 {
                    Voxel::HIDDEN
                } else {
                    block(id, Light::DARK)
                };
                section.set(x, y, z, voxel);
            }
            if hash.is_multiple_of(11) {
                section.set(x, height, z, block(blocks::WATER, Light::new(13, 0)));
            }
        }
    }
    section
}

#[test]
fn remips_at_l5_and_l6_equal_the_chain_near_key_extremes() {
    let classes = fixture_classifier();
    for (x, y, z) in [
        (
            MIN_SECTION_HORIZONTAL,
            MIN_SECTION_Y,
            MAX_SECTION_HORIZONTAL,
        ),
        (
            MAX_SECTION_HORIZONTAL,
            MAX_SECTION_Y,
            MIN_SECTION_HORIZONTAL,
        ),
        (-1, -1, 0),
    ] {
        // The level-6 ancestor of the level-0 section at (x, y, z), and its tree.
        let top = SectionPos::new(0, x, y, z)
            .expect("level 0")
            .ancestor(MAX_LEVEL)
            .expect("level 6");
        assert_eq!(top.level(), MAX_LEVEL);
        let level5: Vec<SectionPos> = Octant::ALL
            .into_iter()
            .filter_map(|octant| top.child(octant))
            .collect();
        assert_eq!(level5.len(), 8, "{top:?} has every child");
        let level4: Level = level5
            .iter()
            .flat_map(|pos| Octant::ALL.into_iter().filter_map(|o| pos.child(o)))
            .enumerate()
            // Biomes vary by column only, so stacked children agree as sampled ones do.
            .map(|(index, pos)| {
                let biome = (pos.x() + 2 * pos.z()).rem_euclid(5) as u32;
                (pos, varied_section(index as u32, biome))
            })
            .collect();
        assert_eq!(level4.len(), 64);

        // Full re-mips, level by level.
        let (remipped5, _) = mip_level(&level4, &classes);
        let (remipped6, _) = mip_level(&remipped5, &classes);
        assert_eq!(remipped6.len(), 1);

        // The chain over every level-4 section, with the same neighbour planes.
        let mut order: Vec<SectionPos> = level4.keys().copied().collect();
        order.sort_by_key(|pos| std::cmp::Reverse(pos.key()));
        let mut store: HashMap<SectionPos, (DenseSection, ChildMask)> = HashMap::new();
        for pos in order {
            let ancestors = [pos.parent().expect("l5"), top];
            let mut chain = vec![level4[&pos].clone()];
            let mut masks = vec![ChildMask::UNKNOWN];
            for ancestor in ancestors {
                let (section, mask) = store
                    .remove(&ancestor)
                    .unwrap_or_else(|| (unknown_section(), ChildMask::UNKNOWN));
                chain.push(section);
                masks.push(mask);
            }
            masks[0].set(Octant::ALL[0], ChildState::NonEmpty);
            let neighbours = [
                neighbourhood(&level4, pos, &classes),
                neighbourhood(&remipped5, ancestors[0], &classes),
            ];
            mip_chain(
                &mut chain,
                &mut masks,
                &neighbours,
                pos,
                VoxelBox::FULL,
                &classes,
            );
            for ((ancestor, section), mask) in ancestors
                .into_iter()
                .zip(chain.drain(1..))
                .zip(masks.drain(1..))
            {
                store.insert(ancestor, (section, mask));
            }
        }
        for (pos, expected) in remipped5.iter().chain(&remipped6) {
            let (actual, _) = &store[pos];
            assert_eq!(actual.voxels(), expected.voxels(), "voxels {pos:?}");
            assert_eq!(actual.biomes(), expected.biomes(), "biomes {pos:?}");
        }
        let top_mask = store[&top].1;
        assert!(top_mask.all_known());
        assert_eq!(top_mask.summary(), ChildState::NonEmpty);
    }
}
