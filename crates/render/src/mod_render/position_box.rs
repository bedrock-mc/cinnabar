//! A translucent world-space cuboid with camera-facing solid edge strips.
use mod_render::geometry::{KIND_GROUND, KIND_STRIP, ModVertex, STYLE_BILLBOARD};

pub(super) const VERTICES: usize = 36 + 12 * 6;
const CORAL: [f32; 3] = [0.888, 0.195, 0.125];
const EDGE_WIDTH: f32 = 0.012;
// The primitive fragment shader owns the matching edge-coverage branch.
const EDGE_STYLE: f32 = 30.0;

pub(super) fn valid(bounds: &[[f32; 3]; 2]) -> bool {
    bounds.iter().flatten().all(|v| v.is_finite())
        && (0..3).all(|axis| bounds[1][axis] > bounds[0][axis])
}

pub(super) fn append(out: &mut Vec<ModVertex>, [min, max]: [[f32; 3]; 2]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModRenderScene;
    use std::sync::Arc;

    #[test]
    fn full_cuboid_faces_and_twelve_edges_have_finite_world_geometry() {
        let bounds = [[-0.3, 0.0, -0.3], [0.3, 1.8, 0.3]];
        let mut vertices = Vec::new();
        append(&mut vertices, bounds);
        assert_eq!(vertices.len(), VERTICES);
        assert!(
            vertices
                .iter()
                .all(|v| v.anchor.iter().chain(v.shape.iter()).all(|v| v.is_finite()))
        );
        assert_eq!(
            vertices
                .iter()
                .filter(|v| v.anchor[3] == KIND_STRIP)
                .count(),
            72
        );
        for (axis, (&low, &high)) in bounds[0].iter().zip(&bounds[1]).enumerate() {
            assert!(vertices[..36].iter().any(|v| v.anchor[axis] == low));
            assert!(vertices[..36].iter().any(|v| v.anchor[axis] == high));
        }
    }

    #[test]
    fn marker_composes_with_guest_output_and_unchanged_bounds_reuse_geometry() {
        let mut scene = ModRenderScene::default();
        scene.set_position_box(Some([[0.0; 3], [1.0; 3]]));
        let vertices = Arc::clone(&scene.marker_vertices);
        scene.set_position_box(Some([[0.0; 3], [1.0; 3]]));
        assert!(Arc::ptr_eq(&vertices, &scene.marker_vertices));
        let mut primitives = mod_render::Primitives::default();
        primitives.billboards.push(mod_render::Billboard {
            position: [0.0; 3],
            width: 1.0,
            height: 1.0,
            color: [1.0; 4],
            pattern: mod_render::BillboardPattern::Solid,
            upright: false,
        });
        scene.apply(
            &mod_render::RenderOutput {
                passes: Vec::new(),
                primitives: Arc::new(primitives),
            },
            1,
        );
        assert_eq!(scene.vertex_count(), VERTICES + 6);
        let guest = Arc::clone(&scene.vertices);
        scene.set_position_box(Some([[1.0; 3], [2.0; 3]]));
        assert!(
            Arc::ptr_eq(&guest, &scene.vertices),
            "marker motion must not rebuild guest geometry"
        );
        scene.set_position_box(None);
        assert_eq!(scene.vertex_count(), 6);
        scene.set_position_box(Some([[f32::NAN; 3], [1.0; 3]]));
        assert_eq!(scene.vertex_count(), 6);
    }
}
