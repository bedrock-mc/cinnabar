//! The mesher against the scalar reference on fixtures and random sections.

use super::*;
use crate::{fixtures::terrain_block, geometry::MAX_LEVEL};

/// Fixture terrain with water, leaves, glass and caves; opaque voxels enclosed by opaque
/// voxels are hidden, as sampling makes them.
fn terrain(_level: u8, [x, y, z]: [i64; 3]) -> Voxel {
    let opaque = |x: i64, y: i64, z: i64| {
        let (id, _) = terrain_block(x as i32, y as i32, z as i32);
        matches!(
            id,
            blocks::STONE | blocks::DIRT | blocks::GRASS | blocks::SAND
        )
    };
    let (id, waterlogged) = terrain_block(x as i32, y as i32, z as i32);
    if opaque(x, y, z)
        && [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .iter()
        .all(|(dx, dy, dz)| opaque(x + dx, y + dy, z + dz))
    {
        return Voxel::HIDDEN;
    }
    let sky = if y > 46 { 15 } else { (y.rem_euclid(9)) as u8 };
    let light = Light::new(sky, ((x + z).rem_euclid(3)) as u8);
    if id == blocks::AIR {
        return Voxel::air(light);
    }
    Voxel::new(
        MaterialId::new(id).expect("id"),
        light,
        FULL_COVERAGE,
        waterlogged || id == blocks::WATER,
    )
}

#[test]
fn fixture_terrain_matches_the_reference_with_every_neighbour_known() {
    let generate = terrain;
    let mut world = World::new(&generate);
    let mut mesher = Mesher::new();
    for y in 0..3 {
        let at = pos(0, 1, y, -1);
        world.build_around(at, |_| true);
        for culling in [ExteriorCulling::Off, ExteriorCulling::SectionLocal] {
            let mesh = mesh_checked(&world, at, &mut mesher, culling);
            assert!(mesh.waiting().is_empty());
        }
    }
}

#[test]
fn unknown_neighbours_close_the_section_and_are_recorded() {
    let generate = |_: u8, _: [i64; 3]| block(blocks::STONE, Light::DARK);
    let mut world = World::new(&generate);
    let at = pos(2, 0, 0, 0);
    world.build_around(at, |_| false);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    for face in Face::ALL {
        let quads = mesh.group(MeshList::Main, Bucket::Opaque, face);
        assert_eq!(quads.len(), 4, "{face:?}: four 16×16 quads close the face");
        assert!(
            quads
                .iter()
                .all(|quad| quad.light() == crate::mesh::FALLBACK_LIGHT)
        );
    }
    let waiting: Vec<_> = mesh.waiting().positions(at).collect();
    assert_eq!(
        waiting.len(),
        6 + 3 + 24,
        "every same-level and finer slab, and the coarser slabs across the parent's faces"
    );
    world.build_around(at, |_| true);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    assert!(mesh.is_empty(), "solid all round");
    assert!(mesh.waiting().is_empty());
}

#[test]
fn hidden_voxels_mesh_only_where_exposed() {
    // Hidden stone with one sealed pocket and one shaft to the top.
    let generate = |_: u8, [x, y, z]: [i64; 3]| {
        let pocket = (10..13).contains(&x) && (10..13).contains(&y) && (10..13).contains(&z);
        let shaft = x == 20 && z == 20 && y >= 16;
        if pocket || shaft {
            Voxel::air(Light::new(4, 0))
        } else {
            Voxel::HIDDEN
        }
    };
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    let mut mesher = Mesher::new();
    let all = mesh_checked(&world, at, &mut mesher, ExteriorCulling::Off);
    assert!(
        all.quads()
            .iter()
            .all(|quad| quad.material() == MaterialId::HIDDEN)
    );
    let pocket_faces = 6 * 3 * 3;
    let shaft_faces = 4 * 16 + 1;
    let area: usize = all.quads().iter().map(|q| q.size()[0] * q.size()[1]).sum();
    assert_eq!(area, pocket_faces + shaft_faces);
    let culled = mesh_checked(&world, at, &mut mesher, ExteriorCulling::SectionLocal);
    let area: usize = culled
        .quads()
        .iter()
        .map(|q| q.size()[0] * q.size()[1])
        .sum();
    assert_eq!(area, shaft_faces, "the sealed pocket is culled");
}

#[test]
fn water_meshes_translucent_tops_and_floors() {
    let generate = |_: u8, [_, y, _]: [i64; 3]| match y {
        ..4 => block(blocks::SAND, Light::DARK),
        4..20 => block(blocks::WATER, Light::new((y - 4) as u8, 0)),
        _ => Voxel::air(Light::FULL_SKY),
    };
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    let tops = mesh.group(MeshList::Main, Bucket::Translucent, Face::PosY);
    assert_eq!(tops.len(), 4);
    assert!(
        tops.iter()
            .all(|quad| quad.origin()[1] == 19 && quad.light() == Light::FULL_SKY)
    );
    let floor = mesh.group(MeshList::Main, Bucket::Opaque, Face::PosY);
    assert_eq!(floor.len(), 4);
    assert!(floor.iter().all(|quad| quad.light() == Light::new(0, 0)));
    assert_eq!(
        mesh.translucent(),
        crate::mesh::translucent::TranslucentClass::CoplanarOrOneNormal
    );
    assert_eq!(mesh.quads().len(), 8, "nothing else shows");
}

#[test]
fn waterlogged_see_through_voxels_show_the_water_surface() {
    let wet_glass = |light| {
        Voxel::new(
            MaterialId::new(blocks::GLASS).expect("id"),
            light,
            FULL_COVERAGE,
            true,
        )
    };
    let generate = move |_: u8, [x, y, z]: [i64; 3]| match y {
        ..4 => block(blocks::SAND, Light::DARK),
        4..=10 if x.rem_euclid(16) == 5 && z.rem_euclid(16) == 5 => wet_glass(Light::FULL_SKY),
        4..=10 => block(blocks::WATER, Light::FULL_SKY),
        _ => Voxel::air(Light::FULL_SKY),
    };
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    let tops = mesh.group(MeshList::Main, Bucket::Translucent, Face::PosY);
    assert_eq!(
        tops.len(),
        4,
        "the surface over the waterlogged column merges"
    );
    assert!(tops.iter().all(|quad| quad.origin()[1] == 10
        && quad.material() == MaterialId::new(blocks::WATER).expect("id")));
    for face in [Face::NegX, Face::PosX, Face::NegZ, Face::PosZ] {
        assert!(
            mesh.group(MeshList::Main, Bucket::Translucent, face)
                .is_empty(),
            "{face:?}: water hides against waterlogged voxels"
        );
        assert_eq!(
            mesh.group(MeshList::Main, Bucket::Opaque, face).len(),
            4,
            "{face:?}: each of the four columns shows through the water"
        );
    }
    let opaque_tops = mesh.group(MeshList::Main, Bucket::Opaque, Face::PosY);
    assert!(
        opaque_tops.iter().all(|quad| quad.origin()[1] == 3),
        "only the sand floor shows upwards in the opaque bucket"
    );
    assert_eq!(mesh.quads().len(), 4 + 4 + 4 * 4, "nothing else shows");
}

#[test]
fn checkerboards_mesh_every_voxel_face() {
    for (name, odd) in [
        ("stone and air", Voxel::air(Light::FULL_SKY)),
        ("stone and glass", block(blocks::GLASS, Light::FULL_SKY)),
    ] {
        let generate = move |_: u8, [x, y, z]: [i64; 3]| {
            if (x + y + z).rem_euclid(2) == 0 {
                block(blocks::STONE, Light::DARK)
            } else {
                odd
            }
        };
        let mut world = World::new(&generate);
        let at = pos(0, 0, 0, 0);
        world.build_around(at, |_| true);
        let mut mesher = Mesher::new();
        let all = mesh_checked(&world, at, &mut mesher, ExteriorCulling::Off);
        let voxels = SECTION_SIDE * SECTION_SIDE * SECTION_SIDE;
        assert_eq!(all.quads().len(), voxels / 2 * 6, "{name}: no face merges");
        let kept = mesh_checked(&world, at, &mut mesher, ExteriorCulling::SectionLocal);
        assert!(
            kept.quads().len() < all.quads().len() / 2,
            "{name}: only voxels open to the border or a chunk-column plane keep faces"
        );
    }
}

#[test]
fn see_through_blocks_hide_only_their_own_material() {
    let generate = |_: u8, [x, _, _]: [i64; 3]| match x {
        ..8 => block(blocks::GLASS, Light::FULL_SKY),
        8..16 => block(blocks::WATER, Light::FULL_SKY),
        16..24 => block(blocks::GLASS, Light::FULL_SKY),
        _ => block(blocks::LEAVES, Light::FULL_SKY),
    };
    let mut world = World::new(&generate);
    let at = pos(0, 0, 0, 0);
    world.build_around(at, |_| true);
    let mesh = mesh_checked(
        &world,
        at,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    let x_faces = |bucket, face| -> Vec<usize> {
        mesh.group(MeshList::Main, bucket, face)
            .iter()
            .map(|quad| quad.origin()[0])
            .collect()
    };
    let mut glass_and_water = x_faces(Bucket::Opaque, Face::PosX);
    glass_and_water.dedup();
    assert_eq!(
        glass_and_water,
        [7],
        "glass shows against water, not leaves"
    );
    let mut water = x_faces(Bucket::Translucent, Face::PosX);
    water.dedup();
    assert_eq!(water, [15], "water shows against glass");
}

/// A random generator of one of three shapes: per-voxel noise, 4³ blocks or layers with
/// holes, over every class, unknown materials and a few light values.
pub(super) fn random_generator(seed: u64) -> impl Fn(u8, [i64; 3]) -> Voxel {
    const IDS: [u32; 9] = [
        blocks::AIR,
        blocks::AIR,
        blocks::STONE,
        blocks::DIRT,
        blocks::WATER,
        blocks::LEAVES,
        blocks::GLASS,
        blocks::SAND,
        UNKNOWN_MATERIAL,
    ];
    let shape = seed % 3;
    move |level, at| {
        let cell = match shape {
            0 => at,
            1 => at.map(|value| value >> 2),
            _ => [at[0] >> 3, at[1], at[2] >> 3],
        };
        let roll = hash(level, cell, seed);
        let id = match shape {
            2 => {
                let hole = hash(level, at, seed ^ 1).is_multiple_of(7);
                if hole {
                    blocks::AIR
                } else {
                    IDS[(roll % 9) as usize]
                }
            }
            _ => IDS[(roll % 9) as usize],
        };
        let light = Light::new((roll >> 8) as u8 % 3 * 7, (roll >> 12) as u8 % 2 * 9);
        match id {
            blocks::AIR => Voxel::air(light),
            _ if roll >> 20 & 15 == 0 => Voxel::HIDDEN,
            _ => Voxel::new(
                MaterialId::new(id).expect("id"),
                light,
                (roll >> 16) as u8 % 3,
                roll >> 18 & 1 == 1,
            ),
        }
    }
}

#[test]
fn random_sections_match_the_reference() {
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut mesher = Mesher::new();
    for case in 0..48 {
        let seed = rng.next();
        let generate = random_generator(seed);
        let mut world = World::new(&generate);
        let level = (rng.below(u64::from(MAX_LEVEL) + 1)) as u8;
        let at = pos(
            level,
            rng.below(5) as i32 - 2,
            rng.below(3) as i32 - 1,
            rng.below(5) as i32 - 2,
        );
        let known_share = rng.below(4);
        world.build_around(at, |neighbour| {
            known_share == 3
                || hash(
                    neighbour.level(),
                    [neighbour.x(), neighbour.y(), neighbour.z()].map(i64::from),
                    seed,
                ) % 4
                    < known_share + 1
        });
        let culling = if case % 2 == 0 {
            ExteriorCulling::SectionLocal
        } else {
            ExteriorCulling::Off
        };
        mesh_checked(&world, at, &mut mesher, culling);
    }
}
