//! Filled world-space unit blocks with optional physical-pixel edge strips.
use crate::geometry::{
    KIND_GROUND, KIND_STRIP, ModVertex, STYLE_BILLBOARD, STYLE_UNTAPERED_RIBBON,
};
use mod_api::native_render::BlockOutline;

pub const VERTICES_PER_BLOCK: usize = 36;
/// A filled cube plus twelve independently expanded edge strips.
pub const MAX_VERTICES_PER_BLOCK: usize = VERTICES_PER_BLOCK + 12 * 6;

/// Builds solid unit cubes at the supplied block positions.
pub fn build(positions: &[[i32; 3]], color: [f32; 4]) -> Vec<ModVertex> {
    build_coloured(positions.iter().map(|position| (*position, color)))
}

/// Uses the same full-block geometry for independent per-block colours.
pub fn build_coloured(blocks: impl IntoIterator<Item = ([i32; 3], [f32; 4])>) -> Vec<ModVertex> {
    build_styled(
        blocks
            .into_iter()
            .map(|(position, colour)| (position, colour, None)),
    )
}

/// Emits optional outlines without consuming retained drawing-instance slots.
pub fn build_styled<'a>(
    blocks: impl IntoIterator<Item = ([i32; 3], [f32; 4], Option<&'a BlockOutline>)>,
) -> Vec<ModVertex> {
    use std::collections::{BTreeMap, BTreeSet};
    let blocks: Vec<_> = blocks.into_iter().collect();
    let occupied: BTreeSet<_> = blocks
        .iter()
        .map(|(position, _, _)| position.map(i64::from))
        .collect();
    let mut edges = BTreeMap::<([i64; 3], usize), (u8, Option<&BlockOutline>)>::new();
    let mut vertices = Vec::with_capacity(blocks.len() * VERTICES_PER_BLOCK);
    let mut emitted = BTreeSet::new();
    for (position, color, outline) in blocks {
        let position = position.map(i64::from);
        if !emitted.insert(position) {
            continue;
        }
        let points: [[i64; 3]; 8] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| position[axis] + ((i >> axis) & 1) as i64)
        });
        for (face, [a, b, c, d]) in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ]
        .into_iter()
        .enumerate()
        {
            let axis = 2 - face / 2;
            let mut neighbour = position;
            neighbour[axis] += if face % 2 == 0 { -1 } else { 1 };
            if occupied.contains(&neighbour) {
                continue;
            }
            if color[3] > 0. {
                for index in [a, b, c, a, c, d] {
                    let [x, y, z] = points[index].map(|v| v as f32);
                    vertices.push(ModVertex {
                        anchor: [x, y, z, KIND_GROUND],
                        color,
                        style: [STYLE_BILLBOARD, 0., 0., 1.],
                        ..Default::default()
                    });
                }
            }
            for (a, b) in [(a, b), (b, c), (c, d), (d, a)] {
                let start = std::array::from_fn(|axis| points[a][axis].min(points[b][axis]));
                let axis = (0..3)
                    .find(|axis| points[a][*axis] != points[b][*axis])
                    .unwrap();
                let entry = edges.entry((start, axis)).or_insert((0, None));
                entry.0 |= 1 << face;
                if entry.1.is_none() {
                    entry.1 = outline;
                }
            }
        }
    }
    for ((start, axis), (normals, outline)) in edges {
        // Two coplanar exterior faces meet without a crease. Only the union's boundary remains.
        let Some(outline) = outline.filter(|_| normals.count_ones() > 1) else {
            continue;
        };
        for [side, along] in [
            [-1., 0.],
            [1., 0.],
            [1., 1.],
            [-1., 0.],
            [1., 1.],
            [-1., 1.],
        ] {
            let mut point = start.map(|v| v as f32);
            point[axis] += along;
            let mut direction = [0.; 3];
            direction[axis] = 1.;
            vertices.push(ModVertex {
                anchor: [point[0], point[1], point[2], KIND_STRIP],
                shape: [
                    direction[0],
                    direction[1],
                    direction[2],
                    outline.width * 0.5,
                ],
                color: outline.colour,
                uv: [side, along, 0., 0.],
                style: [STYLE_UNTAPERED_RIBBON, 0., 1., 1.],
            });
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_only_emits_all_twelve_pixel_width_edges_without_extra_instances() {
        let outline = BlockOutline {
            colour: [0., 1., 1., 0.85],
            width: 1.5,
        };
        let vertices = build_styled([([2, 3, 4], [0.; 4], Some(&outline))]);
        assert_eq!(vertices.len(), 12 * 6);
        assert!(vertices[..0].iter().all(|v| v.color[3] == 0.));
        let edges = &vertices[..];
        let mut segments = std::collections::BTreeSet::new();
        for quad in edges.as_chunks::<6>().0 {
            let a = quad[0].anchor[..3]
                .iter()
                .map(|v| *v as i32)
                .collect::<Vec<_>>();
            let b = quad[2].anchor[..3]
                .iter()
                .map(|v| *v as i32)
                .collect::<Vec<_>>();
            assert_eq!(a.iter().zip(&b).map(|(a, b)| (b - a).abs()).sum::<i32>(), 1);
            assert!(segments.insert((a, b)));
            assert!(quad.iter().all(|v| v.anchor[3] == KIND_STRIP
                && v.shape[3] == 0.75
                && v.color == outline.colour
                && v.style[0] == STYLE_UNTAPERED_RIBBON));
        }
        assert_eq!(segments.len(), 12);
    }
    #[test]
    fn adjacent_blocks_have_no_internal_faces_or_coplanar_outline_seams() {
        let outline = BlockOutline {
            colour: [1.; 4],
            width: 1.5,
        };
        let vertices = build_styled(([-1, 0, 1]).map(|x| ([x, -4, 16], [1.; 4], Some(&outline))));
        let faces: Vec<_> = vertices
            .iter()
            .filter(|v| v.anchor[3] == KIND_GROUND)
            .collect();
        assert_eq!(faces.len(), 14 * 6); // two end faces and twelve exposed side faces
        for triangle in faces.chunks_exact(3) {
            // No face lies on either internal x-plane.
            assert!(
                ![0., 1.]
                    .iter()
                    .any(|x| triangle.iter().all(|v| v.anchor[0] == *x))
            );
        }
        let edges: Vec<_> = vertices
            .iter()
            .filter(|v| v.anchor[3] == KIND_STRIP)
            .collect();
        assert_eq!(edges.len(), 20 * 6); // four long edges (three segments each), eight end edges
        for segment in edges.chunks_exact(6) {
            if segment[0].anchor[0] == segment[2].anchor[0] {
                assert!([-1., 2.].contains(&segment[0].anchor[0]));
            }
        }
    }
}
