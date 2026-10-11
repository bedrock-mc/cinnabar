//! A translucent world-space cuboid with camera-facing solid edge strips.
use crate::geometry::{KIND_GROUND, KIND_STRIP, ModVertex, STYLE_BILLBOARD};

pub const VERTICES: usize = 36 + 12 * 6;
const CORAL: [f32; 3] = [0.888, 0.195, 0.125];
const EDGE_WIDTH: f32 = 0.012;
// The primitive fragment shader owns the matching edge-coverage branch.
const EDGE_STYLE: f32 = 30.0;

/// Accepts finite bounds with a positive extent on every axis.
pub fn valid(bounds: &[[f32; 3]; 2]) -> bool {
    bounds.iter().flatten().all(|v| v.is_finite())
        && (0..3).all(|axis| bounds[1][axis] > bounds[0][axis])
}

/// Appends the cuboid faces and camera-facing edge strips for the bounds.
pub fn append(out: &mut Vec<ModVertex>, [min, max]: [[f32; 3]; 2]) {
    let points: [[f32; 3]; 8] = std::array::from_fn(|i| {
        std::array::from_fn(|axis| {
            if i & (1 << axis) == 0 {
                min[axis]
            } else {
                max[axis]
            }
        })
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
            out.push(ModVertex {
                anchor: [x, y, z, KIND_GROUND],
                color: [CORAL[0], CORAL[1], CORAL[2], 0.12],
                style: [STYLE_BILLBOARD, 0.0, 0.0, 1.0],
                ..Default::default()
            });
        }
    }
    for a in 0..8 {
        for axis in 0..3 {
            if a & (1 << axis) != 0 {
                continue;
            }
            let b = a | (1 << axis);
            let direction = std::array::from_fn::<_, 3, _>(|i| if i == axis { 1.0 } else { 0.0 });
            for (side, end) in [
                (-1.0, a),
                (1.0, a),
                (1.0, b),
                (-1.0, a),
                (1.0, b),
                (-1.0, b),
            ] {
                let [x, y, z] = points[end];
                out.push(ModVertex {
                    anchor: [x, y, z, KIND_STRIP],
                    shape: [direction[0], direction[1], direction[2], EDGE_WIDTH * 0.5],
                    color: [CORAL[0], CORAL[1], CORAL[2], 0.9],
                    uv: [side, 0.0, 0.0, 0.0],
                    style: [EDGE_STYLE, 0.0, 0.0, 1.0],
                });
            }
        }
    }
}
