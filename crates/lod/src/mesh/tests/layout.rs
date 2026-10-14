//! Groups, clusters, translucent classes, emission order and slab positions.

use super::*;
use crate::{
    geometry::MAX_LEVEL,
    mesh::{
        GROUP_COUNT, group_index,
        input::{dependents, touching_children},
        output::CLUSTER_QUADS,
        translucent::TranslucentClass,
    },
};

/// Rolling terrain with water and tunnels, so every group and some clusters fill.
fn busy(_: u8, [x, y, z]: [i64; 3]) -> Voxel {
    let height = 10 + (x * 7 + z * 3).rem_euclid(13);
    if y > height {
        if y < 16 {
            block(blocks::WATER, Light::new(10, 0))
        } else {
            Voxel::air(Light::FULL_SKY)
        }
    } else if (x + 2 * z + 3 * y).rem_euclid(5) == 0 {
        block(blocks::GLASS, Light::new(3, 1))
    } else {
        block(blocks::DIRT, Light::DARK)
    }
}

#[test]
fn clusters_cover_each_group_in_order() {
    let generate = busy;
    let mut world = World::new(&generate);
    let mut mesher = Mesher::new();
    for level in [0, 1, 3] {
        let at = pos(level, 1, 0, 0);
        world.build_around(at, |_| true);
        let mesh = mesh_checked(&world, at, &mut mesher, ExteriorCulling::SectionLocal);
        assert!(mesh.clusters().len() > Face::ALL.len());
        let mut next = 0;
        for cluster in mesh.clusters() {
            assert_eq!(cluster.key, at.key());
            assert_eq!(
                cluster.first as usize, next,
                "clusters tile the quads in order"
            );
            assert!((1..=CLUSTER_QUADS as u32).contains(&cluster.count));
            let range = cluster.first as usize..(cluster.first + cluster.count) as usize;
            let group = mesh.group_range(cluster.list, cluster.bucket, cluster.face);
            assert!(group.start <= range.start && range.end <= group.end);
            for quad in &mesh.quads()[range.clone()] {
                assert_eq!(quad.face(), cluster.face);
                let (min, max) = quad.bounds();
                for axis in 0..3 {
                    assert!(usize::from(cluster.bounds.min[axis]) <= min[axis]);
                    assert!(max[axis] <= usize::from(cluster.bounds.max[axis]));
                }
                if crate::mesh::overlaps_resident_columns(level) {
                    let (low, high) = quad.voxel_box();
                    let x_half = usize::from(low[0] >= world::SUB_CHUNK_SIDE);
                    let z_half = usize::from(low[2] >= world::SUB_CHUNK_SIDE);
                    assert_ne!(cluster.chunk_columns & 1 << (x_half | z_half << 1), 0);
                    assert!(high[0] <= SECTION_SIDE);
                } else {
                    assert_eq!(cluster.chunk_columns, 0);
                }
            }
            next = range.end;
        }
        assert_eq!(next, mesh.quads().len());
        let groups: usize = (0..GROUP_COUNT)
            .map(|group| {
                let (list, bucket, face) = (
                    MeshList::ALL[group / 12],
                    Bucket::ALL[group / 6 % 2],
                    Face::ALL[group % 6],
                );
                assert_eq!(group_index(list, bucket, face), group);
                mesh.group(list, bucket, face).len().div_ceil(CLUSTER_QUADS)
            })
            .sum();
        assert_eq!(
            groups,
            mesh.clusters().len(),
            "full clusters until each group's last"
        );
    }
}

#[test]
fn translucent_quads_run_far_to_near_per_face() {
    let generate = busy;
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    for face in Face::ALL {
        let planes: Vec<usize> = mesh
            .group(MeshList::Main, Bucket::Translucent, face)
            .iter()
            .map(|quad| quad.bounds().0[face.axis()])
            .collect();
        let mut sorted = planes.clone();
        sorted.sort_unstable();
        if !face.is_positive() {
            sorted.reverse();
        }
        assert_eq!(planes, sorted, "{face:?}");
    }
}

/// Translucent class of a section of water voxels where `water` holds, air elsewhere.
fn class_of(water: impl Fn([i64; 3]) -> bool) -> TranslucentClass {
    let generate = move |_: u8, at: [i64; 3]| {
        if water(at) {
            block(blocks::WATER, Light::FULL_SKY)
        } else {
            Voxel::air(Light::FULL_SKY)
        }
    };
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    )
    .translucent()
}

#[test]
fn translucent_classes_follow_the_geometry() {
    let inside =
        |[x, y, z]: [i64; 3]| (0..32).contains(&x) && (0..32).contains(&y) && (0..32).contains(&z);
    assert_eq!(class_of(|_| false), TranslucentClass::None);
    // An ocean filling everything below y 20 shows only its top.
    assert_eq!(
        class_of(|[_, y, _]| y < 20),
        TranslucentClass::CoplanarOrOneNormal
    );
    // A slab filling the section between two heights is convex.
    let slab = move |at: [i64; 3]| inside(at) && (8..12).contains(&at[1]);
    assert!(matches!(
        class_of(slab),
        TranslucentClass::StaticPerNormal { .. }
    ));
    // A single cube: convex, any order works.
    let cube =
        |[x, y, z]: [i64; 3]| (4..6).contains(&x) && (4..6).contains(&y) && (4..6).contains(&z);
    let TranslucentClass::StaticPerNormal { order } = class_of(cube) else {
        panic!("a convex cube has a static order");
    };
    let mut faces = order.to_vec();
    faces.sort_by_key(|face| face.index());
    assert_eq!(faces, Face::ALL);
    // Two cubes in an L whose faces see each other on two axes need a per-view order.
    let ell = move |at: [i64; 3]| {
        cube(at) || cube([at[0] - 6, at[1] - 6, at[2]]) || cube([at[0] - 12, at[1], at[2]])
    };
    assert_eq!(class_of(ell), TranslucentClass::General);
}

#[test]
fn slab_positions_and_dependents_are_inverse() {
    let mut rng = Rng(0xdeb5_0000_1111_2222);
    for _ in 0..200 {
        let level = rng.below(u64::from(MAX_LEVEL) + 1) as u8;
        let at = pos(
            level,
            rng.below(9) as i32 - 4,
            rng.below(5) as i32 - 2,
            rng.below(9) as i32 - 4,
        );
        for face in Face::ALL {
            let slabs = FaceSlabs::of(at, face);
            assert_eq!(slabs.same, at.neighbour(face));
            assert_eq!(
                slabs.coarser.is_some(),
                level < MAX_LEVEL && slabs.same.and_then(SectionPos::parent) != at.parent()
            );
            assert_eq!(
                slabs.finer.iter().flatten().count(),
                if level > 0 { 4 } else { 0 }
            );
            for slab in [slabs.same, slabs.coarser]
                .into_iter()
                .chain(slabs.finer)
                .flatten()
            {
                assert!(dependents(slab).contains(&at), "{at:?} reads {slab:?}");
            }
        }
        for dependent in dependents(at) {
            let reads = Face::ALL.iter().any(|&face| {
                let slabs = FaceSlabs::of(dependent, face);
                [slabs.same, slabs.coarser]
                    .into_iter()
                    .chain(slabs.finer)
                    .any(|slab| slab == Some(at))
            });
            assert!(reads, "{dependent:?} is listed but does not read {at:?}");
        }
        for face in Face::ALL {
            for child in touching_children(at, face).into_iter().flatten() {
                assert_eq!(child.parent(), Some(at));
                assert_eq!(
                    child.neighbour(face).and_then(SectionPos::parent),
                    at.neighbour(face)
                );
            }
        }
    }
}

#[test]
fn scratch_stays_bounded_and_air_meshes_nothing() {
    let generate = |_: u8, _: [i64; 3]| Voxel::air(Light::FULL_SKY);
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| false);
    let mut mesher = Mesher::new();
    let mesh = mesher.mesh(
        &world.input(at),
        &mesh_classes(),
        ExteriorCulling::SectionLocal,
    );
    assert!(mesh.is_empty());
    assert_eq!(mesh.translucent(), TranslucentClass::None);
    assert!(
        mesher.heap_bytes() < 400 * 1024,
        "{} bytes of scratch",
        mesher.heap_bytes()
    );
}
