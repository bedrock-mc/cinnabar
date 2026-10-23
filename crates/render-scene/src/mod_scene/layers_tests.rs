use super::*;
use mod_render::retained::{GeometrySpec, Store, Vertex};

#[test]
fn bundle_gpu_reservations_survive_merging_extraction_and_revocation() {
    let outputs: Vec<_> = (1..=2)
        .map(|owner| RenderOutput {
            gpu_reservations: vec![mod_render::GpuReservation {
                owners: vec![owner],
                geometry_bytes: owner * 16,
                limit_bytes: owner * 64,
            }],
            ..Default::default()
        })
        .collect();
    let merged = mod_render::RenderMerge::default().merge(&outputs);
    let mut scene = ModRenderScene::default();
    scene.apply_server(&merged, 1);
    scene.apply(&output(), 1);
    let extracted = ModRenderScene::extract_resource(&scene);
    assert_eq!(extracted.gpu_reservations, merged.gpu_reservations);
    assert_eq!(extracted.gpu_reservations.len(), 2);
    scene.apply_server(&RenderOutput::default(), 0);
    assert!(scene.gpu_reservations.is_empty());
}

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
