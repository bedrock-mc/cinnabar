use super::super::*;

#[derive(Clone, Copy)]
pub(in crate::compiler) struct GateFaceUv {
    rect: [u16; 4],
    rotation: u16,
}

#[derive(Clone, Copy)]
pub(in crate::compiler) struct GateElement {
    min: [i16; 3],
    max: [i16; 3],
    faces: [Option<GateFaceUv>; 6],
}

pub(in crate::compiler) const fn gate_uv(u1: u16, v1: u16, u2: u16, v2: u16) -> Option<GateFaceUv> {
    Some(GateFaceUv {
        rect: [u1, v1, u2, v2],
        rotation: 0,
    })
}

pub(in crate::compiler) const fn gate_uv_rot(
    u1: u16,
    v1: u16,
    u2: u16,
    v2: u16,
    rotation: u16,
) -> Option<GateFaceUv> {
    Some(GateFaceUv {
        rect: [u1, v1, u2, v2],
        rotation,
    })
}

const GATE_CLOSED: [GateElement; 8] = [
    GateElement {
        min: [0, 80, 112],
        max: [32, 256, 144],
        faces: [
            gate_uv(7, 0, 9, 11),
            gate_uv(7, 0, 9, 11),
            gate_uv(0, 7, 2, 9),
            gate_uv(0, 7, 2, 9),
            gate_uv(0, 0, 2, 11),
            gate_uv(0, 0, 2, 11),
        ],
    },
    GateElement {
        min: [224, 80, 112],
        max: [256, 256, 144],
        faces: [
            gate_uv(7, 0, 9, 11),
            gate_uv(7, 0, 9, 11),
            gate_uv(14, 7, 16, 9),
            gate_uv(14, 7, 16, 9),
            gate_uv(14, 0, 16, 11),
            gate_uv(14, 0, 16, 11),
        ],
    },
    GateElement {
        min: [96, 96, 112],
        max: [128, 240, 144],
        faces: [
            gate_uv(7, 1, 9, 10),
            gate_uv(7, 1, 9, 10),
            gate_uv(6, 7, 8, 9),
            gate_uv(6, 7, 8, 9),
            gate_uv(6, 1, 8, 10),
            gate_uv(6, 1, 8, 10),
        ],
    },
    GateElement {
        min: [128, 96, 112],
        max: [160, 240, 144],
        faces: [
            gate_uv(7, 1, 9, 10),
            gate_uv(7, 1, 9, 10),
            gate_uv(8, 7, 10, 9),
            gate_uv(8, 7, 10, 9),
            gate_uv(8, 1, 10, 10),
            gate_uv(8, 1, 10, 10),
        ],
    },
    GateElement {
        min: [32, 96, 112],
        max: [96, 144, 144],
        faces: [
            None,
            None,
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 10),
            gate_uv(2, 7, 6, 10),
        ],
    },
    GateElement {
        min: [32, 192, 112],
        max: [96, 240, 144],
        faces: [
            None,
            None,
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 1, 6, 4),
            gate_uv(2, 1, 6, 4),
        ],
    },
    GateElement {
        min: [160, 96, 112],
        max: [224, 144, 144],
        faces: [
            None,
            None,
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 10),
            gate_uv(10, 7, 14, 10),
        ],
    },
    GateElement {
        min: [160, 192, 112],
        max: [224, 240, 144],
        faces: [
            None,
            None,
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 1, 14, 4),
            gate_uv(10, 1, 14, 4),
        ],
    },
];

const GATE_OPEN: [GateElement; 8] = [
    GATE_CLOSED[0],
    GATE_CLOSED[1],
    GateElement {
        min: [0, 96, 208],
        max: [32, 240, 240],
        faces: [
            gate_uv(13, 1, 15, 10),
            gate_uv(13, 1, 15, 10),
            gate_uv(0, 13, 2, 15),
            gate_uv(0, 13, 2, 15),
            gate_uv(0, 1, 2, 10),
            gate_uv(0, 1, 2, 10),
        ],
    },
    GateElement {
        min: [224, 96, 208],
        max: [256, 240, 240],
        faces: [
            gate_uv(13, 1, 15, 10),
            gate_uv(13, 1, 15, 10),
            gate_uv(14, 13, 16, 15),
            gate_uv(14, 13, 16, 15),
            gate_uv(14, 1, 16, 10),
            gate_uv(14, 1, 16, 10),
        ],
    },
    GateElement {
        min: [0, 96, 144],
        max: [32, 144, 208],
        faces: [
            gate_uv(13, 7, 15, 10),
            gate_uv(13, 7, 15, 10),
            gate_uv(0, 9, 2, 13),
            gate_uv(0, 9, 2, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [0, 192, 144],
        max: [32, 240, 208],
        faces: [
            gate_uv(13, 1, 15, 4),
            gate_uv(13, 1, 15, 4),
            gate_uv(0, 9, 2, 13),
            gate_uv(0, 9, 2, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 96, 144],
        max: [256, 144, 208],
        faces: [
            gate_uv(13, 7, 15, 10),
            gate_uv(13, 7, 15, 10),
            gate_uv(14, 9, 16, 13),
            gate_uv(14, 9, 16, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 192, 144],
        max: [256, 240, 208],
        faces: [
            gate_uv(13, 1, 15, 4),
            gate_uv(13, 1, 15, 4),
            gate_uv(14, 9, 16, 13),
            gate_uv(14, 9, 16, 13),
            None,
            None,
        ],
    },
];

const BAMBOO_GATE_CLOSED: [GateElement; 8] = [
    GateElement {
        min: [0, 80, 112],
        max: [32, 256, 144],
        faces: [
            gate_uv(14, 2, 16, 13),
            gate_uv(14, 2, 16, 13),
            gate_uv(16, 13, 14, 15),
            gate_uv(14, 0, 16, 2),
            gate_uv(14, 2, 16, 13),
            gate_uv(14, 2, 16, 13),
        ],
    },
    GateElement {
        min: [224, 80, 112],
        max: [256, 256, 144],
        faces: [
            gate_uv(0, 2, 2, 13),
            gate_uv(0, 2, 2, 13),
            gate_uv(2, 13, 0, 15),
            gate_uv(0, 0, 2, 2),
            gate_uv(0, 2, 2, 13),
            gate_uv(0, 2, 2, 13),
        ],
    },
    GateElement {
        min: [96, 96, 112],
        max: [128, 240, 144],
        faces: [
            gate_uv(8, 3, 10, 12),
            None,
            gate_uv(8, 14, 10, 12),
            gate_uv(8, 1, 10, 3),
            gate_uv(8, 3, 10, 12),
            gate_uv(6, 3, 8, 12),
        ],
    },
    GateElement {
        min: [128, 96, 112],
        max: [160, 240, 144],
        faces: [
            None,
            gate_uv(6, 3, 8, 12),
            gate_uv(6, 14, 8, 12),
            gate_uv(6, 1, 8, 3),
            gate_uv(6, 3, 8, 12),
            gate_uv(8, 3, 10, 12),
        ],
    },
    GateElement {
        min: [32, 96, 112],
        max: [96, 144, 144],
        faces: [
            None,
            None,
            gate_uv(10, 14, 14, 12),
            gate_uv(10, 1, 14, 3),
            gate_uv(10, 3, 14, 6),
            gate_uv(10, 9, 14, 12),
        ],
    },
    GateElement {
        min: [32, 192, 112],
        max: [96, 240, 144],
        faces: [
            None,
            None,
            gate_uv(10, 14, 14, 12),
            gate_uv(10, 1, 14, 3),
            gate_uv(10, 3, 14, 6),
            gate_uv(10, 9, 14, 12),
        ],
    },
    GateElement {
        min: [160, 96, 112],
        max: [224, 144, 144],
        faces: [
            None,
            None,
            gate_uv(2, 14, 6, 12),
            gate_uv(2, 1, 6, 3),
            gate_uv(2, 3, 6, 6),
            gate_uv(2, 9, 6, 12),
        ],
    },
    GateElement {
        min: [160, 192, 112],
        max: [224, 240, 144],
        faces: [
            None,
            None,
            gate_uv(2, 14, 6, 12),
            gate_uv(2, 1, 6, 3),
            gate_uv(2, 3, 6, 6),
            gate_uv(2, 9, 6, 12),
        ],
    },
];

const BAMBOO_GATE_OPEN: [GateElement; 8] = [
    BAMBOO_GATE_CLOSED[0],
    BAMBOO_GATE_CLOSED[1],
    GateElement {
        min: [0, 96, 208],
        max: [32, 240, 240],
        faces: [
            gate_uv(8, 3, 10, 12),
            gate_uv(8, 3, 10, 12),
            gate_uv(8, 14, 10, 12),
            gate_uv(8, 1, 10, 3),
            gate_uv(8, 3, 10, 12),
            gate_uv(8, 3, 10, 12),
        ],
    },
    GateElement {
        min: [224, 96, 208],
        max: [256, 240, 240],
        faces: [
            gate_uv(6, 3, 8, 12),
            gate_uv(6, 3, 8, 12),
            gate_uv(6, 14, 8, 12),
            gate_uv(6, 1, 8, 3),
            gate_uv(6, 3, 8, 12),
            gate_uv(6, 3, 8, 12),
        ],
    },
    GateElement {
        min: [0, 96, 144],
        max: [32, 144, 208],
        faces: [
            gate_uv(2, 3, 6, 6),
            gate_uv(2, 9, 6, 12),
            gate_uv_rot(2, 12, 6, 14, 270),
            gate_uv_rot(2, 1, 6, 3, 270),
            None,
            None,
        ],
    },
    GateElement {
        min: [0, 192, 144],
        max: [32, 240, 208],
        faces: [
            gate_uv(2, 3, 6, 6),
            gate_uv(2, 9, 6, 12),
            gate_uv_rot(2, 12, 6, 14, 270),
            gate_uv_rot(2, 1, 6, 3, 270),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 96, 144],
        max: [256, 144, 208],
        faces: [
            gate_uv(10, 3, 14, 6),
            gate_uv(10, 9, 14, 12),
            gate_uv_rot(10, 12, 14, 14, 270),
            gate_uv_rot(10, 1, 14, 3, 270),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 192, 144],
        max: [256, 240, 208],
        faces: [
            gate_uv(14, 3, 10, 6),
            gate_uv(10, 9, 14, 12),
            gate_uv_rot(10, 12, 14, 14, 270),
            gate_uv_rot(10, 1, 14, 3, 270),
            None,
            None,
        ],
    },
];

pub(in crate::compiler) fn gate_quads(
    materials: [u32; 6],
    orientation: u32,
    open: bool,
    in_wall: bool,
    bamboo: bool,
) -> [Vec<ModelQuad>; 2] {
    let elements = match (bamboo, open) {
        (false, false) => &GATE_CLOSED,
        (false, true) => &GATE_OPEN,
        (true, false) => &BAMBOO_GATE_CLOSED,
        (true, true) => &BAMBOO_GATE_OPEN,
    };
    let mut parts = [Vec::new(), Vec::new()];
    for (element_index, element) in elements.iter().enumerate() {
        let mut min = element.min;
        let mut max = element.max;
        if in_wall {
            min[1] -= 48;
            max[1] -= 48;
        }
        let (min, max) = rotate_gate_bounds(min, max, orientation);
        for (source_index, uv) in element.faces.iter().copied().enumerate() {
            let Some(uv) = uv else { continue };
            let target = rotate_gate_face(BlockFace::ALL[source_index], orientation);
            parts[usize::from(element_index >= 4)].push(gate_quad(
                materials[target as usize],
                min,
                max,
                target,
                uv,
            ));
        }
    }
    parts
}

pub(in crate::compiler) fn rotate_gate_face(face: BlockFace, orientation: u32) -> BlockFace {
    const ROTATED: [[BlockFace; 6]; 4] = [
        BlockFace::ALL,
        [
            BlockFace::North,
            BlockFace::South,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::East,
            BlockFace::West,
        ],
        [
            BlockFace::East,
            BlockFace::West,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::South,
            BlockFace::North,
        ],
        [
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::West,
            BlockFace::East,
        ],
    ];
    ROTATED[orientation as usize][face as usize]
}

pub(in crate::compiler) fn rotate_gate_bounds(
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
    orientation: u32,
) -> ([i16; 3], [i16; 3]) {
    match orientation {
        0 => ([min_x, min_y, min_z], [max_x, max_y, max_z]),
        1 => ([256 - max_z, min_y, min_x], [256 - min_z, max_y, max_x]),
        2 => (
            [256 - max_x, min_y, 256 - max_z],
            [256 - min_x, max_y, 256 - min_z],
        ),
        3 => ([min_z, min_y, 256 - max_x], [max_z, max_y, 256 - min_x]),
        _ => unreachable!("gate selectors are checked before geometry generation"),
    }
}

pub(in crate::compiler) fn gate_quad(
    material: u32,
    min: [i16; 3],
    max: [i16; 3],
    face: BlockFace,
    uv: GateFaceUv,
) -> ModelQuad {
    let mut quad = cuboid_quads([material; 6], min, max)[face as usize];
    let [u1, v1, u2, v2] = uv.rect.map(|coordinate| coordinate * 256);
    quad.uvs = match face {
        BlockFace::West | BlockFace::South => [[u1, v2], [u2, v2], [u2, v1], [u1, v1]],
        BlockFace::East | BlockFace::North => [[u1, v2], [u1, v1], [u2, v1], [u2, v2]],
        BlockFace::Down => [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        BlockFace::Up => [[u1, v1], [u1, v2], [u2, v2], [u2, v1]],
    };
    quad.uvs
        .rotate_left(((360 - uv.rotation) / 90 % 4) as usize);
    quad.flags = match face {
        BlockFace::West => 3,
        BlockFace::East => 4,
        BlockFace::Down => 1,
        BlockFace::Up => 2,
        BlockFace::North => 5,
        BlockFace::South => 6,
    };
    quad
}
