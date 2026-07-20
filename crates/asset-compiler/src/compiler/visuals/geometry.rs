use super::super::*;

pub(in crate::compiler) fn cuboid_quads(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
) -> [ModelQuad; 6] {
    debug_assert!(
        min.iter().zip(max).all(|(&min, max)| min < max),
        "cuboid bounds must have positive volume"
    );
    let [min_x, min_y, min_z] = min;
    let [max_x, max_y, max_z] = max;
    let make = |face: BlockFace, positions: [[i16; 3]; 4], face_id: u32| ModelQuad {
        uvs: positions.map(|[x, y, z]| match face {
            BlockFace::West | BlockFace::East => {
                [(z as u16) * 16, (4096 - i32::from(y) * 16) as u16]
            }
            BlockFace::North | BlockFace::South => {
                [(x as u16) * 16, (4096 - i32::from(y) * 16) as u16]
            }
            BlockFace::Down | BlockFace::Up => [(x as u16) * 16, (z as u16) * 16],
        }),
        positions,
        material: materials[face as usize],
        // Thin model cuboids deliberately never advertise a full-face cull
        // boundary. Their registry coverage remains conservative too.
        flags: face_id,
    };
    [
        make(
            BlockFace::West,
            [
                [min_x, min_y, min_z],
                [min_x, min_y, max_z],
                [min_x, max_y, max_z],
                [min_x, max_y, min_z],
            ],
            3,
        ),
        make(
            BlockFace::East,
            [
                [max_x, min_y, min_z],
                [max_x, max_y, min_z],
                [max_x, max_y, max_z],
                [max_x, min_y, max_z],
            ],
            4,
        ),
        make(
            BlockFace::Down,
            [
                [min_x, min_y, min_z],
                [max_x, min_y, min_z],
                [max_x, min_y, max_z],
                [min_x, min_y, max_z],
            ],
            1,
        ),
        make(
            BlockFace::Up,
            [
                [min_x, max_y, min_z],
                [min_x, max_y, max_z],
                [max_x, max_y, max_z],
                [max_x, max_y, min_z],
            ],
            2,
        ),
        make(
            BlockFace::North,
            [
                [min_x, min_y, min_z],
                [min_x, max_y, min_z],
                [max_x, max_y, min_z],
                [max_x, min_y, min_z],
            ],
            5,
        ),
        make(
            BlockFace::South,
            [
                [min_x, min_y, max_z],
                [max_x, min_y, max_z],
                [max_x, max_y, max_z],
                [min_x, max_y, max_z],
            ],
            6,
        ),
    ]
}
