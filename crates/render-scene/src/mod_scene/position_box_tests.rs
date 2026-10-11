#[cfg(test)]
mod tests {
    use crate::ModRenderScene;
    use mod_render::geometry::KIND_STRIP;
    use mod_render::position_box::*;
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
