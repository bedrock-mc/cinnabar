//! The carried dried-ghast body and its separately textured feet.

use super::{GuiBlockQuad, cube_face};
use crate::BlockFace;

/// Material slot after the six ordinary block faces.
pub const DRIED_GHAST_TENTACLES: u32 = BlockFace::ALL.len() as u32;
const UV_INSET: f32 = 0.0025;

/// Emits the small body and six feet, using face indices for the body and the final slot for feet.
#[must_use]
pub fn dried_ghast_quads() -> Vec<GuiBlockQuad> {
    let mut out = Vec::new();
    for face in BlockFace::ALL {
        append_face(
            &mut out,
            face,
            [[3., 0., 3.], [13., 10., 13.]],
            [0., 0., 10., 10.],
            16.,
            face as u32,
        );
    }
    for (min, max, uv, front_first) in [
        ([5., 0., 0.], [7., 1., 3.], [0., 0.], true),
        ([9., 0., 0.], [11., 1., 3.], [0., 4.], true),
        ([13., 0., 5.], [16., 1., 7.], [10., 6.], false),
        ([13., 0., 9.], [16., 1., 11.], [10., 9.], false),
        ([0., 0., 9.], [3., 1., 11.], [10., 0.], false),
        ([0., 0., 5.], [3., 1., 7.], [10., 3.], false),
    ] {
        let [w, h, d] = std::array::from_fn(|axis| max[axis] - min[axis]);
        let [u, v] = uv;
        let sides = if front_first {
            [
                BlockFace::South,
                BlockFace::West,
                BlockFace::North,
                BlockFace::East,
            ]
        } else {
            [
                BlockFace::West,
                BlockFace::South,
                BlockFace::East,
                BlockFace::North,
            ]
        };
        let mut x = u;
        for face in sides {
            let width = if matches!(face, BlockFace::West | BlockFace::East) {
                d
            } else {
                w
            };
            append_face(
                &mut out,
                face,
                [min, max],
                [x, v + d, width, h],
                32.,
                DRIED_GHAST_TENTACLES,
            );
            x += width;
        }
        for (face, x) in [(BlockFace::Up, u + d), (BlockFace::Down, u + d + w)] {
            append_face(
                &mut out,
                face,
                [min, max],
                [x, v, w, d],
                32.,
                DRIED_GHAST_TENTACLES,
            );
        }
    }
    out
}

/// Expands one atlas rectangle onto a block-unit face with the carried texture inset.
fn append_face(
    out: &mut Vec<GuiBlockQuad>,
    face: BlockFace,
    [min, max]: [[f32; 3]; 2],
    [u, v, w, h]: [f32; 4],
    side: f32,
    material: u32,
) {
    let (corners, uvs) = cube_face(face);
    let low = [u / side + UV_INSET, v / side + UV_INSET];
    let high = [(u + w) / side - UV_INSET, (v + h) / side - UV_INSET];
    out.push(GuiBlockQuad {
        corners: corners.map(|point| {
            std::array::from_fn(|axis| (min[axis] + point[axis] * (max[axis] - min[axis])) / 16.0)
        }),
        uvs: uvs
            .map(|uv| std::array::from_fn(|axis| low[axis] + uv[axis] * (high[axis] - low[axis]))),
        material,
        two_sided: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dried_ghast_body_and_feet_keep_their_atlas_regions() {
        let quads = dried_ghast_quads();
        let body = quads
            .iter()
            .find(|quad| quad.material == BlockFace::South as u32)
            .unwrap();
        assert!(body.corners.iter().all(|p| p[2] == 13.0 / 16.0));
        assert!(
            body.uvs
                .iter()
                .flatten()
                .all(|v| (UV_INSET..=0.625 - UV_INSET).contains(v))
        );
        let first_foot_top = quads
            .iter()
            .find(|quad| {
                quad.material == DRIED_GHAST_TENTACLES
                    && quad.corners.iter().all(|p| p[1] == 1.0 / 16.0)
            })
            .unwrap();
        for (axis, bounds) in [(0, [0.09625, 0.15375]), (1, [0.0025, 0.09125])] {
            let min = first_foot_top
                .uvs
                .iter()
                .map(|uv| uv[axis])
                .fold(f32::INFINITY, f32::min);
            let max = first_foot_top
                .uvs
                .iter()
                .map(|uv| uv[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((min - bounds[0]).abs() < 0.000001);
            assert!((max - bounds[1]).abs() < 0.000001);
        }
    }
}
