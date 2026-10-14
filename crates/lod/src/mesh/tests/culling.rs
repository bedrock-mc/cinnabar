//! Section-local exterior culling: only faces of pockets sealed inside the section go.

use std::collections::HashSet;

use super::*;

/// Unit faces of every quad of `mesh`.
fn unit_faces(mesh: &SectionMesh) -> usize {
    mesh.quads()
        .iter()
        .map(|quad| quad.size()[0] * quad.size()[1])
        .sum()
}

/// Stone with an air pocket over `pocket` (section-local ranges per axis), open to the top
/// when `shaft` is set.
fn pocket_world(pocket: [std::ops::Range<i64>; 3], shaft: bool) -> impl Fn(u8, [i64; 3]) -> Voxel {
    move |_, at| {
        let inside = (0..3).all(|axis| pocket[axis].contains(&at[axis]));
        let up = shaft
            && at[0] == pocket[0].start
            && at[2] == pocket[2].start
            && at[1] >= pocket[1].start;
        if inside || up {
            Voxel::air(Light::new(0, 7))
        } else {
            block(blocks::STONE, Light::DARK)
        }
    }
}

fn culled_and_kept(level: u8, generate: &Generator<'_>) -> (usize, usize) {
    let mut world = World::new(generate);
    let at = pos(level, 0, 0, 0);
    world.build_around(at, |_| true);
    let mut mesher = Mesher::new();
    let all = mesh_checked(&world, at, &mut mesher, ExteriorCulling::Off);
    let kept = mesh_checked(&world, at, &mut mesher, ExteriorCulling::SectionLocal);
    (unit_faces(&all), unit_faces(&kept))
}

#[test]
fn sealed_pockets_lose_their_faces() {
    let generate = pocket_world([4..9, 10..14, 3..7], false);
    let (all, kept) = culled_and_kept(0, &generate);
    assert_eq!(all, 2 * (5 * 4 + 4 * 4 + 5 * 4));
    assert_eq!(kept, 0, "the enclosed cave is culled");
}

#[test]
fn air_connected_to_the_border_keeps_every_face() {
    let generate = pocket_world([4..9, 10..14, 3..7], true);
    let (all, kept) = culled_and_kept(0, &generate);
    assert!(all > 2 * (5 * 4 + 4 * 4 + 5 * 4));
    assert_eq!(all, kept);
    // Rolling terrain with tunnels through the section: all air reaches the border.
    let terrain = |_: u8, [x, y, z]: [i64; 3]| {
        let height = 12 + (x * 3 + z * 5).rem_euclid(9);
        let tunnel = (y == 4 || y == 5) && (z.rem_euclid(6) == 1 || x.rem_euclid(7) == 2);
        if y > height || tunnel {
            Voxel::air(Light::new(15, 0))
        } else {
            block(blocks::DIRT, Light::DARK)
        }
    };
    let (all, kept) = culled_and_kept(0, &terrain);
    assert_eq!(all, kept);
}

#[test]
fn pockets_beside_chunk_column_planes_stay_at_level_zero() {
    // The plane between a level-0 section's chunk columns runs between x 15 and 16.
    let generate = pocket_world([16..19, 10..14, 3..7], false);
    let (all, kept) = culled_and_kept(0, &generate);
    assert_eq!(all, kept);
    let generate = pocket_world([17..19, 10..14, 3..7], false);
    let (_, kept) = culled_and_kept(0, &generate);
    assert_eq!(
        kept, 0,
        "one voxel away from the plane the pocket is sealed"
    );
}

#[test]
fn plane_seeds_and_cluster_columns_follow_one_resident_overlap_rule() {
    // A pocket beside x 16, a chunk-column plane at every level whose voxels are narrower
    // than a column.
    let generate = pocket_world([16..18, 10..14, 9..11], false);
    for level in 0..=crate::geometry::MAX_LEVEL {
        let (all, kept) = culled_and_kept(level, &generate);
        assert!(all > 0);
        let overlaps = crate::mesh::overlaps_resident_columns(level);
        assert_eq!(kept, if overlaps { all } else { 0 }, "level {level}");

        let mut world = World::new(&generate);
        let at = pos(level, 0, 0, 0);
        world.build_around(at, |_| true);
        let mesh = mesh_checked(&world, at, &mut Mesher::new(), ExteriorCulling::Off);
        assert!(!mesh.clusters().is_empty());
        for cluster in mesh.clusters() {
            assert_eq!(cluster.chunk_columns != 0, overlaps, "level {level}");
        }
    }
}

#[test]
fn sealed_pockets_lose_their_faces_at_every_coarser_level() {
    for level in 1..=crate::geometry::MAX_LEVEL {
        let generate = pocket_world([4..9, 10..14, 3..7], false);
        let (all, kept) = culled_and_kept(level, &generate);
        assert!(all > 0);
        assert_eq!(kept, 0, "level {level}");
    }
}

#[test]
fn a_pocket_a_finer_neighbour_opens_keeps_its_faces() {
    // A one-voxel pocket at x 31 of level-1 section A, closed by A's stone and by the
    // same-level neighbour B's solid voxel across +X; one of B's level-0 children touching
    // A is air, so a camera there sees into the pocket when B renders one level finer.
    let pocket = [31_i64, 10, 11];
    let generate = move |level: u8, at: [i64; 3]| {
        let open = match level {
            1 => at == pocket,
            0 => at == [2 * (pocket[0] + 1), 2 * pocket[1], 2 * pocket[2]],
            _ => false,
        };
        if open {
            Voxel::air(Light::new(0, 7))
        } else {
            block(blocks::STONE, Light::DARK)
        }
    };
    let mut world = World::new(&generate);
    let at = pos(1, 0, 0, 0);
    world.build_around(at, |_| true);
    let mut mesher = Mesher::new();
    let kept = mesh_checked(&world, at, &mut mesher, ExteriorCulling::SectionLocal);
    let walls = mesh_checked(&world, at, &mut mesher, ExteriorCulling::Off);
    // Unit faces of the main list whose front voxel is the pocket.
    let pocket_faces = |mesh: &SectionMesh| {
        let mut count = 0;
        for face in Face::ALL {
            for quad in mesh.group(MeshList::Main, Bucket::Opaque, face) {
                let (min, max) = quad.voxel_box();
                for x in min[0]..max[0] {
                    for y in min[1]..max[1] {
                        for z in min[2]..max[2] {
                            let mut front = [x, y, z].map(|value| value as i64);
                            front[face.axis()] += if face.is_positive() { 1 } else { -1 };
                            count += usize::from(front == pocket);
                        }
                    }
                }
            }
        }
        count
    };
    assert_eq!(pocket_faces(&walls), 5, "the pocket has five walls");
    assert_eq!(
        pocket_faces(&kept),
        pocket_faces(&walls),
        "the finer neighbour's air reaches the pocket"
    );
}

/// Unit faces of every quad of `mesh` with their list, bucket and payload.
fn unit_face_set(mesh: &SectionMesh) -> HashSet<(MeshList, Bucket, Face, [usize; 3], u32)> {
    let mut faces = HashSet::new();
    for (list, bucket, quad) in listed(mesh) {
        let (min, max) = quad.voxel_box();
        for x in min[0]..max[0] {
            for y in min[1]..max[1] {
                for z in min[2]..max[2] {
                    faces.insert((list, bucket, quad.face(), [x, y, z], quad.payload()));
                }
            }
        }
    }
    faces
}

/// Asserts that culling `at` only drops faces, per unit face, and returns how many it
/// dropped.
fn culling_only_drops(world: &World<'_>, at: SectionPos, mesher: &mut Mesher) -> usize {
    let all = unit_face_set(&mesh_checked(world, at, mesher, ExteriorCulling::Off));
    let kept = unit_face_set(&mesh_checked(
        world,
        at,
        mesher,
        ExteriorCulling::SectionLocal,
    ));
    let extra: Vec<_> = kept.difference(&all).take(4).collect();
    assert!(extra.is_empty(), "{at:?}: culling added {extra:?}");
    all.len() - kept.len()
}

#[test]
fn culled_faces_are_a_subset_of_unculled_faces() {
    let mut mesher = Mesher::new();
    let mut dropped = 0;
    // Random sections of every level, with neighbours partly unknown.
    let mut rng = Rng(0x5eed_c011_0000_0001);
    for _ in 0..48 {
        let seed = rng.next();
        let generate = super::oracle::random_generator(seed);
        let mut world = World::new(&generate);
        let level = rng.below(u64::from(crate::geometry::MAX_LEVEL) + 1) as u8;
        let at = pos(level, rng.below(5) as i32 - 2, 0, rng.below(5) as i32 - 2);
        let known_share = rng.below(4);
        world.build_around(at, |neighbour| {
            hash(
                neighbour.level(),
                [neighbour.x(), neighbour.y(), neighbour.z()].map(i64::from),
                seed,
            ) % 4
                <= known_share
        });
        dropped += culling_only_drops(&world, at, &mut mesher);
    }
    // Both sides of one-level transitions; level-0 sections straddle chunk columns.
    for level in 0..crate::geometry::MAX_LEVEL {
        for face in Face::ALL {
            let generate =
                super::cracks::mixed(0x50b5_e700 + u64::from(level) * 16 + face.index() as u64);
            let mut world = World::new(&generate);
            let (fine, coarse) = super::cracks::transition(level, face);
            world.build_around(fine, |_| true);
            world.build_around(coarse, |_| true);
            dropped += culling_only_drops(&world, fine, &mut mesher);
            dropped += culling_only_drops(&world, coarse, &mut mesher);
        }
    }
    assert!(dropped > 0, "the fixtures have sealed pockets");
}
