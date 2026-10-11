//! Vessel surfaces use block-relative UV crops with normals directed into each cavity.

use super::hollow_containers::VesselState;
use {super::super::*, assets::BlockFace};

/// Selects one face from a nondegenerate cuboid without advertising neighbor culling.
fn face(material: u32, min: [i16; 3], max: [i16; 3], direction: BlockFace) -> ModelQuad {
    super::geometry::vanilla_cuboid_quads([material; 6], min, max)[direction as usize]
}

/// Returns both vessel bodies and their state-selected contents, retaining the open rim.
pub(in crate::compiler) fn quads(state: VesselState, materials: &[u32]) -> Vec<ModelQuad> {
    let [side, bottom, top, inner, ..] = materials else {
        unreachable!("vessel material slots");
    };
    let composter = matches!(state, VesselState::Composter(_));
    let base = if composter { 0 } else { 48 };
    let outside = if composter {
        [
            materials[5],
            materials[4],
            *bottom,
            *top,
            *side,
            materials[6],
        ]
    } else {
        [*side, *side, *bottom, *top, *side, *side]
    };
    let mut quads = super::geometry::vanilla_cuboid_quads(outside, [0, base, 0], [256; 3]).to_vec();
    if !composter {
        quads.remove(BlockFace::Up as usize);
        for (min, max) in [
            ([0, base, 0], [32, 256, 256]),
            ([224, base, 0], [256; 3]),
            ([32, base, 0], [224, 256, 32]),
            ([32, base, 224], [224, 256, 256]),
        ] {
            quads.push(face(*top, min, max, BlockFace::Up));
        }
    }
    for (min, max, direction) in [
        ([0, base, 0], [32, 256, 256], BlockFace::East),
        ([224, base, 0], [256; 3], BlockFace::West),
        ([0, base, 0], [256, 256, 32], BlockFace::South),
        ([0, base, 224], [256; 3], BlockFace::North),
    ] {
        quads.push(face(*side, min, max, direction));
    }
    let height = match state {
        VesselState::Composter(0) => 32,
        VesselState::Composter(8) => 240,
        VesselState::Composter(level) => 16 + i16::from(level) * 32,
        VesselState::Cauldron { .. } => 64,
    };
    quads.push(face(*inner, [32, 0, 32], [224, height, 224], BlockFace::Up));
    if !composter {
        // Each foot has an L-shaped footprint: two strips, six vertical edges and no hidden top.
        for flip_x in [false, true] {
            for flip_z in [false, true] {
                let transform = |[x, y, z]: [i16; 3]| {
                    [
                        if flip_x { 256 - x } else { x },
                        y,
                        if flip_z { 256 - z } else { z },
                    ]
                };
                let polygon = [[0, 0], [64, 0], [64, 32], [32, 32], [32, 64], [0, 64]];
                for index in 0..polygon.len() {
                    let [x0, z0] = polygon[index];
                    let [x1, z1] = polygon[(index + 1) % polygon.len()];
                    let mut quad = ModelQuad {
                        positions: [[x0, 0, z0], [x1, 0, z1], [x1, base, z1], [x0, base, z0]]
                            .map(transform),
                        uvs: [[0; 2]; 4],
                        material: *side,
                        flags: 0,
                    };
                    if flip_x == flip_z {
                        quad.positions.reverse();
                    }
                    let axis_x = x0 == x1;
                    let [a, b, c, _] = quad.positions;
                    let nx = i32::from(b[1] - a[1]) * i32::from(c[2] - a[2])
                        - i32::from(b[2] - a[2]) * i32::from(c[1] - a[1]);
                    let nz = i32::from(b[0] - a[0]) * i32::from(c[1] - a[1])
                        - i32::from(b[1] - a[1]) * i32::from(c[0] - a[0]);
                    quad.flags = if axis_x {
                        if nx < 0 { 3 } else { 4 }
                    } else if nz < 0 {
                        5
                    } else {
                        6
                    };
                    quad.uvs = quad.positions.map(|[x, y, z]| {
                        let u = if axis_x { z } else { x };
                        [
                            (if quad.flags == 4 || quad.flags == 5 {
                                256 - u
                            } else {
                                u
                            }) as u16
                                * 16,
                            (4096 - i32::from(y) * 16) as u16,
                        ]
                    });
                    quads.push(quad);
                }
                for (min, max) in [([0, 0, 0], [64, base, 32]), ([0, 0, 32], [32, base, 64])] {
                    let a = transform(min);
                    let b = transform(max);
                    quads.push(face(
                        *bottom,
                        std::array::from_fn(|i| a[i].min(b[i])),
                        std::array::from_fn(|i| a[i].max(b[i])),
                        BlockFace::Down,
                    ));
                }
            }
        }
        if let VesselState::Cauldron {
            level: level @ 1.., ..
        } = state
        {
            let height = 48 + i16::from(level) * 32;
            quads.push(face(
                materials[4],
                [32, 0, 32],
                [224, height, 224],
                BlockFace::Up,
            ));
        }
    }
    quads
}
