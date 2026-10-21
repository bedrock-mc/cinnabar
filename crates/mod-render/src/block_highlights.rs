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
    let blocks = blocks.into_iter();
    let mut vertices = Vec::with_capacity(blocks.size_hint().0 * VERTICES_PER_BLOCK);
    for (position, color, outline) in blocks {
        let points: [[f32; 3]; 8] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| position[axis] as f32 + ((i >> axis) & 1) as f32)
        });
        for [a, b, c, d] in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            for index in [a, b, c, a, c, d] {
                let [x, y, z] = points[index];
                vertices.push(ModVertex {
                    anchor: [x, y, z, KIND_GROUND],
                    color,
                    style: [STYLE_BILLBOARD, 0.0, 0.0, 1.0],
                    ..Default::default()
                });
            }
        }
        if let Some(outline) = outline {
            for start in 0..8 {
                for axis in 0..3 {
                    if start & (1 << axis) != 0 {
                        continue;
                    }
                    let end = start | (1 << axis);
                    for [side, along] in [
                        [-1., 0.],
                        [1., 0.],
                        [1., 1.],
                        [-1., 0.],
                        [1., 1.],
                        [-1., 1.],
                    ] {
                        let point = points[if along == 0. { start } else { end }];
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
            }
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
        assert_eq!(vertices.len(), MAX_VERTICES_PER_BLOCK);
        assert!(
            vertices[..VERTICES_PER_BLOCK]
                .iter()
                .all(|v| v.color[3] == 0.)
        );
        let edges = &vertices[VERTICES_PER_BLOCK..];
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
}
