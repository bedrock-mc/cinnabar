//! Open carried shrieker walls, inner floor and bone caps.

use super::{GuiBlockQuad, cube_face};
use crate::BlockFace;

/// Emits separate outer and inner wall surfaces with cropped artwork.
#[must_use]
pub fn sculk_shrieker_quads() -> Vec<GuiBlockQuad> {
    use BlockFace::{Down, East, North, South, Up, West};
    let body = ([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
    let teeth = ([0.0625, 0.5, 0.0625], [0.9375; 3]);
    let inset = 0.875 - 0.0005;
    let mut out = vec![surface(Down, body, [0.; 3], Down as u32)];
    for (outside, inside, offset) in [
        (North, South, [0., 0., -inset]),
        (East, West, [inset, 0., 0.]),
        (South, North, [0., 0., inset]),
        (West, East, [-inset, 0., 0.]),
    ] {
        for face in [outside, inside] {
            let translation = if face == outside { [0.; 3] } else { offset };
            for bounds in [body, teeth] {
                out.push(surface(face, bounds, translation, face as u32));
            }
        }
    }
    out.push(surface(Up, body, [0.; 3], BlockFace::ALL.len() as u32));
    out.push(surface(Up, teeth, [0., 0.0005, 0.], Up as u32));
    out.push(surface(Down, body, [0., 0.9375, 0.], Up as u32));
    out
}

/// Crops a box face's UVs to its bounds before applying a surface translation.
fn surface(
    face: BlockFace,
    (min, max): ([f32; 3], [f32; 3]),
    offset: [f32; 3],
    material: u32,
) -> GuiBlockQuad {
    let (corners, uvs) = cube_face(face);
    let corners = corners.map(|point| {
        std::array::from_fn(|axis| min[axis] + point[axis] * (max[axis] - min[axis]) + offset[axis])
    });
    let (u_axis, v_axis) = if face.is_horizontal() {
        (
            if matches!(face, BlockFace::North | BlockFace::South) {
                0
            } else {
                2
            },
            1,
        )
    } else {
        (0, 2)
    };
    let uvs = uvs.map(|[u, v]| {
        [
            min[u_axis] + u * (max[u_axis] - min[u_axis]),
            if face.is_horizontal() {
                1.0 - max[v_axis] + v * (max[v_axis] - min[v_axis])
            } else {
                min[v_axis] + v * (max[v_axis] - min[v_axis])
            },
        ]
    });
    GuiBlockQuad {
        corners,
        uvs,
        material,
        two_sided: false,
    }
}
