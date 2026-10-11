//! Filled world-space unit blocks; no edge strips or outlines.
use crate::geometry::{KIND_GROUND, ModVertex, STYLE_BILLBOARD};

pub const VERTICES_PER_BLOCK: usize = 36;

/// Builds solid unit cubes at the supplied block positions.
pub fn build(positions: &[[i32; 3]], color: [f32; 4]) -> Vec<ModVertex> {
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
