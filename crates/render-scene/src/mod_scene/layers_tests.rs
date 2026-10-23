use super::*;
use mod_render::retained::{GeometrySpec, Store, Vertex};

/// Supplies an admitted resource snapshot from one independent drawing owner.
fn output() -> RenderOutput {
    let mut store = Store::default();
    store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    store.commit();
    RenderOutput {
        retained: vec![Arc::clone(store.snapshot())],
        ..Default::default()
    }
}

#[test]
fn revoking_client_parts_preserves_personal_drawing() {
    let mut scene = ModRenderScene::default();
    let personal = output();
    scene.apply(&personal, 1);
    scene.apply_server(&output(), 2);
    assert_eq!(scene.retained.len(), 2);
    scene.apply_server(&RenderOutput::default(), 0);
    assert_eq!(scene.retained.len(), 1);
    assert!(Arc::ptr_eq(&scene.retained[0], &personal.retained[0]));
    let generation = scene.generation();
    scene.apply_server(&RenderOutput::default(), 0);
    assert_eq!(scene.generation(), generation);
}
