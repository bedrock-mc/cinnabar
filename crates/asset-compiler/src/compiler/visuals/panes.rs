use super::super::*;

pub(in crate::compiler) fn pane_quads(body: u32, edge: u32, mask: u32) -> Vec<ModelQuad> {
    debug_assert!(mask <= 15);
    let mut quads = cuboid_quads(
        [body, body, edge, edge, body, body],
        [112, 0, 112],
        [144, 256, 144],
    )
    .into_iter()
    .enumerate()
    .filter_map(|(face, quad)| {
        let touching_arm = match face {
            face if face == BlockFace::West as usize => 8,
            face if face == BlockFace::East as usize => 2,
            face if face == BlockFace::North as usize => 1,
            face if face == BlockFace::South as usize => 4,
            _ => 0,
        };
        (mask & touching_arm == 0).then_some(quad)
    })
    .collect::<Vec<_>>();
    let arms = [
        (
            1,
            [112, 0, 0],
            [144, 256, 112],
            [body, body, edge, edge, edge, edge],
            BlockFace::South as usize,
            BlockFace::North as usize,
        ),
        (
            2,
            [144, 0, 112],
            [256, 256, 144],
            [edge, edge, edge, edge, body, body],
            BlockFace::West as usize,
            BlockFace::East as usize,
        ),
        (
            4,
            [112, 0, 144],
            [144, 256, 256],
            [body, body, edge, edge, edge, edge],
            BlockFace::North as usize,
            BlockFace::South as usize,
        ),
        (
            8,
            [0, 0, 112],
            [112, 256, 144],
            [edge, edge, edge, edge, body, body],
            BlockFace::East as usize,
            BlockFace::West as usize,
        ),
    ];
    for (bit, min, max, materials, hidden_face, outward_face) in arms {
        if mask & bit == 0 {
            continue;
        }
        for (face, mut quad) in cuboid_quads(materials, min, max).into_iter().enumerate() {
            if face == hidden_face {
                continue;
            }
            if face == outward_face {
                let face_id = quad.flags & 7;
                quad.flags |= face_id << 4;
            }
            quads.push(quad);
        }
    }
    quads
}
