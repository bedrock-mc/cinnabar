//! Inventory plant surfaces whose leaves and stems use separate texture slots.

use super::{BlockFace, GuiBlockQuad, cube_face};

/// A full-height crossed stem inside a double-sided canopy with no floor.
pub(super) fn azalea(faces: [u32; 6]) -> Vec<GuiBlockQuad> {
    let mut materials = [faces[BlockFace::North as usize]; 6];
    materials[BlockFace::Up as usize] = faces[BlockFace::Up as usize];
    let mut quads: Vec<_> = BlockFace::ALL
        .into_iter()
        .filter(|face| *face != BlockFace::Down)
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            GuiBlockQuad {
                corners,
                uvs,
                material: materials[face as usize],
                two_sided: true,
            }
        })
        .collect();
    let [near, far] = [0.5 - 0.45, 0.5 + 0.45];
    for (start, end) in [([near, near], [far, far]), ([far, near], [near, far])] {
        quads.push(GuiBlockQuad {
            corners: [
                [start[0], 1.0, start[1]],
                [start[0], 0.0, start[1]],
                [end[0], 0.0, end[1]],
                [end[0], 1.0, end[1]],
            ],
            uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            material: faces[BlockFace::East as usize],
            two_sided: true,
        });
    }
    quads
}
