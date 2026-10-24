use {super::super::*, assets::BlockFace};

/// Crops each native block face to its cuboid coordinates and outside-view orientation.
pub(in crate::compiler) fn vanilla_cuboid_quads(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
) -> [ModelQuad; 6] {
    let mut quads = cuboid_quads(materials, min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        for uv in &mut quad.uvs {
            match face {
                BlockFace::North | BlockFace::East => uv[0] = 4096 - uv[0],
                BlockFace::Down => uv[1] = 4096 - uv[1],
                _ => {}
            }
        }
    }
    quads
}

/// Full-cube native quads with each face's UVs flipped by its native `Flip` value
/// (1 and 2 are opposite quarter turns, 3 a half turn), keeping each quad's own face culling.
pub(in crate::compiler) fn flipped_cube_quads(
    materials: [u32; 6],
    flips: [u8; 6],
) -> [ModelQuad; 6] {
    let mut quads = vanilla_cuboid_quads(materials, [0; 3], [256; 3]);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        for uv in &mut quad.uvs {
            let [u, v] = *uv;
            *uv = match flips[face as usize] {
                1 => [v, 4096 - u],
                2 => [4096 - v, u],
                3 => [4096 - u, 4096 - v],
                _ => [u, v],
            };
        }
        quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
    }
    quads
}

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

/// A texture region in 1/4096 tile units: `[u0, u1, v0, v1]`.
pub(in crate::compiler) type UvRect = [u16; 4];

/// Whether a cuboid face is its top or bottom.
pub(in crate::compiler) fn is_horizontal(quad: &ModelQuad) -> bool {
    quad.positions
        .iter()
        .all(|position| position[1] == quad.positions[0][1])
}

/// Replaces a cuboid face's position-derived UVs with `rect`, keeping which corner reads
/// each end of the region. `quarter_turn` maps the face's v axis onto u and its reversed u
/// axis onto v.
pub(in crate::compiler) fn stretch_uvs(
    mut quad: ModelQuad,
    [u0, u1, v0, v1]: UvRect,
    quarter_turn: bool,
) -> ModelQuad {
    let max_u = quad.uvs.iter().map(|uv| uv[0]).max().unwrap_or(0);
    let max_v = quad.uvs.iter().map(|uv| uv[1]).max().unwrap_or(0);
    for uv in &mut quad.uvs {
        let (at_max_u, at_max_v) = (uv[0] == max_u, uv[1] == max_v);
        let (at_max_u, at_max_v) = if quarter_turn {
            (at_max_v, !at_max_u)
        } else {
            (at_max_u, at_max_v)
        };
        *uv = [
            if at_max_u { u1 } else { u0 },
            if at_max_v { v1 } else { v0 },
        ];
    }
    quad
}
