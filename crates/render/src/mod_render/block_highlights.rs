//! Filled world-space unit blocks; no edge strips or outlines.
use mod_render::geometry::{KIND_GROUND, ModVertex, STYLE_BILLBOARD};

pub use mod_api::MAX_BLOCK_HIGHLIGHTS;
pub(super) const VERTICES_PER_BLOCK: usize = 36;

pub(super) fn build(positions: &[[i32; 3]], color: [f32; 4]) -> Vec<ModVertex> {
    let mut vertices = Vec::with_capacity(positions.len() * VERTICES_PER_BLOCK);
    for position in positions {
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
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModRenderScene;
    use std::sync::Arc;

    const PINK: [f32; 4] = [1.0, 0.08, 0.45, 1.0];

    #[test]
    fn cubes_cover_six_entire_faces_without_edge_geometry() {
        let position = [-17, 23, 91];
        let vertices = build(&[position], PINK);
        assert_eq!(vertices.len(), VERTICES_PER_BLOCK);
        for face in vertices.chunks_exact(6) {
            let points: Vec<_> = face
                .iter()
                .map(|v| Vec3::from_array(v.anchor[..3].try_into().unwrap()))
                .collect();
            let normal = (points[1] - points[0]).cross(points[2] - points[0]);
            assert_eq!(normal.length(), 1.0, "each triangle fills half a unit face");
            let second = (points[4] - points[3]).cross(points[5] - points[3]);
            assert_eq!(normal, second);
            assert_eq!(
                (0..3)
                    .filter(|axis| face
                        .iter()
                        .all(|v| v.anchor[*axis] == face[0].anchor[*axis]))
                    .count(),
                1
            );
        }
        assert!(vertices.iter().all(|v| v.anchor[3] == KIND_GROUND
            && v.color == PINK
            && v.style[0] == STYLE_BILLBOARD));
        for (axis, &low) in position.iter().enumerate() {
            assert_eq!(
                vertices
                    .iter()
                    .map(|v| v.anchor[axis])
                    .fold(f32::INFINITY, f32::min),
                low as f32
            );
            assert_eq!(
                vertices
                    .iter()
                    .map(|v| v.anchor[axis])
                    .fold(f32::NEG_INFINITY, f32::max),
                low as f32 + 1.0
            );
        }
    }

    use bevy::prelude::Vec3;

    #[test]
    fn unchanged_blocks_reuse_geometry_and_preserve_other_markers() {
        let mut scene = ModRenderScene::default();
        scene.set_position_box(Some([[0.0; 3], [1.0; 3]]));
        let marker = Arc::clone(&scene.marker_vertices);
        let guest = Arc::clone(&scene.vertices);
        scene.set_block_highlights(&[[2, 3, 4]], PINK);
        let vertices = Arc::clone(&scene.block_vertices);
        scene.set_block_highlights(&[[2, 3, 4]], PINK);
        assert!(Arc::ptr_eq(&vertices, &scene.block_vertices));
        assert!(Arc::ptr_eq(&marker, &scene.marker_vertices));
        assert!(Arc::ptr_eq(&guest, &scene.vertices));
        scene.set_block_highlights(&[], PINK);
        assert!(scene.block_vertices.is_empty());
        assert!(Arc::ptr_eq(&marker, &scene.marker_vertices));
    }

    #[test]
    fn publication_caps_blocks_and_rejects_invalid_color() {
        let mut scene = ModRenderScene::default();
        scene.set_block_highlights(&vec![[0; 3]; MAX_BLOCK_HIGHLIGHTS + 1], PINK);
        assert_eq!(
            scene.block_vertices.len(),
            MAX_BLOCK_HIGHLIGHTS * VERTICES_PER_BLOCK
        );
        scene.set_block_highlights(&[[0; 3]], [f32::NAN; 4]);
        assert!(scene.block_vertices.is_empty());
        scene.clear();
        assert_eq!(scene.vertex_count(), 0);
    }
}
